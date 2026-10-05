//! Module 3 chat: persisted channel messages, one in-process fanout hub, HTTP and WebSocket.
//! HTTP sends and socket commands share `send`, so validation and permissions are identical.
use crate::{
    App, moderation,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Path, Query, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
    routing::{get, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::broadcast;

const HISTORY: i64 = 100;
const FRAME: usize = 8 * 1024;
const MAX_SOCKETS: usize = 10_000;

/// Fanout for a single API instance. Running replicas needs a tested cross-instance design first.
#[derive(Clone)]
pub struct Hub {
    tx: broadcast::Sender<Arc<Event>>,
    sockets: Arc<AtomicUsize>,
}
impl Default for Hub {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(4096).0,
            sockets: Arc::default(),
        }
    }
}
pub struct Event {
    channel: String,
    author: Option<String>,
    seq: i64,
    payload: Value,
}
impl Hub {
    pub fn publish(&self, channel: &str, author: Option<&str>, seq: i64, payload: Value) {
        let _ = self.tx.send(Arc::new(Event {
            channel: channel.into(),
            author: author.map(Into::into),
            seq,
            payload,
        }));
    }
}
struct SocketSlot(Arc<AtomicUsize>);
impl Drop for SocketSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    seq: i64,
    author_id: String,
    body: String,
    created_at: DateTime<Utc>,
    author: Value,
    role: Option<String>,
    mentions: Vec<String>,
    reply: Option<Value>,
}
impl Row {
    fn json(mut self, app: &App) -> Value {
        profiles::hydrate(app, &mut self.author);
        json!({"id": self.id, "seq": self.seq, "author": self.author, "body": self.body, "created_at": self.created_at, "role": self.role, "mentions":self.mentions, "reply":self.reply})
    }
}
fn select() -> String {
    format!(
        "SELECT m.id,m.seq,m.author_id,m.body,m.created_at,{} AS author,m.role,
        ARRAY(SELECT username FROM channel_users WHERE id=ANY(m.mention_ids) AND eligible) AS mentions,
        CASE WHEN m.reply_to IS NOT NULL THEN jsonb_build_object('id',m.reply_to,
            'author_id',r.author_id,'username',ra.username,
            'body',left(regexp_replace(r.body,E'[\\n\\r]+',' ','g'),80)) END AS reply
        FROM chat_messages m JOIN channel_users a ON a.id=m.author_id
        LEFT JOIN chat_messages r ON r.id=m.reply_to AND r.channel_id=m.channel_id
            AND r.deleted_at IS NULL AND (r.expires_at>now() OR EXISTS(SELECT 1 FROM chat_pins WHERE message_id=r.id))
        LEFT JOIN channel_users ra ON ra.id=r.author_id",
        profiles::chip_sql("a")
    )
}

// Apply the same block rule to a quote as to its original message; never expose its internal ID.
fn visible_message(mut message: Value, hidden: &HashSet<String>) -> Value {
    if let Some(reply) = message["reply"].as_object_mut() {
        let blocked = reply
            .remove("author_id")
            .and_then(|v| v.as_str().map(|id| hidden.contains(id)))
            .unwrap_or(false);
        if blocked {
            reply.insert("body".into(), Value::Null);
            reply.insert("username".into(), Value::Null);
        }
    }
    message
}

const VISIBLE: &str = "m.deleted_at IS NULL AND (m.expires_at>now() OR EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id))";

async fn pinned(app: &App, channel: &str, hidden: &HashSet<String>) -> Res<Value> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} JOIN chat_pins p ON p.message_id=m.id AND p.channel_id=m.channel_id WHERE m.channel_id=$1 AND {VISIBLE}", select()
    ))).bind(channel).fetch_optional(&app.db).await?;
    Ok(row
        .filter(|r| !hidden.contains(&r.author_id))
        .map(|r| visible_message(r.json(app), hidden))
        .unwrap_or(Value::Null))
}

