//! The staff console's home, jobs and audit log (docs/ADMIN.md): what needs attention now, failed or
//! stuck background work with a retry that still runs once, and the read-only, searchable audit log.
use crate::{
    App,
    profiles::{Fail, Res, make_cursor, parse_cursor},
    safety,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

/// Queues staff can see and retry: (name, label, table, has delivered_at, error column). A row is
/// pending until it's deleted or delivered. Table and column names are fixed here, never input.
const QUEUES: [(&str, &str, &str, bool, &str); 8] = [
    ("video", "Video processing", "video_jobs", false, "NULL"),
    ("beacon", "Beacon processing", "beacon_jobs", false, "NULL"),
    ("push", "Browser push", "push_jobs", false, "NULL"),
    ("mail", "Email", "mail_jobs", false, "NULL"),
    ("staff_push", "Staff push", "staff_push_jobs", true, "NULL"),
    ("board_webhook", "Board webhooks", "outbox", true, "error"),
    (
        "event_webhook",
        "Event webhooks",
        "hook_deliveries",
        true,
        "error",
    ),
    ("discord", "Discord posts", "discord_posts", true, "error"),
];
fn pending(delivered: bool) -> &'static str {
    if delivered {
        "delivered_at IS NULL"
    } else {
        "true"
    }
}
/// Tried and failed, and either given up or overdue by 10 minutes.
fn stuck(delivered: bool) -> String {
    format!(
        "{} AND attempts>0 AND (available_at='infinity' OR available_at<now()-interval '10 minutes')",
        pending(delivered)
    )
}

async fn queue_stats(app: &App) -> Res<Vec<Value>> {
    let mut out = Vec::new();
    for (name, label, table, delivered, _) in QUEUES {
        // Fixed SQL from the constants above.
        let (waiting, stuck_count, gave_up): (i64, i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FILTER (WHERE {p}),count(*) FILTER (WHERE {s}),count(*) FILTER (WHERE {p} AND available_at='infinity') FROM {table}",
            p = pending(delivered),
            s = stuck(delivered)
        )))
        .fetch_one(&app.db)
        .await?;
        out.push(json!({"name": name, "label": label, "waiting": waiting, "stuck": stuck_count, "gave_up": gave_up, "retry": true}));
    }
    // Shown, retried elsewhere: checkpoints from /admin/factions; payouts never by hand here.
    let checkpoints: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM faction_weeks WHERE completed_at IS NULL AND last_error IS NOT NULL",
    )
    .fetch_one(&app.db)
    .await?;
    out.push(json!({"name": "checkpoints", "label": "Faction checkpoints", "waiting": checkpoints, "stuck": checkpoints, "gave_up": 0, "retry": false, "link": "/admin/factions"}));
    let payouts: i64 = sqlx::query_scalar("SELECT count(*) FROM payout_runs WHERE status='failed'")
        .fetch_one(&app.db)
        .await?;
    out.push(json!({"name": "payouts", "label": "Failed payouts", "waiting": payouts, "stuck": payouts, "gave_up": 0, "retry": false}));
    Ok(out)
}

/// GET /api/admin/home: what needs attention, for the console's first page.
async fn home(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let take_down: (i64, i64, Option<DateTime<Utc>>) = sqlx::query_as("SELECT count(*),count(*) FILTER (WHERE deadline<now()),min(deadline) FROM take_down_requests WHERE resolved_at IS NULL")
        .fetch_one(&app.db).await?;
    let counts: Value = sqlx::query_scalar("SELECT jsonb_build_object(
            'reports',(SELECT count(*) FROM reports WHERE status='OPEN'),
            'appeals',(SELECT count(*) FROM appeals WHERE status='PENDING'),
            'copyright',(SELECT count(*) FROM copyright_cases WHERE status IN ('OPEN','COUNTER_PENDING')),
            'integrity',(SELECT count(*) FROM integrity_cases WHERE status='OPEN'),
            'media',(SELECT count(*) FROM channel_emotes WHERE reviewed_at IS NULL AND status='VISIBLE'),
            'live',(SELECT count(*) FROM broadcasts WHERE state IN ('LIVE','RECONNECTING')))")
        .fetch_one(&app.db).await?;
    let paused: Vec<String> =
        sqlx::query_scalar("SELECT name FROM feature_switches WHERE off ORDER BY name")
            .fetch_all(&app.db)
            .await?;
    let stuck: i64 = queue_stats(&app)
        .await?
        .iter()
        .map(|q| q["stuck"].as_i64().unwrap_or(0))
        .sum();
    Ok(Json(json!({
        "take_down": {"open": take_down.0, "overdue": take_down.1, "next_deadline": take_down.2},
        "counts": counts, "jobs_stuck": stuck, "paused": paused,
    })))
}

