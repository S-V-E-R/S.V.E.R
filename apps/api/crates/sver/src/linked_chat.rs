//! Linked chat (docs/LINKED_CHAT.md): while a streamer is live, chat from their linked accounts on
//! other platforms appears in their S.V.E.R chat with the platform's badge, and the streamer can
//! reply out. Twitch is connected through EventSub webhooks (signed with a secret derived from the
//! site key), so no connection is held open and nothing is lost across API restarts. Outside
//! messages have their own table: nothing that counts S.V.E.R chat can see them.
use crate::{
    App, auth,
    moderation::{self, Role},
    oauth::Provider,
    profiles::{Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use axum_extra::extract::cookie::CookieJar;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgConnection;
use std::{
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

const PLATFORMS: [&str; 3] = ["twitch", "youtube", "kick"];
/// Messages, single deletes, and the clears Twitch moderation sends: a ban or timeout clears that
/// chatter's messages, and /clear clears them all. All four need only the linking scopes.
const EVENTS: [&str; 4] = [
    "channel.chat.message",
    "channel.chat.message_delete",
    "channel.chat.clear_user_messages",
    "channel.chat.clear",
];

mod kick;
mod youtube;

/// The permissions asked for when linking a platform for chat; None where it isn't available yet.
/// Twitch: read the streamer's chat and post as them (`user:bot`/`channel:bot` let webhooks carry chat).
/// YouTube: its one scope that both reads live chat and posts to it.
/// Kick: who they are, posting to chat, and chat events.
pub fn scopes(platform: &str) -> Option<&'static str> {
    match platform {
        "twitch" => Some("user:read:chat user:write:chat user:bot channel:bot"),
        "youtube" => Some(youtube::SCOPE),
        "kick" => Some(kick::SCOPES),
        _ => None,
    }
}
fn label(platform: &str) -> &'static str {
    match platform {
        "youtube" => "YouTube",
        "kick" => "Kick",
        _ => "Twitch",
    }
}
fn twitch(app: &App) -> Option<&Provider> {
    app.config.providers.iter().find(|p| p.name == "twitch")
}
fn api_base() -> String {
    std::env::var("TWITCH_API_BASE").unwrap_or_else(|_| "https://api.twitch.tv".into())
}
fn id_base() -> String {
    std::env::var("TWITCH_ID_BASE").unwrap_or_else(|_| "https://id.twitch.tv".into())
}
fn down() -> Fail {
    Fail::unavailable("Twitch couldn't be reached. Try again shortly.")
}
/// The EventSub signing secret: derived from the site key, so it never needs storing.
pub fn webhook_secret(app: &App) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(&app.config.key).expect("any key length");
    mac.update(b"twitch-eventsub");
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn callback(app: &App) -> String {
    format!("{}/api/integrations/twitch/eventsub", app.config.origin)
}

/// Saves a freshly linked account (from the OAuth callback); tokens are sealed.
pub async fn save(
    app: &App,
    db: &mut PgConnection,
    owner: &str,
    platform: &str,
    subject: &str,
    handle: &str,
    tokens: &Value,
) -> crate::Result<()> {
    let access = tokens["access_token"]
        .as_str()
        .ok_or_else(|| crate::Error::bad("The platform didn't return access."))?;
    let refresh = tokens["refresh_token"].as_str();
    // Google lets people untick a permission on its consent screen; chat needs the YouTube one.
    // Google always lists what was granted; a refresh that doesn't say keeps what was linked.
    if platform == "youtube"
        && tokens["scope"]
            .as_str()
            .is_some_and(|s| !s.split(' ').any(|x| x == youtube::SCOPE))
    {
        return Err(crate::Error::bad(
            "Allow S.V.E.R to manage your YouTube account, so it can read and post in your live chat.",
        ));
    }
    let taken: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM linked_chat_accounts WHERE platform=$1 AND subject=$2 AND owner_id<>$3)")
        .bind(platform).bind(subject).bind(owner).fetch_one(&mut *db).await?;
    if taken {
        return Err(crate::Error::bad(
            "That account is linked to another S.V.E.R channel.",
        ));
    }
    sqlx::query("INSERT INTO linked_chat_accounts(owner_id,platform,subject,handle,access_sealed,refresh_sealed,expires_at) VALUES($1,$2,$3,$4,$5,$6,now()+make_interval(secs=>$7))
        ON CONFLICT(owner_id,platform) DO UPDATE SET subject=EXCLUDED.subject,handle=EXCLUDED.handle,access_sealed=EXCLUDED.access_sealed,refresh_sealed=coalesce(EXCLUDED.refresh_sealed,linked_chat_accounts.refresh_sealed),expires_at=EXCLUDED.expires_at,status=CASE WHEN linked_chat_accounts.status='revoked' THEN 'idle' ELSE linked_chat_accounts.status END,detail=NULL")
        .bind(owner).bind(platform).bind(subject).bind(handle)
        .bind(sec::seal(app, "linked-chat", access)?)
        .bind(refresh.map(|r| sec::seal(app, "linked-chat", r)).transpose()?)
        .bind(tokens["expires_in"].as_f64().unwrap_or(3600.0))
        .execute(db).await?;
    Ok(())
}

// ---- Outside messages ----

/// An outside message as chat JSON, for SELECTs and for the insert's RETURNING.
macro_rules! outside_json {
    () => {
        "jsonb_build_object('id',id,'seq',seq,'body',body,'created_at',created_at,'role',NULL,'mentions','[]'::jsonb,'reply',NULL,
    'author',jsonb_build_object('username',NULL,'display_name',sender_name,'avatar',NULL,'linked',false,'deleted',false),
    'outside',jsonb_build_object('platform',platform,'sender_id',sender_id,'name',sender_name,'login',sender_login,'role',sender_role,
        'url',CASE platform WHEN 'twitch' THEN 'https://www.twitch.tv/'||sender_login WHEN 'kick' THEN 'https://kick.com/'||sender_login WHEN 'youtube' THEN 'https://www.youtube.com/channel/'||sender_login ELSE NULL END))"
    };
}
const OUTSIDE: &str = concat!("SELECT ", outside_json!(), " FROM outside_chat_messages");