/// An active pin is retained until removed; ordinary chat still expires after seven days.
pub async fn expire(app: &App) -> crate::Result<()> {
    let mut tx = app.db.begin().await?;
    // Lock candidates, then recheck pins in a fresh statement snapshot. A pin committed while
    // cleanup was waiting on a message must not disappear with that message's ordinary expiry.
    let ids: Vec<String> = sqlx::query_scalar("SELECT m.id FROM chat_messages m WHERE expires_at<=now() AND NOT EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id) ORDER BY expires_at LIMIT 1000 FOR UPDATE SKIP LOCKED")
        .fetch_all(&mut *tx).await?;
    let removed: Vec<(String, String)> = sqlx::query_as("DELETE FROM chat_messages m WHERE id=ANY($1) AND NOT EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id) RETURNING channel_id,id")
        .bind(ids).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for (channel, id) in removed {
        app.chat
            .publish(&channel, None, 0, json!({"type":"delete","id":id}));
    }
    Ok(())
}

/// Whole ASCII usernames only: no email addresses, partial long names or HTML parsing.
fn mention_names(body: &str) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut names = HashSet::new();
    for (i, c) in chars.iter().enumerate() {
        if *c != '@'
            || (i > 0 && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '_' | '@')))
        {
            continue;
        }
        let name: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
            .collect();
        if (3..=25).contains(&name.len())
            && !chars
                .get(i + 1 + name.len())
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            names.insert(name.to_ascii_lowercase());
        }
    }
    names.into_iter().collect()
}

/// Refresh a moderation change for already-connected viewers after its database commit.
pub async fn notify_changed(app: &App, id: &str) -> Res<()> {
    let row: Option<(String, bool)> =
        sqlx::query_as("SELECT channel_id,deleted_at IS NOT NULL FROM chat_messages WHERE id=$1")
            .bind(id)
            .fetch_optional(&app.db)
            .await?;
    let Some((channel, hidden)) = row else {
        return Ok(());
    };
    if hidden {
        app.chat
            .publish(&channel, None, 0, json!({"type":"delete","id":id}));
    } else {
        let message: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{} WHERE m.id=$1 AND {VISIBLE}",
            select()
        )))
        .bind(id)
        .fetch_optional(&app.db)
        .await?;
        if let Some(message) = message {
            let author = message.author_id.clone();
            app.chat.publish(
                &channel,
                Some(&author),
                0,
                json!({"type":"message","message":message.json(app)}),
            );
        }
    }
    Ok(())
}

/// The channel's owner id; unknown, deleted and restricted channels get the same 404 as profiles.
async fn channel(app: &App, name: &str) -> Res<String> {
    let mut conn = app.db.acquire().await?;
    Ok(profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id)
}
/// People whose messages the viewer doesn't see: blocks in either direction.
async fn hidden(app: &App, viewer: Option<&str>) -> Res<HashSet<String>> {
    let Some(viewer) = viewer else {
        return Ok(HashSet::new());
    };
    let ids: Vec<String> = sqlx::query_scalar("SELECT blocked_id FROM user_blocks WHERE blocker_id=$1 UNION SELECT blocker_id FROM user_blocks WHERE blocked_id=$1")
        .bind(viewer).fetch_all(&app.db).await?;
    Ok(ids.into_iter().collect())
}
async fn history(app: &App, channel: &str, hidden: &HashSet<String>) -> Res<Vec<Value>> {
    // select() contains only fixed SQL and a literal chip alias; message values are bound.
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE m.channel_id=$1 AND {VISIBLE} ORDER BY m.seq DESC LIMIT $2",
        select()
    )))
    .bind(channel)
    .bind(HISTORY)
    .fetch_all(&app.db)
    .await?;
    Ok(rows
        .into_iter()
        .rev()
        .filter(|r| !hidden.contains(&r.author_id))
        .map(|r| visible_message(r.json(app), hidden))
        .collect())
}

#[derive(Deserialize)]
pub struct Send {
    id: String,
    body: String,
    reply_to: Option<String>,
}

