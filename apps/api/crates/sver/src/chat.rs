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
    pub(crate) channel: String,
    pub(crate) author: Option<String>,
    seq: i64,
    pub(crate) payload: Value,
}
impl Hub {
    /// Every event on this instance; the board overlay socket filters its own channel's.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.tx.subscribe()
    }
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
pub(crate) struct Row {
    id: String,
    seq: i64,
    author_id: String,
    body: String,
    created_at: DateTime<Utc>,
    author: Value,
    role: Option<String>,
    mentions: Vec<String>,
    reply: Option<Value>,
    /// The MAGNet Hype lane a message came from; None for a channel's own chat.
    origin: Option<String>,
    /// Valor paid with this message (a tribute, docs/SUPPORT.md).
    tribute: Option<i32>,
    /// The author's active subscription to this channel: tier and months, for the badge.
    sub: Option<Value>,
    /// Paid for with the channel's "highlight my message" reward.
    highlighted: bool,
    /// The author's creator tier (1-3), for their badge.
    creator_tier: Option<i16>,
    /// The Skill this paid message played.
    skill: Option<String>,
    /// Lifetime Engagement Valor the author earned in this channel (their loyalty rank).
    earned: Option<i64>,
    /// A KLIPY GIF (docs/COMMUNITY.md "GIFs in chat"); the body then holds its alt text.
    gif: Option<Value>,
}
impl Row {
    async fn hydrated(self, app: &App) -> Res<Value> {
        let mut value = self.json(app);
        crate::guilds::hydrate_badges(app, std::slice::from_mut(&mut value)).await?;
        Ok(value)
    }
    pub(crate) fn json(mut self, app: &App) -> Value {
        profiles::hydrate(app, &mut self.author);
        json!({"id": self.id, "seq": self.seq, "author": self.author, "body": self.body, "created_at": self.created_at, "role": self.role, "mentions":self.mentions, "reply":self.reply, "origin":self.origin, "tribute":self.tribute, "sub":self.sub, "highlighted":self.highlighted, "creator_tier":self.creator_tier, "skill":self.skill, "gif":self.gif, "loyalty":crate::progression::loyalty(self.earned.unwrap_or(0))})
    }
}
pub(crate) fn select() -> String {
    format!(
        "SELECT m.id,m.seq,m.author_id,m.body,m.created_at,{} || jsonb_build_object('guild',{}) AS author,m.role,m.origin,m.tribute,m.highlighted,m.skill,m.gif,
        (SELECT t.tier FROM creator_tiers t WHERE t.user_id=m.author_id AND t.tier>0) AS creator_tier,
        (SELECT e.earned FROM engagement e WHERE e.channel_id=m.channel_id AND e.user_id=m.author_id) AS earned,
        (SELECT jsonb_build_object('tier',s.tier,'months',s.months) FROM channel_subs s WHERE s.channel_id=m.channel_id AND s.user_id=m.author_id AND s.paid_through>now()) AS sub,
        ARRAY(SELECT username FROM channel_users WHERE id=ANY(m.mention_ids) AND eligible) AS mentions,
        CASE WHEN m.reply_to IS NOT NULL THEN jsonb_build_object('id',m.reply_to,
            'author_id',r.author_id,'username',ra.username,
            'body',left(regexp_replace(r.body,E'[\\n\\r]+',' ','g'),80)) END AS reply
        FROM chat_messages m JOIN channel_users a ON a.id=m.author_id
        LEFT JOIN chat_messages r ON r.id=m.reply_to AND r.channel_id=m.channel_id
            AND r.squad_id IS NOT DISTINCT FROM m.squad_id
            AND r.deleted_at IS NULL AND (r.expires_at>now() OR EXISTS(SELECT 1 FROM chat_pins WHERE message_id=r.id))
        LEFT JOIN channel_users ra ON ra.id=r.author_id",
        profiles::chip_sql("a"), crate::guilds::badge_sql("a.id")
    )
}

// Apply the same block rule to a quote as to its original message; never expose its internal ID.
pub(crate) fn visible_message(mut message: Value, hidden: &HashSet<String>) -> Value {
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

pub(crate) const VISIBLE: &str = "m.deleted_at IS NULL AND (m.expires_at>now() OR EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id))";
/// Public read projection for recording replay. The chat module retains ownership of its
/// visibility rules; recordings supply their segment-to-wall-clock mapping around this query.
pub fn replay_source_sql() -> String {
    format!(
        "SELECT m.id,m.author_id,m.channel_id,m.created_at,jsonb_build_object('id',m.id,'body',m.body,'author',{},'role',m.role,'created_at',m.created_at) AS message FROM chat_messages m JOIN channel_users a ON a.id=m.author_id WHERE {VISIBLE} AND a.eligible",
        profiles::chip_sql("a")
    )
}
pub async fn replay_redacted(db: &mut sqlx::PgConnection, ids: &[String]) -> Res<HashSet<String>> {
    Ok(sqlx::query_scalar::<_,String>("SELECT m.id FROM chat_messages m LEFT JOIN channel_users u ON u.id=m.author_id WHERE m.id=ANY($1) AND (m.deleted_at IS NOT NULL OR NOT coalesce(u.eligible,false))").bind(ids).fetch_all(db).await?.into_iter().collect())
}

async fn pinned(app: &App, channel: &str, hidden: &HashSet<String>) -> Res<Value> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} JOIN chat_pins p ON p.message_id=m.id AND p.channel_id=m.channel_id WHERE m.channel_id=$1 AND m.squad_id IS NULL AND {VISIBLE}", select()
    ))).bind(channel).fetch_optional(&app.db).await?;
    let mut value = row
        .filter(|r| !hidden.contains(&r.author_id))
        .map(|r| visible_message(r.json(app), hidden))
        .unwrap_or(Value::Null);
    crate::guilds::hydrate_badges(app, std::slice::from_mut(&mut value)).await?;
    Ok(value)
}