/// The latest outside messages for a channel's chat history (merged by `seq`).
pub async fn recent(app: &App, channel: &str, limit: i64) -> Res<Vec<Value>> {
    let mut rows: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "{OUTSIDE} WHERE channel_id=$1 AND hidden_at IS NULL AND expires_at>now() ORDER BY seq DESC LIMIT $2"
    )))
    .bind(channel)
    .bind(limit)
    .fetch_all(&app.db)
    .await?;
    rows.reverse();
    Ok(rows)
}
fn clean(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    text.trim().chars().take(500).collect()
}
/// One message from a platform: stored and shown unless the sender is muted on S.V.E.R or the
/// channel's banned words or link rule would refuse it. True when it was shown.
#[allow(clippy::too_many_arguments)]
pub async fn receive(
    app: &App,
    platform: &str,
    subject: &str,
    message_id: &str,
    sender_id: &str,
    login: &str,
    name: &str,
    role: Option<&str>,
    text: &str,
) -> Res<bool> {
    let body = clean(text);
    if body.is_empty() || message_id.is_empty() || sender_id.is_empty() {
        return Ok(false);
    }
    if crate::switches::off(
        &mut *app.db.acquire().await?,
        &format!("linked_chat_{platform}"),
    )
    .await?
    {
        return Ok(false);
    }
    // The channel, its link status, whether S.V.E.R muted the sender, and its content rules: one
    // round trip, then the insert, then a status write only when it wasn't connected yet.
    type Facts = (String, String, bool, Option<Vec<String>>, Option<bool>);
    let facts: Option<Facts> = sqlx::query_as("SELECT a.owner_id,a.status,
            EXISTS(SELECT 1 FROM outside_chat_mutes m WHERE m.channel_id=a.owner_id AND m.platform=$1 AND m.sender_id=$3 AND (m.broadcast_id IS NULL OR EXISTS(SELECT 1 FROM broadcasts b WHERE b.id=m.broadcast_id AND b.state<>'ENDED'))),
            s.banned_words,s.block_links
        FROM linked_chat_accounts a LEFT JOIN chat_settings s ON s.channel_id=a.owner_id
        WHERE a.platform=$1 AND a.subject=$2 AND a.enabled")
        .bind(platform).bind(subject).bind(sender_id).fetch_optional(&app.db).await?;
    let Some((channel, status, muted, words, links)) = facts else {
        return Ok(false);
    };
    if muted
        || moderation::check_content(
            &body,
            &words.unwrap_or_default(),
            links.unwrap_or(false),
            false,
        )
        .is_err()
    {
        return Ok(false);
    }
    let id = format!("{platform}:{message_id}");
    let login: String = clean(login).chars().take(50).collect();
    let name: String = clean(name).chars().take(50).collect();
    let name = if name.is_empty() { login.clone() } else { name };
    let inserted: Option<(i64, Value)> = sqlx::query_as(concat!("INSERT INTO outside_chat_messages(id,channel_id,platform,sender_id,sender_name,sender_login,sender_role,body) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING RETURNING seq,", outside_json!()))
        .bind(&id).bind(&channel).bind(platform).bind(sender_id).bind(&name).bind(&login).bind(role).bind(&body)
        .fetch_optional(&app.db).await?;
    let Some((seq, message)) = inserted else {
        return Ok(false);
    };
    app.chat.publish(
        &channel,
        None,
        seq,
        json!({"type":"message","message":message}),
    );
    if status != "connected" {
        sqlx::query("UPDATE linked_chat_accounts SET status='connected',detail=NULL,status_at=now() WHERE owner_id=$1 AND platform=$2 AND status<>'connected'")
            .bind(&channel).bind(platform).execute(&app.db).await?;
    }
    Ok(true)
}
/// A message from a S.V.E.R system sender, such as the channel bot (platform `bot`): stored and
/// shown like an outside message, so it keeps its place in history and counts toward nothing.
pub async fn post_system(
    app: &App,
    channel: &str,
    platform: &str,
    sender_id: &str,
    name: &str,
    text: &str,
) -> Res<()> {
    let body = clean(text);
    if body.is_empty() {
        return Ok(());
    }
    let id = format!("{platform}:{}", uuid::Uuid::new_v4());
    let (seq, message): (i64, Value) = sqlx::query_as(concat!("INSERT INTO outside_chat_messages(id,channel_id,platform,sender_id,sender_name,sender_login,body) VALUES($1,$2,$3,$4,$5,$4,$6) RETURNING seq,", outside_json!()))
        .bind(&id).bind(channel).bind(platform).bind(sender_id).bind(name).bind(&body)
        .fetch_one(&app.db).await?;
    app.chat.publish(
        channel,
        None,
        seq,
        json!({"type":"message","message":message}),
    );
    Ok(())
}
/// A report on an outside message (docs/LINKED_CHAT.md "Moderation"): its sender isn't a S.V.E.R
/// account, so the channel it showed in stands as the owner; the snapshot keeps the text past the
/// 7-day expiry. Bot lines are the channel's own and aren't reported here.
pub async fn report_target(
    db: &mut PgConnection,
    id: &str,
) -> Res<Option<(String, String, Value)>> {
    Ok(sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('body',m.body,'channel',c.username,'platform',m.platform,'sender',m.sender_name,'sender_id',m.sender_id,'sent_at',m.created_at)
        FROM outside_chat_messages m JOIN channel_users c ON c.id=m.channel_id
        WHERE m.id=$1 AND m.platform<>'bot' AND m.hidden_at IS NULL AND m.expires_at>now() AND c.eligible")
        .bind(id).fetch_optional(db).await?)
}
/// Hides a reported outside message (a staff removal): the channel it showed in, and whether it
/// was visible. The caller tells open chats after committing.
pub async fn remove(db: &mut PgConnection, id: &str) -> Res<(String, bool)> {
    let (channel, visible): (String, bool) = sqlx::query_as(
        "SELECT channel_id,hidden_at IS NULL FROM outside_chat_messages WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(Fail::missing)?;
    if visible {
        sqlx::query("UPDATE outside_chat_messages SET hidden_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *db)
            .await?;
    }
    Ok((channel, visible))
}
/// Which outside messages to hide.
enum Hide<'a> {
    /// One message, by its ID.
    Message(&'a str),
    /// Everything one sender said here: platform, sender ID.
    Sender(&'a str, &'a str),
    /// Everything from one platform (a chat clear there).
    Platform(&'a str),
}
/// Hides outside messages on S.V.E.R and tells connected viewers.
async fn hide(app: &App, channel: &str, which: Hide<'_>) -> Res<()> {
    const SET: &str = "UPDATE outside_chat_messages SET hidden_at=now() WHERE channel_id=$1 AND hidden_at IS NULL";
    let rows: Vec<String> = match which {
        Hide::Message(id) => {
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("{SET} AND id=$2 RETURNING id")))
                .bind(channel)
                .bind(id)
                .fetch_all(&app.db)
                .await?
        }
        Hide::Sender(platform, sender) => {
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "{SET} AND platform=$2 AND sender_id=$3 RETURNING id"
            )))
            .bind(channel)
            .bind(platform)
            .bind(sender)
            .fetch_all(&app.db)
            .await?
        }
        Hide::Platform(platform) => {
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "{SET} AND platform=$2 RETURNING id"
            )))
            .bind(channel)
            .bind(platform)
            .fetch_all(&app.db)
            .await?
        }
    };
    // Sequence 0, like every other delete: open sockets skip events at or before their snapshot,
    // and the hidden message may well be in it.
    for id in rows {
        app.chat
            .publish(channel, None, 0, json!({"type":"delete","id":id}));
    }
    Ok(())
}
/// Outside messages and the bot's lines are kept 7 days (docs/LINKED_CHAT.md), like S.V.E.R chat;
/// a mute for one broadcast ends with it. Runs in the 30-second jobs pass, 1,000 rows at a time.
pub async fn expire(app: &App) -> Res<()> {
    let removed: Vec<(String, String)> = sqlx::query_as("DELETE FROM outside_chat_messages WHERE id IN (SELECT id FROM outside_chat_messages WHERE expires_at<=now() ORDER BY expires_at LIMIT 1000) RETURNING channel_id,id")
        .fetch_all(&app.db)
        .await?;
    for (channel, id) in removed {
        app.chat
            .publish(&channel, None, 0, json!({"type":"delete","id":id}));
    }
    sqlx::query("DELETE FROM outside_chat_mutes m WHERE m.broadcast_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM broadcasts b WHERE b.id=m.broadcast_id AND b.state<>'ENDED')")
        .execute(&app.db)
        .await?;
    Ok(())
}

