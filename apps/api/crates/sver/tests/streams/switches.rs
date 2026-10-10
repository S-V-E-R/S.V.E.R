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
    for statement in [
        "DELETE FROM feature_switches",
        "DELETE FROM moderation_actions WHERE action LIKE 'switch_%' OR action LIKE 'banner_%'",
        "DELETE FROM staff_roles WHERE user_id='sw-staff'",
    ] {
        e.sql(statement).await;
    }
}