/// Tells open chats (and the Hype room a message came from) that it is gone.
pub(crate) fn publish_delete(app: &App, channel: Option<&str>, origin: Option<&str>, id: &str) {
    if let Some(channel) = channel {
        app.chat
            .publish(channel, None, 0, json!({"type":"delete","id":id}));
    }
    if let Some(lane) = origin {
        app.chat.publish(
            &format!("magnet:{lane}"),
            None,
            0,
            json!({"type":"delete","id":id}),
        );
    }
}

/// An active pin is retained until removed; ordinary chat still expires after seven days.
pub async fn expire(app: &App) -> crate::Result<()> {
    let mut tx = app.db.begin().await?;
    // Lock candidates, then recheck pins in a fresh statement snapshot. A pin committed while
    // cleanup was waiting on a message must not disappear with that message's ordinary expiry.
    let ids: Vec<String> = sqlx::query_scalar("SELECT m.id FROM chat_messages m WHERE expires_at<=now() AND NOT EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id) ORDER BY expires_at LIMIT 1000 FOR UPDATE SKIP LOCKED")
        .fetch_all(&mut *tx).await?;
    let redacted = replay_redacted(&mut tx, &ids)
        .await
        .map_err(|_| crate::Error::internal())?;
    crate::videos::redact_messages(&mut tx, &redacted.into_iter().collect::<Vec<_>>())
        .await
        .map_err(|_| crate::Error::internal())?;
    let removed: Vec<(Option<String>, Option<String>, String)> = sqlx::query_as("DELETE FROM chat_messages m WHERE id=ANY($1) AND NOT EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id) RETURNING coalesce(squad_id,channel_id),origin,id")
        .bind(ids).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for (channel, origin, id) in removed {
        publish_delete(app, channel.as_deref(), origin.as_deref(), &id);
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
    let row: Option<(Option<String>, Option<String>, bool)> =
        sqlx::query_as("SELECT coalesce(squad_id,channel_id),origin,deleted_at IS NOT NULL FROM chat_messages WHERE id=$1")
            .bind(id)
            .fetch_optional(&app.db)
            .await?;
    let Some((channel, origin, hidden)) = row else {
        return Ok(());
    };
    if hidden {
        publish_delete(app, channel.as_deref(), origin.as_deref(), id);
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
            let event = json!({"type":"message","message":message.hydrated(app).await?});
            if let Some(channel) = channel {
                app.chat.publish(&channel, Some(&author), 0, event.clone());
            }
            if let Some(lane) = origin {
                app.chat
                    .publish(&format!("magnet:{lane}"), Some(&author), 0, event);
            }
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
pub(crate) async fn hidden(app: &App, viewer: Option<&str>) -> Res<HashSet<String>> {
    let Some(viewer) = viewer else {
        return Ok(HashSet::new());
    };
    let ids: Vec<String> = sqlx::query_scalar("SELECT blocked_id FROM user_blocks WHERE blocker_id=$1 UNION SELECT blocker_id FROM user_blocks WHERE blocked_id=$1")
        .bind(viewer).fetch_all(&app.db).await?;
    Ok(ids.into_iter().collect())
}
pub(crate) async fn history(
    app: &App,
    channel: &str,
    hidden: &HashSet<String>,
    squad: Option<&str>,
) -> Res<Vec<Value>> {
    // select() contains only fixed SQL and a literal chip alias; message values are bound.
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE m.channel_id=$1 AND m.squad_id IS NOT DISTINCT FROM $3 AND {VISIBLE} ORDER BY m.seq DESC LIMIT $2",
        select()
    )))
    .bind(channel)
    .bind(HISTORY)
    .bind(squad)
    .fetch_all(&app.db)
    .await?;
    let mut messages: Vec<Value> = rows
        .into_iter()
        .rev()
        .filter(|r| !hidden.contains(&r.author_id))
        .map(|r| visible_message(r.json(app), hidden))
        .collect();
    crate::guilds::hydrate_badges(app, &mut messages).await?;
    // Linked chat (docs/LINKED_CHAT.md): outside messages share the chat sequence; keep the
    // newest HISTORY of both, in order.
    if squad.is_none() {
        messages.extend(crate::linked_chat::recent(app, channel, HISTORY).await?);
        messages.sort_by_key(|m| m["seq"].as_i64().unwrap_or_default());
        let extra = messages.len().saturating_sub(HISTORY as usize);
        messages.drain(..extra);
    }
    Ok(messages)
}

#[derive(Deserialize)]
pub struct Send {
    id: String,
    body: String,
    reply_to: Option<String>,
    /// Valor to pay with the message (at least 10); the channel must be able to earn.
    #[serde(default)]
    tribute: Option<i64>,
    /// Spend the channel's Engagement Valor on "highlight my message".
    #[serde(default)]
    highlight: bool,
    /// Play a Skill (docs/CROWDSYNC.md "Skills"), paid in Valor like a tribute.
    #[serde(default)]
    skill: Option<String>,
    /// A GIF the browser picked from KLIPY search; the body is then ignored.
    #[serde(default)]
    gif: Option<Gif>,
}

