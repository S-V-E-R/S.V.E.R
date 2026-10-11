//! Skirmish Hour (docs/FACTIONS.md "Skirmish Hour"; Joe's decision October 10, 2026): staff
//! schedule an hour, announced on the war map and hubs, in which influence counts double. The
//! doubling happens before the per-person daily and hourly caps, so the caps still bind and
//! farming gains nothing extra. Scheduling and cancelling are step-up and audited.
use crate::{
    App,
    profiles::{Fail, Res, new_id},
    safety,
};
use axum::{
    Json,
    extract::{Path, State},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// How much influence counts during a Skirmish Hour.
pub const MULTIPLIER: i64 = 2;

/// Whether a Skirmish Hour is running at `at`.
pub async fn active(db: &mut PgConnection, at: DateTime<Utc>) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at<=$1 AND starts_at+interval '1 hour'>$1)")
        .bind(at).fetch_one(db).await?)
}

/// For the war map and hubs: the running Skirmish Hour, if any, and the next one.
pub async fn summary(db: &mut PgConnection, at: DateTime<Utc>) -> Res<Value> {
    let window = |starts: DateTime<Utc>| json!({"starts_at": starts, "ends_at": starts + Duration::hours(1)});
    let current: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT starts_at FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at<=$1 AND starts_at+interval '1 hour'>$1")
        .bind(at).fetch_optional(&mut *db).await?;
    let next: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT starts_at FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at>$1 ORDER BY starts_at LIMIT 1")
        .bind(at).fetch_optional(&mut *db).await?;
    Ok(json!({"multiplier": MULTIPLIER, "active": current.map(window), "next": next.map(window)}))
}

/// GET /api/admin/skirmishes: scheduled and recent Skirmish Hours.
pub async fn list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'starts_at',s.starts_at,'cancelled_at',s.cancelled_at,'by',u.username) FROM faction_skirmishes s LEFT JOIN users u ON u.id=s.created_by WHERE s.starts_at>now()-interval '30 days' ORDER BY s.starts_at DESC LIMIT 100")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"items": items})))
}

#[derive(Deserialize)]
pub struct Schedule {
    starts_at: DateTime<Utc>,
}
/// POST /api/admin/skirmishes: schedules one, at least an hour ahead (so it can be announced),
/// within 60 days, on the hour, and not overlapping another.
pub async fn schedule(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Schedule>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let at = input.starts_at;
    let now = Utc::now();
    if at < now + Duration::hours(1) || at > now + Duration::days(60) || at.timestamp() % 3600 != 0
    {
        return Err(Fail::field(
            "starts_at",
            "Start it on the hour, at least an hour and at most 60 days from now.",
        ));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('skirmishes',93))")
        .execute(&mut *tx)
        .await?;
    let overlaps: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at>$1-interval '1 hour' AND starts_at<$1+interval '1 hour')")
        .bind(at).fetch_one(&mut *tx).await?;
    if overlaps {
        return Err(Fail::conflict("Another Skirmish Hour overlaps that time."));
    }
    let id = new_id();
    sqlx::query("INSERT INTO faction_skirmishes(id,starts_at,created_by) VALUES($1,$2,$3)")
        .bind(&id)
        .bind(at)
        .bind(&staff.id)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "skirmish_schedule",
        "faction_skirmish",
        &id,
        &[],
        "Skirmish Hour scheduled",
        json!({"starts_at": at}),
        false,
    )
    .await?;
    tx.commit().await?;
    list(State(app), jar).await
}

/// DELETE /api/admin/skirmishes/{id}: cancels one that hasn't started.
pub async fn cancel(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let cancelled = sqlx::query("UPDATE faction_skirmishes SET cancelled_at=now() WHERE id=$1 AND cancelled_at IS NULL AND starts_at>now()")
        .bind(&id).execute(&mut *tx).await?.rows_affected();
    if cancelled == 0 {
        return Err(Fail::conflict(
            "Only a Skirmish Hour that hasn't started can be cancelled.",
        ));
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "skirmish_cancel",
        "faction_skirmish",
        &id,
        &[],
        "Skirmish Hour cancelled",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    list(State(app), jar).await
}
