//! Staff stream operations (live list, audited stop, categories) and spike protection
//! (the prompt flag, followers-only chat for 10 minutes, who can still chat, ending early).
use super::Env;
use super::bans::staff;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn say(e: &Env, token: &str) -> StatusCode {
    call(
        e,
        "POST",
        "/api/channels/OpsOwner/chat",
        Some(token),
        json!({"id":id(),"body":"hello"}),
    )
    .await
    .0
}

pub async fn exercise(e: &Env) {
    let admin = staff(e, "so-staff", "OpsStaff").await;
    let owner = person(e, "so-owner", "OpsOwner", true).await;
    let newcomer = person(e, "so-new", "OpsNewcomer", true).await;
    let regular = person(e, "so-fan", "OpsRegular", true).await;
    let fresh = person(e, "so-fresh", "OpsFresh", true).await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state,health) VALUES('so-b','so-owner','so-b',1,'LIVE','s','v','c',now(),now(),now(),'skipped','{\"video_codec\":\"H264\",\"b_frames\":true}')").await;

    // The live list: staff only, with counts and health.
    assert_eq!(
        call(e, "GET", "/api/admin/streams", Some(&owner), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, list) = call(e, "GET", "/api/admin/streams", Some(&admin), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let item = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "so-b")
        .unwrap()
        .clone();
    assert_eq!(item["channel"]["username"], "OpsOwner");
    assert_eq!(item["health"]["b_frames"], true);
    assert_eq!(item["counts"]["public"], 0);

    // Spike protection: the prompt appears for the owner while a provisional window is open.
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,provisional_until) VALUES('so-b','b:spike',now()+interval '30 seconds',now()+interval '5 minutes')").await;
    let (_, view) = call(
        e,
        "GET",
        "/api/channels/OpsOwner/chat/moderation",
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(
        (&view["spike"], &view["followers_only_until"]),
        (&json!(true), &Value::Null)
    );
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/channels/OpsOwner/chat/protect",
            Some(&newcomer),
            json!({"on":true})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, on) = call(
        e,
        "PUT",
        "/api/channels/OpsOwner/chat/protect",
        Some(&owner),
        json!({"on":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(on["followers_only_until"].is_string());
    let minutes: f64 = sqlx::query_scalar("SELECT extract(epoch FROM followers_only_until-now())::float8/60 FROM chat_settings WHERE channel_id='so-owner'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(
        (9.0..=10.0).contains(&minutes),
        "always ends after 10 minutes: {minutes}"
    );
    // Only followers of at least 10 minutes (and the channel's roles) can chat meanwhile.
    e.sql("INSERT INTO follows(follower_id,following_id,created_at) VALUES('so-fan','so-owner',now()-interval '1 day'),('so-fresh','so-owner',now())").await;
    assert_eq!(say(e, &newcomer).await, StatusCode::FORBIDDEN);
    assert_eq!(say(e, &fresh).await, StatusCode::FORBIDDEN);
    assert_eq!(say(e, &regular).await, StatusCode::OK);
    assert_eq!(say(e, &owner).await, StatusCode::OK);
    let (_, history) = call(e, "GET", "/api/channels/OpsOwner/chat", None, Value::Null).await;
    assert!(history["followers_only_until"].is_string());
    // Ending early opens chat again, and both changes are logged.
    call(
        e,
        "PUT",
        "/api/channels/OpsOwner/chat/protect",
        Some(&owner),
        json!({"on":false}),
    )
    .await;
    assert_eq!(say(e, &newcomer).await, StatusCode::OK);
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_moderation_log WHERE channel_id='so-owner' AND action LIKE 'followers_only_%'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(logged, 2);

    // Stop: a reason is required; the broadcast ends as revoked and the action is audited.
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/streams/so-b/stop",
            Some(&admin),
            json!({"reason":" "})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/streams/so-b/stop",
            Some(&owner),
            json!({"reason":"no"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, _) = call(
        e,
        "POST",
        "/api/admin/streams/so-b/stop",
        Some(&admin),
        json!({"reason":"Operational stop for testing"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (state, reason): (String, Option<String>) =
        sqlx::query_as("SELECT state,end_reason FROM broadcasts WHERE id='so-b'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("ENDED", Some("revoked"))
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action='stop_stream' AND target_id='so-b' AND actor_id='so-staff'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(audited, 1);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/streams/so-b/stop",
            Some(&admin),
            json!({"reason":"again"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );

    // Categories: add (genre fixed), duplicates and bad genres refused, rename, hide.
    let (status, cats) = call(
        e,
        "POST",
        "/api/admin/categories",
        Some(&admin),
        json!({"name":"Just Chatting","genre":"irl"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        cats["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "just-chatting" && c["genre"] == "irl" && c["active"] == true)
    );
    for body in [
        json!({"name":"Just  chatting!","genre":"irl"}),
        json!({"name":"Cooking","genre":"Food Stuff"}),
    ] {
        let status = call(e, "POST", "/api/admin/categories", Some(&admin), body)
            .await
            .0;
        assert!(
            matches!(status, StatusCode::CONFLICT | StatusCode::BAD_REQUEST),
            "{status}"
        );
    }
    assert_eq!(
        call(
            e,
            "PATCH",
            "/api/admin/categories/just-chatting",
            Some(&admin),
            json!({"name":"Minecraft"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, cats) = call(
        e,
        "PATCH",
        "/api/admin/categories/just-chatting",
        Some(&admin),
        json!({"name":"Chatting","active":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        cats["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "just-chatting" && c["name"] == "Chatting" && c["active"] == false)
    );

    e.sql("DELETE FROM stream_categories WHERE id='just-chatting'")
        .await;
    for statement in [
        "DELETE FROM moderation_actions WHERE actor_id='so-staff'",
        "DELETE FROM staff_roles WHERE user_id='so-staff'",
        "DELETE FROM users WHERE id LIKE 'so-%'",
    ] {
        e.sql(statement).await;
    }
}
