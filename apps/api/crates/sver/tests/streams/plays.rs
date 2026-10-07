use super::{
    Env,
    chat::{call, person},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn bridge(e: &Env, key: &str, ready: bool) -> (StatusCode, Value) {
    beat(e, key, json!({"ready":ready,"input_mode":"chat"})).await
}
async fn beat(e: &Env, key: &str, body: Value) -> (StatusCode, Value) {
    let response = sver::router(e.app.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/plays/bridge")
                .header("origin", &e.app.config.origin)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap(),
    )
}
pub async fn exercise(e: &Env) {
    let viewer = person(e, "plays-viewer", "GameViewer", true).await;
    let unverified = person(e, "plays-unverified", "GamePending", false).await;
    person(e, "plays-host", "GameHost", true).await;
    let key = sver::security::token();
    sqlx::query("INSERT INTO plays_runtime(channel_id,game,bridge_hash) VALUES('plays-host','Synthetic game',$1)").bind(sver::security::digest(&key)).execute(&e.app.db).await.unwrap();
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('plays-broadcast','plays-host','00000000000000000000000000000042',1,'LIVE','test-server','test-service','plays-client',now(),now(),now()+interval '15 seconds')").await;
    let path = "/api/channels/GameHost/plays";
    assert_eq!(
        bridge(e, &sver::security::token(), true).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(bridge(e, &key, true).await.0, StatusCode::OK);
    let state = call(e, "GET", path, None, Value::Null).await.1;
    assert_eq!(state["connected"], true);
    assert_eq!(state["can_vote"], false);
    assert!(!state.to_string().contains(&key));
    assert!(!state.to_string().contains("bridge_hash"));
    let vote = json!({"command":"up","round":state["round"]});
    assert_eq!(
        call(e, "POST", path, None, vote.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(e, "POST", path, Some(&unverified), vote.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            e,
            "POST",
            path,
            Some(&viewer),
            json!({"command":"reset","round":state["round"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            path,
            Some(&viewer),
            json!({"command":"up","round":0})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    // Fresh round, then concurrent button requests: exactly one counts.
    let state = call(e, "GET", path, Some(&viewer), Value::Null).await.1;
    let vote = json!({"command":"up","round":state["round"]});
    let (a, b) = tokio::join!(
        call(e, "POST", path, Some(&viewer), vote.clone()),
        call(e, "POST", path, Some(&viewer), vote)
    );
    assert_eq!(
        [a.0, b.0].iter().filter(|s| **s == StatusCode::OK).count(),
        1
    );
    let body = json!({"id":uuid::Uuid::new_v4().to_string(),"body":"down"});
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/GameHost/chat",
            Some(&viewer),
            body.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/GameHost/chat",
            Some(&viewer),
            body
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM plays_votes")
            .fetch_one(&e.app.db)
            .await
            .unwrap(),
        1
    );
    e.sql("UPDATE plays_votes SET round=floor(extract(epoch FROM clock_timestamp())/5)::bigint-1")
        .await;
    e.sql("UPDATE plays_runtime SET last_round=0").await;
    let result = bridge(e, &key, true).await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(result.1["command"], "up");
    assert!(
        bridge(e, &key, true).await.1["command"].is_null(),
        "bridge replay must never press twice"
    );
    e.sql("DELETE FROM plays_votes").await;
    e.sql("INSERT INTO plays_votes SELECT floor(extract(epoch FROM clock_timestamp())/5)::bigint-5,'plays-viewer','start'").await;
    e.sql("UPDATE plays_runtime SET last_round=0").await;
    assert!(
        bridge(e, &key, true).await.1["command"].is_null(),
        "missed inputs expire"
    );
    bridge(e, &key, false).await;
    let state = call(e, "GET", path, Some(&viewer), Value::Null).await.1;
    assert_eq!(state["connected"], false);
    assert_eq!(
        call(
            e,
            "POST",
            path,
            Some(&viewer),
            json!({"command":"a","round":state["round"]})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    bridge(e, &key, true).await;
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('plays-host','plays-viewer')")
        .await;
    assert_eq!(
        call(
            e,
            "POST",
            path,
            Some(&viewer),
            json!({"command":"a","round":state["round"]})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    e.sql("DELETE FROM user_blocks WHERE blocker_id='plays-host'")
        .await;
    for kind in ["ban", "timeout"] {
        sqlx::query("INSERT INTO channel_restrictions(channel_id,user_id,kind,until) VALUES('plays-host','plays-viewer',$1,CASE WHEN $1='timeout' THEN now()+interval '1 minute' END)")
            .bind(kind).execute(&e.app.db).await.unwrap();
        assert_eq!(
            call(
                e,
                "POST",
                path,
                Some(&viewer),
                json!({"command":"a","round":state["round"]})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        e.sql("DELETE FROM channel_restrictions WHERE channel_id='plays-host'")
            .await;
    }
    // Health: the host watchdog's problem reaches staff admins once, after two minutes.
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('plays-viewer','admin')")
        .await;
    let mails = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mail_jobs WHERE user_id='plays-viewer'")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    let before = mails().await;
    let sick = json!({"ready":true,"input_mode":"chat","problem":"The Plays picture stopped advancing; the game was restarted."});
    assert_eq!(beat(e, &key, sick.clone()).await.0, StatusCode::OK);
    sver::plays::tick(&e.app).await.unwrap();
    assert_eq!(mails().await, before, "not before two minutes");
    e.sql("UPDATE plays_runtime SET problem_since=now()-interval '3 minutes'")
        .await;
    assert_eq!(
        beat(e, &key, sick).await.0,
        StatusCode::OK,
        "a repeat keeps the start time"
    );
    sver::plays::tick(&e.app).await.unwrap();
    assert_eq!(mails().await, before + 1, "admins are emailed");
    sver::plays::tick(&e.app).await.unwrap();
    assert_eq!(mails().await, before + 1, "at most once an hour");
    assert_eq!(
        beat(
            e,
            &key,
            json!({"ready":true,"input_mode":"chat","problem":"x".repeat(201)})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(bridge(e, &key, true).await.0, StatusCode::OK);
    let cleared: Option<String> = sqlx::query_scalar("SELECT problem FROM plays_runtime")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(cleared, None, "a healthy beat clears the problem");
    e.sql("DELETE FROM staff_roles WHERE user_id='plays-viewer'")
        .await;
    e.sql("DELETE FROM mail_jobs WHERE user_id='plays-viewer'")
        .await;
    e.sql("UPDATE plays_runtime SET heartbeat_at=now()-interval '1 minute'")
        .await;
    assert_eq!(
        call(e, "GET", path, None, Value::Null).await.1["connected"],
        false
    );
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='plays-broadcast'")
        .await;
    assert_eq!(bridge(e, &key, true).await.1["live"], false);
    e.sql("DELETE FROM plays_runtime").await;
    e.sql("DELETE FROM users WHERE id IN ('plays-host','plays-viewer','plays-unverified')")
        .await;
}
