//! YouTube linked chat (docs/LINKED_CHAT.md). YouTube has no chat webhooks, so while the streamer
//! is live on S.V.E.R the supervisor finds their active YouTube broadcast and reads its live chat
//! at the pace YouTube asks for, never faster than `YOUTUBE_POLL_SECONDS` (default 10): each read
//! spends the app's daily API quota. Where it is and how far it has read live in
//! `linked_chat_accounts` (`chat_id`, `page_token`, `poll_at`), so a restart picks up where it was.
use super::*;

pub(super) const SCOPE: &str = "https://www.googleapis.com/auth/youtube";
/// YouTube's own limit on a chat message.
const MAX_REPLY: usize = 200;
const WAITING: &str = "Waiting for your YouTube stream to go live.";
const DOWN: &str = "YouTube chat disconnected, reconnecting.";

fn youtube(app: &App) -> Option<&Provider> {
    app.config.providers.iter().find(|p| p.name == "youtube")
}
fn api_base() -> String {
    std::env::var("YOUTUBE_API_BASE")
        .unwrap_or_else(|_| "https://www.googleapis.com/youtube/v3".into())
}
fn min_poll() -> f64 {
    std::env::var("YOUTUBE_POLL_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10.0)
}

enum Read {
    Ok(Value),
    /// The live chat is over or gone (the YouTube broadcast ended): look for a new one.
    Ended,
    /// The app's daily quota ran out; it resets at midnight Pacific.
    Quota,
    /// YouTube refused the streamer's access.
    Revoked,
    Retry,
}
/// One YouTube Data API call as the streamer, refreshing their token once on a 401.
async fn call(
    app: &App,
    p: &Provider,
    owner: &str,
    method: reqwest::Method,
    path: &str,
    query: &[(&str, &str)],
    body: Option<&Value>,
) -> Read {
    for attempt in 0..2 {
        let token = match user_token(app, p, owner, attempt > 0).await {
            Ok((_, token)) => token,
            // Google refused the refresh (access removed, or a Testing-mode token past 7 days).
            Err(e) if e.status == StatusCode::BAD_REQUEST => return Read::Revoked,
            Err(_) => return Read::Retry,
        };
        let mut request = app
            .http
            .request(method.clone(), format!("{}{path}", api_base()))
            .query(query)
            .bearer_auth(&token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let Ok(response) = request.send().await else {
            return Read::Retry;
        };
        let status = response.status().as_u16();
        if status == 401 && attempt == 0 {
            continue;
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Read::Ok(body);
        }
        let reason = body["error"]["errors"][0]["reason"]
            .as_str()
            .unwrap_or_default();
        return match (status, reason) {
            (_, "quotaExceeded" | "dailyLimitExceeded") => Read::Quota,
            (_, "liveChatEnded" | "liveChatNotFound" | "liveChatDisabled") => Read::Ended,
            (401, _) | (403, "insufficientPermissions" | "forbidden") => Read::Revoked,
            _ => Read::Retry,
        };
    }
    Read::Revoked
}

/// Where a link stands after a read: status, its notice, and seconds until the next one.
async fn settle(app: &App, owner: &str, status: &str, detail: Option<&str>, wait: f64) -> Res<()> {
    sqlx::query("UPDATE linked_chat_accounts SET status=$2,detail=$3,status_at=CASE WHEN status=$2 AND detail IS NOT DISTINCT FROM $3 THEN status_at ELSE now() END,poll_at=now()+make_interval(secs=>$4)
        WHERE owner_id=$1 AND platform='youtube'")
        .bind(owner).bind(status).bind(detail).bind(wait).execute(&app.db).await?;
    Ok(())
}
async fn failed(app: &App, owner: &str, read: Read) -> Res<()> {
    match read {
        Read::Ended => {
            sqlx::query("UPDATE linked_chat_accounts SET chat_id=NULL,page_token=NULL WHERE owner_id=$1 AND platform='youtube'")
                .bind(owner).execute(&app.db).await?;
            settle(app, owner, "connecting", Some(WAITING), 30.0).await
        }
        Read::Quota => {
            let why = "YouTube's daily chat limit for S.V.E.R ran out. YouTube chat comes back when it resets.";
            settle(app, owner, "reconnecting", Some(why), 900.0).await
        }
        Read::Revoked => {
            let why = "YouTube didn't allow reading your chat. Link YouTube again.";
            settle(app, owner, "revoked", Some(why), 0.0).await
        }
        Read::Ok(_) | Read::Retry => settle(app, owner, "reconnecting", Some(DOWN), 30.0).await,
    }
}

/// Every supervisor pass: for each linked YouTube account whose channel is live and due a read,
/// find the active broadcast's chat or read what's new in it; once the S.V.E.R stream ends, forget
/// the chat. Accounts are read one after another.
// ponytail: sequential reads in the 5-second pass; spawn per account if many streamers link YouTube.
pub(super) async fn tick(app: &App) -> Res<()> {
    let Some(p) = youtube(app) else {
        return Ok(());
    };
    if crate::switches::off(&mut *app.db.acquire().await?, "linked_chat_youtube").await? {
        return Ok(());
    }
    type Row = (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        bool,
        bool,
    );
    let rows: Vec<Row> = sqlx::query_as("SELECT a.owner_id,a.subject,a.status,a.chat_id,a.page_token,
            a.enabled AND u.email_verified AND u.mfa_enabled AND u.deleted_at IS NULL AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=a.owner_id AND b.state IN ('LIVE','RECONNECTING')),
            coalesce(a.poll_at<=now(),true)
        FROM linked_chat_accounts a JOIN users u ON u.id=a.owner_id
        WHERE a.platform='youtube' AND (a.chat_id IS NOT NULL OR a.status IN ('connecting','connected','reconnecting')
            OR (a.enabled AND a.status<>'revoked' AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=a.owner_id AND b.state='LIVE')))")
        .fetch_all(&app.db)
        .await?;
    for (owner, subject, status, chat, page, live, due) in rows {
        if !live {
            sqlx::query("UPDATE linked_chat_accounts SET chat_id=NULL,page_token=NULL,poll_at=NULL,status=CASE WHEN status='revoked' THEN status ELSE 'idle' END,status_at=now() WHERE owner_id=$1 AND platform='youtube'")
                .bind(&owner).execute(&app.db).await?;
            continue;
        }
        if !due || status == "revoked" {
            continue;
        }
        let Some(chat) = chat else {
            find_chat(app, p, &owner).await?;
            continue;
        };
        let mut query = vec![
            ("liveChatId", chat.as_str()),
            ("part", "snippet,authorDetails"),
            ("maxResults", "2000"),
        ];
        if let Some(page) = &page {
            query.push(("pageToken", page));
        }
        let path = "/liveChat/messages";
        let body = match call(app, p, &owner, reqwest::Method::GET, path, &query, None).await {
            Read::Ok(body) => body,
            other => {
                failed(app, &owner, other).await?;
                continue;
            }
        };
        for item in body["items"].as_array().into_iter().flatten() {
            take(app, &owner, &subject, item).await?;
        }
        // `offlineAt` means the YouTube broadcast ended; a new one gets a new chat.
        if !body["offlineAt"].is_null() {
            failed(app, &owner, Read::Ended).await?;
            continue;
        }
        let wait = (body["pollingIntervalMillis"].as_f64().unwrap_or(0.0) / 1000.0).max(min_poll());
        sqlx::query("UPDATE linked_chat_accounts SET page_token=$2 WHERE owner_id=$1 AND platform='youtube'")
            .bind(&owner).bind(body["nextPageToken"].as_str()).execute(&app.db).await?;
        settle(app, &owner, "connected", None, wait).await?;
    }
    Ok(())
}
/// The streamer's active YouTube broadcast's live chat; until there is one, wait for it.
async fn find_chat(app: &App, p: &Provider, owner: &str) -> Res<()> {
    let query = [
        ("part", "snippet"),
        ("broadcastStatus", "active"),
        ("broadcastType", "all"),
    ];
    let path = "/liveBroadcasts";
    let body = match call(app, p, owner, reqwest::Method::GET, path, &query, None).await {
        Read::Ok(body) => body,
        Read::Ended => Value::Null,
        other => return failed(app, owner, other).await,
    };
    let chat = body["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|b| b["snippet"]["liveChatId"].as_str());
    match chat {
        Some(chat) => {
            sqlx::query("UPDATE linked_chat_accounts SET chat_id=$2,page_token=NULL WHERE owner_id=$1 AND platform='youtube'")
                .bind(owner).bind(chat).execute(&app.db).await?;
            settle(app, owner, "connecting", None, 0.0).await
        }
        None => settle(app, owner, "connecting", Some(WAITING), 30.0).await,
    }
}
/// One live chat item: a message (or Super Chat) shows, a delete hides it, a ban (or timeout)
/// hides what that person said. Everything else (memberships, polls, stickers) is left out.
async fn take(app: &App, owner: &str, subject: &str, item: &Value) -> Res<()> {
    let s = &item["snippet"];
    let a = &item["authorDetails"];
    match s["type"].as_str() {
        Some("textMessageEvent" | "superChatEvent") => {
            let role = if a["isChatOwner"] == true {
                Some("broadcaster")
            } else if a["isChatModerator"] == true {
                Some("moderator")
            } else if a["isChatSponsor"] == true {
                Some("subscriber")
            } else {
                None
            };
            let sender = a["channelId"].as_str().unwrap_or_default();
            receive(
                app,
                "youtube",
                subject,
                item["id"].as_str().unwrap_or_default(),
                sender,
                sender,
                a["displayName"].as_str().unwrap_or_default(),
                role,
                s["displayMessage"].as_str().unwrap_or_default(),
            )
            .await?;
        }
        Some("messageDeletedEvent") => {
            if let Some(id) = s["messageDeletedDetails"]["deletedMessageId"].as_str() {
                hide(app, owner, Hide::Message(&format!("youtube:{id}"))).await?;
            }
        }
        Some("userBannedEvent") => {
            if let Some(id) = s["userBannedDetails"]["bannedUserDetails"]["channelId"].as_str() {
                hide(app, owner, Hide::Sender("youtube", id)).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The streamer's reply, posted to their YouTube live chat as their channel. YouTube has no reply
/// threads, so a reply to someone starts with their name.
pub(super) async fn reply(
    app: &App,
    owner: &str,
    body: &str,
    reply_to: Option<&str>,
) -> Res<Json<Value>> {
    let p =
        youtube(app).ok_or_else(|| Fail::bad("Replies aren't available for that platform yet."))?;
    let chat: Option<String> = sqlx::query_scalar("SELECT chat_id FROM linked_chat_accounts WHERE owner_id=$1 AND platform='youtube' AND status<>'revoked'")
        .bind(owner).fetch_optional(&app.db).await?.flatten();
    let chat = chat.ok_or_else(|| {
        Fail::bad("Your YouTube stream isn't live, so there's no YouTube chat to post in.")
    })?;
    let text = match reply_to {
        Some(id) => {
            let name: String = sqlx::query_scalar("SELECT sender_name FROM outside_chat_messages WHERE id=$1 AND channel_id=$2 AND platform='youtube'")
                .bind(id).bind(owner).fetch_optional(&app.db).await?
                .ok_or_else(Fail::missing)?;
            format!("@{} {body}", name.trim_start_matches('@'))
        }
        None => body.to_string(),
    };
    if text.chars().count() > MAX_REPLY {
        return Err(Fail::bad(format!(
            "YouTube chat messages can be up to {MAX_REPLY} characters."
        )));
    }
    let message = json!({"snippet": {"liveChatId": chat, "type": "textMessageEvent", "textMessageDetails": {"messageText": text}}});
    let path = "/liveChat/messages";
    let query = [("part", "snippet")];
    match call(
        app,
        p,
        owner,
        reqwest::Method::POST,
        path,
        &query,
        Some(&message),
    )
    .await
    {
        Read::Ok(_) => Ok(Json(json!({"sent": true}))),
        Read::Quota => Err(Fail::bad(
            "YouTube didn't post your reply: S.V.E.R's daily YouTube limit ran out.",
        )),
        Read::Ended => Err(Fail::bad(
            "YouTube didn't post your reply: your YouTube chat has ended.",
        )),
        Read::Revoked => Err(Fail::bad(
            "YouTube didn't post your reply. Link YouTube again in Creator Studio.",
        )),
        Read::Retry => Err(Fail::bad(
            "YouTube didn't post your reply. Try again shortly.",
        )),
    }
}

/// Unlinking revokes S.V.E.R's access at Google (best effort; the tokens are already deleted).
pub(super) async fn revoke(app: &App, sealed: &str) {
    let Ok(token) = sec::unseal(app, "linked-chat", sealed) else {
        return;
    };
    let url = std::env::var("YOUTUBE_REVOKE_URL")
        .unwrap_or_else(|_| "https://oauth2.googleapis.com/revoke".into());
    let _ = app
        .http
        .post(url)
        .form(&[("token", token.as_str())])
        .send()
        .await;
}