/// A KLIPY result as the browser received it. KLIPY's terms have media load straight from their
/// URLs, so only those URLs are stored, after checking they point at KLIPY's media host.
#[derive(Deserialize)]
pub struct Gif {
    slug: String,
    title: String,
    /// The still frame shown until the viewer hovers or taps.
    still: String,
    /// The animated version (webp).
    play: String,
    width: i32,
    height: i32,
}
const GIF_HOST: &str = "https://static.klipy.com/";
impl Gif {
    /// The stored reference, or None when it isn't a KLIPY GIF.
    fn checked(&self) -> Option<Value> {
        let url = |u: &str| {
            u.len() <= 300
                && u.strip_prefix(GIF_HOST).is_some_and(|rest| {
                    !rest.is_empty()
                        && rest
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
                })
        };
        let ok = (1..=100).contains(&self.slug.len())
            && self
                .slug
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && url(&self.still)
            && url(&self.play)
            && (1..=2000).contains(&self.width)
            && (1..=2000).contains(&self.height);
        ok.then(|| json!({"slug": self.slug, "still": self.still, "play": self.play, "width": self.width, "height": self.height}))
    }
    /// Alt text and the text every other reader (linked chat, apps, replay) sees.
    fn body(&self) -> String {
        let title: String = self
            .title
            .chars()
            .filter(|c| !c.is_control())
            .take(100)
            .collect();
        match title.trim() {
            "" => "GIF".into(),
            t => format!("GIF: {t}"),
        }
    }
}
/// The KLIPY app key, sent to browsers (KLIPY wants searches made from the viewer's device);
/// unset turns GIFs off everywhere.
fn klipy_key() -> Option<String> {
    std::env::var("KLIPY_APP_KEY")
        .ok()
        .filter(|k| !k.is_empty())
}
/// Who may send GIFs in a channel: off, everyone, followers or subscribers.
pub(crate) async fn gif_setting(app: &App, channel: &str) -> Res<String> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT gifs FROM chat_settings WHERE channel_id=$1),'everyone')",
    )
    .bind(channel)
    .fetch_one(&app.db)
    .await?)
}
/// What the chat composer needs: the key and who may send GIFs here, or null when nobody can.
async fn gifs(app: &App, channel: &str) -> Res<Value> {
    let who = gif_setting(app, channel).await?;
    Ok(match klipy_key() {
        Some(key) if who != "off" => json!({"key": key, "who": who}),
        _ => Value::Null,
    })
}
/// The channel's GIF setting for this sender; channel roles are exempt from the audience limit.
async fn check_gif(app: &App, channel: &str, user: &crate::auth::User) -> Res<()> {
    let who = gif_setting(app, channel).await?;
    let allowed = match who.as_str() {
        "everyone" => true,
        "off" => false,
        _ if moderation::role_of(app, channel, user).await?.is_some() => true,
        "followers" => {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2)",
            )
            .bind(&user.id)
            .bind(channel)
            .fetch_one(&app.db)
            .await?
        }
        _ => crate::subs::active_tier(app, channel, &user.id)
            .await?
            .is_some(),
    };
    if allowed {
        return Ok(());
    }
    Err(Fail::denied(match who.as_str() {
        "off" => "GIFs are off in this chat.",
        "followers" => "GIFs here are for followers.",
        _ => "GIFs here are for subscribers.",
    }))
}

/// Persists one message to a channel's own chat and fans it out.
async fn send(
    app: &App,
    jar: &CookieJar,
    channel: &str,
    input: Send,
    squad: Option<&str>,
) -> Res<Value> {
    let user = profiles::signed_in(app, jar).await?;
    send_from(app, user, Some(channel), input, None, squad).await
}

