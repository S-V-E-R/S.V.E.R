//! Skirmish Hour (docs/FACTIONS.md "Skirmish Hour"; Joe's decision October 10, 2026): staff
//! schedule an hour, announced on the war map and hubs, in which influence counts double. The
//! doubling happens before the per-person daily and hourly caps, so the caps still bind and
//! farming gains nothing extra. Scheduling and cancelling are step-up and audited.
//!
//! Raid Night (docs/FACTIONS.md "Raid Night", the second event) shares the schedule: three hours in
//! which raid influence counts double, again before the caps. Events never overlap.
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

/// How much influence counts during an event (all of it in a Skirmish Hour, raids in a Raid Night).
pub const MULTIPLIER: i64 = 2;

/// An event kind's length in hours.
fn hours(kind: &str) -> Option<i64> {
    match kind {
        "skirmish" => Some(1),
        "raid_night" => Some(3),
        _ => None,
    }
}

/// The kind of event running at `at` ("skirmish" or "raid_night"), if any.
pub async fn active(db: &mut PgConnection, at: DateTime<Utc>) -> Res<Option<String>> {
    Ok(sqlx::query_scalar("SELECT kind FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at<=$1 AND ends_at>$1")
        .bind(at).fetch_optional(db).await?)
}

/// For the war map and hubs: the running event, if any, and the next one.
pub async fn summary(db: &mut PgConnection, at: DateTime<Utc>) -> Res<Value> {
    let current: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('kind',kind,'starts_at',starts_at,'ends_at',ends_at) FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at<=$1 AND ends_at>$1")
        .bind(at).fetch_optional(&mut *db).await?;
    let next: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('kind',kind,'starts_at',starts_at,'ends_at',ends_at) FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at>$1 ORDER BY starts_at LIMIT 1")
        .bind(at).fetch_optional(&mut *db).await?;
    Ok(json!({"multiplier": MULTIPLIER, "active": current, "next": next}))
}

/// GET /api/admin/skirmishes: scheduled and recent Skirmish Hours.
pub async fn list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'kind',s.kind,'starts_at',s.starts_at,'ends_at',s.ends_at,'cancelled_at',s.cancelled_at,'by',u.username) FROM faction_skirmishes s LEFT JOIN users u ON u.id=s.created_by WHERE s.starts_at>now()-interval '30 days' ORDER BY s.starts_at DESC LIMIT 100")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"items": items})))
}

#[derive(Deserialize)]
pub struct Schedule {
    starts_at: DateTime<Utc>,
    #[serde(default = "skirmish")]
    kind: String,
}
fn skirmish() -> String {
    "skirmish".into()
}
/// POST /api/admin/skirmishes: schedules a Skirmish Hour or Raid Night, at least an hour ahead (so
/// it can be announced), within 60 days, on the hour, and not overlapping another event.
pub async fn schedule(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Schedule>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let at = input.starts_at;
    let Some(hours) = hours(&input.kind) else {
        return Err(Fail::field("kind", "Choose Skirmish Hour or Raid Night."));
    };
    let ends = at + Duration::hours(hours);
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
    let overlaps: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_skirmishes WHERE cancelled_at IS NULL AND starts_at<$2 AND ends_at>$1)")
        .bind(at).bind(ends).fetch_one(&mut *tx).await?;
    if overlaps {
        return Err(Fail::conflict("Another faction event overlaps that time."));
    }
    let id = new_id();
    sqlx::query("INSERT INTO faction_skirmishes(id,starts_at,created_by,kind,ends_at) VALUES($1,$2,$3,$4,$5)")
        .bind(&id)
        .bind(at)
        .bind(&staff.id)
        .bind(&input.kind)
        .bind(ends)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "skirmish_schedule",
        "faction_skirmish",
        &id,
        &[],
        "Faction event scheduled",
        json!({"kind": input.kind, "starts_at": at}),
        false,
    )
    .await?;
    tx.commit().await?;
    list(State(app), jar).await
}

/// DELETE /api/admin/skirmishes/{id}: cancels an event that hasn't started.
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
            "Only an event that hasn't started can be cancelled.",
        ));
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "skirmish_cancel",
        "faction_skirmish",
        &id,
        &[],
        "Faction event cancelled",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    list(State(app), jar).await
}
