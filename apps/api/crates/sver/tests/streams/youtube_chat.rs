//! YouTube linked chat (docs/LINKED_CHAT.md) against a fake Google: YouTube is chat-only, linking
//! needs the YouTube permission, the supervisor finds the live chat and reads it page by page,
//! YouTube deletes and bans hide messages here, replies post to the live chat, a spent quota
//! pauses with a notice, and unlinking revokes the access.
use super::*;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;

pub const SCOPE: &str = "https://www.googleapis.com/auth/youtube";

#[derive(Default)]
pub struct Google {
    reads: usize,
    quota: bool,
    posted: Vec<Value>,
    revoked: Vec<String>,
}
pub type Shared = Arc<Mutex<Google>>;

fn item(id: &str, kind: &str, sender: &str, snippet: Value, author: Value) -> Value {
    let mut snippet = snippet;
    snippet["type"] = json!(kind);
    let mut author = author;
    author["channelId"] = json!(sender);
    author["displayName"] = json!(format!("Name {sender}"));
    json!({"id": id, "snippet": snippet, "authorDetails": author})
}
async fn messages(State(g): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Response {
    let mut g = g.lock().unwrap();
    if g.quota {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":{"code":403,"errors":[{"reason":"quotaExceeded"}]}})),
        )
            .into_response();
    }
    assert_eq!(q["liveChatId"], "chat-1");
    g.reads += 1;
    let body = match q.get("pageToken").map(String::as_str) {
        None => json!({"nextPageToken":"p1","pollingIntervalMillis":0,"items":[
            item("y1","textMessageEvent","UCviewer",json!({"displayMessage":"hello from youtube"}),json!({"isChatModerator":true})),
            item("y2","superChatEvent","UCfan",json!({"displayMessage":"take my money"}),json!({"isChatSponsor":true})),
            item("y3","textMessageEvent","UCspam",json!({"displayMessage":"spam"}),json!({})),
            item("y4","textMessageEvent","UCspam",json!({"displayMessage":"more spam"}),json!({})),
            item("y5","newSponsorEvent","UCfan",json!({}),json!({})),
        ]}),
        Some("p1") => json!({"nextPageToken":"p2","pollingIntervalMillis":0,"items":[
            item("y6","messageDeletedEvent","UCmod",json!({"messageDeletedDetails":{"deletedMessageId":"y1"}}),json!({})),
            item("y7","userBannedEvent","UCmod",json!({"userBannedDetails":{"bannedUserDetails":{"channelId":"UCspam"}}}),json!({})),
        ]}),
        // The YouTube broadcast ended.
        _ => {
            json!({"nextPageToken":"p3","pollingIntervalMillis":0,"offlineAt":"2026-10-23T12:00:00Z","items":[]})
        }
    };
    Json(body).into_response()
}
fn google(g: Shared) -> Router {
    Router::new()
        .route(
            "/token",
            post(|| async {
                Json(json!({"access_token":"yt-fresh","token_type":"Bearer","expires_in":3600,"scope":SCOPE}))
            }),
        )
        .route(
            "/yt/liveBroadcasts",
            get(|| async { Json(json!({"items":[{"id":"b1","snippet":{"liveChatId":"chat-1"}}]})) }),
        )
        .route(
            "/yt/liveChat/messages",
            get(messages).post(|State(g): State<Shared>, Json(body): Json<Value>| async move {
                g.lock().unwrap().posted.push(body);
                Json(json!({"id":"sent"}))
            }),
        )
        .route(
            "/revoke",
            post(|State(g): State<Shared>, body: String| async move {
                g.lock().unwrap().revoked.push(body);
            }),
        )
        .with_state(g)
}
/// Serves the fake Google; its base URL.
pub async fn serve(g: Shared) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, google(g)).await.unwrap() });
    format!("http://{address}")
}
type Link = (String, Option<String>, Option<String>, Option<String>);
async fn account(e: &Env) -> Link {
    sqlx::query_as("SELECT status,detail,chat_id,page_token FROM linked_chat_accounts WHERE owner_id='stream-owner' AND platform='youtube'")
        .fetch_one(&e.app.db).await.unwrap()
}
async fn shown(e: &Env) -> Vec<Value> {
    e.call("GET", "/api/channels/Streamer/chat", Value::Null)
        .await["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["outside"]["platform"] == "youtube")
        .cloned()
        .collect()
}
fn bodies(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .map(|m| m["body"].as_str().unwrap().to_owned())
        .collect()
}
/// One supervisor pass with the poll wait skipped.
async fn pass(e: &Env) {
    e.sql("UPDATE linked_chat_accounts SET poll_at=NULL").await;
    sver::linked_chat::tick(&e.app).await.unwrap();
}

pub async fn exercise(e: &Env, g: Shared) {
    // YouTube is for linked chat only, never sign-in, and isn't offered as a sign-in option.
    let (status, _) = e
        .request(
            "POST",
            "/api/auth/oauth/youtube/start",
            json!({"intent":"login"}),
            false,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let config = e.call("GET", "/api/auth/config", Value::Null).await;
    assert_eq!(config["providers"], json!([]), "{config}");
    let start = e
        .call(
            "POST",
            "/api/auth/oauth/youtube/start",
            json!({"intent":"chat"}),
        )
        .await;
    let url = start["url"].as_str().unwrap();
    assert!(
        url.contains("access_type=offline") && url.contains("prompt=consent"),
        "{url}"
    );
    let studio = e.call("GET", "/api/me/linked-chat", Value::Null).await;
    assert_eq!(studio["available"]["youtube"], true);

    // Linking needs the YouTube permission; Google lets people untick it.
    let tokens = |scope: &str| json!({"access_token":"yt-access","refresh_token":"yt-refresh","token_type":"Bearer","expires_in":3600,"scope":scope});
    let save = |scope: &str| {
        let tokens = tokens(scope);
        async move {
            let mut db = e.app.db.acquire().await.unwrap();
            sver::linked_chat::save(
                &e.app,
                &mut db,
                "stream-owner",
                "youtube",
                "UCowner",
                "@streamer",
                &tokens,
            )
            .await
        }
    };
    assert!(save("openid email").await.is_err());
    save(SCOPE).await.unwrap();

    // Not live: nothing is read.
    pass(e).await;
    assert_eq!(g.lock().unwrap().reads, 0);
    assert_eq!(account(e).await.0, "idle");

    // Live: the supervisor finds the live chat, then reads it.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('yt-b','stream-owner','yt-b',1,'LIVE','s','v','c',now(),now(),now())").await;
    pass(e).await;
    assert_eq!(account(e).await.2.as_deref(), Some("chat-1"));
    pass(e).await;
    let messages = shown(e).await;
    assert_eq!(
        bodies(&messages),
        ["hello from youtube", "take my money", "spam", "more spam"]
    );
    assert_eq!(
        (
            &messages[0]["outside"]["role"],
            &messages[1]["outside"]["role"],
            &messages[0]["outside"]["url"]
        ),
        (
            &json!("moderator"),
            &json!("subscriber"),
            &json!("https://www.youtube.com/channel/UCviewer")
        )
    );
    let (status, detail, _, page) = account(e).await;
    assert_eq!(
        (status.as_str(), detail, page.as_deref()),
        ("connected", None, Some("p1"))
    );

    // The streamer replies; YouTube has no threads, so the reply names who it answers.
    let reply = e
        .call(
            "POST",
            "/api/me/linked-chat/reply",
            json!({"platform":"youtube","body":"thanks!","reply_to":"youtube:y2"}),
        )
        .await;
    assert_eq!(reply["sent"], true);
    assert_eq!(
        g.lock().unwrap().posted[0]["snippet"],
        json!({"liveChatId":"chat-1","type":"textMessageEvent","textMessageDetails":{"messageText":"@Name UCfan thanks!"}})
    );
    let (status, _) = e
        .request(
            "POST",
            "/api/me/linked-chat/reply",
            json!({"platform":"youtube","body":"x".repeat(201)}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "YouTube's 200-character limit"
    );

    // A delete on YouTube hides that message here; a ban hides what that person said.
    pass(e).await;
    assert_eq!(bodies(&shown(e).await), ["take my money"]);

    // The YouTube broadcast ending sends the supervisor back to waiting for a new one.
    pass(e).await;
    let (status, detail, chat, _) = account(e).await;
    assert_eq!(
        (status.as_str(), detail.as_deref(), chat),
        (
            "connecting",
            Some("Waiting for your YouTube stream to go live."),
            None
        )
    );

    // A spent daily quota pauses YouTube chat with a notice instead of failing silently.
    pass(e).await;
    g.lock().unwrap().quota = true;
    pass(e).await;
    let (status, detail, _, _) = account(e).await;
    assert_eq!(status, "reconnecting");
    assert!(detail.unwrap().contains("daily chat limit"));
    let waits: bool = sqlx::query_scalar("SELECT poll_at>now()+interval '10 minutes' FROM linked_chat_accounts WHERE platform='youtube'")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(waits, "it waits before spending quota again");
    g.lock().unwrap().quota = false;

    // The S.V.E.R stream ending forgets the chat.
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='yt-b'")
        .await;
    pass(e).await;
    let (status, _, chat, page) = account(e).await;
    assert_eq!((status.as_str(), chat, page), ("idle", None, None));

    // Unlinking deletes the tokens and revokes the access at Google.
    let mine = e
        .call("GET", "/api/me/linked-chat", Value::Null)
        .await
        .to_string();
    assert!(
        !mine.contains("yt-access") && !mine.contains("yt-refresh"),
        "tokens are never returned"
    );
    e.call("DELETE", "/api/me/linked-chat/youtube", Value::Null)
        .await;
    assert_eq!(g.lock().unwrap().revoked, ["token=yt-refresh"]);
    e.sql("DELETE FROM outside_chat_messages").await;
}