/// Persists one message and fans it out. Acknowledged only after the insert commits.
async fn send(app: &App, jar: &CookieJar, channel: &str, input: Send) -> Res<Value> {
    // Rechecked on every send, so a revoked session or new restriction takes effect at once.
    let user = profiles::signed_in(app, jar).await?;
    if uuid::Uuid::parse_str(&input.id).is_err() {
        return Err(Fail::bad("Invalid message ID."));
    }
    let body = input.body.trim();
    let length = body.chars().count();
    if length == 0 || length > 500 {
        return Err(Fail::field("body", "Messages are 1–500 characters."));
    }
    if body.matches('\n').count() > 4 {
        return Err(Fail::field("body", "Use at most four line breaks."));
    }
    if body.chars().any(|c| c.is_control() && c != '\n') {
        return Err(Fail::field("body", "Messages can only contain text."));
    }
    let existing: Option<Row> =
        // select() contains only fixed SQL and a literal chip alias; message values are bound.
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1 AND m.author_id=$2 AND m.channel_id=$3 AND {VISIBLE}", select())))
            .bind(&input.id)
            .bind(&user.id)
            .bind(channel)
            .fetch_optional(&app.db)
            .await?;
    if let Some(row) = existing {
        return Ok(visible_message(
            row.json(app),
            &hidden(app, Some(&user.id)).await?,
        ));
    }
    let standing: Option<(bool, bool)> =
        sqlx::query_as("SELECT email_verified, eligible FROM channel_users WHERE id=$1")
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    match standing {
        Some((false, _)) => return Err(Fail::denied("Verify your email address to chat.")),
        Some((true, true)) => {}
        _ => return Err(Fail::denied("Your account can't chat right now.")),
    }
    let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1))")
        .bind(channel).bind(&user.id).fetch_one(&app.db).await?;
    if blocked {
        return Err(Fail::denied("You can't chat in this channel."));
    }
    crate::moderation::check_send(app, channel, &user, body).await?;
    sec::reserve(app, vec![format!("chat-second:{}", user.id)], 2, 1).await?;
    sec::reserve(app, vec![format!("chat-ten:{}", user.id)], 20, 10).await?;
    let hidden = hidden(app, Some(&user.id)).await?;
    let role = moderation::role_of(app, channel, &user)
        .await?
        .map(|r| r.name());
    let mut tx = app.db.begin().await?;
    if let Some(reply) = &input.reply_to {
        // Lock against deletion while accepting the reply. Later reads always join the current body.
        let author: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT m.author_id FROM chat_messages m WHERE m.id=$1 AND m.channel_id=$2 AND {VISIBLE} FOR SHARE")))
            .bind(reply).bind(channel).fetch_optional(&mut *tx).await?;
        if author.is_none_or(|a| hidden.contains(&a)) {
            return Err(Fail::field(
                "reply_to",
                "Reply to a visible message in this channel.",
            ));
        }
    }
    let mentions: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM channel_users WHERE lower(username)=ANY($1) AND eligible",
    )
    .bind(mention_names(body))
    .fetch_all(&mut *tx)
    .await?;
    let inserted = sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body,reply_to,mention_ids,role) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(id) DO NOTHING")
        .bind(&input.id).bind(channel).bind(&user.id).bind(body).bind(&input.reply_to).bind(mentions).bind(role).execute(&mut *tx).await?.rows_affected();
    if inserted == 0 {
        return Err(Fail::conflict("That message ID is already in use."));
    }
    crate::factions::chat(app, &mut tx, channel, &user.id, &input.id).await?;
    crate::plays::chat_vote(&mut tx, channel, &user, body).await?;
    tx.commit().await?;
    // select() contains only fixed SQL and a literal chip alias; message values are bound.
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1", select())))
        .bind(&input.id)
        .fetch_one(&app.db)
        .await?;
    let (seq, author) = (row.seq, row.author_id.clone());
    let message = row.json(app);
    app.chat.publish(
        channel,
        Some(&author),
        seq,
        json!({"type":"message","message":message}),
    );
    Ok(visible_message(message, &hidden))
}

