//! Creator tiers (docs/SUPPORT.md "Creator tiers"): Scout, Trailblazer, Pioneer and Pathfinder,
//! checked weekly (Monday 00:01 Eastern) against the last 90 days. A streamer moves up when they
//! meet every requirement of a higher tier; tiers never go down, and an open staff integrity case
//! holds promotion. The tier sets the subscription split; viewer numbers use the Trusted count.
use crate::{
    App,
    profiles::{self, Res},
};
use axum::{Json, Router, extract::State, routing::get};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::sync::atomic::{AtomicI64, Ordering};

pub const NAMES: [&str; 4] = ["Scout", "Trailblazer", "Pioneer", "Pathfinder"];
/// The streamer's share of subscription revenue (card and gifts), by tier.
pub const SPLITS: [i64; 4] = [65, 70, 75, 80];
/// VOD retention by tier, in hours (Module 8).
pub const VOD_HOURS: [i64; 4] = [24, 48, 72, 168];

/// Last 90 days, as the requirements count them.
#[derive(sqlx::FromRow, Debug, Default, Clone)]
pub struct Metrics {
    pub streams: i64,
    pub hours: f64,
    pub avg_viewers: f64,
    pub followers: i64,
    pub subscribers: i64,
    pub unique_viewers: i64,
    pub days_active: i64,
}
/// Requirements for tiers 1 to 3.
const REQUIRED: [Metrics; 3] = [
    Metrics {
        streams: 8,
        hours: 15.0,
        avg_viewers: 5.0,
        followers: 150,
        subscribers: 0,
        unique_viewers: 0,
        days_active: 10,
    },
    Metrics {
        streams: 30,
        hours: 80.0,
        avg_viewers: 20.0,
        followers: 800,
        subscribers: 8,
        unique_viewers: 300,
        days_active: 40,
    },
    Metrics {
        streams: 120,
        hours: 300.0,
        avg_viewers: 60.0,
        followers: 2_500,
        subscribers: 40,
        unique_viewers: 3_000,
        days_active: 120,
    },
];
fn meets(m: &Metrics, r: &Metrics) -> bool {
    m.streams >= r.streams
        && m.hours >= r.hours
        && m.avg_viewers >= r.avg_viewers
        && m.followers >= r.followers
        && m.subscribers >= r.subscribers
        && m.unique_viewers >= r.unique_viewers
        && m.days_active >= r.days_active
}
/// The highest tier whose every requirement is met.
pub fn earned(m: &Metrics) -> i16 {
    (1..=3)
        .rev()
        .find(|&t| meets(m, &REQUIRED[t as usize - 1]))
        .unwrap_or(0)
}
fn metrics_json(m: &Metrics) -> Value {
    json!({"streams": m.streams, "hours": (m.hours * 10.0).floor() / 10.0, "avg_viewers": (m.avg_viewers * 10.0).floor() / 10.0,
        "followers": m.followers, "subscribers": m.subscribers, "unique_viewers": m.unique_viewers, "days_active": m.days_active})
}

pub async fn tier_of(db: &mut PgConnection, user: &str) -> Res<i16> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT tier FROM creator_tiers WHERE user_id=$1),0::smallint)",
    )
    .bind(user)
    .fetch_one(db)
    .await?)
}
/// The streamer's subscription split, in percent.
pub async fn split(db: &mut PgConnection, owner: &str) -> Res<i64> {
    Ok(SPLITS[tier_of(db, owner).await? as usize])
}

pub async fn metrics(db: &mut PgConnection, owner: &str) -> Res<Metrics> {
    Ok(sqlx::query_as("SELECT
        (SELECT count(*) FROM broadcasts WHERE owner_id=$1 AND started_at>now()-interval '90 days') AS streams,
        (SELECT coalesce(sum(extract(epoch FROM coalesce(ended_at,now())-started_at)),0)::float8/3600 FROM broadcasts WHERE owner_id=$1 AND started_at>now()-interval '90 days') AS hours,
        (SELECT coalesce(avg(s.trusted),0)::float8 FROM integrity_snapshots s JOIN broadcasts b ON b.id=s.broadcast_id WHERE b.owner_id=$1 AND s.taken_at>now()-interval '90 days') AS avg_viewers,
        (SELECT count(*) FROM follows WHERE following_id=$1) AS followers,
        (SELECT count(*) FROM channel_subs WHERE channel_id=$1 AND paid_through>now()) AS subscribers,
        (SELECT count(*) FROM creator_viewers WHERE owner_id=$1 AND last_seen>now()-interval '90 days') AS unique_viewers,
        (SELECT count(DISTINCT (started_at AT TIME ZONE 'America/New_York')::date) FROM broadcasts WHERE owner_id=$1 AND started_at>now()-interval '90 days') AS days_active")
        .bind(owner).fetch_one(db).await?)
}
async fn held(db: &mut PgConnection, owner: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM integrity_cases WHERE owner_id=$1 AND status='OPEN')",
    )
    .bind(owner)
    .fetch_one(db)
    .await?)
}

