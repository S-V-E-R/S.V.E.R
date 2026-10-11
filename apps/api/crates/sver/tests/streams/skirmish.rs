//! Faction event scheduling (docs/FACTIONS.md "Skirmish Hour", "Raid Night"): staff only, on the
//! hour, an hour or more ahead, no overlaps, cancellable before it starts, audited, on the war map.
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
    let event = |token: &str, at: Value, kind: &str| {
        let (token, kind) = (token.to_string(), kind.to_string());
        async move {
            call(
                e,
                "POST",
                "/api/admin/skirmishes",
                Some(&token),
                json!({"starts_at": at, "kind": kind}),
            )
            .await
        }
    };
    let schedule = |token: &str, at: Value| event(token, at, "skirmish");
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
    assert_eq!(
        event(&staff, json!(hour(2)), "raid_night").await.0,
        StatusCode::CONFLICT,
        "a three-hour Raid Night can't cover a Skirmish Hour"
    );
    assert_eq!(
        event(&staff, json!(hour(6)), "parade").await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, night) = event(&staff, json!(hour(4)), "raid_night").await;
    assert_eq!(status, StatusCode::OK, "{night}");
    assert_eq!(night["items"][0]["kind"], "raid_night");
    let ends = night["items"][0]["ends_at"].as_str().unwrap();
    assert_eq!(chrono::DateTime::parse_from_rfc3339(ends).unwrap(), hour(7));
    let (_, war) = call(e, "GET", "/api/factions/war", None, Value::Null).await;
    if !war["season"].is_null() {
        assert_eq!(war["skirmish"]["multiplier"], 2);
        assert!(war["skirmish"]["next"]["starts_at"].is_string(), "{war}");
        assert_eq!(war["skirmish"]["next"]["kind"], "skirmish");
    }
    let id = made["items"][0]["id"].as_str().unwrap().to_string();
    let (status, cancelled) = call(
        e,
        "DELETE",
        &format!("/api/admin/skirmishes/{id}"),
        Some(&staff),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cancelled["items"][1]["cancelled_at"].is_string());
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM moderation_actions WHERE action IN ('skirmish_schedule','skirmish_cancel')",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(audited, 3);
}
