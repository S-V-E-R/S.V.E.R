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
        json!({"name":"Music Workshop","genre":"music","note":"Catalog test"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        cats["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "music-workshop" && c["genre"] == "music" && c["active"] == true)
    );
    for body in [
        json!({"name":"Music  Workshop!","genre":"music","note":"Catalog test"}),
        json!({"name":"Cooking","genre":"Food Stuff","note":"Catalog test"}),
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
            "/api/admin/categories/music-workshop",
            Some(&admin),
            json!({"name":"Minecraft","note":"Catalog test"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, cats) = call(
        e,
        "PATCH",
        "/api/admin/categories/music-workshop",
        Some(&admin),
        json!({"name":"Music session","active":false,"note":"Catalog test"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        cats["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "music-workshop"
                && c["name"] == "Music session"
                && c["active"] == false)
    );

    e.sql("INSERT INTO game_catalog(source_id,name,genres) VALUES('Q900000001','Synthetic Unmapped',ARRAY['action-adventure game']),('Q900000002','Synthetic Dismissed','{}')").await;
    assert_eq!(
        call(
            e,
            "GET",
            "/api/admin/game-catalog",
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, queue) = call(
        e,
        "GET",
        "/api/admin/game-catalog",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(queue["status"]["pending"], 2);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/game-catalog/Q900000001",
            Some(&owner),
            json!({"genre":"mmos_rpgs","note":"No access"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/game-catalog/Q900000001",
            Some(&admin),
            json!({"genre":"invented","note":"Invalid genre"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(call(e,"POST","/api/admin/game-catalog/Q900000001",Some(&admin),json!({"genre":"mmos_rpgs","name":"Synthetic Unmapped (2026)","note":"Published genre reviewed"})).await.0,StatusCode::OK);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/game-catalog/Q900000002",
            Some(&admin),
            json!({"genre":null,"note":"Not appropriate for this catalog"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let row: (String, String) =
        sqlx::query_as("SELECT name,genre FROM stream_categories WHERE id='wikidata-q900000001'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        row,
        ("Synthetic Unmapped (2026)".into(), "mmos_rpgs".into())
    );
    let (_, queue) = call(
        e,
        "GET",
        "/api/admin/game-catalog",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(queue["status"]["pending"], 0);
    e.sql("DELETE FROM game_catalog WHERE source_id IN ('Q900000001','Q900000002')")
        .await;
    e.sql("DELETE FROM stream_categories WHERE id='wikidata-q900000001'")
        .await;
    e.sql("DELETE FROM stream_categories WHERE id='music-workshop'")
        .await;
    for statement in [
        "DELETE FROM moderation_actions WHERE actor_id='so-staff'",
        "DELETE FROM staff_roles WHERE user_id='so-staff'",
        "DELETE FROM users WHERE id LIKE 'so-%'",
    ] {
        e.sql(statement).await;
    }
}
