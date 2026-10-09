//! Linked chat (docs/LINKED_CHAT.md): signed Twitch EventSub webhooks become outside messages in
//! S.V.E.R chat, never counted, hidden by platform deletes, S.V.E.R mutes and banned words.
use super::*;
use hmac::{Hmac, KeyInit, Mac};

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
    webhook(e, "notification", chat("m3", "tw-x", "first"), true).await;
    webhook(e, "notification", chat("m4", "tw-y", "second"), true).await;
    e.call(
        "POST",
        "/api/channels/Streamer/chat/outside/twitch:m3/hide",
        Value::Null,
    )
    .await;
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

    // Unlinking deletes the tokens; messages for an unlinked account are ignored.
    e.call("DELETE", "/api/me/linked-chat/twitch", Value::Null)
        .await;
    let linked: i64 = sqlx::query_scalar("SELECT count(*) FROM linked_chat_accounts")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(linked, 0);
    webhook(e, "notification", chat("m7", "tw-v", "anyone?"), true).await;
    assert_eq!(outside(e).await.len(), 1);
    e.sql("DELETE FROM outside_chat_messages").await;
    e.sql("DELETE FROM outside_chat_mutes").await;
}