/// Persists one message and fans it out. Acknowledged only after the insert commits. `origin` is
/// a MAGNet Hype lane: with a channel, a Hype message merged into that channel's chat under all of
/// its rules; without one, a message in the lane's own room (docs/MAGNET.md "Hype chat").
pub(crate) async fn send_from(
    app: &App,
    user: crate::auth::User,
    channel: Option<&str>,
    input: Send,
    origin: Option<&str>,
    squad: Option<&str>,
) -> Res<Value> {
    // Callers resolve the person on every send, so a revoked session or new restriction takes
    // effect at once.
    let members = if let Some(id) = squad {
        let members = crate::squads::chat_context(app, id, Some(&user.id)).await?;
        if members.first().map(String::as_str) != channel {
            return Err(Fail::conflict("This co-stream has ended."));
        }
        members
    } else {
        channel.into_iter().map(str::to_owned).collect()
    };
    if uuid::Uuid::parse_str(&input.id).is_err() {
        return Err(Fail::bad("Invalid message ID."));
    }
    let gif = match &input.gif {
        Some(_) if klipy_key().is_none() => {
            return Err(Fail::denied("GIFs aren't available right now."));
        }
        Some(_) if channel.is_none() || origin.is_some() => {
            return Err(Fail::field("gif", "GIFs work in a channel's own chat."));
        }
        Some(g) => Some((
            g.checked()
                .ok_or_else(|| Fail::field("gif", "Pick a GIF from the GIF search."))?,
            g.body(),
        )),
        None => None,
    };
    let body = match &gif {
        Some((_, alt)) => alt.as_str(),
        None => input.body.trim(),
    };
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
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1 AND m.author_id=$2 AND m.channel_id IS NOT DISTINCT FROM $3 AND m.squad_id IS NOT DISTINCT FROM $4 AND m.origin IS NOT DISTINCT FROM $5 AND {VISIBLE}", select())))
            .bind(&input.id)
            .bind(&user.id)
            .bind(channel)
            .bind(squad)
            .bind(origin)
            .fetch_optional(&app.db)
            .await?;
    if let Some(row) = existing {
        return Ok(visible_message(
            row.hydrated(app).await?,
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
    // Before slow mode and rate limits, so a refused GIF costs the sender nothing.
    if gif.is_some() {
        for member in &members {
            check_gif(app, member, &user).await?;
        }
    }
    let role = match channel {
        Some(channel) => {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1))")
                .bind(channel).bind(&user.id).fetch_one(&app.db).await?;
            if blocked {
                return Err(Fail::denied("You can't chat in this channel."));
            }
            crate::moderation::check_send(app, channel, &user, body).await?;
            moderation::role_of(app, channel, &user)
                .await?
                .map(|r| r.name())
        }
        None if input.reply_to.is_some() => {
            return Err(Fail::field(
                "reply_to",
                "Replies work inside a channel's chat.",
            ));
        }
        None => None,
    };
    if input.highlight && (channel.is_none() || origin.is_some() || squad.is_some()) {
        return Err(Fail::field(
            "highlight",
            "Highlights work in a channel's own chat.",
        ));
    }
    // Who a tribute pays: the channel, or in a merged co-stream every member who can earn,
    // split equally (docs/SUPPORT.md "Co-streams").
    let mut earners: Vec<String> = Vec::new();
    let skill = match (&input.skill, channel) {
        (Some(_), _) if input.tribute.is_some() => {
            return Err(Fail::field("skill", "A Skill is its own payment."));
        }
        (Some(id), Some(channel)) if origin.is_none() && squad.is_none() => {
            Some(crate::skills::check(app, channel, id).await?)
        }
        (Some(_), _) => {
            return Err(Fail::field("skill", "Skills play in a channel's own chat."));
        }
        (None, _) => None,
    };
    let tribute = input.tribute.or(skill.map(|s| s.valor));
    if let Some(valor) = tribute {
        if channel.is_none() || origin.is_some() {
            return Err(Fail::field("tribute", "Tributes go to a channel's chat."));
        }
        if !(10..=1_000_000).contains(&valor) {
            return Err(Fail::field("tribute", "A tribute is at least 10 Valor."));
        }
        if members.contains(&user.id) {
            return Err(Fail::field("tribute", "You can't pay tribute to yourself."));
        }
        let mut db = app.db.acquire().await?;
        for member in &members {
            if crate::support::can_earn(&mut db, member).await? {
                earners.push(member.clone());
            }
        }
        if earners.is_empty() {
            return Err(Fail::field(
                "tribute",
                "This channel can't receive tributes yet.",
            ));
        }
    }
    // Flood protection for small channels: Hype senders get one message every 3 seconds.
    if let Some(lane) = origin {
        let room = channel.unwrap_or(lane);
        sec::reserve(app, vec![format!("hype-slow:{room}:{}", user.id)], 1, 3).await?;
    }
    sec::reserve(app, vec![format!("chat-second:{}", user.id)], 2, 1).await?;
    sec::reserve(app, vec![format!("chat-ten:{}", user.id)], 20, 10).await?;
    let hidden = hidden(app, Some(&user.id)).await?;
    let role = if squad.is_some() {
        for member in members.iter().skip(1) {
            crate::moderation::check_send(app, member, &user, body).await?;
        }
        crate::squads::chat_role(app, &members, &user)
            .await?
            .map(|r| r.name())
    } else {
        role
    };
    let mut tx = app.db.begin().await?;
    if let Some(id) = squad {
        crate::squads::lock_chat(&mut tx, id, &members).await?;
    }
    if let (Some(reply), Some(channel)) = (&input.reply_to, channel) {
        // Lock against deletion while accepting the reply. Later reads always join the current body.
        let author: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT m.author_id FROM chat_messages m WHERE m.id=$1 AND m.channel_id=$2 AND m.squad_id IS NOT DISTINCT FROM $3 AND {VISIBLE} FOR SHARE")))
            .bind(reply).bind(channel).bind(squad).fetch_optional(&mut *tx).await?;
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
    let inserted = sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body,reply_to,mention_ids,role,origin,squad_id,tribute,highlighted,skill,gif) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) ON CONFLICT(id) DO NOTHING")
        .bind(&input.id).bind(channel).bind(&user.id).bind(body).bind(&input.reply_to).bind(mentions).bind(role).bind(origin).bind(squad).bind(tribute.map(|v| v as i32)).bind(input.highlight).bind(skill.map(|s| s.id)).bind(gif.as_ref().map(|(g, _)| g.clone())).execute(&mut *tx).await?.rows_affected();
    if inserted == 0 {
        return Err(Fail::conflict("That message ID is already in use."));
    }
    if let (Some(valor), Some(channel)) = (tribute, channel) {
        // The message and the Valor move commit together; the lock serializes the buyer's spends.
        let wallet = format!("valor:{}", user.id);
        crate::ledger::lock(&mut tx, &wallet).await?;
        let balance = crate::ledger::balance(&mut tx, &wallet, "valor").await?;
        if balance < 0 {
            return Err(Fail::field(
                "tribute",
                "Your Valor is locked until a payment problem is settled.",
            ));
        }
        if balance < valor {
            return Err(Fail::field("tribute", "You don't have enough Valor."));
        }
        let earned = valor * crate::ledger::EARN_PER_VALOR;
        let shares: Vec<(String, i64)> = crate::support::split_equally(&earners, earned)
            .into_iter()
            .map(|(m, s)| (format!("usd:earnings:{m}"), s))
            .collect();
        let mut entries = vec![
            (wallet.as_str(), "valor", -valor),
            ("valor:spent", "valor", valor),
            ("usd:platform", "usd", -earned),
        ];
        entries.extend(shares.iter().map(|(a, s)| (a.as_str(), "usd", *s)));
        crate::ledger::post(
            &mut tx,
            "tribute",
            &format!("tribute:{}", input.id),
            json!({"message": input.id, "channel": channel, "earners": earners, "from": user.id, "valor": valor, "skill": skill.map(|s| s.id)}),
            &entries,
        )
        .await?;
        crate::events::emit(
            &mut tx,
            channel,
            "tributes",
            json!({"user": user.username, "valor": valor, "message": body}),
        )
        .await?;
        if let Some(skill) = skill {
            crate::events::emit(
                &mut tx,
                channel,
                "skills",
                json!({"user": user.username, "skill": skill.id, "valor": valor}),
            )
            .await?;
        }
    }
    // A moderator's counter command (`!deaths`) changed a counter; viewers are told after commit.
    let mut counters = false;
    let mut rallied = false;
    if let Some(channel) = channel
        && squad.is_none()
    {
        if input.highlight {
            crate::engagement::highlight(&mut tx, channel, &user.id, &input.id).await?;
        }
        crate::engagement::chatted(&mut tx, &app.config.engagement, channel, &user.id).await?;
        crate::factions::chat(app, &mut tx, channel, &user.id, &input.id).await?;
        crate::plays::chat_vote(&mut tx, channel, &user, body).await?;
        if role.is_some() && origin.is_none() {
            let mut command = body.splitn(2, char::is_whitespace);
            if command.next() == Some("!marker") {
                crate::videos::marker(
                    &mut tx,
                    channel,
                    command.next().unwrap_or(""),
                    Some(&input.id),
                )
                .await?;
            }
        }
        counters = crate::crowd::chat_command(app, &mut tx, channel, &user, body).await?;
        // Surge counts each real viewer once a minute; "!rally" also rallies for their faction.
        crate::surge::participated(&mut tx, channel, &user.id).await?;
        rallied = body.eq_ignore_ascii_case("!rally")
            && crate::surge::rally_in(&mut tx, channel, &user.id).await?;
    }
    tx.commit().await?;
    if counters && let Some(channel) = channel {
        crate::crowd::publish_counters(app, channel).await?;
    }
    if let Some(channel) = channel.filter(|_| squad.is_none()) {
        if rallied {
            crate::surge::publish_rally(app, channel).await?;
        }
        if let Some(skill) = skill {
            crate::skills::played(app, channel, &user, skill).await?;
        }
        if origin.is_none() {
            crate::surge::combo(app, channel, &user.id, body).await?;
        }
    }
    // select() contains only fixed SQL and a literal chip alias; message values are bound.
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1", select())))
        .bind(&input.id)
        .fetch_one(&app.db)
        .await?;
    let (seq, author) = (row.seq, row.author_id.clone());
    let message = row.hydrated(app).await?;
    let event = json!({"type":"message","message":message});
    if let Some(channel) = squad.or(channel) {
        app.chat.publish(channel, Some(&author), seq, event.clone());
    }
    if let Some(lane) = origin {
        app.chat
            .publish(&format!("magnet:{lane}"), Some(&author), 0, event);
    }
    // Custom !commands get the channel bot's reply after the message itself (docs/COMMUNITY.md).
    if let Some(channel) = channel.filter(|_| squad.is_none() && origin.is_none()) {
        crate::commands::respond(app, channel, &user, body).await;
    }
    Ok(visible_message(message, &hidden))
}

