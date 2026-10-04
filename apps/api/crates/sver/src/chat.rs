//! Module 3 chat: persisted channel messages, one in-process fanout hub, HTTP and WebSocket.
//! HTTP sends and socket commands share `send`, so validation and permissions are identical.
use crate::{
    App,
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
    routing::get,
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
}
impl Row {
    fn json(mut self, app: &App) -> Value {
        profiles::hydrate(app, &mut self.author);
        json!({"id": self.id, "seq": self.seq, "author": self.author, "body": self.body, "created_at": self.created_at, "role": self.role})
    }
}
fn select() -> String {
    format!(
        "SELECT m.id,m.seq,m.author_id,m.body,m.created_at,{} AS author,CASE WHEN m.author_id=m.channel_id THEN 'owner' WHEN EXISTS(SELECT 1 FROM channel_moderators cm WHERE cm.channel_id=m.channel_id AND cm.user_id=m.author_id) THEN 'moderator' END AS role FROM chat_messages m JOIN channel_users a ON a.id=m.author_id",
        profiles::chip_sql("a")
    )
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
            "{} WHERE m.id=$1 AND m.deleted_at IS NULL AND m.expires_at>now()",
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
        "{} WHERE m.channel_id=$1 AND m.deleted_at IS NULL ORDER BY m.seq DESC LIMIT $2",
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
        .map(|r| r.json(app))
        .collect())
}

#[derive(Deserialize)]
pub struct Send {
    id: String,
    body: String,
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
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1 AND m.author_id=$2", select())))
            .bind(&input.id)
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    if let Some(row) = existing {
        return Ok(row.json(app));
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
    let inserted = sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO NOTHING")
        .bind(&input.id).bind(channel).bind(&user.id).bind(body).execute(&app.db).await?.rows_affected();
    if inserted == 0 {
        return Err(Fail::conflict("That message ID is already in use."));
    }
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
    Ok(message)
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
        json!({"messages": history(&app, &channel, &hidden).await?}),
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
    let first = json!({"type":"snapshot","messages":snapshot}).to_string();
    if ws.send(Message::Text(first.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(e) if e.channel == channel
                    && !e.author.as_ref().is_some_and(|a| hidden.contains(a))
                    && (e.seq == 0 || e.seq > cursor) => {
                    if ws.send(Message::Text(e.payload.to_string().into())).await.is_err() {
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
        .route("/api/chat/ws", get(socket))
}
