//! Raven's Eye (docs/ADMIN.md): staff-only, finished days stored once, today live. Runs late in the
//! stream suite so the day already has broadcasts, follows and chat to count.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::Value;

pub async fn exercise(e: &Env) {
    let viewer = person(e, "re-viewer", "RavenViewer", true).await;
    assert_eq!(
        call(
            e,
            "GET",
            "/api/admin/ravens-eye",
            Some(&viewer),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let staff = person(e, "re-staff", "RavenStaff", true).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='re-staff'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='re-staff'")
        .await;
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('re-staff','admin')")
        .await;

    sver::ravens_eye::tick(&e.app).await.unwrap();
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM ravens_eye_days")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(stored, 10, "the newest 10 finished days per pass");
    let today: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ravens_eye_days WHERE day=(now() AT TIME ZONE 'UTC')::date)",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(!today, "today is never stored");

    let (status, view) = call(
        e,
        "GET",
        "/api/admin/ravens-eye?days=7",
        Some(&staff),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let days = view["days"].as_array().unwrap();
    let live = days.last().unwrap();
    assert_eq!(live["partial"], true);
    // Earlier tests leave broadcasts and sign-ups today; follows and chat may already be gone.
    for key in ["broadcasts", "signups", "streamers"] {
        assert!(live["stats"][key].as_i64().unwrap() > 0, "{key}: {live}");
    }
    for key in ["follows", "messages", "viewers", "peak_viewers"] {
        assert!(live["stats"][key].as_i64().is_some(), "{key}: {live}");
    }
    let tribute = &live["stats"]["money"]["tribute"];
    assert!(
        tribute["usd_cents"].is_i64() && tribute["count"].as_i64().unwrap() > 0,
        "{live}"
    );
    assert!(live["stats"]["money"].is_object());
    assert!(live["stats"]["top_categories"].is_array());

    // Streams by category: the triggers record the category actually streamed, and a change
    // while live splits the broadcast between the two.
    e.sql("INSERT INTO stream_settings(owner_id,title,category_id) VALUES('re-viewer','Raven test','minecraft') ON CONFLICT(owner_id) DO UPDATE SET category_id='minecraft'").await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,observed_at) VALUES('re-live','re-viewer','re-live',1,'LIVE','s','v','c',now()-interval '30 minutes',now(),now(),now())").await;
    e.sql("INSERT INTO integrity_snapshots(broadcast_id,taken_at,raw,counted,trusted,excluded,pending) VALUES('re-live',now()-interval '10 minutes',60,40,20,0,0)").await;
    e.sql("UPDATE stream_settings SET category_id='art' WHERE owner_id='re-viewer'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='re-live'")
        .await;
    let spans: Vec<(String, bool)> = sqlx::query_as("SELECT name,ended_at IS NOT NULL FROM broadcast_categories WHERE broadcast_id='re-live' ORDER BY started_at")
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        spans,
        [("Minecraft".into(), true), ("Art".into(), true)],
        "one closed span per category"
    );
    let (_, view) = call(
        e,
        "GET",
        "/api/admin/ravens-eye?days=7",
        Some(&staff),
        Value::Null,
    )
    .await;
    let categories = view["streams"]["categories"].as_array().unwrap();
    let find = |name: &str| categories.iter().find(|c| c["category"] == name).cloned();
    let minecraft = find("Minecraft").expect("Minecraft was streamed");
    assert!(minecraft["hours"].as_f64().unwrap() >= 0.5, "{minecraft}");
    assert!(
        minecraft["watch_hours"].as_f64().unwrap() >= 1.0,
        "{minecraft}"
    );
    assert!(
        minecraft["peak_viewers"].as_i64().unwrap() >= 60,
        "{minecraft}"
    );
    assert!(minecraft["streamers"].as_i64().unwrap() >= 1);
    let genre: String =
        sqlx::query_scalar("SELECT genre FROM stream_categories WHERE id='minecraft'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(minecraft["genre"], genre.as_str());
    assert!(
        find("Art").is_some(),
        "the rest of the broadcast counts as Art"
    );
    let totals = &view["streams"]["totals"];
    assert!(
        totals["streamers"].as_i64().unwrap() >= 1 && totals["hours"].as_f64().unwrap() >= 0.5,
        "{totals}"
    );
    assert_eq!(
        call(
            e,
            "GET",
            "/api/admin/ravens-eye?days=400",
            Some(&staff),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}
