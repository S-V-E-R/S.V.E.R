//! Platform reports for chat messages and live streams, handled through the Module 2 admin queue.
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn report(e: &Env, token: &str, kind: &str, target: &str) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        "/api/reports",
        Some(token),
        json!({"target_type":kind,"target_id":target,"reason":"harassment","note":"synthetic"}),
    )
    .await
}
async fn act(e: &Env, staff: &str, kind: &str, target: &str, action: &str) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("/api/admin/reports/{kind}/{target}/actions"),
        Some(staff),
        json!({"action":action,"note":"reviewed"}),
    )
    .await
}

pub async fn exercise(e: &Env) {
    let author = person(e, "rep-author", "RepAuthor", true).await;
    let reporter = person(e, "rep-reporter", "RepReporter", true).await;
    let staff = person(e, "rep-staff", "RepStaff", true).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='rep-staff'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='rep-staff'")
        .await;
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('rep-staff','admin')")
        .await;

    // Chat messages: reportable while visible; the snapshot keeps the body.
    let (_, sent) = call(
        e,
        "POST",
        "/api/channels/streamer/chat",
        Some(&author),
        json!({"id":id(),"body":"reportable words"}),
    )
    .await;
    let message = sent["message"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        report(e, &reporter, "chat_message", "missing-message")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        report(e, &reporter, "chat_message", &message).await.0,
        StatusCode::OK
    );
    let snapshot: Value = sqlx::query_scalar(
        "SELECT snapshot FROM reports WHERE target_type='chat_message' AND target_id=$1",
    )
    .bind(&message)
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(snapshot["value"]["body"], "reportable words");
    assert_eq!(snapshot["value"]["channel"], "Streamer");
    // Only staff act on platform reports; channel roles never grant /admin.
    assert_eq!(
        act(e, &reporter, "chat_message", &message, "remove_content")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        act(e, &staff, "chat_message", &message, "remove_content")
            .await
            .0,
        StatusCode::OK
    );
    let (_, history) = call(e, "GET", "/api/channels/streamer/chat", None, Value::Null).await;
    assert!(
        !history.to_string().contains("reportable words"),
        "removed message leaves history"
    );
    let status: String = sqlx::query_scalar(
        "SELECT status FROM reports WHERE target_type='chat_message' AND target_id=$1",
    )
    .bind(&message)
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(status, "ACTIONED");
    // A deleted message can no longer be reported.
    assert_eq!(
        report(e, &reporter, "chat_message", &message).await.0,
        StatusCode::NOT_FOUND
    );

    // Live streams: only a public broadcast is reportable; removal stops it and queues the disconnect.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('rep-b','stream-owner','pub-rep',1,'STARTING','s','v','c',now(),now(),now()+interval '15 seconds')").await;
    assert_eq!(
        report(e, &reporter, "live_stream", "rep-b").await.0,
        StatusCode::NOT_FOUND,
        "STARTING is not public"
    );
    e.sql("UPDATE broadcasts SET state='LIVE' WHERE id='rep-b'")
        .await;
    assert_eq!(
        report(e, &reporter, "live_stream", "rep-b").await.0,
        StatusCode::OK
    );
    let snapshot: Value = sqlx::query_scalar(
        "SELECT snapshot FROM reports WHERE target_type='live_stream' AND target_id='rep-b'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(snapshot["value"]["title"], "Streamer's stream");
    assert_eq!(snapshot["value"]["broadcast_id"], "rep-b");
    assert_eq!(
        act(e, &staff, "live_stream", "rep-b", "remove_content")
            .await
            .0,
        StatusCode::OK
    );
    let (state, reason): (String, Option<String>) =
        sqlx::query_as("SELECT state,end_reason FROM broadcasts WHERE id='rep-b'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("ENDED", Some("revoked"))
    );
    let queued: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_stop_jobs WHERE owner_id='stream-owner' AND client_id='c')")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(queued, "a durable disconnect is queued");
    let (_, live) = call(e, "GET", "/api/channels/streamer/live", None, Value::Null).await;
    assert_eq!(live["live"], false);

    // Leave the later lifecycle checks a clean owner.
    e.sql("DELETE FROM stream_stop_jobs WHERE client_id='c'")
        .await;
    e.sql("DELETE FROM broadcasts WHERE id='rep-b'").await;
    e.sql("DELETE FROM reports").await;
    e.sql("DELETE FROM moderation_actions").await;
    e.sql("DELETE FROM chat_messages").await;
    e.sql("DELETE FROM users WHERE id LIKE 'rep-%'").await;
}
