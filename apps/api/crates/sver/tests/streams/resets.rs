//! Staff username resets for impersonation: permissions, the neutral replacement, the held old
//! name (no redirect, not claimable), preserved relationships and what the user sees.
use super::Env;
use super::bans::staff;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn reset(e: &Env, token: &str, name: &str, body: Value) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("/api/admin/users/{name}/username-reset"),
        Some(token),
        body,
    )
    .await
}

pub async fn exercise(e: &Env) {
    let target = person(e, "reset-user", "FakeStreamer", true).await;
    let outsider = person(e, "reset-outsider", "ResetOutsider", true).await;
    let admin = staff(e, "reset-staff", "ResetStaff").await;
    let mut tx = e.app.db.begin().await.unwrap();
    sver::profiles::ensure_profile(&mut tx, "reset-user")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/follows/streamer",
            Some(&target),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );

    // Staff only, a reason is required, and staff accounts can't be reset.
    let reasoned = json!({"reason":"Impersonating another streamer"});
    assert_eq!(
        reset(e, &outsider, "FakeStreamer", reasoned.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        reset(e, &admin, "FakeStreamer", json!({"reason":" "}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        reset(e, &admin, "ResetStaff", reasoned.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        reset(e, &admin, "nobody-here", reasoned.clone()).await.0,
        StatusCode::NOT_FOUND
    );

    let (status, done) = reset(e, &admin, "FakeStreamer", reasoned).await;
    assert_eq!(status, StatusCode::OK);
    let new = done["username"].as_str().unwrap().to_string();
    assert!(
        new.len() == 12 && new.starts_with("user") && new[4..].bytes().all(|b| b.is_ascii_digit()),
        "{new}"
    );

    // The account moved with its relationships; the old name no longer leads anywhere.
    assert_eq!(
        call(e, "GET", &format!("/api/channels/{new}"), None, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "GET", "/api/channels/FakeStreamer", None, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, resolved) = call(
        e,
        "GET",
        "/api/channels/FakeStreamer/resolve",
        None,
        Value::Null,
    )
    .await;
    assert!(
        resolved.get("redirect_to").is_none_or(Value::is_null),
        "no redirect: {resolved}"
    );
    let still_following: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id='reset-user' AND following_id='stream-owner')")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(still_following, "follows are preserved");
    assert_eq!(
        call(e, "GET", "/api/auth/me", Some(&target), Value::Null)
            .await
            .0,
        StatusCode::OK,
        "sessions survive"
    );

    // The impersonating name is held for everyone, without a redirect.
    let (redirect, days): (bool, f64) = sqlx::query_as("SELECT redirect, (extract(epoch FROM released_at-now())/86400)::float8 FROM username_holds WHERE handle_canonical='fakestreamer'")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(!redirect && days > 89.0, "held 90 days without redirect");
    assert!(
        sver::rename::held(&mut e.app.db.acquire().await.unwrap(), "FakeStreamer", None)
            .await
            .unwrap()
    );

    // The user sees the reason on their standing page; staff see it in the audit trail.
    let (_, standing) = call(e, "GET", "/api/me/standing", Some(&target), Value::Null).await;
    assert_eq!(
        standing["username_resets"][0]["reason"],
        "Impersonating another streamer"
    );
    assert_eq!(
        standing["username_resets"][0]["old_username"],
        "FakeStreamer"
    );
    assert_eq!(standing["username_resets"][0]["new_username"], new);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action='username_reset' AND target_id='reset-user'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(audited, 1);

    for statement in [
        "DELETE FROM username_holds WHERE user_id LIKE 'reset-%'",
        "DELETE FROM moderation_actions",
        "DELETE FROM follows",
        "DELETE FROM staff_roles",
    ] {
        e.sql(statement).await;
    }
    e.sql("DELETE FROM users WHERE id LIKE 'reset-%'").await;
}