pub async fn read(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    let hidden = hidden(&app, viewer.as_ref().map(|v| v.id.as_str())).await?;
    Ok(Json(
        json!({"messages": history(&app, &channel, &hidden).await?, "pinned":pinned(&app, &channel, &hidden).await?, "emotes":crate::emotes::catalog(&app, &channel).await?, "followers_only_until":crate::moderation::followers_only(&app, &channel).await?}),
    ))
}
pub async fn post(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Send>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    Ok(Json(
        json!({"message": send(&app, &jar, &channel, input).await?}),
    ))
}

#[derive(Deserialize)]
pub struct Pin {
    message_id: Option<String>,
    reason: String,
}
/// A null message unpins. An upsert makes concurrent replacements leave exactly one pin.
pub async fn pin(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Pin>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = moderation::actor(&app, &jar, &channel).await?;
    if !matches!(role, moderation::Role::Owner | moderation::Role::Moderator) {
        return Err(Fail::denied(
            "Only the channel owner or an appointed moderator can pin messages.",
        ));
    }
    let reason = moderation::reason(&input.reason)?;
    let hidden = hidden(&app, Some(&user.id)).await?;
    let mut tx = app.db.begin().await?;
    if let Some(id) = &input.message_id {
        let author: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT m.author_id FROM chat_messages m WHERE m.id=$1 AND m.channel_id=$2 AND {VISIBLE} FOR UPDATE"
        ))).bind(id).bind(&channel).fetch_optional(&mut *tx).await?;
        if author.is_none_or(|a| hidden.contains(&a)) {
            return Err(Fail::missing());
        }
        sqlx::query("INSERT INTO chat_pins(channel_id,message_id) VALUES($1,$2) ON CONFLICT(channel_id) DO UPDATE SET message_id=EXCLUDED.message_id")
            .bind(&channel).bind(id).execute(&mut *tx).await?;
    } else {
        sqlx::query("DELETE FROM chat_pins WHERE channel_id=$1")
            .bind(&channel)
            .execute(&mut *tx)
            .await?;
    }
    moderation::log(
        &mut tx,
        &channel,
        &user.id,
        role,
        if input.message_id.is_some() {
            "pin_message"
        } else {
            "unpin_message"
        },
        None,
        input.message_id.as_deref(),
        json!({}),
        &reason,
    )
    .await?;
    tx.commit().await?;
    // Read current state at delivery, so concurrent replacements cannot publish stale pins.
    app.chat.publish(&channel, None, 0, json!({"type":"pin"}));
    Ok(Json(
        json!({"pinned":pinned(&app, &channel, &hidden).await?}),
    ))
}

#[derive(Deserialize)]
pub struct Join {
    channel: String,
}
/// Same-origin socket on the session cookie (never a token in the URL); one channel per connection.
pub async fn socket(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Query(join): Query<Join>,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.config.origin.as_str()) {
        return Err(Fail::denied("Invalid request origin."));
    }
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("chat-connect:{ip}")], 30, 60).await?;
    let channel = channel(&app, &join.channel).await?;
    if app.chat.sockets.load(Ordering::Relaxed) >= MAX_SOCKETS {
        return Err(Fail::unavailable("Chat is busy. Please try again shortly."));
    }
    Ok(upgrade
        .max_message_size(FRAME)
        .max_frame_size(FRAME)
        .on_upgrade(move |ws| session(app, jar, channel, ws)))
}

