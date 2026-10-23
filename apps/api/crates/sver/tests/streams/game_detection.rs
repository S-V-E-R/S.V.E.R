//! Game detection (docs/LIVE_STREAMS.md "Game detection"): opt-in from Studio, a suggestion only
//! after STREAK checks see the same other game, "Not this game", a category change clearing it,
//! the per-game example cap, and opting out deleting the channel's examples.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sver::streams::detect;

async fn detection(e: &Env, token: &str) -> Value {
    call(e, "GET", "/api/me/stream", Some(token), Value::Null)
        .await
        .1["detection"]
        .clone()
}
async fn examples(e: &Env) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM game_examples WHERE owner_id='gd-owner'")
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "gd-owner", "GdOwner", true).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='gd-owner'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='gd-owner'")
        .await;
    let plain = person(e, "gd-plain", "GdPlain", true).await;
    e.sql("INSERT INTO stream_categories(id,name,genre) SELECT v.id,v.name,genre FROM stream_categories, (VALUES ('gd-mw4','GD Modern Warfare 4'),('gd-fn','GD Fortnite')) v(id,name) WHERE stream_categories.id='coding'").await;

    // Off by default; tests run without a model, so Studio is told it can't run here.
    assert_eq!(
        detection(e, &owner).await,
        json!({"available": false, "on": false, "category": null, "suggestion": null})
    );
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/stream/detection",
        None,
        json!({"on": true}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/stream/detection",
        Some(&plain),
        json!({"on": true}),
    )
    .await;
    assert!(
        status.is_client_error(),
        "streaming needs two-factor sign-in"
    );
    let (status, on) = call(
        e,
        "PUT",
        "/api/me/stream/detection",
        Some(&owner),
        json!({"on": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{on}");
    assert_eq!(on["on"], true);

    // Live on MW4: two checks seeing Fortnite are not enough, the third is.
    e.sql("UPDATE stream_settings SET category_id='gd-mw4' WHERE owner_id='gd-owner'")
        .await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state,detect_guess,detect_streak) VALUES('gd-b','gd-owner','gd-b',1,'LIVE','s','v','c',now(),now(),now(),'skipped','gd-fn',2)").await;
    let seen = detection(e, &owner).await;
    assert_eq!(seen["category"], "GD Modern Warfare 4");
    assert_eq!(seen["suggestion"], Value::Null);
    e.sql("UPDATE broadcasts SET detect_streak=3 WHERE id='gd-b'")
        .await;
    assert_eq!(
        detection(e, &owner).await["suggestion"],
        json!({"category_id": "gd-fn", "name": "GD Fortnite", "mature": false})
    );

    // "Not this game" clears it and remembers the game for the rest of the broadcast.
    let (status, dismissed) = call(
        e,
        "POST",
        "/api/me/stream/detection/dismiss",
        Some(&owner),
        json!({"category_id": "gd-fn"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dismissed}");
    assert_eq!(dismissed["suggestion"], Value::Null);
    let remembered: Vec<String> =
        sqlx::query_scalar("SELECT detect_dismissed FROM broadcasts WHERE id='gd-b'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(remembered, ["gd-fn"]);
    let (status, _) = call(
        e,
        "POST",
        "/api/me/stream/detection/dismiss",
        Some(&owner),
        json!({"category_id": ""}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Choosing a category clears a pending guess and opens a fresh labeling window.
    e.sql("UPDATE broadcasts SET detect_guess='gd-fn',detect_streak=3 WHERE id='gd-b'")
        .await;
    let mut db = e.app.db.acquire().await.unwrap();
    detect::category_changed(&mut db, "gd-owner").await.unwrap();
    drop(db);
    assert_eq!(detection(e, &owner).await["suggestion"], Value::Null);
    let fresh: bool = sqlx::query_scalar("SELECT category_at>now()-interval '1 minute' FROM stream_settings WHERE owner_id='gd-owner'")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(fresh);

    // Each channel keeps only its newest examples per game.
    let embedding = vec![1.0 / (detect::DIM as f32).sqrt(); detect::DIM];
    for _ in 0..detect::EXAMPLES_PER_GAME + 2 {
        detect::add_example(&e.app.db, "gd-owner", "gd-mw4", &embedding)
            .await
            .unwrap();
    }
    assert_eq!(examples(e).await, detect::EXAMPLES_PER_GAME);

    // Opting out deletes the channel's examples and any suggestion state.
    let (status, off) = call(
        e,
        "PUT",
        "/api/me/stream/detection",
        Some(&owner),
        json!({"on": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{off}");
    assert_eq!(off["on"], false);
    assert_eq!(examples(e).await, 0);
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='gd-b'")
        .await;
}
