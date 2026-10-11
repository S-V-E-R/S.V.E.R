//! Faction events (docs/FACTIONS.md "Faction events"; Joe's decisions October 10, 2026), each
//! doubling influence before the per-person daily and hourly caps, so the caps still bind and
//! farming gains nothing extra. Events never stack: any number of them still means 2x.
//! - Skirmish Hour: staff schedule an hour in which all influence counts double.
//! - Raid Night: staff schedule three hours in which viewers who arrived by a cross-faction raid
//!   earn double watch influence.
//! - Territory Spotlight: each UTC day one genre, the current week's closest race between the top
//!   two factions, counts double; picked on first use and kept for the day.
//!
//! Scheduling and cancelling are step-up and audited; the war map and hubs announce them.
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

/// How much influence counts during an event.
pub const MULTIPLIER: i64 = 2;
/// King of the Hill: extra territories for whoever holds the Hill when the season ends.
pub const HILL_BONUS: i64 = 2;

/// This season's Hill genre (King of the Hill), while the season runs.
pub async fn hill(db: &mut PgConnection, at: DateTime<Utc>) -> Res<Option<String>> {
    Ok(sqlx::query_scalar("SELECT hill FROM faction_seasons WHERE starts_at<=$1 AND ends_at>$1 AND finished_at IS NULL ORDER BY number DESC LIMIT 1")
        .bind(at)
        .fetch_optional(db)
        .await?
        .flatten())
}
/// Scheduled event kinds and their length in hours.
const KINDS: [(&str, i64); 2] = [("skirmish", 1), ("raid_night", 3)];
/// SQL for a scheduled event's end, by kind.
const ENDS: &str = "starts_at+make_interval(hours=>CASE kind WHEN 'raid_night' THEN 3 ELSE 1 END)";

async fn active(db: &mut PgConnection, at: DateTime<Utc>, kind: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS(SELECT 1 FROM faction_skirmishes WHERE cancelled_at IS NULL AND kind=$2 AND starts_at<=$1 AND {ENDS}>$1)"
    )))
    .bind(at)
    .bind(kind)
    .fetch_one(db)
    .await?)
}

