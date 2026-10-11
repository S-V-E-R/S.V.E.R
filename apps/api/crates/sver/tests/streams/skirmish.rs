//! Skirmish Hour scheduling (docs/FACTIONS.md "Skirmish Hour"): staff only, on the hour, an hour
//! or more ahead, no overlaps, cancellable before it starts, audited, and shown on the war map.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

pub async fn exercise(e: &Env) {
    let staff = person(e, "sk-staff", "SkStaff", true).await;
    let viewer = person(e, "sk-viewer", "SkViewer", true).await;
    for statement in [
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='sk-staff'",
        "UPDATE sessions SET mfa_verified=true WHERE user_id='sk-staff'",
        "INSERT INTO staff_roles(user_id,role) VALUES('sk-staff','admin')",
    ] {
        e.sql(statement).await;
    }
    let hour = |h: i64| {
        let now = chrono::Utc::now().timestamp();
        chrono::DateTime::from_timestamp((now / 3600 + h) * 3600, 0).unwrap()
    };
    let schedule = |token: &str, at: Value| {
        let token = token.to_string();
        async move {
            call(
                e,
                "POST",
                "/api/admin/skirmishes",
                Some(&token),
                json!({"starts_at": at}),
            )
            .await
        }
    };
    assert_eq!(
        schedule(&viewer, json!(hour(3))).await.0,
        StatusCode::NOT_FOUND,
        "staff only"
    );
    assert_eq!(
        schedule(&staff, json!(hour(3) + chrono::Duration::minutes(30)))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "on the hour"
    );
    assert_eq!(
        schedule(&staff, json!(hour(0))).await.0,
        StatusCode::BAD_REQUEST,
        "at least an hour ahead"
    );
    let (status, made) = schedule(&staff, json!(hour(3))).await;
    assert_eq!(status, StatusCode::OK, "{made}");
    assert_eq!(
        schedule(&staff, json!(hour(3))).await.0,
        StatusCode::CONFLICT,
        "no overlaps"
    );
    // A Raid Night is its own kind: it may share the hour with a Skirmish Hour.
    let (status, night) = call(
        e,
        "POST",
        "/api/admin/skirmishes",
        Some(&staff),
        json!({"starts_at": hour(3), "kind": "raid_night"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{night}");
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/skirmishes",
            Some(&staff),
            json!({"starts_at": hour(5), "kind": "raid_night"})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "Raid Nights last three hours"
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/skirmishes",
            Some(&staff),
            json!({"starts_at": hour(8), "kind": "parade"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, war) = call(e, "GET", "/api/factions/war", None, Value::Null).await;
    if !war["season"].is_null() {
        assert_eq!(war["skirmish"]["multiplier"], 2);
        assert!(war["skirmish"]["next"]["starts_at"].is_string(), "{war}");
        assert!(
            war["skirmish"]["raid_night"]["next"]["starts_at"].is_string(),
            "{war}"
        );
    }
    let id = made["items"][0]["id"].as_str().unwrap().to_string();
    let night = night["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "raid_night")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        e,
        "DELETE",
        &format!("/api/admin/skirmishes/{night}"),
        Some(&staff),
        Value::Null,
    )
    .await;
    let (status, cancelled) = call(
        e,
        "DELETE",
        &format!("/api/admin/skirmishes/{id}"),
        Some(&staff),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cancelled["items"][0]["cancelled_at"].is_string());
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM moderation_actions WHERE action IN ('skirmish_schedule','skirmish_cancel')",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(audited, 4);
}
