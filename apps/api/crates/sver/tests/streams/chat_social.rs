//! Chat context shares the real HTTP/socket paths and the disposable database.
use super::{
    Env,
    chat::{call, id, next_json, person},
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const CHAT: &str = "/api/channels/streamer/chat";
const PIN: &str = "/api/channels/streamer/chat/pin";

async fn say(e: &Env, token: &str, body: Value) -> (StatusCode, Value) {
    // Rate windows have their own coverage; context assertions must not depend on wall time.
    e.sql("DELETE FROM rate_limits").await;
    call(e, "POST", CHAT, Some(token), body).await
}
async fn pin(e: &Env, token: Option<&str>, message: Option<&str>) -> (StatusCode, Value) {
    call(
        e,
        "PUT",
        PIN,
        token,
        json!({"message_id":message,"reason":"Synthetic pin review"}),
    )
    .await
}
fn message<'a>(history: &'a Value, id: &str) -> &'a Value {
    history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == id)
        .unwrap()
}

pub async fn exercise(e: &Env) {
    let alice = person(e, "social-alice", "SocialAlice", true).await;
    let bob = person(e, "social-bob", "SocialBob", true).await;
    let moderator = person(e, "social-mod", "SocialMod", true).await;
    let staff = person(e, "social-staff", "SocialStaff", true).await;
    e.sql("INSERT INTO channel_moderators(channel_id,user_id) VALUES('stream-owner','social-mod')")
        .await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='social-staff'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='social-staff'")
        .await;
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('social-staff','admin')")
        .await;
    e.sql("DELETE FROM chat_settings").await;

    let original = id();
    let body = format!(
        "@sOcIaLbOb @NoSuchPerson hi@SocialMod @@SocialMod <script>alert(1)</script>\n{}",
        "🦀".repeat(90)
    );
    let (status, sent) = say(
        e,
        &alice,
        json!({"id":original,"body":body,"role":"staff","mentions":["SocialMod"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["message"]["mentions"], json!(["SocialBob"]));
    assert!(
        sent["message"]["role"].is_null(),
        "Clients cannot grant badges"
    );
    assert_eq!(sent["message"]["body"], body, "Text is not parsed as HTML");

    for (token, expected) in [
        (&e.cookie, "owner"),
        (&moderator, "moderator"),
        (&staff, "staff"),
    ] {
        let (status, sent) = say(e, token, json!({"id":id(),"body":"role"})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(sent["message"]["role"], expected);
    }
    let reply = id();
    let (status, sent) = say(
        e,
        &bob,
        json!({"id":reply,"body":"reply","reply_to":original}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let quote: String = body.replace('\n', " ").chars().take(80).collect();
    assert_eq!(sent["message"]["reply"]["body"], quote);
    assert_eq!(sent["message"]["reply"]["username"], "SocialAlice");
    assert!(sent["message"]["reply"].get("author_id").is_none());

    // A real message in another channel and a nonexistent ID are both refused.
    let foreign = id();
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/SocialAlice/chat",
            Some(&alice),
            json!({"id":foreign,"body":"foreign"})
        )
        .await
        .0,
        StatusCode::OK
    );
    for target in [&foreign, &id()] {
        assert_eq!(
            say(
                e,
                &bob,
                json!({"id":id(),"body":"wrong channel","reply_to":target})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            pin(e, Some(&moderator), Some(target)).await.0,
            StatusCode::NOT_FOUND
        );
    }

    assert_eq!(
        pin(e, None, Some(&original)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        pin(e, Some(&alice), Some(&original)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        pin(e, Some(&staff), Some(&original)).await.0,
        StatusCode::FORBIDDEN,
        "Staff are not appointed channel moderators"
    );
    assert_eq!(
        call(
            e,
            "PUT",
            PIN,
            Some(&moderator),
            json!({"message_id":original,"reason":""})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        pin(e, Some(&moderator), Some(&original)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "GET", CHAT, None, Value::Null).await.1["pinned"]["id"],
        original
    );

    // A viewer's blocks hide pinned messages and quote bodies on history, retry and live fanout.
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('social-bob','social-alice')")
        .await;
    let blocked = call(e, "GET", CHAT, Some(&bob), Value::Null).await.1;
    assert!(blocked["pinned"].is_null());
    assert!(message(&blocked, &reply)["reply"]["body"].is_null());
    assert_eq!(
        say(
            e,
            &bob,
            json!({"id":id(),"body":"blocked reply","reply_to":original})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let retried = say(e, &bob, json!({"id":reply,"body":"retry"})).await.1;
    assert!(retried["message"]["reply"]["body"].is_null());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let connect = |token: Option<String>| {
        let origin = e.app.config.origin.clone();
        async move {
            let mut req = format!("ws://{address}/api/chat/ws?channel=streamer")
                .into_client_request()
                .unwrap();
            req.headers_mut().insert("origin", origin.parse().unwrap());
            if let Some(token) = token {
                req.headers_mut()
                    .insert("cookie", format!("sver_dev={token}").parse().unwrap());
            }
            tokio_tungstenite::connect_async(req).await.unwrap().0
        }
    };
    let mut guest = connect(None).await;
    let mut watcher = connect(Some(bob.clone())).await;
    assert_eq!(
        next_json(&mut guest).await["pinned"]["id"],
        original,
        "Pin survives a new connection"
    );
    assert!(next_json(&mut watcher).await["pinned"].is_null());
    assert_eq!(
        say(
            e,
            &moderator,
            json!({"id":id(),"body":"live reply","reply_to":original})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        next_json(&mut guest).await["message"]["reply"]["body"],
        quote
    );
    assert!(next_json(&mut watcher).await["message"]["reply"]["body"].is_null());
    assert_eq!(
        pin(e, Some(&e.cookie), Some(&reply)).await.0,
        StatusCode::OK
    );
    let event = next_json(&mut guest).await;
    assert_eq!(event["type"], "pin");
    assert_eq!(event["pinned"]["id"], reply);
    assert!(next_json(&mut watcher).await["pinned"]["reply"]["body"].is_null());

    // Deleting the original removes its quote even from a still-pinned reply.
    assert_eq!(
        call(
            e,
            "DELETE",
            &format!("{CHAT}/messages/{original}"),
            Some(&e.cookie),
            json!({"reason":"Synthetic removal"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(next_json(&mut guest).await["type"], "delete");
    assert_eq!(next_json(&mut watcher).await["type"], "delete");
    let history = call(e, "GET", CHAT, None, Value::Null).await.1;
    assert!(message(&history, &reply)["reply"]["body"].is_null());
    assert!(history["pinned"]["reply"]["body"].is_null());
    // Simulate a writer whose fanout was delayed until after the original was deleted.
    e.app.chat.publish(
        "stream-owner",
        Some("social-bob"),
        0,
        json!({"type":"message","message":sent["message"]}),
    );
    assert!(next_json(&mut guest).await["message"]["reply"]["body"].is_null());
    assert!(next_json(&mut watcher).await["message"]["reply"]["body"].is_null());
    assert_eq!(
        pin(e, Some(&moderator), Some(&original)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        say(e, &alice, json!({"id":original,"body":"retry deleted"}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        say(
            e,
            &moderator,
            json!({"id":id(),"body":"reply deleted","reply_to":original})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // Ordinary expiry leaves a tombstone; pins stay until explicitly removed.
    e.sql("UPDATE chat_messages SET expires_at=now()-interval '1 second'")
        .await;
    sver::chat::expire(&e.app).await.unwrap();
    assert_eq!(
        call(e, "GET", CHAT, None, Value::Null).await.1["pinned"]["id"],
        reply
    );
    assert_eq!(pin(e, Some(&moderator), None).await.0, StatusCode::OK);
    sver::chat::expire(&e.app).await.unwrap();
    assert_eq!(
        call(e, "GET", CHAT, None, Value::Null).await.1["messages"],
        json!([])
    );
    server.abort();

    // The database clears the pin for any removal path, including Take It Down.
    let fresh = id();
    assert_eq!(
        say(e, &e.cookie, json!({"id":fresh,"body":"fresh"}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        pin(e, Some(&moderator), Some(&fresh)).await.0,
        StatusCode::OK
    );
    sqlx::query("UPDATE chat_messages SET deleted_at=now() WHERE id=$1")
        .bind(&fresh)
        .execute(&e.app.db)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_pins")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let restored = id();
    assert_eq!(
        say(e, &e.cookie, json!({"id":restored,"body":"another"}))
            .await
            .0,
        StatusCode::OK
    );
    let other = id();
    assert_eq!(
        say(e, &e.cookie, json!({"id":other,"body":"replacement"}))
            .await
            .0,
        StatusCode::OK
    );
    let (one, two) = tokio::join!(
        pin(e, Some(&moderator), Some(&restored)),
        pin(e, Some(&e.cookie), Some(&other))
    );
    assert_eq!(one.0, StatusCode::OK);
    assert_eq!(two.0, StatusCode::OK);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_pins")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(count, 1, "Concurrent replacements leave one pin");
    e.sql("DELETE FROM channel_moderators WHERE user_id='social-mod'")
        .await;
    assert_eq!(
        pin(e, Some(&moderator), Some(&restored)).await.0,
        StatusCode::FORBIDDEN
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_moderation_log WHERE action IN ('pin_message','unpin_message') AND reason='Synthetic pin review'").fetch_one(&e.app.db).await.unwrap();
    assert_eq!(count, 6, "Only authorized committed changes are audited");
    e.sql("DELETE FROM chat_messages").await;
    e.sql("DELETE FROM users WHERE id LIKE 'social-%'").await;
    e.sql("DELETE FROM rate_limits").await;
}