pub async fn read(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    mature_gate(&app, &channel, viewer.as_ref().map(|v| v.id.as_str())).await?;
    let hidden = hidden(&app, viewer.as_ref().map(|v| v.id.as_str())).await?;
    let fade: i32 = sqlx::query_scalar(
        "SELECT coalesce((SELECT overlay_fade_seconds FROM chat_settings WHERE channel_id=$1),30)",
    )
    .bind(&channel)
    .fetch_one(&app.db)
    .await?;
    let signatures = allow_signatures(&app, &channel).await?;
    Ok(Json(
        json!({"overlay_fade_seconds": fade, "allow_signatures": signatures, "gifs": gifs(&app, &channel).await?, "messages": history(&app, &channel, &hidden, None).await?, "pinned":pinned(&app, &channel, &hidden).await?, "emotes":crate::emotes::catalog(&app, &channel).await?, "followers_only_until":crate::moderation::followers_only(&app, &channel).await?, "subs_only":crate::moderation::subs_only(&app, &channel).await?}),
    ))
}
pub async fn post(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Send>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    mature_gate(&app, &channel, viewer.as_ref().map(|v| v.id.as_str())).await?;
    Ok(Json(
        json!({"message": send(&app, &jar, &channel, input, None).await?}),
    ))
}
/// Whether other channels' signature emotes render in this chat (docs/CHANNEL_ADDITIONS.md).
async fn allow_signatures(app: &App, channel: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT allow_signatures FROM chat_settings WHERE channel_id=$1),true)",
    )
    .bind(channel)
    .fetch_one(&app.db)
    .await?)
}
/// Chat is closed to under-18 accounts in a channel labeled mature.
async fn mature_gate(app: &App, channel: &str, viewer: Option<&str>) -> Res<()> {
    if crate::streams::mature_blocked(&mut *app.db.acquire().await?, channel, viewer).await? {
        return Err(Fail::denied(
            "This channel is labeled mature, so its chat isn't available on your account.",
        ));
    }
    Ok(())
}

