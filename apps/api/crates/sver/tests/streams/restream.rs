//! Restreaming (docs/LINKED_CHAT.md): destinations, key secrecy and the relay supervisor.
use super::*;

pub async fn exercise(e: &Env) {
    let added = e
        .call(
            "POST",
            "/api/me/restream",
            json!({"platform":"twitch","key":"live-test-hidden"}),
        )
        .await;
    assert_eq!(
        added["destinations"][0]["server"],
        "rtmp://live.twitch.tv/app"
    );
    assert!(
        !added.to_string().contains("test-hidden"),
        "keys are never returned"
    );
    for bad in [
        json!({"platform":"kick","key":"k1"}),
        json!({"platform":"custom","server":"https://example.test/app","key":"k1"}),
        json!({"platform":"custom","server":"rtmp://example.test/app?key=1","key":"k1"}),
        json!({"platform":"youtube","key":"has space"}),
        json!({"platform":"mixer","key":"k1"}),
    ] {
        let (status, _) = e
            .request("POST", "/api/me/restream", bad.clone(), true, true, false)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    e.call(
        "POST",
        "/api/me/restream",
        json!({"platform":"youtube","key":"abcd-efgh"}),
    )
    .await;
    let three = e
        .call("POST", "/api/me/restream", json!({"platform":"kick","server":"rtmps://ingest.example.test/app/","key":"sk_us-west"}))
        .await;
    assert_eq!(
        three["destinations"][2]["server"],
        "rtmps://ingest.example.test/app"
    );
    let (status, _) = e
        .request(
            "POST",
            "/api/me/restream",
            json!({"platform":"twitch","key":"k4"}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "three at most");
    let stored: String =
        sqlx::query_scalar("SELECT string_agg(key_sealed,' ') FROM restream_destinations")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert!(!stored.contains("test-hidden"), "keys are sealed at rest");
    for d in three["destinations"].as_array().unwrap() {
        e.call(
            "DELETE",
            &format!("/api/me/restream/{}", d["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    }

    // The supervisor: a relay that can't connect retries with backoff, then shows "rejected"; the
    // status goes back to idle only when the key or server is saved again.
    e.sql("INSERT INTO users(id,email,username,email_verified,mfa_enabled,mfa_secret,date_of_birth) VALUES('rs-owner','rs@example.test','RsOwner',true,true,'synthetic','1990-01-01')").await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('rs-b','rs-owner','rs-b',1,'LIVE','s','v','c',now(),now(),now(),'skipped')").await;
    let sealed = sver::security::seal(&e.app, "restream", "rs-key").unwrap();
    sqlx::query("INSERT INTO restream_destinations(id,owner_id,platform,server,key_sealed) VALUES('rs-d','rs-owner','custom','rtmp://127.0.0.1:9/app',$1)")
        .bind(sealed).execute(&e.app.db).await.unwrap();
    let state = || async {
        sqlx::query_scalar::<_, String>("SELECT status FROM restream_destinations WHERE id='rs-d'")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    let source = "rtmp://127.0.0.1:9/rebuild";
    let mut seen = Vec::new();
    // A refused connection is instant on Linux and about 2 seconds on Windows.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        sver::restream::tick(&e.app, source).await.unwrap();
        let now = state().await;
        if seen.last() != Some(&now) {
            seen.push(now.clone());
        }
        if now == "rejected" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert_eq!(
        seen.first().map(String::as_str),
        Some("starting"),
        "{seen:?}"
    );
    assert!(seen.contains(&"reconnecting".to_string()), "{seen:?}");
    assert_eq!(
        seen.last().map(String::as_str),
        Some("rejected"),
        "{seen:?}"
    );
    assert_eq!(sver::restream::running(), 0);
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='rs-b'")
        .await;
    sver::restream::tick(&e.app, source).await.unwrap();
    assert_eq!(
        state().await,
        "rejected",
        "a rejection stays visible after the stream"
    );
    e.sql("DELETE FROM restream_destinations WHERE id='rs-d'")
        .await;
}
