//! Ads, design first (docs/ADS.md): a test ad break counts trusted, signed-in, unsubscribed
//! viewers and settles once into the separate test book, never into payable earnings.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

pub async fn exercise(e: &Env) {
    person(e, "ad-owner", "AdOwner", true).await;
    for (id, name) in [
        ("ad-one", "AdViewerOne"),
        ("ad-two", "AdViewerTwo"),
        ("ad-sub", "AdSubscriber"),
        ("ad-counted", "AdCounted"),
    ] {
        person(e, id, name, true).await;
    }
    let staff = person(e, "ad-staff", "AdStaff", true).await;
    for statement in [
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='ad-staff'",
        "UPDATE sessions SET mfa_verified=true WHERE user_id='ad-staff'",
        "INSERT INTO staff_roles(user_id,role) VALUES('ad-staff','admin')",
        "INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('ad-b','ad-owner','ad-b',1,'LIVE','s','v','c',now(),now(),now())",
        "INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('ad-b','u:ad-one',now()+interval '1 minute','trusted'),('ad-b','u:ad-two',now()+interval '1 minute','trusted'),('ad-b','u:ad-sub',now()+interval '1 minute','trusted'),('ad-b','u:ad-counted',now()+interval '1 minute','counted'),('ad-b','u:ad-owner',now()+interval '1 minute','trusted')",
        "INSERT INTO channel_subs(channel_id,user_id,tier,paid_through,months) VALUES('ad-owner','ad-sub',1,now()+interval '1 month',1)",
    ] {
        e.sql(statement).await;
    }
    let post = |path: &'static str, body: Value| {
        let token = staff.clone();
        async move { call(e, "POST", path, Some(&token), body).await }
    };
    assert_eq!(
        post(
            "/api/admin/ads/test-breaks",
            json!({"username": "AdViewerOne"})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "only live channels"
    );
    let (status, started) =
        post("/api/admin/ads/test-breaks", json!({"username": "AdOwner"})).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(
        started["segments"][0]["viewers"], 2,
        "trusted, signed in, not the owner, not subscribed"
    );
    let id = started["segments"][0]["id"].as_str().unwrap().to_string();
    let settle = |cents: i64| {
        let (token, path) = (
            staff.clone(),
            format!("/api/admin/ads/test-breaks/{id}/settle"),
        );
        async move {
            call(
                e,
                "POST",
                &path,
                Some(&token),
                json!({"revenue_cents": cents}),
            )
            .await
        }
    };
    let (status, settled) = settle(1_000).await;
    assert_eq!(status, StatusCode::OK, "{settled}");
    // $10.00 at Scout: 65% streamer, 5% split by 2 viewers, the rest S.V.E.R (tenths of a cent).
    let balance = |account: &'static str| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT coalesce(sum(amount),0)::bigint FROM ledger_entries WHERE account=$1",
        )
        .bind(account)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
    };
    assert_eq!(balance("adtest:usd:earnings:ad-owner").await, 6_500);
    assert_eq!(balance("adtest:usd:adv:ad-one").await, 250);
    assert_eq!(balance("adtest:usd:adv:ad-sub").await, 0);
    assert_eq!(balance("adtest:usd:platform").await, 3_000);
    assert_eq!(
        balance("usd:earnings:ad-owner").await,
        0,
        "test revenue never becomes payable"
    );
    assert_eq!(settle(1_000).await.0, StatusCode::CONFLICT, "settles once");
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM moderation_actions WHERE action IN ('ad_test_break','ad_test_settle')",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(audited, 2);
    let viewer = person(e, "ad-plain", "AdPlain", true).await;
    assert_eq!(
        call(e, "GET", "/api/admin/ads", Some(&viewer), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND,
        "staff only"
    );
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='ad-b'")
        .await;
}