type Job = (
    String,
    i32,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<String>,
);
/// GET /api/admin/jobs: each queue's counts and up to 50 stuck jobs per queue, oldest first.
async fn jobs(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let mut items = Vec::new();
    for (name, _, table, delivered, error) in QUEUES {
        let rows: Vec<Job> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT id::text,attempts,created_at,CASE WHEN available_at='infinity' THEN NULL ELSE available_at END,{error} FROM {table} WHERE {s} ORDER BY created_at LIMIT 50",
            s = stuck(delivered)
        )))
        .fetch_all(&app.db)
        .await?;
        items.extend(
            rows.into_iter()
                .map(|(id, attempts, created, next, error)| {
                    json!({"queue": name, "id": id, "attempts": attempts, "created_at": created,
                "next_try": next, "gave_up": next.is_none(), "error": error})
                }),
        );
    }
    Ok(Json(
        json!({"queues": queue_stats(&app).await?, "items": items}),
    ))
}
#[derive(Deserialize)]
pub struct Retry {
    note: Option<String>,
}
/// POST /api/admin/jobs/{queue}/{id}/retry: due now. The worker's lease still runs it once.
async fn retry(
    State(app): State<App>,
    jar: CookieJar,
    Path((queue, id)): Path<(String, String)>,
    Json(input): Json<Retry>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let (_, _, table, delivered, _) = QUEUES
        .iter()
        .find(|q| q.0 == queue)
        .copied()
        .ok_or_else(Fail::missing)?;
    let mut tx = app.db.begin().await?;
    let updated = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET available_at=now() WHERE id::text=$1 AND {}",
        pending(delivered)
    )))
    .bind(&id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(Fail::conflict(
            "That job already finished or no longer exists.",
        ));
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "job_retry",
        "job",
        &format!("{queue}:{id}"),
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    jobs(State(app), jar).await
}

#[derive(Deserialize)]
pub struct Search {
    actor: Option<String>,
    action: Option<String>,
    target: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    cursor: Option<String>,
}
/// GET /api/admin/audit: every staff action, newest first, by staff member, action, target and
/// date. Read-only: there is no way to edit or delete a row.
async fn audit(
    State(app): State<App>,
    jar: CookieJar,
    Query(q): Query<Search>,
) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let cursor = parse_cursor(&q.cursor)?;
    let blank = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let rows: Vec<(String, DateTime<Utc>, Value)> = sqlx::query_as("SELECT m.id,m.created_at,jsonb_build_object('id',m.id,'action',m.action,'target_type',m.target_type,'target_id',m.target_id,'note',m.note,'detail',m.detail,'self_review',m.self_review,'created_at',m.created_at,'actor',a.username)
        FROM moderation_actions m LEFT JOIN users a ON a.id=m.actor_id
        WHERE ($1::text IS NULL OR lower(a.username)=lower($1)) AND ($2::text IS NULL OR m.action=$2)
        AND ($3::text IS NULL OR m.target_id=$3 OR m.target_type=$3)
        AND ($4::date IS NULL OR m.created_at>=$4) AND ($5::date IS NULL OR m.created_at<$5+1)
        AND ($6::timestamptz IS NULL OR (m.created_at,m.id)<($6,$7))
        ORDER BY m.created_at DESC,m.id DESC LIMIT 51")
        .bind(blank(&q.actor)).bind(blank(&q.action)).bind(blank(&q.target)).bind(q.from).bind(q.to)
        .bind(cursor.as_ref().map(|c| c.0)).bind(cursor.as_ref().map(|c| c.1.clone()).unwrap_or_default())
        .fetch_all(&app.db).await?;
    let next = (rows.len() > 50).then(|| make_cursor(rows[49].1, &rows[49].0));
    Ok(Json(
        json!({"actions": rows.into_iter().take(50).map(|r| r.2).collect::<Vec<_>>(), "next_cursor": next}),
    ))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/admin/home", get(home))
        .route("/api/admin/jobs", get(jobs))
        .route("/api/admin/jobs/{queue}/{id}/retry", post(retry))
        .route("/api/admin/audit", get(audit))
}