/// Re-checks one streamer: promotes to the highest earned tier, never down, unless an open
/// integrity case holds promotion. Returns the tier after the check.
pub async fn check(db: &mut PgConnection, owner: &str) -> Res<i16> {
    let current = tier_of(db, owner).await?;
    let target = if held(db, owner).await? {
        current
    } else {
        earned(&metrics(db, owner).await?).max(current)
    };
    sqlx::query("INSERT INTO creator_tiers(user_id,tier,promoted_at,checked_at) VALUES($1,$2,CASE WHEN $2>0 THEN now() END,now())
        ON CONFLICT(user_id) DO UPDATE SET tier=greatest(creator_tiers.tier,EXCLUDED.tier),
            promoted_at=CASE WHEN EXCLUDED.tier>creator_tiers.tier THEN now() ELSE creator_tiers.promoted_at END, checked_at=now()")
        .bind(owner).bind(target).execute(&mut *db).await?;
    crate::videos::extend_retention(db, owner, target).await?;
    Ok(target)
}

static LAST_MINUTE: AtomicI64 = AtomicI64::new(0);
/// From the media loop: records unique trusted viewers once a minute and runs the weekly check
/// once per week (Monday 00:01 Eastern).
pub async fn tick(app: &App) -> Res<()> {
    let now = Utc::now().timestamp();
    if now - LAST_MINUTE.load(Ordering::Relaxed) < 60 {
        return Ok(());
    }
    LAST_MINUTE.store(now, Ordering::Relaxed);
    sqlx::query("INSERT INTO creator_viewers(owner_id,viewer_key) SELECT DISTINCT b.owner_id,l.viewer_key FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id
        WHERE l.level='trusted' AND l.expires_at>now() AND b.state IN ('LIVE','RECONNECTING')
        ON CONFLICT(owner_id,viewer_key) DO UPDATE SET last_seen=now()")
        .execute(&app.db).await?;
    weekly(app).await
}
/// The weekly promotion pass; a period runs once (support_runs), even across restarts.
pub async fn weekly(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let started = sqlx::query("INSERT INTO support_runs(kind,period) SELECT 'tiers', CASE WHEN p>now() THEN p-interval '7 days' ELSE p END
        FROM (SELECT (date_trunc('week', now() AT TIME ZONE 'America/New_York')+interval '1 minute') AT TIME ZONE 'America/New_York' AS p) w
        ON CONFLICT DO NOTHING")
        .execute(&mut *tx).await?.rows_affected();
    if started == 0 {
        return Ok(());
    }
    // ponytail: one pass over every recent streamer in one transaction; batch it if streamer
    // counts grow large.
    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT owner_id FROM broadcasts WHERE started_at>now()-interval '90 days'",
    )
    .fetch_all(&mut *tx)
    .await?;
    for owner in &owners {
        check(&mut tx, owner).await?;
    }
    tx.commit().await?;
    eprintln!("tiers_event=weekly checked={}", owners.len());
    Ok(())
}

/// GET /api/me/tier: the streamer's tier, split, progress toward the next tier and whether
/// promotion is held.
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let tier = tier_of(&mut db, &user.id).await? as usize;
    let metrics = metrics(&mut db, &user.id).await?;
    let next = (tier < 3).then(|| {
        json!({"tier": tier + 1, "name": NAMES[tier + 1], "split": SPLITS[tier + 1], "requirements": metrics_json(&REQUIRED[tier])})
    });
    Ok(Json(json!({
        "tier": tier, "name": NAMES[tier], "split": SPLITS[tier], "vod_hours": VOD_HOURS[tier],
        "metrics": metrics_json(&metrics), "next": next, "held": held(&mut db, &user.id).await?,
    })))
}

pub fn routes() -> Router<App> {
    Router::new().route("/api/me/tier", get(mine))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_need_every_requirement() {
        let mut m = Metrics::default();
        assert_eq!(earned(&m), 0);
        m = REQUIRED[0].clone();
        assert_eq!(earned(&m), 1);
        m.followers -= 1;
        assert_eq!(earned(&m), 0, "one short holds the tier");
        m = REQUIRED[2].clone();
        assert_eq!(
            earned(&m),
            3,
            "a streamer can move up more than one tier at once"
        );
        m.subscribers = 39;
        assert_eq!(earned(&m), 2);
        assert_eq!(SPLITS, [65, 70, 75, 80]);
    }
}
