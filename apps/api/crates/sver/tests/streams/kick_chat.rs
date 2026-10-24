//! Kick linked chat (docs/LINKED_CHAT.md) against a fake Kick: only deliveries signed with Kick's
//! key get in, the supervisor subscribes when the channel goes live and unsubscribes when it ends,
//! messages show with roles and emote names, a Kick ban hides that person's messages, replies post
//! as the streamer, and unlinking removes the subscriptions and revokes the access.
use super::*;
use axum::{extract::RawQuery, response::IntoResponse};
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::SigningKey,
    pkcs8::{EncodePublicKey, LineEnding},
    rand_core::OsRng,
    sha2::Sha256,
    signature::{SignatureEncoding, Signer},
};

#[derive(Default)]
pub struct Kick {
    public_key: String,
    subscribed: Vec<Value>,
    unsubscribed: Vec<String>,
    posted: Vec<Value>,
    revoked: Vec<String>,
}
pub type Shared = Arc<Mutex<Kick>>;

fn fake(k: Shared) -> Router {
    Router::new()
        .route(
            "/oauth/token",
            post(|| async {
                Json(json!({"access_token":"kick-app","token_type":"Bearer","expires_in":3600}))
            }),
        )
        .route(
            "/oauth/revoke",
            post(|State(k): State<Shared>, RawQuery(q): RawQuery| async move {
                k.lock().unwrap().revoked.push(q.unwrap_or_default());
            }),
        )
        .route(
            "/public/v1/public-key",
            get(|State(k): State<Shared>| async move {
                let key = k.lock().unwrap().public_key.clone();
                Json(json!({"data":{"public_key": key}}))
            }),
        )
        .route(
            "/public/v1/events/subscriptions",
            post(|State(k): State<Shared>, Json(body): Json<Value>| async move {
                k.lock().unwrap().subscribed.push(body);
                Json(json!({"data":[
                    {"name":"chat.message.sent","version":1,"subscription_id":"sub-chat","error":null},
                    {"name":"moderation.banned","version":1,"subscription_id":"sub-ban","error":null}]}))
            })
            .delete(|State(k): State<Shared>, RawQuery(q): RawQuery| async move {
                k.lock().unwrap().unsubscribed.push(q.unwrap_or_default());
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/public/v1/chat",
            post(|State(k): State<Shared>, Json(body): Json<Value>| async move {
                k.lock().unwrap().posted.push(body);
                Json(json!({"data":{"is_sent":true,"message_id":"sent-1"}})).into_response()
            }),
        )
        .with_state(k)
}
/// Serves the fake Kick with a fresh signing key; its base URL and the key to sign with.
pub async fn serve(k: Shared) -> (String, SigningKey<Sha256>) {
    let private = RsaPrivateKey::new(&mut OsRng, 1024).unwrap();
    k.lock().unwrap().public_key = RsaPublicKey::from(&private)
        .to_public_key_pem(LineEnding::LF)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, fake(k)).await.unwrap() });
    (format!("http://{address}"), SigningKey::new(private))
}

