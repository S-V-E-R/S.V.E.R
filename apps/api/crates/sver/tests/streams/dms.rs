//! Direct messages (docs/COMMUNITY.md "Direct messages"): who can message whom, sealed bodies,
//! unread, blocks, under-18 rules, reports with a sealed and audited excerpt, and expiry.
use super::chat::{call, person};
use super::*;
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

pub async fn exercise(e: &Env) {
    let a = person(e, "dm-a", "DmAlpha", true).await;
    let b = person(e, "dm-b", "DmBravo", true).await;
    let send = |token: &str, to: &str, body: &str| {
        let (token, to, body) = (token.to_string(), to.to_string(), body.to_string());
        async move {
            e.sql("DELETE FROM rate_limits WHERE key LIKE 'dm:%'").await;
            call(
                e,
                "POST",
                &format!("/api/dms/{to}"),
                Some(&token),
                json!({"body": body}),
            )
            .await
        }
    };

    // Default: people who follow each other.
    let (status, refused) = send(&a, "DmBravo", "hi").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    // The player card and channel page offer Message only when sending would work.
    let can_message = || async {
        let card = call(e, "GET", "/api/users/DmBravo/card", Some(&a), Value::Null)
            .await
            .1;
        let channel = call(e, "GET", "/api/channels/DmBravo", Some(&a), Value::Null)
            .await
            .1;
        (
            card["viewer"]["can_message"].clone(),
            channel["viewer"]["can_message"].clone(),
        )
    };
    assert_eq!(can_message().await, (json!(false), json!(false)));
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('dm-a','dm-b'),('dm-b','dm-a')")
        .await;
    assert_eq!(can_message().await, (json!(true), json!(true)));
    let (status, sent) = send(&a, "DmBravo", "hello there, secret words").await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let stored: String = sqlx::query_scalar("SELECT body_sealed FROM dm_messages LIMIT 1")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(!stored.contains("secret"), "bodies are sealed at rest");
    let (_, unread) = call(e, "GET", "/api/dms/unread", Some(&b), Value::Null).await;
    assert_eq!(unread["unread"], 1);
    // One notification per unread streak, never with the message text.
    let notes = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT payload::text FROM notifications WHERE user_id='dm-b' AND kind='dm'",
        )
        .fetch_all(&e.app.db)
        .await
        .unwrap()
    };
    let first = notes().await;
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(
        !first[0].contains("secret") && first[0].contains("DmAlpha"),
        "{first:?}"
    );
    let (_, list) = call(e, "GET", "/api/dms", Some(&b), Value::Null).await;
    assert_eq!(
        list["conversations"][0]["last"],
        "hello there, secret words"
    );
    let (_, thread) = call(e, "GET", "/api/dms/DmAlpha", Some(&b), Value::Null).await;
    assert_eq!(
        (&thread["messages"][0]["sender"], &thread["can_send"]),
        (&json!("DmAlpha"), &json!(true))
    );
    call(e, "POST", "/api/dms/DmAlpha/read", Some(&b), Value::Null).await;
    assert_eq!(
        call(e, "GET", "/api/dms/unread", Some(&b), Value::Null)
            .await
            .1["unread"],
        0
    );
    // After reading, a new streak of two messages notifies once more.
    send(&a, "DmBravo", "new streak").await;
    send(&a, "DmBravo", "same streak").await;
    assert_eq!(notes().await.len(), 2);
    call(e, "POST", "/api/dms/DmAlpha/read", Some(&b), Value::Null).await;
    assert_eq!(
        send(&a, "DmBravo", &"x".repeat(1001)).await.0,
        StatusCode::BAD_REQUEST
    );

    // The recipient's setting, and blocks (which hide the conversation for both).
    call(
        e,
        "PUT",
        "/api/me/dm-settings",
        Some(&b),
        json!({"policy":"nobody"}),
    )
    .await;
    assert_eq!(
        send(&a, "DmBravo", "still there?").await.0,
        StatusCode::FORBIDDEN
    );
    call(
        e,
        "PUT",
        "/api/me/dm-settings",
        Some(&b),
        json!({"policy":"mutuals"}),
    )
    .await;
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('dm-b','dm-a')")
        .await;
    assert_eq!(send(&a, "DmBravo", "hey").await.0, StatusCode::FORBIDDEN);
    assert!(
        call(e, "GET", "/api/dms", Some(&a), Value::Null).await.1["conversations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        call(e, "GET", "/api/dms/DmBravo", Some(&a), Value::Null)
            .await
            .1["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    e.sql("DELETE FROM user_blocks WHERE blocker_id='dm-b'")
        .await;

    // Under-18: off by default, and always a mutual follow.
    let m = person(e, "dm-m", "DmMinor", true).await;
    e.sql("UPDATE users SET date_of_birth=current_date-interval '15 years' WHERE id='dm-m'")
        .await;
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('dm-a','dm-m'),('dm-m','dm-a')")
        .await;
    assert_eq!(
        call(e, "GET", "/api/me/dm-settings", Some(&m), Value::Null)
            .await
            .1["policy"],
        "nobody"
    );
    assert_eq!(send(&a, "DmMinor", "hi").await.0, StatusCode::FORBIDDEN);
    call(
        e,
        "PUT",
        "/api/me/dm-settings",
        Some(&m),
        json!({"policy":"following"}),
    )
    .await;
    assert_eq!(send(&a, "DmMinor", "hi").await.0, StatusCode::OK);
    e.sql("DELETE FROM follows WHERE follower_id='dm-m'").await;
    assert_eq!(
        send(&a, "DmMinor", "again").await.0,
        StatusCode::FORBIDDEN,
        "no mutual follow"
    );

    // Clearing hides it from your own view only.
    call(e, "DELETE", "/api/dms/DmAlpha", Some(&b), Value::Null).await;
    assert!(
        call(e, "GET", "/api/dms/DmAlpha", Some(&b), Value::Null)
            .await
            .1["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        call(e, "GET", "/api/dms/DmBravo", Some(&a), Value::Null)
            .await
            .1["messages"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "the other side still sees all three messages"
    );

    // A report keeps a sealed excerpt; staff read it only through the audited endpoint.
    let id = sent["message"]["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        e,
        "POST",
        "/api/reports",
        Some(&a),
        json!({"target_type":"dm_message","target_id":id,"reason":"harassment"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "you can't report your own message"
    );
    let (status, filed) = call(
        e,
        "POST",
        "/api/reports",
        Some(&b),
        json!({"target_type":"dm_message","target_id":id,"reason":"harassment"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filed}");
    let snapshot: String =
        sqlx::query_scalar("SELECT snapshot::text FROM reports WHERE target_type='dm_message'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert!(
        !snapshot.contains("secret"),
        "the report's excerpt is sealed"
    );
    let staff = person(e, "dm-staff", "DmStaff", true).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='dm-staff'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='dm-staff'")
        .await;
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('dm-staff','admin')")
        .await;
    let report: String =
        sqlx::query_scalar("SELECT id FROM reports WHERE target_type='dm_message'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        call(
            e,
            "GET",
            &format!("/api/admin/dm-reports/{report}"),
            Some(&b),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, excerpt) = call(
        e,
        "GET",
        &format!("/api/admin/dm-reports/{report}"),
        Some(&staff),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(excerpt["messages"][0]["body"], "hello there, secret words");
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action='dm_read'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(audited, 1);

    // Expiry skips a message an open report holds.
    e.sql("UPDATE dm_messages SET expires_at=now()-interval '1 day'")
        .await;
    sver::dms::tick(&e.app).await.unwrap();
    let left: Vec<String> = sqlx::query_scalar("SELECT id FROM dm_messages")
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    assert_eq!(left, vec![id], "only the reported message is held");
    for statement in [
        "DELETE FROM reports WHERE target_type='dm_message'",
        "DELETE FROM moderation_actions WHERE action='dm_read'",
        "DELETE FROM staff_roles WHERE user_id='dm-staff'",
        "DELETE FROM dm_conversations",
    ] {
        e.sql(statement).await;
    }
    let (c, d) = notify_after_clear(e).await;
    socket_ends_with_session(e, &c, &d).await;
    e.sql("DELETE FROM dm_conversations").await;
    private_mode(e).await;
}

async fn charlie_to_delta(e: &Env, token: &str, body: &str) {
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'dm:%'").await;
    let (status, sent) = call(
        e,
        "POST",
        "/api/dms/DmDelta",
        Some(token),
        json!({"body": body}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
}
/// "Delete for me" while messages are unread doesn't silence the next one, and the notification
/// opens the conversation.
async fn notify_after_clear(e: &Env) -> (String, String) {
    let c = person(e, "dm-c", "DmCharlie", true).await;
    let d = person(e, "dm-d", "DmDelta", true).await;
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('dm-c','dm-d'),('dm-d','dm-c')")
        .await;
    let notes = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT payload::text FROM notifications WHERE user_id='dm-d' AND kind='dm' ORDER BY created_at",
        )
        .fetch_all(&e.app.db)
        .await
        .unwrap()
    };
    charlie_to_delta(e, &c, "one").await;
    assert_eq!(notes().await.len(), 1);
    call(e, "DELETE", "/api/dms/DmCharlie", Some(&d), Value::Null).await;
    charlie_to_delta(e, &c, "two").await;
    let after = notes().await;
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(after[1].contains("/messages?with=DmCharlie"), "{after:?}");
    (c, d)
}
/// An open DM socket ends with its session: after signing out everywhere, nothing more arrives.
async fn socket_ends_with_session(e: &Env, c: &str, d: &str) {
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
    let mut request = format!("ws://{address}/api/dms/socket")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", e.app.config.origin.parse().unwrap());
    request
        .headers_mut()
        .insert("cookie", format!("sver_dev={d}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    // DM sockets listen on their own hub, not the site-wide chat bus.
    e.app.chat.publish(
        "dm:dm-d",
        None,
        1,
        json!({"type": "dm", "with": "DmCharlie", "message": {"body": "from the chat bus"}}),
    );
    charlie_to_delta(e, c, "while signed in").await;
    let live = super::chat::next_json(&mut socket).await;
    assert_eq!(
        (&live["type"], &live["message"]["body"]),
        (&json!("dm"), &json!("while signed in"))
    );
    // The event carries what the conversation list needs, so the page doesn't refetch it.
    assert_eq!(
        (&live["with"], &live["message"]["sender"]),
        (&json!("DmCharlie"), &json!("DmCharlie"))
    );
    assert!(live["message"]["created_at"].is_string(), "{live}");
    e.sql("DELETE FROM sessions WHERE user_id='dm-d'").await;
    charlie_to_delta(e, c, "after signing out").await;
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                assert!(
                    !text.contains("after signing out"),
                    "delivered after sign-out"
                )
            }
            Ok(Some(Ok(Message::Close(frame)))) => {
                assert_eq!(frame.map(|f| u16::from(f.code)), Some(4001));
                break;
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_)) | None) => break,
            Err(_) => panic!("the socket stayed open after sign-out"),
        }
    }
    server.abort();
}

/// Private mode: device keys, envelopes the server can't read, and franking-verified reports.
async fn private_mode(e: &Env) {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    let a = person(e, "dm-pa", "DmPrivA", true).await;
    let b = person(e, "dm-pb", "DmPrivB", true).await;
    e.sql(
        "INSERT INTO follows(follower_id,following_id) VALUES('dm-pa','dm-pb'),('dm-pb','dm-pa')",
    )
    .await;
    let key = |n: u8| B64.encode([&[4u8][..], &[n; 64][..]].concat());
    let private = |token: &str, on: bool| {
        let token = token.to_string();
        async move {
            call(
                e,
                "PUT",
                "/api/dms/DmPrivB/private",
                Some(&token),
                json!({"on": on}),
            )
            .await
            .0
        }
    };
    assert_eq!(
        private(&a, true).await,
        StatusCode::CONFLICT,
        "both need a device key"
    );
    let add = |token: &str, n: u8| {
        let token = token.to_string();
        async move {
            call(
                e,
                "POST",
                "/api/me/dm-devices",
                Some(&token),
                json!({"public_key": key(n)}),
            )
            .await
        }
    };
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/dm-devices",
            Some(&a),
            json!({"public_key": "bad"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let a_dev = add(&a, 1).await.1["id"].as_str().unwrap().to_string();
    let b_dev = add(&b, 2).await.1["id"].as_str().unwrap().to_string();
    e.sql("UPDATE users SET date_of_birth=current_date-interval '15 years' WHERE id='dm-pb'")
        .await;
    assert_eq!(
        private(&a, true).await,
        StatusCode::FORBIDDEN,
        "two adults only"
    );
    e.sql("UPDATE users SET date_of_birth='1990-01-01' WHERE id='dm-pb'")
        .await;
    assert_eq!(private(&a, true).await, StatusCode::OK);
    let say = |body: Value| {
        let token = a.clone();
        async move {
            e.sql("DELETE FROM rate_limits WHERE key LIKE 'dm:%'").await;
            call(e, "POST", "/api/dms/DmPrivB", Some(&token), body).await
        }
    };
    assert_eq!(
        say(json!({"body": "in the clear"})).await.0,
        StatusCode::CONFLICT,
        "a private conversation takes only envelopes"
    );
    let frank = [7u8; 32];
    let commitment = sver::dms::franking_hex(&frank, "secret plan");
    let envelope = |to: &str| {
        json!({"v": 1, "epk": key(3), "iv": B64.encode([1u8; 12]), "ct": B64.encode([9u8; 40]),
            "keys": {to: {"iv": B64.encode([2u8; 12]), "k": B64.encode([5u8; 48])}}})
    };
    assert_eq!(
        say(json!({"envelope": envelope("not-a-device"), "commitment": commitment}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        say(json!({"envelope": envelope(&a_dev), "commitment": commitment}))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "the recipient must be able to read it"
    );
    let (status, sent) = say(json!({"envelope": envelope(&b_dev), "commitment": commitment})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(
        (&sent["message"]["private"], &sent["message"]["body"]),
        (&json!(true), &Value::Null)
    );
    let stored: String = sqlx::query_scalar("SELECT body_sealed FROM dm_messages WHERE id=$1")
        .bind(sent["message"]["id"].as_str().unwrap())
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(stored, "", "the server keeps no readable body");
    let (_, list) = call(e, "GET", "/api/dms", Some(&b), Value::Null).await;
    assert_eq!(list["conversations"][0]["last"], "Private message");
    let (_, thread) = call(e, "GET", "/api/dms/DmPrivA", Some(&b), Value::Null).await;
    assert_eq!(
        (&thread["private"], &thread["messages"][0]["envelope"]["v"]),
        (&json!(true), &json!(1))
    );

    // A message that can't be read or verified can still be reported, without any text.
    let (_, second) = say(json!({"envelope": envelope(&b_dev), "commitment": commitment})).await;
    let (status, _) = call(
        e,
        "POST",
        "/api/reports",
        Some(&b),
        json!({"target_type": "dm_message",
        "target_id": second["message"]["id"], "reason": "harassment"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "metadata-only report");
    // A report carries the recipient's plaintext and is accepted only if it matches the commitment.
    let id = sent["message"]["id"].as_str().unwrap().to_string();
    let report = |body: &str| {
        let (token, id, body) = (b.clone(), id.clone(), body.to_string());
        async move {
            call(
                e,
                "POST",
                "/api/reports",
                Some(&token),
                json!({"target_type": "dm_message", "target_id": id, "reason": "harassment",
                "evidence": {"body": body, "frank": B64.encode(frank)}}),
            )
            .await
        }
    };
    assert_eq!(
        report("something else").await.0,
        StatusCode::BAD_REQUEST,
        "evidence must match"
    );
    let (status, filed) = report("secret plan").await;
    assert_eq!(status, StatusCode::OK, "{filed}");
    let staff = person(e, "dm-pstaff", "DmPrivStaff", true).await;
    for statement in [
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='dm-pstaff'",
        "UPDATE sessions SET mfa_verified=true WHERE user_id='dm-pstaff'",
        "INSERT INTO staff_roles(user_id,role) VALUES('dm-pstaff','admin')",
    ] {
        e.sql(statement).await;
    }
    let report_id: String = sqlx::query_scalar(
        "SELECT id FROM reports WHERE target_type='dm_message' AND target_id=$1",
    )
    .bind(&id)
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    let (_, excerpt) = call(
        e,
        "GET",
        &format!("/api/admin/dm-reports/{report_id}"),
        Some(&staff),
        Value::Null,
    )
    .await;
    assert_eq!(
        (
            &excerpt["messages"][0]["body"],
            &excerpt["messages"][0]["verified"],
            excerpt["messages"].as_array().unwrap().len()
        ),
        (&json!("secret plan"), &json!(true), 1),
        "only the verified message"
    );
    assert_eq!(private(&a, false).await, StatusCode::OK);
    assert_eq!(
        say(json!({"body": "back to normal", "commitment": "ab"}))
            .await
            .0,
        StatusCode::OK
    );
    for statement in [
        "DELETE FROM reports WHERE target_type='dm_message'",
        "DELETE FROM moderation_actions WHERE action='dm_read'",
        "DELETE FROM staff_roles WHERE user_id='dm-pstaff'",
        "DELETE FROM dm_conversations",
    ] {
        e.sql(statement).await;
    }
}