async fn session(app: App, jar: CookieJar, channel: String, mut ws: WebSocket) {
    app.chat.sockets.fetch_add(1, Ordering::Relaxed);
    let _slot = SocketSlot(app.chat.sockets.clone());
    // Subscribe before reading history so nothing posted in between is missed.
    let mut events = app.chat.tx.subscribe();
    let viewer = profiles::viewer(&app, &jar).await.ok().flatten();
    let Ok(hidden) = hidden(&app, viewer.as_ref().map(|v| v.id.as_str())).await else {
        return;
    };
    let Ok(snapshot) = history(&app, &channel, &hidden).await else {
        return;
    };
    let cursor = snapshot.last().and_then(|m| m["seq"].as_i64()).unwrap_or(0);
    let Ok(pin) = pinned(&app, &channel, &hidden).await else {
        return;
    };
    let Ok(mut emotes) = crate::emotes::catalog(&app, &channel).await else {
        return;
    };
    let Ok(followers_only) = crate::moderation::followers_only(&app, &channel).await else {
        return;
    };
    let first = json!({"type":"snapshot","messages":snapshot,"pinned":pin,"emotes":emotes,"followers_only_until":followers_only}).to_string();
    if ws.send(Message::Text(first.into())).await.is_err() {
        return;
    }
    // Also catches quarantine, appeals and account removals performed by the worker.
    // ponytail: one small catalog query per socket every five seconds; share invalidations
    // across sockets if measured viewer load makes this polling significant.
    let mut refresh = tokio::time::interval(std::time::Duration::from_secs(5));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = refresh.tick() => {
                let Ok(current) = crate::emotes::catalog(&app, &channel).await else { return; };
                if current != emotes {
                    emotes = current;
                    if ws.send(Message::Text(json!({"type":"emotes","emotes":emotes}).to_string().into())).await.is_err() { return; }
                }
            }
            event = events.recv() => match event {
                Ok(e) if e.channel == channel
                    && (e.seq == 0 || e.seq > cursor) => {
                    let Ok(hidden) = crate::chat::hidden(&app, viewer.as_ref().map(|v| v.id.as_str())).await else { return; };
                    if e.author.as_ref().is_some_and(|a| hidden.contains(a)) { continue; }
                    let mut payload = e.payload.clone();
                    if payload["type"] == "emotes" {
                        let Ok(current) = crate::emotes::catalog(&app, &channel).await else { return; };
                        emotes = current;
                        payload["emotes"] = json!(emotes);
                    } else if payload["type"] == "pin" {
                        let Ok(pin) = pinned(&app, &channel, &hidden).await else { return; };
                        payload["pinned"] = pin;
                    } else if payload["type"] == "message" {
                        // Re-read after queueing: deletion or a new block must not leak a stale quote.
                        // ponytail: one indexed read per recipient/event; batch fanout if measured chat load requires it.
                        let row = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1 AND {VISIBLE}", select())))
                            .bind(payload["message"]["id"].as_str().unwrap_or(""))
                            .fetch_optional(&app.db).await;
                        let Ok(Some(row)) = row else {
                            if row.is_err() { return; }
                            continue;
                        };
                        payload["message"] = visible_message(row.json(&app), &hidden);
                    }
                    if ws.send(Message::Text(payload.to_string().into())).await.is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                // A reader that fell behind reconnects and reloads rather than buffering without bound.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let _ = ws.send(Message::Close(Some(CloseFrame { code: 4000, reason: "resync".into() }))).await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = ws.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let reply = match serde_json::from_str::<Send>(&text) {
                        Ok(input) => {
                            let id = input.id.clone();
                            match send(&app, &jar, &channel, input).await {
                                Ok(message) => json!({"type":"ack","id":id,"message":message}),
                                Err(fail) => json!({"type":"error","id":id,"message":fail.message}),
                            }
                        }
                        Err(_) => json!({"type":"error","message":"Invalid chat command."}),
                    };
                    if ws.send(Message::Text(reply.to_string().into())).await.is_err() {
                        return;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/chat", get(read).post(post))
        .route("/api/channels/{username}/chat/pin", put(pin))
        .route("/api/chat/ws", get(socket))
}

#[cfg(test)]
mod tests {
    #[test]
    fn mentions_are_whole_usernames_not_email_or_markup() {
        let mut names = super::mention_names(
            "@Real_User, (@SECOND) @real_user hello@Mailbox.test @@double @ab @abcdefghijklmnopqrstuvwxyz @nameé <img src=x onerror=alert(1)>",
        );
        names.sort();
        assert_eq!(names, ["real_user", "second"]);
    }
}