/// Today's Territory Spotlight genre: the week's closest top-two race among this season's
/// territories, or, before anyone has influence, a genre picked by the date. Kept for the day.
pub async fn spotlight(db: &mut PgConnection, at: DateTime<Utc>) -> Res<Option<String>> {
    let day = at.date_naive();
    if let Some(genre) = sqlx::query_scalar("SELECT genre FROM faction_spotlights WHERE day=$1")
        .bind(day)
        .fetch_optional(&mut *db)
        .await?
    {
        return Ok(Some(genre));
    }
    let Some(week) = super::engine::current(db, at).await? else {
        return Ok(None);
    };
    let closest: Option<String> = sqlx::query_scalar("WITH s AS (SELECT i.genre,i.faction,sum(i.points) p FROM faction_influence i JOIN faction_territories t ON t.genre=i.genre AND t.season_id=$2 WHERE i.week_id=$1 GROUP BY i.genre,i.faction),
            r AS (SELECT genre,p,row_number() OVER (PARTITION BY genre ORDER BY p DESC,faction) n FROM s)
        SELECT a.genre FROM r a JOIN r b ON b.genre=a.genre AND b.n=2 WHERE a.n=1 ORDER BY a.p-b.p,a.genre LIMIT 1")
        .bind(week.id).bind(week.season_id).fetch_optional(&mut *db).await?;
    let genre = match closest {
        Some(g) => Some(g),
        None => sqlx::query_scalar("SELECT genre FROM faction_territories WHERE season_id=$1 ORDER BY md5($2||genre),genre LIMIT 1")
            .bind(week.season_id).bind(day.to_string()).fetch_optional(&mut *db).await?,
    };
    let Some(genre) = genre else { return Ok(None) };
    sqlx::query(
        "INSERT INTO faction_spotlights(day,genre) VALUES($1,$2) ON CONFLICT(day) DO NOTHING",
    )
    .bind(day)
    .bind(&genre)
    .execute(&mut *db)
    .await?;
    Ok(
        sqlx::query_scalar("SELECT genre FROM faction_spotlights WHERE day=$1")
            .bind(day)
            .fetch_optional(db)
            .await?,
    )
}

/// The multiplier for one award: 2 when any event applies to it, otherwise 1.
pub async fn multiplier(
    db: &mut PgConnection,
    at: DateTime<Utc>,
    genre: &str,
    raided: bool,
) -> Res<i64> {
    let applies = active(db, at, "skirmish").await?
        || (raided && active(db, at, "raid_night").await?)
        || spotlight(db, at).await?.as_deref() == Some(genre)
        || hill(db, at).await?.as_deref() == Some(genre);
    Ok(if applies { MULTIPLIER } else { 1 })
}

/// For the war map and hubs: each scheduled kind's running and next window, and today's spotlight.
pub async fn summary(db: &mut PgConnection, at: DateTime<Utc>) -> Res<Value> {
    let mut out = json!({"multiplier": MULTIPLIER});
    for (kind, hours) in KINDS {
        let window = |starts: DateTime<Utc>| json!({"starts_at": starts, "ends_at": starts + Duration::hours(hours)});
        let current: Option<DateTime<Utc>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT starts_at FROM faction_skirmishes WHERE cancelled_at IS NULL AND kind=$2 AND starts_at<=$1 AND {ENDS}>$1")))
            .bind(at).bind(kind).fetch_optional(&mut *db).await?;
        let next: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT starts_at FROM faction_skirmishes WHERE cancelled_at IS NULL AND kind=$2 AND starts_at>$1 ORDER BY starts_at LIMIT 1")
            .bind(at).bind(kind).fetch_optional(&mut *db).await?;
        if kind == "skirmish" {
            out["active"] = json!(current.map(window));
            out["next"] = json!(next.map(window));
        } else {
            out["raid_night"] = json!({"active": current.map(window), "next": next.map(window)});
        }
    }
    let spot = spotlight(db, at).await?;
    let name: Option<String> = match &spot {
        Some(g) => {
            sqlx::query_scalar("SELECT name FROM faction_genres WHERE id=$1")
                .bind(g)
                .fetch_optional(&mut *db)
                .await?
        }
        None => None,
    };
    out["spotlight"] = json!(spot.map(|g| json!({"genre": g, "name": name})));
    out["hill"] = match hill(db, at).await? {
        Some(genre) => {
            let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT g.name,t.holder FROM faction_genres g LEFT JOIN faction_territories t ON t.genre=g.id AND t.season_id=(SELECT id FROM faction_seasons WHERE hill=$1 AND finished_at IS NULL ORDER BY number DESC LIMIT 1) WHERE g.id=$1")
                .bind(&genre).fetch_optional(&mut *db).await?;
            let (name, holder) = row.unwrap_or((genre.clone(), None));
            json!({"genre": genre, "name": name, "holder": holder, "bonus": HILL_BONUS})
        }
        None => Value::Null,
    };
    Ok(out)
}

/// GET /api/admin/skirmishes: scheduled and recent Skirmish Hours and Raid Nights.
pub async fn list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'kind',s.kind,'starts_at',s.starts_at,'cancelled_at',s.cancelled_at,'by',u.username) FROM faction_skirmishes s LEFT JOIN users u ON u.id=s.created_by WHERE s.starts_at>now()-interval '30 days' ORDER BY s.starts_at DESC LIMIT 100")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"items": items})))
}

#[derive(Deserialize)]
pub struct Schedule {
    starts_at: DateTime<Utc>,
    #[serde(default)]
    kind: Option<String>,
}
/// POST /api/admin/skirmishes: schedules a Skirmish Hour (the default) or Raid Night, at least an
/// hour ahead (so it can be announced), within 60 days, on the hour, not overlapping the same kind.
pub async fn schedule(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Schedule>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let kind = input.kind.as_deref().unwrap_or("skirmish");
    let hours = KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, h)| *h)
        .ok_or_else(|| Fail::field("kind", "Choose a Skirmish Hour or a Raid Night."))?;
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
    let overlaps: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT EXISTS(SELECT 1 FROM faction_skirmishes WHERE cancelled_at IS NULL AND kind=$2 AND starts_at<$1+make_interval(hours=>$3) AND {ENDS}>$1)")))
        .bind(at).bind(kind).bind(hours as i32).fetch_one(&mut *tx).await?;
    if overlaps {
        return Err(Fail::conflict("Another one of these overlaps that time."));
    }
    let id = new_id();
    sqlx::query("INSERT INTO faction_skirmishes(id,kind,starts_at,created_by) VALUES($1,$2,$3,$4)")
        .bind(&id)
        .bind(kind)
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
        "Faction event scheduled",
        json!({"starts_at": at, "kind": kind}),
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
