//! Module 7 CrowdSync, phase 2: polls (one vote per real viewer, live results, windows with a
//! grace for delayed viewers), predictions (Engagement Valor stakes only, winners split the pool,
//! cancelling refunds, only the owner resolves) and counters (owner/moderator updates from the API
//! and chat commands, never below zero).
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

const CHANNEL: &str = "/api/channels/cwowner";

async fn crowd(e: &Env, token: &str) -> Value {
    call(
        e,
        "GET",
        &format!("{CHANNEL}/crowd"),
        Some(token),
        Value::Null,
    )
    .await
    .1
}
async fn start(e: &Env, token: &str, body: Value) -> (StatusCode, Value) {
    call(e, "POST", &format!("{CHANNEL}/polls"), Some(token), body).await
}
async fn vote(e: &Env, token: &str, poll: &str, body: Value) -> (StatusCode, Value) {
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'poll-vote:%'")
        .await;
    call(
        e,
        "POST",
        &format!("{CHANNEL}/polls/{poll}/vote"),
        Some(token),
        body,
    )
    .await
}
async fn close(e: &Env, token: &str, poll: &str, body: Value) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("{CHANNEL}/polls/{poll}/close"),
        Some(token),
        body,
    )
    .await
}
async fn balance(e: &Env, user: &str) -> i64 {
    sqlx::query_scalar("SELECT balance FROM engagement WHERE channel_id='cw-owner' AND user_id=$1")
        .bind(user)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}
