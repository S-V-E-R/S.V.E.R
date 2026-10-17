//! Linked chat (docs/LINKED_CHAT.md): signed Twitch EventSub webhooks become outside messages in
//! S.V.E.R chat, live on open sockets, never counted, hidden by platform deletes and clears, S.V.E.R
//! mutes and banned words, and deleted after 7 days.
use super::*;
use hmac::{Hmac, KeyInit, Mac};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const PATH: &str = "/api/integrations/twitch/eventsub";

async fn webhook(e: &Env, kind: &str, body: Value, good: bool) -> (StatusCode, String) {
    let id = uuid::Uuid::new_v4().to_string();
    let at = chrono::Utc::now().to_rfc3339();
    let body = body.to_string();
    let secret = if good {
        sver::linked_chat::webhook_secret(&e.app)
    } else {
        "not-the-secret".into()
    };
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(id.as_bytes());
    mac.update(at.as_bytes());
    mac.update(body.as_bytes());
    let signature: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let request = Request::builder()
        .method("POST")
        .uri(PATH)
        .header("content-type", "application/json")
        .header("twitch-eventsub-message-id", id)
        .header("twitch-eventsub-message-timestamp", at)
        .header(
            "twitch-eventsub-message-signature",
            format!("sha256={signature}"),
        )
        .header("twitch-eventsub-message-type", kind)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    let response = sver::router(e.app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
fn chat(message_id: &str, sender: &str, text: &str) -> Value {
    json!({"subscription":{"type":"channel.chat.message","condition":{"broadcaster_user_id":"tw-owner","user_id":"tw-owner"}},
        "event":{"broadcaster_user_id":"tw-owner","chatter_user_id":sender,"chatter_user_login":format!("{sender}_login"),
            "chatter_user_name":format!("Name {sender}"),"message_id":message_id,"message":{"text":text},
            "badges":[{"set_id":"moderator","id":"1"}]}})
}
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
/// A guest's chat socket on the Streamer channel, past its snapshot.
async fn watch(e: &Env, address: SocketAddr) -> (Socket, Value) {
    let mut request = format!("ws://{address}/api/chat/ws?channel=streamer")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", e.app.config.origin.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let snapshot = super::chat::next_json(&mut socket).await;
    assert_eq!(snapshot["type"], "snapshot");
    (socket, snapshot)
}
async fn outside(e: &Env) -> Vec<Value> {
    let read = e
        .call("GET", "/api/channels/Streamer/chat", Value::Null)
        .await;
    read["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| !m["outside"].is_null())
        .cloned()
        .collect()
}

pub async fn exercise(e: &Env) {
    let sealed = sver::security::seal(&e.app, "linked-chat", "synthetic-access").unwrap();
    sqlx::query("INSERT INTO linked_chat_accounts(owner_id,platform,subject,handle,access_sealed) VALUES('stream-owner','twitch','tw-owner','streamer_tw',$1)")
        .bind(sealed).execute(&e.app.db).await.unwrap();
    let counts = || async {
        sqlx::query_as::<_, (i64, i64)>("SELECT (SELECT count(*) FROM chat_messages),(SELECT coalesce(sum(earned),0)::bigint FROM engagement)")
            .fetch_one(&e.app.db).await.unwrap()
    };
    let before = counts().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let (mut watching, _) = watch(e, address).await;

    // Only Twitch's signature gets in; the verification challenge is echoed.
    assert_eq!(
        webhook(e, "notification", chat("m0", "tw-v", "forged"), false)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, challenge) = webhook(e, "webhook_callback_verification", json!({"challenge":"pogchamp-123","subscription":{"condition":{"broadcaster_user_id":"tw-owner"}}}), true).await;
    assert_eq!(
        (status, challenge.as_str()),
        (StatusCode::OK, "pogchamp-123")
    );

    // A message shows with its platform badge data, once, and counts toward nothing.
    assert_eq!(
        webhook(
            e,
            "notification",
            chat("m1", "tw-v", "hello from twitch"),
            true
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    // Viewers with chat open get it as it arrives, not only after a reload.
    let live = super::chat::next_json(&mut watching).await;
    assert_eq!(
        (
            &live["type"],
            &live["message"]["body"],
            &live["message"]["outside"]["platform"]
        ),
        (
            &json!("message"),
            &json!("hello from twitch"),
            &json!("twitch")
        ),
        "{live}"
    );
    assert_eq!(
        webhook(
            e,
            "notification",
            chat("m1", "tw-v", "hello from twitch"),
            true
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let shown = outside(e).await;
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(
        (
            &shown[0]["body"],
            &shown[0]["outside"]["platform"],
            &shown[0]["outside"]["role"],
            &shown[0]["author"]["username"]
        ),
        (
            &json!("hello from twitch"),
            &json!("twitch"),
            &json!("moderator"),
            &Value::Null
        )
    );
    assert_eq!(
        shown[0]["outside"]["url"],
        "https://www.twitch.tv/tw-v_login"
    );
    assert_eq!(
        counts().await,
        before,
        "outside messages aren't S.V.E.R chat or Valor"
    );
    let status: String =
        sqlx::query_scalar("SELECT status FROM linked_chat_accounts WHERE owner_id='stream-owner'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(status, "connected");

    // The channel's banned words apply to how outside messages are shown.
    e.sql("INSERT INTO chat_settings(channel_id,banned_words) VALUES('stream-owner',ARRAY['zonkword']) ON CONFLICT(channel_id) DO UPDATE SET banned_words=ARRAY['zonkword']").await;
    webhook(
        e,
        "notification",
        chat("m2", "tw-v", "a ZONKWORD here"),
        true,
    )
    .await;
    assert_eq!(outside(e).await.len(), 1);
    e.sql("UPDATE chat_settings SET banned_words='{}' WHERE channel_id='stream-owner'")
        .await;

    // A delete on Twitch removes it here; moderators hide messages and mute senders on S.V.E.R only.
    webhook(e, "notification", json!({"subscription":{"type":"channel.chat.message_delete","condition":{"broadcaster_user_id":"tw-owner"}},"event":{"message_id":"m1"}}), true).await;
    assert!(outside(e).await.is_empty());
    assert_eq!(
        super::chat::next_json(&mut watching).await,
        json!({"type":"delete","id":"twitch:m1"})
    );
    drop(watching);
    webhook(e, "notification", chat("m3", "tw-x", "first"), true).await;
    webhook(e, "notification", chat("m4", "tw-y", "second"), true).await;
    // A hide reaches viewers who loaded the message with their chat, not only later arrivals.
    let (mut late, snapshot) = watch(e, address).await;
    assert!(
        snapshot["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == "twitch:m3"),
        "{snapshot}"
    );
    e.call(
        "POST",
        "/api/channels/Streamer/chat/outside/twitch:m3/hide",
        Value::Null,
    )
    .await;
    assert_eq!(
        super::chat::next_json(&mut late).await,
        json!({"type":"delete","id":"twitch:m3"})
    );
    drop(late);
    server.abort();
    let left = outside(e).await;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["id"], "twitch:m4");
    e.call(
        "POST",
        "/api/channels/Streamer/chat/outside-mutes",
        json!({"platform":"twitch","sender_id":"tw-y","permanent":true}),
    )
    .await;
    assert!(outside(e).await.is_empty(), "a mute hides what they said");
    webhook(e, "notification", chat("m5", "tw-y", "still here?"), true).await;
    assert!(outside(e).await.is_empty(), "and what they say next");
    let mine = e.call("GET", "/api/me/linked-chat", Value::Null).await;
    assert_eq!(mine["accounts"][0]["handle"], "streamer_tw");
    assert_eq!(mine["mutes"][0]["name"], "Name tw-y");
    assert!(
        !mine.to_string().contains("synthetic-access"),
        "tokens are never returned"
    );
    e.call(
        "DELETE",
        "/api/channels/Streamer/chat/outside-mutes/twitch/tw-y",
        Value::Null,
    )
    .await;
    webhook(e, "notification", chat("m6", "tw-y", "back"), true).await;
    assert_eq!(outside(e).await.len(), 1);

    // A ban or timeout on Twitch clears that chatter's messages here too, and /clear clears all.
    webhook(e, "notification", chat("m8", "tw-z", "spam"), true).await;
    webhook(e, "notification", chat("m9", "tw-z", "more spam"), true).await;
    webhook(e, "notification", chat("m10", "tw-w", "fine"), true).await;
    assert_eq!(outside(e).await.len(), 4);
    let clear = |kind: &str, event: Value| json!({"subscription":{"type":kind,"condition":{"broadcaster_user_id":"tw-owner","user_id":"tw-owner"}},"event":event});
    webhook(
        e,
        "notification",
        clear(
            "channel.chat.clear_user_messages",
            json!({"broadcaster_user_id":"tw-owner","target_user_id":"tw-z","target_user_login":"tw-z_login","target_user_name":"Name tw-z"}),
        ),
        true,
    )
    .await;
    let left: Vec<String> = outside(e)
        .await
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(left, ["twitch:m6", "twitch:m10"]);
    webhook(
        e,
        "notification",
        clear(
            "channel.chat.clear",
            json!({"broadcaster_user_id":"tw-owner"}),
        ),
        true,
    )
    .await;
    assert!(outside(e).await.is_empty(), "a clear there clears here");

    // Twitch giving up on our endpoint (our own outage) only needs subscribing again; access
    // removed on Twitch needs linking again.
    let account = || async {
        sqlx::query_as::<_, (String, Vec<String>)>(
            "SELECT status,subscriptions FROM linked_chat_accounts WHERE owner_id='stream-owner'",
        )
        .fetch_one(&e.app.db)
        .await
        .unwrap()
    };
    let revoke = |id: &str, why: &str| json!({"subscription":{"id":id,"status":why,"type":"channel.chat.message","condition":{"broadcaster_user_id":"tw-owner","user_id":"tw-owner"}}});
    e.sql("UPDATE linked_chat_accounts SET status='connected',subscriptions=ARRAY['sub-msg','sub-del'] WHERE owner_id='stream-owner'").await;
    assert_eq!(
        webhook(
            e,
            "revocation",
            revoke("sub-msg", "notification_failures_exceeded"),
            true
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(account().await, ("reconnecting".into(), vec![]));
    e.sql("UPDATE linked_chat_accounts SET status='connected',subscriptions=ARRAY['sub-new'] WHERE owner_id='stream-owner'").await;
    webhook(
        e,
        "revocation",
        revoke("sub-del", "notification_failures_exceeded"),
        true,
    )
    .await;
    assert_eq!(
        account().await,
        ("connected".into(), vec!["sub-new".into()]),
        "a late notice for a replaced subscription changes nothing"
    );
    webhook(
        e,
        "revocation",
        revoke("sub-new", "authorization_revoked"),
        true,
    )
    .await;
    assert_eq!(account().await, ("revoked".into(), vec![]));

    // Unlinking deletes the tokens; messages for an unlinked account are ignored.
    e.call("DELETE", "/api/me/linked-chat/twitch", Value::Null)
        .await;
    let linked: i64 = sqlx::query_scalar("SELECT count(*) FROM linked_chat_accounts")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(linked, 0);
    webhook(e, "notification", chat("m7", "tw-v", "anyone?"), true).await;
    assert!(outside(e).await.is_empty());

    // After a stream, Studio compares S.V.E.R chat with each linked platform's chat; the
    // streamer's own messages and the bot's lines don't count.
    e.sql("DELETE FROM outside_chat_messages").await;
    e.sql("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES('lc-fan','lc-fan@example.test','LcFan',true,'1990-01-01') ON CONFLICT DO NOTHING").await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,ended_at) VALUES('lc-b','stream-owner','lc-b',1,'ENDED','s','v','c',now()-interval '3 hours',now()-interval '3 hours',now()-interval '3 hours',now()-interval '2 hours')").await;
    e.sql("INSERT INTO chat_messages(id,channel_id,author_id,body,created_at) VALUES('lc-m1','stream-owner','lc-fan','hi',now()-interval '2 hours 30 minutes'),('lc-m2','stream-owner','lc-fan','gg',now()-interval '2 hours 20 minutes'),('lc-m3','stream-owner','stream-owner','thanks',now()-interval '2 hours 10 minutes')").await;
    e.sql("INSERT INTO outside_chat_messages(id,seq,channel_id,platform,sender_id,sender_name,sender_login,sender_role,body,created_at) VALUES
        ('lc-o1',nextval(pg_get_serial_sequence('chat_messages','seq')),'stream-owner','twitch','t1','A','a',NULL,'yo',now()-interval '2 hours 30 minutes'),
        ('lc-o2',nextval(pg_get_serial_sequence('chat_messages','seq')),'stream-owner','twitch','t2','B','b',NULL,'yo',now()-interval '2 hours 25 minutes'),
        ('lc-o3',nextval(pg_get_serial_sequence('chat_messages','seq')),'stream-owner','twitch','t0','Me','me','broadcaster','hey',now()-interval '2 hours 20 minutes'),
        ('lc-o4',nextval(pg_get_serial_sequence('chat_messages','seq')),'stream-owner','bot','bot','PYRE','pyre',NULL,'Welcome',now()-interval '2 hours 15 minutes')").await;
    let mine = e.call("GET", "/api/me/linked-chat", Value::Null).await;
    let stream = mine["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["broadcast"] == "lc-b")
        .unwrap()
        .clone();
    assert_eq!(
        (&stream["sver"]["messages"], &stream["sver"]["chatters"]),
        (&json!(2), &json!(1)),
        "{stream}"
    );
    assert_eq!(
        stream["linked"],
        json!({"twitch": {"messages": 2, "chatters": 2}})
    );

    // Outside messages and bot lines are kept 7 days; a mute for an ended stream is gone from
    // Studio, then from the table.
    e.sql("INSERT INTO outside_chat_mutes(channel_id,platform,sender_id,broadcast_id) VALUES('stream-owner','twitch','tw-old','lc-b')").await;
    let mine = e.call("GET", "/api/me/linked-chat", Value::Null).await;
    assert!(
        mine["mutes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["sender_id"] != "tw-old"),
        "{mine}"
    );
    e.sql("UPDATE outside_chat_messages SET expires_at=now()-interval '1 second' WHERE id IN ('lc-o1','lc-o4')").await;
    sver::linked_chat::expire(&e.app).await.unwrap();
    let kept: Vec<String> = sqlx::query_scalar("SELECT id FROM outside_chat_messages ORDER BY id")
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    assert_eq!(kept, ["lc-o2", "lc-o3"]);
    let mutes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outside_chat_mutes WHERE sender_id='tw-old'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(mutes, 0);
    e.sql("DELETE FROM chat_messages WHERE id LIKE 'lc-m%'")
        .await;
    e.sql("DELETE FROM outside_chat_messages").await;
    e.sql("DELETE FROM outside_chat_mutes").await;
}
