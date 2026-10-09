//! Direct messages (docs/COMMUNITY.md "Direct messages"): who can message whom, sealed bodies,
//! unread, blocks, under-18 rules, reports with a sealed and audited excerpt, and expiry.
use super::chat::{call, person};
use super::*;

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
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('dm-a','dm-b'),('dm-b','dm-a')")
        .await;
    let (status, sent) = send(&a, "DmBravo", "hello there, secret words").await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let stored: String = sqlx::query_scalar("SELECT body_sealed FROM dm_messages LIMIT 1")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(!stored.contains("secret"), "bodies are sealed at rest");
    let (_, unread) = call(e, "GET", "/api/dms/unread", Some(&b), Value::Null).await;
    assert_eq!(unread["unread"], 1);
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
        1
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
}