async fn say(e: &Env, token: &str, body: &str) -> StatusCode {
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat-%'")
        .await;
    call(
        e,
        "POST",
        &format!("{CHANNEL}/chat"),
        Some(token),
        json!({"id": id(), "body": body}),
    )
    .await
    .0
}
async fn counter(e: &Env, id: &str) -> (i32, i32) {
    sqlx::query_as("SELECT value,extra FROM counters WHERE channel_id='cw-owner' AND id=$1")
        .bind(id)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "cw-owner", "CwOwner", true).await;
    let a = person(e, "cw-a", "CwA", true).await;
    let b = person(e, "cw-b", "CwB", true).await;
    let away = person(e, "cw-away", "CwAway", true).await;
    let moderator = person(e, "cw-mod", "CwMod", true).await;
    e.sql("INSERT INTO channel_moderators(channel_id,user_id) VALUES('cw-owner','cw-mod')")
        .await;
    let three = json!({"kind": "poll", "question": "Next boss?", "options": ["Ruin", "Gale", "Ember"], "seconds": 60});

    // ---- Polls ----
    assert_eq!(
        start(e, &moderator, three.clone()).await.0,
        StatusCode::CONFLICT,
        "only while live"
    );
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('cw-b','cw-owner','cw-b',1,'LIVE','s','v','c',now()-interval '5 minutes',now(),now(),'skipped')").await;
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('cw-b','u:cw-a',now()+interval '1 minute','counted'),('cw-b','u:cw-b',now()+interval '1 minute','trusted'),('cw-b','u:cw-mod',now()+interval '1 minute','trusted')").await;
    assert_eq!(
        start(e, &a, three.clone()).await.0,
        StatusCode::FORBIDDEN,
        "viewers can't start polls"
    );
    let one = json!({"kind": "poll", "question": "Q?", "options": ["Only"], "seconds": 60});
    assert_eq!(start(e, &moderator, one).await.0, StatusCode::BAD_REQUEST);
    let (status, made) = start(e, &moderator, three.clone()).await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let poll = made["id"].as_str().unwrap().to_string();
    assert_eq!(
        start(e, &owner, three.clone()).await.0,
        StatusCode::CONFLICT,
        "one poll at a time"
    );

    assert_eq!(
        vote(e, &a, &poll, json!({"option": 0})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        vote(e, &a, &poll, json!({"option": 1})).await.0,
        StatusCode::CONFLICT,
        "one vote"
    );
    assert_eq!(
        vote(e, &away, &poll, json!({"option": 1})).await.0,
        StatusCode::FORBIDDEN,
        "not watching"
    );
    assert_eq!(
        vote(e, &owner, &poll, json!({"option": 1})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        vote(e, &b, &poll, json!({"option": 7})).await.0,
        StatusCode::BAD_REQUEST
    );
    let state = crowd(e, &a).await;
    assert_eq!(state["poll"]["counts"], json!([1, 0, 0]));
    assert_eq!(state["poll"]["mine"]["option"], 0);
    assert_eq!(state["can_run"], false);
    // Delayed viewers: a vote a few seconds after the end still counts; later ones don't.
    e.sql("UPDATE polls SET ends_at=now()-interval '3 seconds' WHERE kind='poll' AND channel_id='cw-owner'").await;
    assert_eq!(
        vote(e, &b, &poll, json!({"option": 2})).await.0,
        StatusCode::OK,
        "within the grace"
    );
    assert_eq!(
        close(e, &moderator, &poll, json!({"action": "end"}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        vote(e, &moderator, &poll, json!({"option": 2})).await.0,
        StatusCode::CONFLICT,
        "ended"
    );
    let state = crowd(e, &owner).await;
    assert_eq!(
        (&state["poll"]["status"], &state["poll"]["counts"]),
        (&json!("ended"), &json!([1, 0, 1]))
    );
    assert_eq!(
        (&state["can_run"], &state["can_resolve"]),
        (&json!(true), &json!(true))
    );
    // A window that passed (plus grace) ends on the worker's tick.
    let (_, next) = start(e, &moderator, three.clone()).await;
    let next = next["id"].as_str().unwrap().to_string();
    e.sql("UPDATE polls SET ends_at=now()-interval '20 seconds' WHERE status='open' AND channel_id='cw-owner'").await;
    sver::crowd::tick(&e.app).await.unwrap();
    assert_eq!(
        vote(e, &a, &next, json!({"option": 0})).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(crowd(e, &a).await["poll"]["status"], "ended");

    // ---- Predictions: Engagement Valor only ----
    e.sql("INSERT INTO engagement(channel_id,user_id,balance,earned) VALUES('cw-owner','cw-a',1000,1000),('cw-owner','cw-b',1000,1000)").await;
    let bet = json!({"kind": "prediction", "question": "Win the round?", "options": ["Yes", "No"], "seconds": 120});
    let (status, made) = start(e, &moderator, bet.clone()).await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let prediction = made["id"].as_str().unwrap().to_string();
    assert_eq!(
        vote(e, &a, &prediction, json!({"option": 0})).await.0,
        StatusCode::BAD_REQUEST,
        "a stake is required"
    );
    assert_eq!(
        vote(e, &a, &prediction, json!({"option": 0, "stake": 20000}))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "capped"
    );
    assert_eq!(
        vote(e, &a, &prediction, json!({"option": 0, "stake": 300}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        vote(e, &b, &prediction, json!({"option": 1, "stake": 100}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        vote(e, &moderator, &prediction, json!({"option": 1, "stake": 5}))
            .await
            .0,
        StatusCode::CONFLICT,
        "no Engagement Valor"
    );
    assert_eq!(
        (balance(e, "cw-a").await, balance(e, "cw-b").await),
        (700, 900)
    );
    assert_eq!(crowd(e, &b).await["prediction"]["pools"], json!([300, 100]));
    let evented: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events WHERE topic LIKE 'channel:%:prediction' AND data->'pools'='[300,100]'::jsonb)")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(evented, "prediction tallies are live events");
    // It locks at its stream time (plus grace), then only the owner resolves it.
    e.sql("UPDATE polls SET ends_at=now()-interval '20 seconds' WHERE id IN (SELECT id FROM polls WHERE kind='prediction' AND status='open')").await;
    sver::crowd::tick(&e.app).await.unwrap();
    assert_eq!(crowd(e, &a).await["prediction"]["status"], "locked");
    assert_eq!(
        vote(e, &b, &prediction, json!({"option": 1, "stake": 5}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        close(
            e,
            &moderator,
            &prediction,
            json!({"action": "resolve", "winner": 1})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        close(
            e,
            &owner,
            &prediction,
            json!({"action": "resolve", "winner": 5})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        close(
            e,
            &owner,
            &prediction,
            json!({"action": "resolve", "winner": 1})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        (balance(e, "cw-a").await, balance(e, "cw-b").await),
        (700, 1300),
        "the winner takes the pool"
    );
    let state = crowd(e, &b).await;
    assert_eq!(
        (
            &state["prediction"]["winner"],
            &state["prediction"]["mine"]["payout"]
        ),
        (&json!(1), &json!(400))
    );
    assert_eq!(
        close(e, &owner, &prediction, json!({"action": "cancel"}))
            .await
            .0,
        StatusCode::CONFLICT,
        "already resolved"
    );
    // Cancelling refunds every stake.
    let (_, made) = start(e, &moderator, bet).await;
    let refund = made["id"].as_str().unwrap().to_string();
    assert_eq!(
        vote(e, &a, &refund, json!({"option": 1, "stake": 200}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(balance(e, "cw-a").await, 500);
    assert_eq!(
        close(e, &moderator, &refund, json!({"action": "cancel"}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(balance(e, "cw-a").await, 700);
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_moderation_log WHERE channel_id='cw-owner' AND action LIKE 'p%'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(logged, 7, "starts, end, resolve and cancel are logged");

    // ---- Counters ----
    let create = |body: Value| call(e, "POST", "/api/me/counters", Some(&owner), body);
    assert_eq!(
        create(json!({"id": "deaths", "kind": "deaths", "label": "Deaths"}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        create(json!({"id": "deaths", "label": "Again"})).await.0,
        StatusCode::BAD_REQUEST,
        "taken"
    );
    assert_eq!(
        create(json!({"id": "no spaces", "label": "X"})).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, made) =
        create(json!({"id": "record", "kind": "tally", "label": "Wins and losses"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(made["counters"].as_array().unwrap().len(), 2);
    let deaths = format!("{CHANNEL}/counters/deaths");
    let one = json!({"amount": 1});
    assert_eq!(
        call(e, "POST", &deaths, Some(&moderator), one.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "POST", &deaths, Some(&a), one).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(counter(e, "deaths").await, (1, 0));
    // Chat commands from the owner and moderators; a viewer's "!deaths" is just a message.
    assert_eq!(say(e, &moderator, "!deaths +5").await, StatusCode::OK);
    assert_eq!(counter(e, "deaths").await, (6, 0));
    assert_eq!(say(e, &a, "!deaths").await, StatusCode::OK);
    assert_eq!(
        counter(e, "deaths").await,
        (6, 0),
        "viewers can't change counters"
    );
    say(e, &owner, "!deaths =2").await;
    say(e, &owner, "!deaths -").await;
    assert_eq!(counter(e, "deaths").await, (1, 0));
    say(e, &owner, "!deaths -5").await;
    assert_eq!(counter(e, "deaths").await, (0, 0), "never below zero");
    say(e, &moderator, "!record win").await;
    say(e, &moderator, "!record loss").await;
    say(e, &moderator, "!record loss").await;
    assert_eq!(counter(e, "record").await, (1, 2));
    assert_eq!(crowd(e, &a).await["counters"][1]["extra"], 2);
    assert_eq!(
        call(
            e,
            "DELETE",
            "/api/me/counters/record",
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );

    e.sql("UPDATE broadcasts SET state='ENDED',started_at=now()-interval '30 days',ended_at=now()-interval '30 days',end_reason='test',reconnect_deadline=NULL WHERE id='cw-b'").await;
}