async fn deliver(
    e: &Env,
    key: &SigningKey<Sha256>,
    kind: &str,
    body: Value,
    at: chrono::DateTime<chrono::Utc>,
) -> StatusCode {
    let id = uuid::Uuid::new_v4().to_string();
    let at = at.to_rfc3339();
    let body = body.to_string();
    let signature = STANDARD.encode(key.sign(format!("{id}.{at}.{body}").as_bytes()).to_bytes());
    let request = Request::builder()
        .method("POST")
        .uri("/api/integrations/kick/webhook")
        .header("content-type", "application/json")
        .header("kick-event-message-id", id)
        .header("kick-event-message-timestamp", at)
        .header("kick-event-signature", signature)
        .header("kick-event-type", kind)
        .header("kick-event-version", "1")
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    sver::router(e.app.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}
fn person(id: u64, name: &str, badges: Value) -> Value {
    let identity = if badges.is_null() {
        Value::Null
    } else {
        json!({"username_color":"#fff","badges":badges})
    };
    json!({"is_anonymous":false,"user_id":id,"username":name,"is_verified":false,"profile_picture":"",
        "channel_slug":name.to_lowercase(),"identity":identity})
}
fn message(id: &str, sender: Value, content: &str) -> Value {
    json!({"message_id":id,"broadcaster":person(4242,"Streamer",Value::Null),"sender":sender,
        "content":content,"emotes":[],"created_at":"2026-10-23T12:00:00Z"})
}
async fn shown(e: &Env) -> Vec<Value> {
    e.call("GET", "/api/channels/Streamer/chat", Value::Null)
        .await["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["outside"]["platform"] == "kick")
        .cloned()
        .collect()
}
async fn account(e: &Env) -> (String, Vec<String>) {
    sqlx::query_as("SELECT status,subscriptions FROM linked_chat_accounts WHERE owner_id='stream-owner' AND platform='kick'")
        .fetch_one(&e.app.db).await.unwrap()
}

pub async fn exercise(e: &Env, k: Shared, key: SigningKey<Sha256>) {
    let now = chrono::Utc::now;
    // Linking asks Kick for chat permissions, and Studio offers it.
    let start = e
        .call(
            "POST",
            "/api/auth/oauth/kick/start",
            json!({"intent":"chat"}),
        )
        .await;
    assert!(
        start["url"]
            .as_str()
            .unwrap()
            .contains("scope=user%3Aread+chat%3Awrite+events%3Asubscribe"),
        "{start}"
    );
    let studio = e.call("GET", "/api/me/linked-chat", Value::Null).await;
    assert_eq!(studio["available"]["kick"], true);
    let tokens = json!({"access_token":"kick-access","refresh_token":"kick-refresh","token_type":"Bearer","expires_in":3600});
    let mut db = e.app.db.acquire().await.unwrap();
    sver::linked_chat::save(
        &e.app,
        &mut db,
        "stream-owner",
        "kick",
        "4242",
        "streamer",
        &tokens,
    )
    .await
    .unwrap();
    drop(db);

    // Not live: no subscription. Live: chat and bans are subscribed with the app token.
    sver::linked_chat::tick(&e.app).await.unwrap();
    assert!(k.lock().unwrap().subscribed.is_empty());
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('kick-b','stream-owner','kick-b',1,'LIVE','s','v','c',now(),now(),now())").await;
    sver::linked_chat::tick(&e.app).await.unwrap();
    assert_eq!(
        k.lock().unwrap().subscribed[0],
        json!({"broadcaster_user_id":4242,"method":"webhook","events":[
            {"name":"chat.message.sent","version":1},{"name":"moderation.banned","version":1}]})
    );
    assert_eq!(
        account(e).await,
        (
            "connected".into(),
            vec!["sub-chat".into(), "sub-ban".into()]
        )
    );

    // Only Kick's signature gets in, and only while fresh.
    let other = SigningKey::<Sha256>::new(RsaPrivateKey::new(&mut OsRng, 1024).unwrap());
    let hello = message(
        "k1",
        person(
            7,
            "Viewer",
            json!([{"text":"Moderator","type":"moderator"}]),
        ),
        "hi [emote:37226:KEKW] there",
    );
    assert_eq!(
        deliver(e, &other, "chat.message.sent", hello.clone(), now()).await,
        StatusCode::FORBIDDEN
    );
    let stale = now() - chrono::Duration::minutes(11);
    assert_eq!(
        deliver(e, &key, "chat.message.sent", hello.clone(), stale).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        deliver(e, &key, "chat.message.sent", hello, now()).await,
        StatusCode::NO_CONTENT
    );
    let streamer = person(4242, "Streamer", Value::Null);
    deliver(
        e,
        &key,
        "chat.message.sent",
        message("k2", streamer, "welcome"),
        now(),
    )
    .await;
    let spammer = person(9, "Spammer", json!([]));
    deliver(
        e,
        &key,
        "chat.message.sent",
        message("k3", spammer, "spam"),
        now(),
    )
    .await;
    let messages = shown(e).await;
    let bodies: Vec<_> = messages
        .iter()
        .map(|m| m["body"].as_str().unwrap())
        .collect();
    assert_eq!(bodies, ["hi KEKW there", "welcome", "spam"]);
    assert_eq!(
        (
            &messages[0]["outside"]["role"],
            &messages[1]["outside"]["role"],
            &messages[0]["outside"]["url"]
        ),
        (
            &json!("moderator"),
            &json!("broadcaster"),
            &json!("https://kick.com/viewer")
        )
    );

    // A ban on Kick hides that person's messages here.
    let ban = json!({"broadcaster":person(4242,"Streamer",Value::Null),"moderator":person(7,"Viewer",Value::Null),
        "banned_user":person(9,"Spammer",Value::Null),"metadata":{"reason":"spam","created_at":"2026-10-23T12:00:00Z","expires_at":null}});
    assert_eq!(
        deliver(e, &key, "moderation.banned", ban, now()).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(shown(e).await.len(), 2);

    // The streamer replies on Kick as their account, threaded to the message.
    let reply = e
        .call(
            "POST",
            "/api/me/linked-chat/reply",
            json!({"platform":"kick","body":"thanks!","reply_to":"kick:k1"}),
        )
        .await;
    assert_eq!(reply["sent"], true);
    assert_eq!(
        k.lock().unwrap().posted[0],
        json!({"type":"user","broadcaster_user_id":4242,"content":"thanks!","reply_to_message_id":"k1"})
    );

    // The stream ending removes the subscriptions.
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='kick-b'")
        .await;
    sver::linked_chat::tick(&e.app).await.unwrap();
    assert_eq!(k.lock().unwrap().unsubscribed, ["id=sub-chat&id=sub-ban"]);
    assert_eq!(account(e).await, ("idle".into(), vec![]));

    // Unlinking deletes the tokens and revokes the access at Kick.
    let mine = e
        .call("GET", "/api/me/linked-chat", Value::Null)
        .await
        .to_string();
    assert!(!mine.contains("kick-access") && !mine.contains("kick-refresh"));
    e.call("DELETE", "/api/me/linked-chat/kick", Value::Null)
        .await;
    assert_eq!(
        k.lock().unwrap().revoked,
        ["token=kick-refresh&token_hint_type=refresh_token"]
    );
    e.sql("DELETE FROM outside_chat_messages").await;
}
