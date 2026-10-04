//! Viewer integrity end to end: the security check, levels, hard evidence, spikes, snapshots,
//! staff cases, the Studio figure, retention and that no raw IP is ever stored.
use super::Env;
use super::bans::staff;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

const BEAT: &str = "/api/channels/streamer/live/beat";

async fn guest_beat(e: &Env, browser: &str, body: Value) -> Value {
    let mut payload = json!({"broadcast_id":"int-b","browser_id":browser,"visible":true});
    for (k, v) in body.as_object().unwrap() {
        payload[k] = v.clone();
    }
    let (status, value) = call(e, "POST", BEAT, None, payload).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}
async fn level(e: &Env, key_like: &str) -> String {
    sqlx::query_scalar(
        "SELECT level FROM playback_leases WHERE broadcast_id='int-b' AND viewer_key LIKE $1",
    )
    .bind(key_like)
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}
async fn tick(e: &Env) {
    sver::integrity::tick(&e.app).await.unwrap();
}

pub async fn exercise(e: &Env) {
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('int-b','stream-owner','pub-int',1,'LIVE','s','v','c',now()-interval '20 minutes',now(),now())").await;

    // Guests must pass the security check before they can count; a failed token changes nothing.
    let first = guest_beat(e, "browser-integrity-01", json!({})).await;
    assert_eq!(
        (first["level"].as_str(), first["needs_turnstile"].as_bool()),
        (Some("pending"), Some(true))
    );
    assert_eq!(
        guest_beat(e, "browser-integrity-01", json!({"turnstile":"fail"})).await["needs_turnstile"],
        true
    );
    e.sql("UPDATE playback_leases SET created_at=created_at-interval '2 minutes'")
        .await;
    tick(e).await;
    assert_eq!(
        level(e, "b:%").await,
        "pending",
        "an unchecked guest never counts"
    );
    let passed = guest_beat(e, "browser-integrity-01", json!({"turnstile":"pass"})).await;
    assert_eq!(
        (
            passed["level"].as_str(),
            passed["needs_turnstile"].as_bool()
        ),
        (Some("counted"), Some(false))
    );

    // No raw IP is stored anywhere in the lease.
    let row: String = sqlx::query_scalar(
        "SELECT row_to_json(l)::text FROM playback_leases l WHERE broadcast_id='int-b' LIMIT 1",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(!row.contains("127.0.0") && !row.contains("::1"), "{row}");

    // A signed-in, verified member skips the check and becomes Trusted after two visible minutes.
    let member = person(e, "int-member", "IntMember", true).await;
    let mut tx = e.app.db.begin().await.unwrap();
    sver::profiles::ensure_profile(&mut tx, "int-member")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, joined) = call(
        e,
        "POST",
        BEAT,
        Some(&member),
        json!({"broadcast_id":"int-b","browser_id":"browser-integrity-mm","visible":true}),
    )
    .await;
    assert_eq!(
        (
            joined["level"].as_str(),
            joined["needs_turnstile"].as_bool()
        ),
        (Some("pending"), Some(false))
    );
    e.sql("UPDATE playback_leases SET created_at=created_at-interval '3 minutes', visible_seconds=130 WHERE viewer_key='u:int-member'").await;
    tick(e).await;
    assert_eq!(level(e, "u:int-member").await, "trusted");
    let mut db = e.app.db.acquire().await.unwrap();
    let (raw, counted, trusted, _, _) = sver::integrity::counts(&mut db, "int-b").await.unwrap();
    assert_eq!(
        (raw, counted, trusted),
        (2, 2, 1),
        "public count includes trusted"
    );
    drop(db);
    let (_, live) = call(e, "GET", "/api/channels/streamer/live", None, Value::Null).await;
    assert_eq!(live["viewers"], 2);

    // Hard evidence: media time racing ahead of the clock excludes the session for good.
    guest_beat(
        e,
        "browser-integrity-02",
        json!({"turnstile":"pass","media_time":10.0}),
    )
    .await;
    e.sql("UPDATE playback_leases SET last_beat_at=now()-interval '10 seconds' WHERE viewer_key LIKE 'b:%' AND beats=1").await;
    guest_beat(e, "browser-integrity-02", json!({"media_time":500.0})).await;
    e.sql("UPDATE playback_leases SET last_beat_at=now()-interval '10 seconds' WHERE viewer_key LIKE 'b:%' AND beats=2").await;
    let faked = guest_beat(e, "browser-integrity-02", json!({"media_time":1000.0})).await;
    assert_eq!(faked["level"], "excluded");
    e.sql(
        "UPDATE playback_leases SET created_at=created_at-interval '5 minutes' WHERE hard_excluded",
    )
    .await;
    tick(e).await;
    let hard: i64 = sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE broadcast_id='int-b' AND hard_excluded AND level='excluded'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(hard, 1, "hard exclusion survives rescoring");

    // A household-sized network is fine; two signals together exclude; recovery when they clear.
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,created_at,turnstile_ok,net_hash,interval_count,interval_mean,interval_m2) SELECT 'int-b','b:farm-'||g,now()+interval '30 seconds',now()-interval '5 minutes',true,'synthetic-net',10,10000,1 FROM generate_series(1,12) g").await;
    tick(e).await;
    let excluded: i64 = sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE viewer_key LIKE 'b:farm-%' AND level='excluded'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(excluded, 12, "metronomic and concentrated");
    e.sql("UPDATE playback_leases SET interval_m2=900000 WHERE viewer_key LIKE 'b:farm-%'")
        .await;
    tick(e).await;
    let recovered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM playback_leases WHERE viewer_key LIKE 'b:farm-%' AND level='counted'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(
        recovered, 12,
        "one signal alone (a busy network) never excludes"
    );

    // A sudden burst after the stream's opening minutes waits out a provisional window.
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,turnstile_ok) SELECT 'int-b','b:burst-'||g,now()+interval '30 seconds',true FROM generate_series(1,25) g").await;
    tick(e).await;
    let provisional: i64 = sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE viewer_key LIKE 'b:burst-%' AND provisional_until>now()+interval '4 minutes'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(provisional, 25);
    e.sql("UPDATE playback_leases SET created_at=created_at-interval '2 minutes' WHERE viewer_key LIKE 'b:burst-%'").await;
    tick(e).await;
    assert_eq!(
        level(e, "b:burst-1").await,
        "pending",
        "still inside the window"
    );
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,ended_at,end_reason) VALUES('int-old','stream-owner','pub-old',1,'ENDED','s','v','c',now()-interval '40 days',now(),now(),now()-interval '31 days','test')").await;
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at) VALUES('int-old','b:old',now()-interval '31 days')").await;

    // Snapshots: about one a minute per live broadcast.
    let snapshots: i64 =
        sqlx::query_scalar("SELECT count(*) FROM integrity_snapshots WHERE broadcast_id='int-b'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        snapshots, 1,
        "several passes within a minute keep one snapshot"
    );

    // A sustained excluded share opens a staff case with aggregate evidence only.
    e.sql("UPDATE playback_leases SET hard_excluded=true, expires_at=now()+interval '2 minutes' WHERE viewer_key LIKE 'b:farm-%' OR viewer_key LIKE 'b:burst-%'").await;
    // Two earlier windows with a high excluded share; the next pass adds the third.
    e.sql("DELETE FROM integrity_snapshots WHERE broadcast_id='int-b'")
        .await;
    e.sql("INSERT INTO integrity_snapshots(broadcast_id,taken_at,raw,counted,trusted,excluded,pending) VALUES('int-b',now()-interval '2 minutes',40,5,1,35,0),('int-b',now()-interval '3 minutes',40,5,1,35,0)").await;
    tick(e).await;
    let evidence: Value = sqlx::query_scalar(
        "SELECT evidence FROM integrity_cases WHERE broadcast_id='int-b' AND status='OPEN'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(
        evidence["windows"].as_array().unwrap().len() == 3
            && evidence["networks_excluded"].as_i64().unwrap() >= 1
    );
    assert!(
        !evidence.to_string().contains("synthetic-net"),
        "no network hashes in evidence"
    );

    // Only staff see and decide cases; the streamer can't clear their own.
    let admin = staff(e, "int-staff", "IntStaff").await;
    assert_eq!(
        e.request(
            "GET",
            "/api/admin/integrity",
            Value::Null,
            true,
            true,
            false
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, cases) = call(e, "GET", "/api/admin/integrity", Some(&admin), Value::Null).await;
    let case = cases["cases"][0]["id"].as_str().unwrap().to_string();
    let decide = format!("/api/admin/integrity/{case}/decision");
    assert_eq!(
        e.request(
            "POST",
            &decide,
            json!({"outcome":"dismiss","note":"mine"}),
            true,
            true,
            false
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            e,
            "POST",
            &decide,
            Some(&admin),
            json!({"outcome":"dismiss"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "note required"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &decide,
            Some(&admin),
            json!({"outcome":"action","note":"Paid bot traffic","hold_payouts":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            &decide,
            Some(&admin),
            json!({"outcome":"dismiss","note":"again"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM moderation_actions WHERE action='integrity_case_decided'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(audited, 1);

    // After the stream, Studio reports how many viewers weren't counted (watched a minute or more).
    e.sql("UPDATE playback_leases SET beats=6 WHERE broadcast_id='int-b'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test' WHERE id='int-b'")
        .await;
    let studio = e.call("GET", "/api/me/stream", Value::Null).await;
    let not_counted = studio["last_broadcast"]["not_counted"].as_i64().unwrap();
    assert!(not_counted >= 1, "{studio}");
    assert!(!studio.to_string().contains("b:"), "no viewer identities");

    // Retention: session rows go 30 days after their broadcast ended.
    tick(e).await;
    let old: i64 =
        sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE broadcast_id='int-old'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(old, 0);

    for statement in [
        "DELETE FROM integrity_cases",
        "DELETE FROM broadcasts WHERE id IN ('int-b','int-old')",
        "DELETE FROM moderation_actions",
        "DELETE FROM staff_roles",
    ] {
        e.sql(statement).await;
    }
    e.sql("DELETE FROM users WHERE id LIKE 'int-%'").await;
}
