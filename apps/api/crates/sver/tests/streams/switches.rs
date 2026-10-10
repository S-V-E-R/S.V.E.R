//! Emergency switches and the site banner (docs/ADMIN.md "Operations"): staff only, a note on every
//! change, audited; a switched-off feature refuses with its message and comes back on.
use super::bans::staff;
use super::chat::{call, person};
use super::*;

pub async fn exercise(e: &Env) {
    let admin = staff(e, "sw-staff", "SwStaff").await;
    let fan = person(e, "sw-fan", "SwFan", true).await;
    assert_eq!(
        call(e, "GET", "/api/admin/switches", Some(&fan), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let flip = |off: bool, note: Value| {
        call(
            e,
            "PUT",
            "/api/admin/switches/dms",
            Some(&admin),
            json!({"off": off, "note": note}),
        )
    };
    assert_eq!(
        flip(true, Value::Null).await.0,
        StatusCode::BAD_REQUEST,
        "a note is required"
    );
    let (status, view) = flip(true, json!("Spam wave")).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let (_, site) = call(e, "GET", "/api/site", None, Value::Null).await;
    assert_eq!(site["paused"][0]["name"], "dms");
    let (status, refused) = call(
        e,
        "POST",
        "/api/dms/streamer",
        Some(&fan),
        json!({"id": uuid::Uuid::new_v4().to_string(), "body": "hi"}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Direct messages"),
        "{refused}"
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action='switch_off' AND target_id='dms' AND note='Spam wave'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(audited, 1);
    assert_eq!(flip(false, json!("Over")).await.0, StatusCode::OK);
    assert_eq!(
        call(e, "GET", "/api/site", None, Value::Null).await.1["paused"],
        json!([])
    );

    // Sign-ups: no new account while it's off.
    call(
        e,
        "PUT",
        "/api/admin/switches/signups",
        Some(&admin),
        json!({"off": true, "note": "Bot wave"}),
    )
    .await;
    let mut db = e.app.db.acquire().await.unwrap();
    let refused = sver::auth::create_user(
        &mut db,
        "sw-new@example.test",
        "SwNew",
        chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
        None,
        false,
    )
    .await;
    assert_eq!(
        refused.err().map(|e| e.0),
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    call(
        e,
        "PUT",
        "/api/admin/switches/signups",
        Some(&admin),
        json!({"off": false, "note": "Over"}),
    )
    .await;

    // The banner: one message for every page, with an optional end.
    let banner = |body: Value| call(e, "PUT", "/api/admin/banner", Some(&admin), body);
    assert_eq!(
        banner(
            json!({"message": "Maintenance at 10", "ends_at": "2000-01-01T00:00:00Z", "note": "x"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        banner(json!({"message": "Maintenance at 10", "note": "Planned"}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "GET", "/api/site", None, Value::Null).await.1["banner"]["message"],
        "Maintenance at 10"
    );
    assert_eq!(
        banner(json!({"message": null, "note": "Done"})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "GET", "/api/site", None, Value::Null).await.1["banner"],
        Value::Null
    );
    drop(db);

    // Home, jobs and the audit log.
    let (status, home) = call(e, "GET", "/api/admin/home", Some(&admin), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{home}");
    assert!(home["counts"]["reports"].is_number() && home["take_down"]["open"].is_number());
    assert_eq!(
        call(e, "GET", "/api/admin/jobs", Some(&fan), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let id: i64 = sqlx::query_scalar("INSERT INTO outbox(channel_id,kind,payload,attempts,available_at,error) VALUES('stream-owner','webhook','{}',8,'infinity','The webhook answered HTTP 500.') RETURNING id")
        .fetch_one(&e.app.db).await.unwrap();
    let (_, jobs) = call(e, "GET", "/api/admin/jobs", Some(&admin), Value::Null).await;
    let item = jobs["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"].as_str() == Some(id.to_string().as_str()))
        .cloned()
        .unwrap();
    assert_eq!(
        (item["queue"].as_str(), item["gave_up"].as_bool()),
        (Some("board_webhook"), Some(true))
    );
    let retry = format!("/api/admin/jobs/board_webhook/{id}/retry");
    assert_eq!(
        call(e, "POST", &retry, Some(&admin), json!({})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            &retry,
            Some(&admin),
            json!({"note": "Endpoint fixed"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let due: bool = sqlx::query_scalar("SELECT available_at<=now() FROM outbox WHERE id=$1")
        .bind(id)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(due, "due again, for the worker to run once");
    let (_, found) = call(
        e,
        "GET",
        "/api/admin/audit?actor=SwStaff&action=job_retry",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(found["actions"].as_array().unwrap().len(), 1);
    assert_eq!(
        found["actions"][0]["target_id"],
        format!("board_webhook:{id}")
    );
    e.sql("DELETE FROM outbox WHERE channel_id='stream-owner' AND payload='{}'")
        .await;
    for statement in [
        "DELETE FROM feature_switches",
        "DELETE FROM moderation_actions WHERE action LIKE 'switch_%' OR action LIKE 'banner_%' OR action='job_retry'",
        "DELETE FROM staff_roles WHERE user_id='sw-staff'",
    ] {
        e.sql(statement).await;
    }
}