// ---- Twitch EventSub webhook ----

fn verified(app: &App, headers: &HeaderMap, body: &[u8]) -> bool {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let (Some(id), Some(at), Some(signature)) = (
        header("twitch-eventsub-message-id"),
        header("twitch-eventsub-message-timestamp"),
        header("twitch-eventsub-message-signature"),
    ) else {
        return false;
    };
    // Replays older than 10 minutes are refused, as Twitch recommends.
    let fresh = chrono::DateTime::parse_from_rfc3339(at)
        .is_ok_and(|t| (chrono::Utc::now() - t.to_utc()).num_minutes().abs() < 10);
    let Some(hex) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Some(expected) = (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|b| u8::from_str_radix(b, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
    else {
        return false;
    };
    let mut mac =
        Hmac::<Sha256>::new_from_slice(webhook_secret(app).as_bytes()).expect("any key length");
    mac.update(id.as_bytes());
    mac.update(at.as_bytes());
    mac.update(body);
    fresh && mac.verify_slice(&expected).is_ok()
}
/// POST /api/integrations/twitch/eventsub
async fn eventsub(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if !verified(&app, &headers, &body) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(event) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let kind = headers
        .get("twitch-eventsub-message-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let subject = event["subscription"]["condition"]["broadcaster_user_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    // Go-live subscriptions belong to the Discord bot's alerts.
    let go_live = event["subscription"]["type"]
        .as_str()
        .is_some_and(|t| t.starts_with("stream."));
    let result: Res<()> = match kind {
        "webhook_callback_verification" => {
            if !go_live {
                let _ = sqlx::query("UPDATE linked_chat_accounts SET status='connected',detail=NULL,status_at=now() WHERE platform='twitch' AND subject=$1")
                .bind(&subject).execute(&app.db).await;
            }
            return event["challenge"]
                .as_str()
                .unwrap_or_default()
                .to_string()
                .into_response();
        }
        _ if go_live => crate::discord::streamers::twitch_event(&app, kind, &event).await,
        "revocation" => revoked(&app, &subject, &event).await,
        "notification" => notification(&app, &subject, &event).await,
        _ => Ok(()),
    };
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        // A 5xx asks Twitch to retry; the message ID makes the retry harmless.
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
/// Twitch ended a subscription. When it gave up because our endpoint kept failing
/// (`notification_failures_exceeded`, our own outage), the access is still good: the supervisor
/// subscribes again on its next pass. Anything else (access removed, the Twitch account gone, a
/// retired event version) needs the streamer to link again.
async fn revoked(app: &App, subject: &str, event: &Value) -> Res<()> {
    let id = event["subscription"]["id"].as_str().unwrap_or_default();
    if event["subscription"]["status"] == "notification_failures_exceeded" {
        // Only a subscription we still hold; a late notice for one already replaced changes nothing.
        sqlx::query("UPDATE linked_chat_accounts SET status='reconnecting',detail='Twitch chat disconnected, reconnecting.',status_at=now()-interval '30 seconds',subscriptions='{}'
            WHERE platform='twitch' AND subject=$1 AND status<>'revoked' AND $2=ANY(subscriptions)")
            .bind(subject).bind(id).execute(&app.db).await?;
        return Ok(());
    }
    sqlx::query("UPDATE linked_chat_accounts SET status='revoked',detail='Twitch stopped sending chat. Link Twitch again to restore it.',status_at=now(),subscriptions=array_remove(subscriptions,$2) WHERE platform='twitch' AND subject=$1")
        .bind(subject).bind(id).execute(&app.db).await?;
    Ok(())
}
/// The S.V.E.R channel a Twitch broadcaster is linked to.
async fn owner_of(app: &App, subject: &str) -> Res<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT owner_id FROM linked_chat_accounts WHERE platform='twitch' AND subject=$1",
    )
    .bind(subject)
    .fetch_optional(&app.db)
    .await?)
}
async fn notification(app: &App, subject: &str, body: &Value) -> Res<()> {
    let e = &body["event"];
    match body["subscription"]["type"].as_str() {
        Some("channel.chat.message") => {
            // Shared chat sessions repeat other channels' messages; only this channel's count.
            if e["source_broadcaster_user_id"]
                .as_str()
                .is_some_and(|s| s != subject)
            {
                return Ok(());
            }
            let badges: Vec<&str> = e["badges"]
                .as_array()
                .map(|b| b.iter().filter_map(|x| x["set_id"].as_str()).collect())
                .unwrap_or_default();
            let role = ["broadcaster", "moderator", "subscriber"]
                .into_iter()
                .find(|r| badges.contains(r));
            receive(
                app,
                "twitch",
                subject,
                e["message_id"].as_str().unwrap_or_default(),
                e["chatter_user_id"].as_str().unwrap_or_default(),
                e["chatter_user_login"].as_str().unwrap_or_default(),
                e["chatter_user_name"].as_str().unwrap_or_default(),
                role,
                e["message"]["text"].as_str().unwrap_or_default(),
            )
            .await?;
        }
        Some("channel.chat.message_delete") => {
            if let (Some(channel), Some(id)) =
                (owner_of(app, subject).await?, e["message_id"].as_str())
            {
                hide(app, &channel, Hide::Message(&format!("twitch:{id}"))).await?;
            }
        }
        // A ban or timeout on Twitch clears that chatter's messages there, so here too.
        Some("channel.chat.clear_user_messages") => {
            if let (Some(channel), Some(target)) =
                (owner_of(app, subject).await?, e["target_user_id"].as_str())
            {
                hide(app, &channel, Hide::Sender("twitch", target)).await?;
            }
        }
        Some("channel.chat.clear") => {
            if let Some(channel) = owner_of(app, subject).await? {
                hide(app, &channel, Hide::Platform("twitch")).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

// ---- Connecting and disconnecting (the supervisor) ----

type Cached = Option<(String, Instant)>;
static APP_TOKEN: LazyLock<Mutex<Cached>> = LazyLock::new(Mutex::default);
fn cached() -> std::sync::MutexGuard<'static, Cached> {
    APP_TOKEN.lock().unwrap_or_else(|e| e.into_inner())
}
async fn app_token(app: &App, p: &Provider) -> Res<String> {
    if let Some((token, until)) = cached().clone()
        && until > Instant::now()
    {
        return Ok(token);
    }
    let body: Value = app
        .http
        .post(format!("{}/oauth2/token", id_base()))
        .form(&[
            ("client_id", p.client_id.as_str()),
            ("client_secret", p.client_secret.as_str()),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await
        .map_err(|_| down())?
        .error_for_status()
        .map_err(|_| down())?
        .json()
        .await
        .map_err(|_| down())?;
    let token = body["access_token"].as_str().ok_or_else(down)?.to_string();
    let life = body["expires_in"]
        .as_u64()
        .unwrap_or(3600)
        .saturating_sub(300);
    *cached() = Some((token.clone(), Instant::now() + Duration::from_secs(life)));
    Ok(token)
}
/// One Helix call with the app token (for the Discord bot's go-live alerts): the status and JSON
/// body, or None without a Twitch app or when Twitch couldn't be reached.
pub(crate) async fn helix(
    app: &App,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Option<(u16, Value)> {
    let p = twitch(app)?;
    for attempt in 0..2 {
        let token = app_token(app, p).await.ok()?;
        let mut request = app
            .http
            .request(method.clone(), format!("{}/helix{path}", api_base()))
            .header("Client-Id", &p.client_id)
            .bearer_auth(&token);
        if let Some(body) = &body {
            request = request.json(body);
        }
        let response = request.send().await.ok()?;
        let status = response.status().as_u16();
        if status == 401 && attempt == 0 {
            *cached() = None;
            continue;
        }
        return Some((status, response.json().await.unwrap_or(Value::Null)));
    }
    None
}
enum Connect {
    Ok(Vec<String>),
    Revoked,
    Retry,
}
async fn subscribe(app: &App, p: &Provider, subject: &str) -> Connect {
    let Ok(token) = app_token(app, p).await else {
        return Connect::Retry;
    };
    let mut ids = Vec::new();
    for kind in EVENTS {
        let sent = app.http.post(format!("{}/helix/eventsub/subscriptions", api_base()))
            .header("Client-Id", &p.client_id).bearer_auth(&token)
            .json(&json!({"type":kind,"version":"1","condition":{"broadcaster_user_id":subject,"user_id":subject},
                "transport":{"method":"webhook","callback":callback(app),"secret":webhook_secret(app)}}))
            .send().await;
        let Ok(response) = sent else {
            return Connect::Retry;
        };
        match response.status().as_u16() {
            202 => match response
                .json::<Value>()
                .await
                .ok()
                .and_then(|b| b["data"][0]["id"].as_str().map(String::from))
            {
                Some(id) => ids.push(id),
                None => return Connect::Retry,
            },
            // Already subscribed (for example after a restart): find it so it can be removed later.
            409 => ids.extend(existing(app, p, &token, subject, kind).await),
            401 => {
                *cached() = None;
                return Connect::Retry;
            }
            403 => return Connect::Revoked,
            _ => return Connect::Retry,
        }
    }
    Connect::Ok(ids)
}
async fn existing(app: &App, p: &Provider, token: &str, subject: &str, kind: &str) -> Vec<String> {
    let Ok(response) = app
        .http
        .get(format!("{}/helix/eventsub/subscriptions", api_base()))
        .query(&[("user_id", subject)])
        .header("Client-Id", &p.client_id)
        .bearer_auth(token)
        .send()
        .await
    else {
        return Vec::new();
    };
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let ours = callback(app);
    body["data"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|r| r["type"] == kind && r["transport"]["callback"] == ours.as_str())
                .filter_map(|r| r["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
async fn unsubscribe(app: &App, p: &Provider, ids: &[String]) {
    let Ok(token) = app_token(app, p).await else {
        return;
    };
    for id in ids {
        let _ = app
            .http
            .delete(format!("{}/helix/eventsub/subscriptions", api_base()))
            .query(&[("id", id)])
            .header("Client-Id", &p.client_id)
            .bearer_auth(&token)
            .send()
            .await;
    }
}
async fn set_status(
    app: &App,
    owner: &str,
    platform: &str,
    status: &str,
    detail: Option<&str>,
) -> Res<()> {
    sqlx::query("UPDATE linked_chat_accounts SET status=$3,detail=$4,status_at=now() WHERE owner_id=$1 AND platform=$2")
        .bind(owner).bind(platform).bind(status).bind(detail).execute(&app.db).await?;
    Ok(())
}

/// Every few seconds: connect linked chats of channels that went live and disconnect those whose
/// broadcast ended. Twitch and YouTube each run even when the other fails.
pub async fn tick(app: &App) -> Res<()> {
    let twitch = match twitch(app) {
        Some(p) => supervise_twitch(app, p).await,
        None => Ok(()),
    };
    let youtube = youtube::tick(app).await;
    let kick = kick::tick(app).await;
    twitch.and(youtube).and(kick)
}
/// Subscribes Twitch chats of channels that went live, and unsubscribes those whose broadcast
/// ended (they stay connected through the RECONNECTING window).
async fn supervise_twitch(app: &App, p: &Provider) -> Res<()> {
    let rows: Vec<(String, String, Vec<String>, bool)> = sqlx::query_as("SELECT a.owner_id,a.subject,a.subscriptions,
            a.enabled AND u.email_verified AND u.mfa_enabled AND u.deleted_at IS NULL AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=a.owner_id AND b.state IN ('LIVE','RECONNECTING'))
        FROM linked_chat_accounts a JOIN users u ON u.id=a.owner_id
        WHERE a.platform='twitch' AND (cardinality(a.subscriptions)>0
            OR (a.enabled AND a.status<>'revoked' AND (a.status<>'reconnecting' OR a.status_at<now()-interval '30 seconds')
                AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=a.owner_id AND b.state='LIVE')))")
        .fetch_all(&app.db)
        .await?;
    for (owner, subject, subscriptions, live) in rows {
        if live && subscriptions.is_empty() {
            match subscribe(app, p, &subject).await {
                Connect::Ok(ids) => {
                    sqlx::query("UPDATE linked_chat_accounts SET subscriptions=$2,status='connecting',detail=NULL,status_at=now() WHERE owner_id=$1 AND platform='twitch'")
                        .bind(&owner).bind(&ids).execute(&app.db).await?;
                }
                Connect::Revoked => {
                    let why = "Twitch didn't allow reading your chat. Link Twitch again.";
                    set_status(app, &owner, "twitch", "revoked", Some(why)).await?
                }
                Connect::Retry => {
                    let why = "Twitch chat disconnected, reconnecting.";
                    set_status(app, &owner, "twitch", "reconnecting", Some(why)).await?
                }
            }
        } else if !live && !subscriptions.is_empty() {
            unsubscribe(app, p, &subscriptions).await;
            sqlx::query("UPDATE linked_chat_accounts SET subscriptions='{}',status=CASE WHEN status='revoked' THEN status ELSE 'idle' END,status_at=now() WHERE owner_id=$1 AND platform='twitch'")
                .bind(&owner).execute(&app.db).await?;
        }
    }
    Ok(())
}

// ---- Streamer API ----

async fn streamer(app: &App, jar: &CookieJar) -> Res<auth::User> {
    let (tx, user, session) = auth::session(app, jar, false).await?;
    tx.commit().await?;
    auth::authorize_streaming(&user, &session)?;
    Ok(user)
}
async fn view(app: &App, owner: &str) -> Res<Json<Value>> {
    let accounts: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('platform',platform,'handle',handle,'enabled',enabled,'status',status,'detail',detail) FROM linked_chat_accounts WHERE owner_id=$1 ORDER BY platform")
        .bind(owner).fetch_all(&app.db).await?;
    let mutes: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('platform',m.platform,'sender_id',m.sender_id,'name',coalesce((SELECT sender_name FROM outside_chat_messages o WHERE o.channel_id=m.channel_id AND o.platform=m.platform AND o.sender_id=m.sender_id ORDER BY seq DESC LIMIT 1),m.sender_id),'permanent',m.broadcast_id IS NULL) FROM outside_chat_mutes m WHERE m.channel_id=$1
            AND (m.broadcast_id IS NULL OR EXISTS(SELECT 1 FROM broadcasts b WHERE b.id=m.broadcast_id AND b.state<>'ENDED')) ORDER BY m.created_at DESC LIMIT 200")
        .bind(owner).fetch_all(&app.db).await?;
    let available: serde_json::Map<String, Value> = PLATFORMS
        .iter()
        .map(|p| {
            let on = scopes(p).is_some() && app.config.providers.iter().any(|x| x.name == *p);
            ((*p).to_string(), json!(on))
        })
        .collect();
    // After a stream (docs/LINKED_CHAT.md "Studio insight"): S.V.E.R chat and viewers beside each
    // linked platform's chat, for streams in the last 7 days (chat is kept that long). The
    // streamer's own messages and the bot's lines don't count.
    let streams: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('broadcast',b.id,'started_at',b.started_at,'ended_at',b.ended_at,
            'sver',jsonb_build_object(
                'messages',(SELECT count(*) FROM chat_messages m WHERE m.channel_id=$1 AND m.author_id<>$1 AND m.origin IS NULL AND m.created_at>=b.started_at AND m.created_at<coalesce(b.ended_at,now())),
                'chatters',(SELECT count(DISTINCT m.author_id) FROM chat_messages m WHERE m.channel_id=$1 AND m.author_id<>$1 AND m.origin IS NULL AND m.created_at>=b.started_at AND m.created_at<coalesce(b.ended_at,now())),
                'peak_viewers',(SELECT coalesce(max(n),0) FROM (SELECT sum(s.counted+s.trusted) n FROM integrity_snapshots s WHERE s.broadcast_id=b.id GROUP BY date_trunc('minute',s.taken_at)) p)),
            'linked',(SELECT coalesce(jsonb_object_agg(platform,jsonb_build_object('messages',c,'chatters',d)),'{}') FROM (SELECT o.platform,count(*) c,count(DISTINCT o.sender_id) d FROM outside_chat_messages o
                WHERE o.channel_id=$1 AND o.platform<>'bot' AND o.sender_role IS DISTINCT FROM 'broadcaster' AND o.created_at>=b.started_at AND o.created_at<coalesce(b.ended_at,now()) GROUP BY o.platform) x))
        FROM broadcasts b WHERE b.owner_id=$1 AND b.started_at>now()-interval '7 days' ORDER BY b.started_at DESC LIMIT 10")
        .bind(owner).fetch_all(&app.db).await?;
    Ok(Json(
        json!({"accounts": accounts, "mutes": mutes, "available": available, "streams": streams}),
    ))
}
/// GET /api/me/linked-chat/status: each linked platform's connection, for the notice in the
/// streamer's own chat view (docs/LINKED_CHAT.md "Connection"). Light enough to poll.
async fn status(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = crate::profiles::signed_in(&app, &jar).await?;
    let accounts: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('platform',platform,'enabled',enabled,'status',status,'detail',detail) FROM linked_chat_accounts WHERE owner_id=$1 ORDER BY platform")
        .bind(&user.id).fetch_all(&app.db).await?;
    Ok(Json(json!({"accounts": accounts})))
}
/// GET /api/me/linked-chat
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = crate::profiles::signed_in(&app, &jar).await?;
    view(&app, &user.id).await
}
#[derive(Deserialize)]
pub struct Toggle {
    enabled: bool,
}
/// PATCH /api/me/linked-chat/{platform}: turn a linked platform's chat on or off.
async fn toggle(
    State(app): State<App>,
    jar: CookieJar,
    Path(platform): Path<String>,
    Json(input): Json<Toggle>,
) -> Res<Json<Value>> {
    let user = streamer(&app, &jar).await?;
    sqlx::query("UPDATE linked_chat_accounts SET enabled=$3 WHERE owner_id=$1 AND platform=$2")
        .bind(&user.id)
        .bind(&platform)
        .bind(input.enabled)
        .execute(&app.db)
        .await?;
    view(&app, &user.id).await
}
/// DELETE /api/me/linked-chat/{platform}: unlinks, revoking and deleting the tokens.
async fn unlink(
    State(app): State<App>,
    jar: CookieJar,
    Path(platform): Path<String>,
) -> Res<Json<Value>> {
    let user = crate::profiles::signed_in(&app, &jar).await?;
    let row: Option<(String, Option<String>, Vec<String>)> = sqlx::query_as("DELETE FROM linked_chat_accounts WHERE owner_id=$1 AND platform=$2 RETURNING access_sealed,refresh_sealed,subscriptions")
        .bind(&user.id).bind(&platform).fetch_optional(&app.db).await?;
    if let Some((access, refresh)) = row
        .as_ref()
        .filter(|_| platform == "youtube")
        .map(|(a, r, _)| (a, r))
    {
        youtube::revoke(&app, refresh.as_deref().unwrap_or(access)).await;
    }
    if let Some((access, refresh, subscriptions)) = row.as_ref().filter(|_| platform == "kick") {
        kick::unlink(&app, refresh.as_deref().unwrap_or(access), subscriptions).await;
    }
    if let (Some((sealed, _, subscriptions)), Some(p)) =
        (row, twitch(&app).filter(|_| platform == "twitch"))
    {
        unsubscribe(&app, p, &subscriptions).await;
        if let Ok(token) = sec::unseal(&app, "linked-chat", &sealed) {
            let _ = app
                .http
                .post(format!("{}/oauth2/revoke", id_base()))
                .form(&[
                    ("client_id", p.client_id.as_str()),
                    ("token", token.as_str()),
                ])
                .send()
                .await;
        }
    }
    view(&app, &user.id).await
}
type Account = (String, String, Option<String>, bool);
/// The streamer's user token on a platform (`p` is that platform's app), refreshed when it's about
/// to expire (or `force`).
async fn user_token(app: &App, p: &Provider, owner: &str, force: bool) -> Res<(String, String)> {
    let platform = p.name.as_str();
    let name = label(platform);
    let (subject, access, refresh, fresh): Account = sqlx::query_as("SELECT subject,access_sealed,refresh_sealed,coalesce(expires_at>now()+interval '1 minute',false) FROM linked_chat_accounts WHERE owner_id=$1 AND platform=$2 AND status<>'revoked'")
        .bind(owner).bind(platform).fetch_optional(&app.db).await?
        .ok_or_else(|| Fail::bad(format!("Link {name} in Creator Studio first.")))?;
    if fresh && !force {
        return Ok((subject, sec::unseal(app, "linked-chat", &access)?));
    }
    let refresh = refresh.ok_or_else(|| Fail::bad(format!("Link {name} again.")))?;
    let refresh = sec::unseal(app, "linked-chat", &refresh)?;
    let url = if platform == "twitch" {
        format!("{}/oauth2/token", id_base())
    } else {
        p.token_url.clone()
    };
    let down = || Fail::unavailable(format!("{name} couldn't be reached. Try again shortly."));
    let response = app
        .http
        .post(url)
        .form(&[
            ("client_id", p.client_id.as_str()),
            ("client_secret", p.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ])
        .send()
        .await
        .map_err(|_| down())?;
    // A refusal (4xx) means the access is gone; the platform's own outage (5xx) only needs a retry.
    if response.status().is_server_error() {
        return Err(down());
    }
    if !response.status().is_success() {
        let why = format!("{name} access ended. Link {name} again.");
        set_status(app, owner, platform, "revoked", Some(&why)).await?;
        return Err(Fail::bad(format!(
            "{name} access ended. Link {name} again in Creator Studio."
        )));
    }
    let tokens: Value = response.json().await.map_err(|_| down())?;
    let mut db = app.db.acquire().await?;
    let handle: String = sqlx::query_scalar(
        "SELECT handle FROM linked_chat_accounts WHERE owner_id=$1 AND platform=$2",
    )
    .bind(owner)
    .bind(platform)
    .fetch_one(&mut *db)
    .await?;
    save(app, &mut db, owner, platform, &subject, &handle, &tokens).await?;
    let access = tokens["access_token"].as_str().unwrap_or_default();
    Ok((subject, access.to_string()))
}
#[derive(Deserialize)]
pub struct Reply {
    platform: String,
    body: String,
    #[serde(default)]
    reply_to: Option<String>,
}
/// POST /api/me/linked-chat/reply: the streamer posts to a linked platform's chat as themselves.
/// The message comes back through the platform like any other, so it shows in S.V.E.R chat too.
async fn reply(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Reply>,
) -> Res<Json<Value>> {
    let user = streamer(&app, &jar).await?;
    if !PLATFORMS.contains(&input.platform.as_str()) {
        return Err(Fail::bad("Replies aren't available for that platform yet."));
    }
    crate::switches::guard(
        &mut *app.db.acquire().await?,
        &format!("linked_chat_{}", input.platform),
    )
    .await?;
    let body = clean(&input.body);
    if body.is_empty() {
        return Err(Fail::bad("Write a reply."));
    }
    sec::reserve(&app, vec![format!("linked-reply:{}", user.id)], 20, 30).await?;
    if input.platform == "youtube" {
        return youtube::reply(&app, &user.id, &body, input.reply_to.as_deref()).await;
    }
    if input.platform == "kick" {
        return kick::reply(&app, &user.id, &body, input.reply_to.as_deref()).await;
    }
    let p = twitch(&app)
        .filter(|_| input.platform == "twitch")
        .ok_or_else(|| Fail::bad("Replies aren't available for that platform yet."))?;
    let parent = match input.reply_to.as_deref() {
        Some(id) => {
            let found: Option<String> = sqlx::query_scalar("SELECT id FROM outside_chat_messages WHERE id=$1 AND channel_id=$2 AND platform='twitch'")
                .bind(id).bind(&user.id).fetch_optional(&app.db).await?;
            Some(
                found
                    .ok_or_else(Fail::missing)?
                    .trim_start_matches("twitch:")
                    .to_string(),
            )
        }
        None => None,
    };
    for attempt in 0..2 {
        let (subject, token) = user_token(&app, p, &user.id, attempt > 0).await?;
        let mut message = json!({"broadcaster_id":subject,"sender_id":subject,"message":body});
        if let Some(parent) = &parent {
            message["reply_parent_message_id"] = json!(parent);
        }
        let response = app
            .http
            .post(format!("{}/helix/chat/messages", api_base()))
            .header("Client-Id", &p.client_id)
            .bearer_auth(&token)
            .json(&message)
            .send()
            .await
            .map_err(|_| down())?;
        if response.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
            continue;
        }
        let ok = response.status().is_success();
        let result: Value = response.json().await.unwrap_or(Value::Null);
        if ok && result["data"][0]["is_sent"] == true {
            return Ok(Json(json!({"sent": true})));
        }
        let why = result["data"][0]["drop_reason"]["message"]
            .as_str()
            .or(result["message"].as_str())
            .unwrap_or("Twitch didn't accept it.");
        return Err(Fail::bad(format!("Twitch didn't post your reply: {why}")));
    }
    Err(Fail::bad(
        "Twitch access ended. Link Twitch again in Creator Studio.",
    ))
}

// ---- Moderation (S.V.E.R only) ----

#[derive(Deserialize)]
pub struct Mute {
    platform: String,
    sender_id: String,
    #[serde(default)]
    permanent: bool,
}
/// POST /api/channels/{username}/chat/outside/{id}/hide
async fn hide_message(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
) -> Res<Json<Value>> {
    let channel = moderation::channel(&app, &name).await?;
    let (user, role) = moderation::actor(&app, &jar, &channel).await?;
    hide(&app, &channel, Hide::Message(&id)).await?;
    let mut tx = app.db.begin().await?;
    let reason = "Hid a linked-chat message";
    moderation::log(
        &mut tx,
        &channel,
        &user.id,
        role,
        "outside_hide",
        None,
        Some(&id),
        json!({}),
        reason,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"hidden": true})))
}
/// POST /api/channels/{username}/chat/outside-mutes: mutes an outside sender on S.V.E.R for this
/// broadcast (or for good) and hides their messages here.
async fn mute(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Mute>,
) -> Res<Json<Value>> {
    let channel = moderation::channel(&app, &name).await?;
    let (user, role) = moderation::actor(&app, &jar, &channel).await?;
    if !PLATFORMS.contains(&input.platform.as_str())
        || input.sender_id.is_empty()
        || input.sender_id.len() > 100
        || input.sender_id.contains('\n')
    {
        return Err(Fail::bad("Choose a linked-chat sender."));
    }
    let broadcast: Option<String> = if input.permanent {
        None
    } else {
        let live: Option<String> = sqlx::query_scalar("SELECT id FROM broadcasts WHERE owner_id=$1 AND state<>'ENDED' ORDER BY started_at DESC LIMIT 1")
            .bind(&channel).fetch_optional(&app.db).await?;
        Some(live.ok_or_else(|| Fail::bad("You're not live; mute them permanently instead."))?)
    };
    sqlx::query("INSERT INTO outside_chat_mutes(channel_id,platform,sender_id,broadcast_id) VALUES($1,$2,$3,$4) ON CONFLICT(channel_id,platform,sender_id) DO UPDATE SET broadcast_id=EXCLUDED.broadcast_id,created_at=now()")
        .bind(&channel).bind(&input.platform).bind(&input.sender_id).bind(&broadcast).execute(&app.db).await?;
    hide(
        &app,
        &channel,
        Hide::Sender(&input.platform, &input.sender_id),
    )
    .await?;
    let mut tx = app.db.begin().await?;
    let detail = json!({"platform": input.platform, "sender_id": input.sender_id, "permanent": input.permanent});
    moderation::log(
        &mut tx,
        &channel,
        &user.id,
        role,
        "outside_mute",
        None,
        None,
        detail,
        "Muted a linked-chat sender",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"muted": true})))
}
/// DELETE /api/channels/{username}/chat/outside-mutes/{platform}/{sender}
async fn unmute(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, platform, sender)): Path<(String, String, String)>,
) -> Res<Json<Value>> {
    let channel = moderation::channel(&app, &name).await?;
    let (user, role): (auth::User, Role) = moderation::actor(&app, &jar, &channel).await?;
    sqlx::query(
        "DELETE FROM outside_chat_mutes WHERE channel_id=$1 AND platform=$2 AND sender_id=$3",
    )
    .bind(&channel)
    .bind(&platform)
    .bind(&sender)
    .execute(&app.db)
    .await?;
    let mut tx = app.db.begin().await?;
    let detail = json!({"platform": platform, "sender_id": sender});
    moderation::log(
        &mut tx,
        &channel,
        &user.id,
        role,
        "outside_unmute",
        None,
        None,
        detail,
        "Unmuted a linked-chat sender",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"muted": false})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/integrations/twitch/eventsub", post(eventsub))
        .route("/api/integrations/kick/webhook", post(kick::webhook))
        .route("/api/me/linked-chat", get(mine))
        .route("/api/me/linked-chat/reply", post(reply))
        .route("/api/me/linked-chat/status", get(status))
        .route(
            "/api/me/linked-chat/{platform}",
            patch(toggle).delete(unlink),
        )
        .route(
            "/api/channels/{username}/chat/outside/{id}/hide",
            post(hide_message),
        )
        .route("/api/channels/{username}/chat/outside-mutes", post(mute))
        .route(
            "/api/channels/{username}/chat/outside-mutes/{platform}/{sender}",
            delete(unmute),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outside_text_is_plain_and_bounded() {
        assert_eq!(clean("  hi\u{0007}there\n "), "hi there");
        assert_eq!(clean(&"x".repeat(900)).chars().count(), 500);
    }
}
