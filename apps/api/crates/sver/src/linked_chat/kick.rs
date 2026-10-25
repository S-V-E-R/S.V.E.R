//! Kick linked chat (docs/LINKED_CHAT.md). Kick sends chat as webhooks to the one URL set in the
//! S.V.E.R app (`/api/integrations/kick/webhook`), each signed with Kick's RSA key (fetched from
//! Kick, never hardcoded, since Kick may rotate it). The supervisor subscribes a linked channel's
//! `chat.message.sent` and `moderation.banned` with the app token when it goes live and removes the
//! subscriptions when it ends. Kick reports bans but not single deletes.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::{
    RsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
    sha2::Sha256,
    signature::Verifier,
};

pub(super) const SCOPES: &str = "user:read chat:write events:subscribe";
const EVENTS: [&str; 2] = ["chat.message.sent", "moderation.banned"];
const DOWN: &str = "Kick chat disconnected, reconnecting.";

fn kick(app: &App) -> Option<&Provider> {
    app.config.providers.iter().find(|p| p.name == "kick")
}
fn api_base() -> String {
    std::env::var("KICK_API_BASE").unwrap_or_else(|_| "https://api.kick.com".into())
}
fn id_base() -> String {
    std::env::var("KICK_ID_BASE").unwrap_or_else(|_| "https://id.kick.com".into())
}

// ---- Webhooks ----