pub async fn squad_read(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let viewer = profiles::viewer(&app, &jar).await?;
    let members =
        crate::squads::chat_context(&app, &id, viewer.as_ref().map(|u| u.id.as_str())).await?;
    let channel = members.first().ok_or_else(Fail::missing)?;
    let hidden = hidden(&app, viewer.as_ref().map(|u| u.id.as_str())).await?;
    Ok(Json(
        json!({"messages":history(&app,channel,&hidden,Some(&id)).await?,"pinned":null,"emotes":crate::emotes::catalog(&app,channel).await?}),
    ))
}
pub async fn squad_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Send>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let members = crate::squads::chat_context(&app, &id, Some(&user.id)).await?;
    let channel = members.first().ok_or_else(Fail::missing)?;
    Ok(Json(
        json!({"message":send(&app,&jar,channel,input,Some(&id)).await?}),
    ))
}
/// Called only after the squad module authorizes a current shared-chat moderator.
pub async fn remove_shared(
    app: &App,
    squad: &str,
    id: &str,
    user: &crate::auth::User,
    members: &[String],
    reason: &str,
) -> Res<()> {
    let reason = moderation::reason(reason)?;
    let author: String =
        sqlx::query_scalar("SELECT author_id FROM chat_messages WHERE id=$1 AND squad_id=$2")
            .bind(id)
            .bind(squad)
            .fetch_optional(&app.db)
            .await?
            .ok_or_else(Fail::missing)?;
    if author != user.id {
        for channel in members {
            if moderation::protected(app, channel, &user.id, &author).await? {
                return Err(Fail::denied("You can't delete this message."));
            }
        }
    }
    let mut tx = app.db.begin().await?;
    crate::squads::lock_chat(&mut tx, squad, members).await?;
    let changed=sqlx::query("UPDATE chat_messages SET deleted_at=now() WHERE id=$1 AND squad_id=$2 AND deleted_at IS NULL").bind(id).bind(squad).execute(&mut *tx).await?.rows_affected();
    if changed > 0 {
        crate::safety::audit(
            &mut tx,
            Some(&user.id),
            "delete_message",
            "squad",
            squad,
            &[],
            &reason,
            json!({"message":id}),
            false,
        )
        .await?;
    }
    tx.commit().await?;
    app.chat
        .publish(squad, None, 0, json!({"type":"delete","id":id}));
    Ok(())
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
            "SELECT m.author_id FROM chat_messages m WHERE m.id=$1 AND m.channel_id=$2 AND m.squad_id IS NULL AND {VISIBLE} FOR UPDATE"
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
    #[serde(default)]
    channel: String,
    squad: Option<String>,
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
    let channel = if let Some(id) = &join.squad {
        let viewer = profiles::viewer(&app, &jar).await?;
        crate::squads::chat_context(&app, id, viewer.as_ref().map(|u| u.id.as_str()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(Fail::missing)?
    } else {
        let channel = channel(&app, &join.channel).await?;
        let viewer = profiles::viewer(&app, &jar).await?;
        mature_gate(&app, &channel, viewer.as_ref().map(|v| v.id.as_str())).await?;
        channel
    };
    if app.chat.sockets.load(Ordering::Relaxed) >= MAX_SOCKETS {
        return Err(Fail::unavailable("Chat is busy. Please try again shortly."));
    }
    Ok(upgrade
        .max_message_size(FRAME)
        .max_frame_size(FRAME)
        .on_upgrade(move |ws| session(app, jar, channel, join.squad, ws)))
}

async fn session(
    app: App,
    jar: CookieJar,
    channel: String,
    squad: Option<String>,
    mut ws: WebSocket,
) {
    let room = squad.as_deref().unwrap_or(&channel);
    app.chat.sockets.fetch_add(1, Ordering::Relaxed);
    let _slot = SocketSlot(app.chat.sockets.clone());
    // Subscribe before reading history so nothing posted in between is missed.
    let mut events = app.chat.tx.subscribe();
    let viewer = profiles::viewer(&app, &jar).await.ok().flatten();
    let Ok(hidden) = hidden(&app, viewer.as_ref().map(|v| v.id.as_str())).await else {
        return;
    };
    let Ok(snapshot) = history(&app, &channel, &hidden, squad.as_deref()).await else {
        return;
    };
    let cursor = snapshot.last().and_then(|m| m["seq"].as_i64()).unwrap_or(0);
    let Ok(mut pin) = pinned(&app, &channel, &hidden).await else {
        return;
    };
    let Ok(mut emotes) = crate::emotes::catalog(&app, &channel).await else {
        return;
    };
    let Ok(followers_only) = crate::moderation::followers_only(&app, &channel).await else {
        return;
    };
    let Ok(subs_only) = crate::moderation::subs_only(&app, &channel).await else {
        return;
    };
    if squad.is_some() {
        pin = Value::Null;
    }
    let Ok(signatures) = allow_signatures(&app, &channel).await else {
        return;
    };
    let Ok(gifs) = gifs(&app, &channel).await else {
        return;
    };
    let first = json!({"type":"snapshot","messages":snapshot,"pinned":pin,"emotes":emotes,"followers_only_until":followers_only,"subs_only":subs_only,"allow_signatures":signatures,"gifs":gifs}).to_string();
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
                if let Some(id) = &squad {
                    let viewer = match profiles::viewer(&app,&jar).await { Ok(v)=>v, Err(_)=>return };
                    if crate::squads::chat_context(&app,id,viewer.as_ref().map(|u|u.id.as_str())).await.is_err() { return; }
                }
                let Ok(current) = crate::emotes::catalog(&app, &channel).await else { return; };
                if current != emotes {
                    emotes = current;
                    if ws.send(Message::Text(json!({"type":"emotes","emotes":emotes}).to_string().into())).await.is_err() { return; }
                }
            }
            event = events.recv() => match event {
                Ok(e) if e.channel == room
                    && (e.seq == 0 || e.seq > cursor) => {
                    if let Some(id) = &squad
                        && crate::squads::chat_context(&app,id,viewer.as_ref().map(|u|u.id.as_str())).await.is_err() { return; }
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
                        let Ok(message) = fresh(&app, &payload, &hidden).await else { return; };
                        let Some(message) = message else { continue; };
                        payload["message"] = message;
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
                            match send(&app, &jar, &channel, input, squad.as_deref()).await {
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

/// A queued chat message re-read as the recipient sees it: deletion or a new block must not leak
/// a stale quote. None when it's gone.
pub(crate) async fn fresh(
    app: &App,
    payload: &Value,
    hidden: &HashSet<String>,
) -> Res<Option<Value>> {
    // ponytail: one indexed read per recipient/event; batch fanout if measured chat load requires it.
    let row = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!(
        "{} WHERE m.id=$1 AND {VISIBLE}",
        select()
    )))
    .bind(payload["message"]["id"].as_str().unwrap_or(""))
    .fetch_optional(&app.db)
    .await?;
    Ok(match row {
        Some(row) => Some(visible_message(row.hydrated(app).await?, hidden)),
        None => None,
    })
}
/// A message sent by an app as the person (events socket, `chat:write`): chat only. Money is never
/// reachable through an app, so tributes, Skills and highlights are refused, and a banned account
/// can't post.
pub(crate) async fn send_as_app(
    app: &App,
    user: &str,
    channel: &str,
    params: &Value,
) -> Res<Value> {
    let input: Send = serde_json::from_value(params.clone())
        .map_err(|_| Fail::bad("Send {\"channel\",\"id\",\"body\"}."))?;
    if input.tribute.is_some() || input.skill.is_some() || input.highlight {
        return Err(Fail::denied("Apps can't spend Valor."));
    }
    let person = crate::devapps::person(app, user)
        .await?
        .ok_or_else(|| Fail::denied("Your account can't chat right now."))?;
    if crate::streams::mature_blocked(&mut *app.db.acquire().await?, channel, Some(user)).await? {
        return Err(Fail::denied(
            "This channel is labeled mature, so its chat isn't available on your account.",
        ));
    }
    send_from(app, person, Some(channel), input, None, None).await
}

/// What a Hype lane's chat is merged with right now: the featured channel's chat, or a merged
/// co-stream's shared chat (stored under its host), and since when.
struct Merge {
    channel: String,
    squad: Option<String>,
    /// Every streamer whose chat rules and bans apply.
    owners: Vec<String>,
    since: DateTime<Utc>,
}
impl Merge {
    /// The broadcast room its messages arrive on.
    fn room(&self) -> &str {
        self.squad.as_deref().unwrap_or(&self.channel)
    }
}
/// Merging stops when MAGNet moves on, when the lane stops, or when a streamer whose chat it would
/// join turns chat merging off.
async fn hype_merge(app: &App, lane: &str) -> Res<Option<Merge>> {
    let featured: Option<(String, DateTime<Utc>)> = sqlx::query_as("SELECT b.owner_id,l.current_since FROM magnet_lanes l JOIN broadcasts b ON b.id=l.current_broadcast WHERE l.id=$1 AND l.enabled AND l.current_since IS NOT NULL")
        .bind(lane)
        .fetch_optional(&app.db)
        .await?;
    let Some((owner, since)) = featured else {
        return Ok(None);
    };
    let squad = crate::squads::active_of(&mut *app.db.acquire().await?, &owner)
        .await?
        .filter(|q| q.mode == "MERGED");
    let merge = match squad {
        Some(q) => Merge {
            channel: q.members[0].0.clone(),
            squad: Some(q.id),
            owners: q.members.into_iter().map(|m| m.0).collect(),
            since,
        },
        None => Merge {
            channel: owner.clone(),
            squad: None,
            owners: vec![owner],
            since,
        },
    };
    let off: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM magnet_settings WHERE user_id=ANY($1) AND NOT chat_merge)",
    )
    .bind(&merge.owners)
    .fetch_one(&app.db)
    .await?;
    if off {
        return Ok(None);
    }
    // Remember the merge, so this window's messages stay in the room's history after the switch.
    sqlx::query(
        "UPDATE magnet_features SET merged=true WHERE lane=$1 AND ended_at IS NULL AND NOT merged",
    )
    .bind(lane)
    .execute(&app.db)
    .await?;
    Ok(Some(merge))
}
async fn hype_lane(app: &App, lane: &str) -> Res<()> {
    let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM magnet_lanes WHERE id=$1)")
        .bind(lane)
        .fetch_one(&app.db)
        .await?;
    if found { Ok(()) } else { Err(Fail::missing()) }
}
/// Banned from, timed out in, or blocked by (or blocking) a featured channel, or restricted in the
/// merged co-stream's shared chat: read-only while merged.
async fn hype_holding(app: &App, merge: &Merge, viewer: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=ANY($1) AND user_id=$2 AND (kind='ban' OR until>now())) OR EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$2 AND blocked_id=ANY($1)) OR (blocker_id=ANY($1) AND blocked_id=$2)) OR EXISTS(SELECT 1 FROM squad_restrictions WHERE squad_id=$3 AND user_id=$2 AND (kind='ban' OR until>now()))")
        .bind(&merge.owners).bind(viewer).bind(&merge.squad).fetch_one(&app.db).await?)
}
/// The room's latest 100: its own messages, plus the featured channel's chat since the feature
/// began while merged. Messages from either side stay in both histories after a switch.
async fn hype_snapshot(
    app: &App,
    lane: &str,
    viewer: Option<&str>,
) -> Res<(Value, Option<String>)> {
    let merge = hype_merge(app, lane).await?;
    let hidden = hidden(app, viewer).await?;
    // select() contains only fixed SQL and a literal chip alias; values are bound.
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE ((m.channel_id IS NULL AND m.origin=$1 AND m.squad_id IS NULL) OR (m.channel_id=$2 AND m.squad_id IS NOT DISTINCT FROM $5 AND m.created_at>=$3) OR EXISTS(SELECT 1 FROM magnet_features f WHERE f.lane=$1 AND f.merged AND f.owner_id=m.channel_id AND m.created_at>=f.started_at AND (f.ended_at IS NULL OR m.created_at<f.ended_at))) AND {VISIBLE} ORDER BY m.seq DESC LIMIT $4",
        select()
    )))
    .bind(lane)
    .bind(merge.as_ref().map(|m| m.channel.as_str()))
    .bind(merge.as_ref().map(|m| m.since))
    .bind(HISTORY)
    .bind(merge.as_ref().and_then(|m| m.squad.as_deref()))
    .fetch_all(&app.db)
    .await?;
    let mut messages: Vec<Value> = rows
        .into_iter()
        .rev()
        .filter(|r| !hidden.contains(&r.author_id))
        .map(|r| visible_message(r.json(app), &hidden))
        .collect();
    crate::guilds::hydrate_badges(app, &mut messages).await?;
    let (merged_with, holding) = match &merge {
        Some(m) => {
            let mut db = app.db.acquire().await?;
            let chip = profiles::channel_user_by_id(&mut db, &m.channel)
                .await?
                .map(|u| profiles::chip(app, &u));
            let holding = match viewer {
                Some(v) => hype_holding(app, m, v).await?,
                None => false,
            };
            (chip, holding)
        }
        None => (None, false),
    };
    Ok((
        json!({"type":"snapshot","messages":messages,"merged_with":merged_with,"co_stream":merge.as_ref().is_some_and(|m| m.squad.is_some()),"holding":holding,"can_send":viewer.is_some() && !holding}),
        merge.map(|m| m.room().to_string()),
    ))
}
/// GET /api/magnet/{lane}/chat
async fn hype_read(
    State(app): State<App>,
    jar: CookieJar,
    Path(lane): Path<String>,
) -> Res<Json<Value>> {
    hype_lane(&app, &lane).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    Ok(Json(
        hype_snapshot(&app, &lane, viewer.as_ref().map(|v| v.id.as_str()))
            .await?
            .0,
    ))
}
/// POST /api/magnet/{lane}/chat: while merged the message goes into the featured channel's chat
/// under all of its rules (with the MAGNet mark); otherwise it stays in the lane's own room.
async fn hype_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(lane): Path<String>,
    Json(input): Json<Send>,
) -> Res<Json<Value>> {
    hype_lane(&app, &lane).await?;
    let user = profiles::signed_in(&app, &jar).await?;
    let message = match hype_merge(&app, &lane).await? {
        Some(m) => {
            if hype_holding(&app, &m, &user.id).await? {
                return Err(Fail::denied("Chat resumes when MAGNet moves on."));
            }
            send_from(
                &app,
                user,
                Some(&m.channel),
                input,
                Some(&lane),
                m.squad.as_deref(),
            )
            .await?
        }
        None => send_from(&app, user, None, input, Some(&lane), None).await?,
    };
    Ok(Json(json!({"message": message})))
}
/// The Hype room over the same-origin WebSocket. Sending goes over HTTPS (same checks).
async fn hype_socket(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(lane): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.config.origin.as_str()) {
        return Err(Fail::denied("Invalid request origin."));
    }
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("chat-connect:{ip}")], 30, 60).await?;
    hype_lane(&app, &lane).await?;
    if app.chat.sockets.load(Ordering::Relaxed) >= MAX_SOCKETS {
        return Err(Fail::unavailable("Chat is busy. Please try again shortly."));
    }
    Ok(upgrade
        .max_message_size(FRAME)
        .max_frame_size(FRAME)
        .on_upgrade(move |ws| hype_session(app, jar, lane, ws)))
}
async fn hype_session(app: App, jar: CookieJar, lane: String, mut ws: WebSocket) {
    app.chat.sockets.fetch_add(1, Ordering::Relaxed);
    let _slot = SocketSlot(app.chat.sockets.clone());
    let mut events = app.chat.tx.subscribe();
    let viewer = profiles::viewer(&app, &jar).await.ok().flatten();
    let viewer = viewer.as_ref().map(|v| v.id.clone());
    let room = format!("magnet:{lane}");
    let Ok((first, mut merged)) = hype_snapshot(&app, &lane, viewer.as_deref()).await else {
        return;
    };
    if ws
        .send(Message::Text(first.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(e) if e.channel == room || merged.as_deref() == Some(e.channel.as_str()) => {
                    // A switch: merge or detach, and resend the room.
                    if e.payload["type"] == "magnet" {
                        let Ok((next, owner)) = hype_snapshot(&app, &lane, viewer.as_deref()).await else { return; };
                        merged = owner;
                        if ws.send(Message::Text(next.to_string().into())).await.is_err() { return; }
                        continue;
                    }
                    if !matches!(e.payload["type"].as_str(), Some("message" | "delete")) { continue; }
                    // A merged Hype message arrives on both; forward it once.
                    if e.channel != room && e.payload["message"]["origin"] == lane.as_str() { continue; }
                    let Ok(hidden) = crate::chat::hidden(&app, viewer.as_deref()).await else { return; };
                    if e.author.as_ref().is_some_and(|a| hidden.contains(a)) { continue; }
                    let mut payload = e.payload.clone();
                    if payload["type"] == "message" {
                        payload["message"] = visible_message(payload["message"].take(), &hidden);
                    }
                    if ws.send(Message::Text(payload.to_string().into())).await.is_err() { return; }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let _ = ws.send(Message::Close(Some(CloseFrame { code: 4000, reason: "resync".into() }))).await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = ws.recv() => match incoming {
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
        .route("/api/magnet/{lane}/chat", get(hype_read).post(hype_post))
        .route("/api/magnet/{lane}/ws", get(hype_socket))
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