/// Kick's public key and when it was fetched.
type Key = Option<(RsaPublicKey, Instant)>;
static KEY: LazyLock<Mutex<Key>> = LazyLock::new(Mutex::default);
fn key() -> std::sync::MutexGuard<'static, Key> {
    KEY.lock().unwrap_or_else(|e| e.into_inner())
}
async fn fetch_key(app: &App) -> Option<RsaPublicKey> {
    let body: Value = app
        .http
        .get(format!("{}/public/v1/public-key", api_base()))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let found = RsaPublicKey::from_public_key_pem(body["data"]["public_key"].as_str()?).ok()?;
    *key() = Some((found.clone(), Instant::now()));
    Some(found)
}
fn signed(key: &RsaPublicKey, id: &str, at: &str, body: &[u8], signature: &str) -> bool {
    let Some(signature) = STANDARD
        .decode(signature)
        .ok()
        .and_then(|s| Signature::try_from(s.as_slice()).ok())
    else {
        return false;
    };
    let mut message = format!("{id}.{at}.").into_bytes();
    message.extend_from_slice(body);
    VerifyingKey::<Sha256>::new(key.clone())
        .verify(&message, &signature)
        .is_ok()
}
/// Whether a delivery is Kick's: a fresh timestamp and a good signature. A failing signature
/// fetches the key again (Kick may have rotated it), at most once a minute so forged requests
/// can't make us hammer Kick.
async fn verified(app: &App, headers: &HeaderMap, body: &[u8]) -> bool {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let (Some(id), Some(at), Some(signature)) = (
        header("kick-event-message-id"),
        header("kick-event-message-timestamp"),
        header("kick-event-signature"),
    ) else {
        return false;
    };
    let fresh = chrono::DateTime::parse_from_rfc3339(at)
        .is_ok_and(|t| (chrono::Utc::now() - t.to_utc()).num_minutes().abs() < 10);
    if !fresh {
        return false;
    }
    let cached = key().clone();
    if let Some((k, _)) = &cached
        && signed(k, id, at, body, signature)
    {
        return true;
    }
    if cached.is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(60)) {
        return false;
    }
    match fetch_key(app).await {
        Some(k) => signed(&k, id, at, body, signature),
        None => false,
    }
}
fn user_id(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => n.as_u64().map(|n| n.to_string()),
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}
/// Kick writes emotes as `[emote:ID:NAME]`; outside emotes show as their names.
fn emote_names(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("[emote:") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 7..];
        match tail
            .find(']')
            .and_then(|end| tail[..end].split_once(':').map(|(_, name)| (end, name)))
        {
            Some((end, name)) => {
                out.push_str(name);
                rest = &tail[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
/// POST /api/integrations/kick/webhook
pub(super) async fn webhook(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if !verified(&app, &headers, &body).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(event) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(subject) = user_id(&event["broadcaster"]["user_id"]) else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let kind = headers
        .get("kick-event-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let result: Res<()> = match kind {
        "chat.message.sent" => chat(&app, &subject, &event).await,
        "moderation.banned" => banned(&app, &subject, &event).await,
        _ => Ok(()),
    };
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        // A 5xx asks Kick to retry; the message ID makes the retry harmless.
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
async fn chat(app: &App, subject: &str, e: &Value) -> Res<()> {
    let sender = &e["sender"];
    let Some(sender_id) = user_id(&sender["user_id"]) else {
        return Ok(());
    };
    let badges: Vec<&str> = sender["identity"]["badges"]
        .as_array()
        .map(|b| b.iter().filter_map(|x| x["type"].as_str()).collect())
        .unwrap_or_default();
    // Kick sends no identity for the broadcaster.
    let role = if sender_id == subject {
        Some("broadcaster")
    } else {
        ["broadcaster", "moderator", "subscriber"]
            .into_iter()
            .find(|r| badges.contains(r))
    };
    receive(
        app,
        "kick",
        subject,
        e["message_id"].as_str().unwrap_or_default(),
        &sender_id,
        sender["channel_slug"].as_str().unwrap_or_default(),
        sender["username"].as_str().unwrap_or_default(),
        role,
        &emote_names(e["content"].as_str().unwrap_or_default()),
    )
    .await?;
    Ok(())
}
/// A ban or timeout on Kick hides that person's messages here too.
async fn banned(app: &App, subject: &str, e: &Value) -> Res<()> {
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT owner_id FROM linked_chat_accounts WHERE platform='kick' AND subject=$1",
    )
    .bind(subject)
    .fetch_optional(&app.db)
    .await?;
    if let (Some(owner), Some(target)) = (owner, user_id(&e["banned_user"]["user_id"])) {
        hide(app, &owner, Hide::Sender("kick", &target)).await?;
    }
    Ok(())
}

// ---- Connecting and disconnecting ----

enum Connect {
    Ok(Vec<String>),
    Revoked,
    Retry,
}
async fn subscribe(app: &App, subject: &str) -> Connect {
    let Some(token) = crate::discord::streamers::kick_token(app).await else {
        return Connect::Retry;
    };
    let Ok(broadcaster) = subject.parse::<u64>() else {
        return Connect::Revoked;
    };
    let events: Vec<Value> = EVENTS
        .iter()
        .map(|name| json!({"name": name, "version": 1}))
        .collect();
    let sent = app
        .http
        .post(format!("{}/public/v1/events/subscriptions", api_base()))
        .bearer_auth(&token)
        .json(&json!({"broadcaster_user_id": broadcaster, "events": events, "method": "webhook"}))
        .send()
        .await;
    let Ok(response) = sent else {
        return Connect::Retry;
    };
    match response.status().as_u16() {
        200..=299 => {}
        401 => {
            crate::discord::streamers::kick_token_reset();
            return Connect::Retry;
        }
        403 => return Connect::Revoked,
        _ => return Connect::Retry,
    }
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let rows = body["data"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = rows
        .iter()
        .filter_map(|r| r["subscription_id"].as_str().map(String::from))
        .collect();
    if ids.len() < EVENTS.len() {
        unsubscribe(app, &ids).await;
        return Connect::Retry;
    }
    Connect::Ok(ids)
}
async fn unsubscribe(app: &App, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let Some(token) = crate::discord::streamers::kick_token(app).await else {
        return;
    };
    let query: Vec<(&str, &str)> = ids.iter().map(|id| ("id", id.as_str())).collect();
    let _ = app
        .http
        .delete(format!("{}/public/v1/events/subscriptions", api_base()))
        .query(&query)
        .bearer_auth(&token)
        .send()
        .await;
}

/// Every supervisor pass: subscribe linked Kick chats of channels that went live, and unsubscribe
/// those whose broadcast ended (they stay connected through the RECONNECTING window).
pub(super) async fn tick(app: &App) -> Res<()> {
    if kick(app).is_none() {
        return Ok(());
    }
    let rows: Vec<(String, String, Vec<String>, bool)> = sqlx::query_as("SELECT a.owner_id,a.subject,a.subscriptions,
            a.enabled AND u.email_verified AND u.mfa_enabled AND u.deleted_at IS NULL AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=a.owner_id AND b.state IN ('LIVE','RECONNECTING'))
        FROM linked_chat_accounts a JOIN users u ON u.id=a.owner_id
        WHERE a.platform='kick' AND (cardinality(a.subscriptions)>0
            OR (a.enabled AND a.status<>'revoked' AND (a.status<>'reconnecting' OR a.status_at<now()-interval '30 seconds')
                AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=a.owner_id AND b.state='LIVE')))")
        .fetch_all(&app.db)
        .await?;
    for (owner, subject, subscriptions, live) in rows {
        if live && subscriptions.is_empty() {
            match subscribe(app, &subject).await {
                Connect::Ok(ids) => {
                    sqlx::query("UPDATE linked_chat_accounts SET subscriptions=$2,status='connected',detail=NULL,status_at=now() WHERE owner_id=$1 AND platform='kick'")
                        .bind(&owner).bind(&ids).execute(&app.db).await?;
                }
                Connect::Revoked => {
                    let why = "Kick didn't allow reading your chat. Link Kick again.";
                    set_status(app, &owner, "kick", "revoked", Some(why)).await?
                }
                Connect::Retry => {
                    set_status(app, &owner, "kick", "reconnecting", Some(DOWN)).await?
                }
            }
        } else if !live && !subscriptions.is_empty() {
            unsubscribe(app, &subscriptions).await;
            sqlx::query("UPDATE linked_chat_accounts SET subscriptions='{}',status=CASE WHEN status='revoked' THEN status ELSE 'idle' END,status_at=now() WHERE owner_id=$1 AND platform='kick'")
                .bind(&owner).execute(&app.db).await?;
        }
    }
    Ok(())
}

// ---- The streamer ----

/// The streamer's reply, posted to their Kick chat as their account (Kick threads replies).
pub(super) async fn reply(
    app: &App,
    owner: &str,
    body: &str,
    reply_to: Option<&str>,
) -> Res<Json<Value>> {
    let p =
        kick(app).ok_or_else(|| Fail::bad("Replies aren't available for that platform yet."))?;
    let parent = match reply_to {
        Some(id) => {
            let found: Option<String> = sqlx::query_scalar("SELECT id FROM outside_chat_messages WHERE id=$1 AND channel_id=$2 AND platform='kick'")
                .bind(id).bind(owner).fetch_optional(&app.db).await?;
            Some(
                found
                    .ok_or_else(Fail::missing)?
                    .trim_start_matches("kick:")
                    .to_string(),
            )
        }
        None => None,
    };
    let down = || Fail::unavailable("Kick couldn't be reached. Try again shortly.");
    for attempt in 0..2 {
        let (subject, token) = user_token(app, p, owner, attempt > 0).await?;
        let broadcaster: u64 = subject.parse().map_err(|_| Fail::bad("Link Kick again."))?;
        let mut message =
            json!({"type": "user", "broadcaster_user_id": broadcaster, "content": body});
        if let Some(parent) = &parent {
            message["reply_to_message_id"] = json!(parent);
        }
        let response = app
            .http
            .post(format!("{}/public/v1/chat", api_base()))
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
        if ok && result["data"]["is_sent"] == true {
            return Ok(Json(json!({"sent": true})));
        }
        let why = result["message"]
            .as_str()
            .unwrap_or("Kick didn't accept it.");
        return Err(Fail::bad(format!("Kick didn't post your reply: {why}")));
    }
    Err(Fail::bad(
        "Kick access ended. Link Kick again in Creator Studio.",
    ))
}
/// Unlinking: removes the chat subscriptions and revokes S.V.E.R's access at Kick (best effort;
/// the tokens are already deleted).
pub(super) async fn unlink(app: &App, sealed: &str, subscriptions: &[String]) {
    unsubscribe(app, subscriptions).await;
    let Ok(token) = sec::unseal(app, "linked-chat", sealed) else {
        return;
    };
    let _ = app
        .http
        .post(format!("{}/oauth/revoke", id_base()))
        .query(&[
            ("token", token.as_str()),
            ("token_hint_type", "refresh_token"),
        ])
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emotes_become_their_names() {
        assert_eq!(
            emote_names("Hello [emote:4148074:HYPERCLAP] [emote:37226:KEKW]!"),
            "Hello HYPERCLAP KEKW!"
        );
        assert_eq!(emote_names("broken [emote:12"), "broken [emote:12");
        assert_eq!(emote_names("plain"), "plain");
    }
}
