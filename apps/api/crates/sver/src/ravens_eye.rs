//! Raven's Eye (docs/ADMIN.md "Raven's Eye"): staff-only platform analytics on Postgres. Each
//! finished UTC day is computed once and kept in `ravens_eye_days`, because some sources expire
//! (chat after 7 days); today is computed live. No IP addresses, devices or personal data: counts,
//! hours, money totals by kind and the top channels and categories.
use crate::{
    App,
    profiles::{Fail, Res},
};
use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Days, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// How far back the first run fills in (chat counts only exist for the last 7 days).
const BACKFILL_DAYS: u64 = 90;

/// One UTC day's numbers. Broadcast hours, streamers and categories count the part of each
/// broadcast inside the day (24/7 channels included); `broadcasts` counts the ones started that day.
/// Watch hours and the peak come from the viewer-integrity snapshots
/// (about one a minute per live broadcast, Counted + Trusted sessions only).
pub async fn compute(db: &mut PgConnection, day: NaiveDate) -> Res<Value> {
    Ok(sqlx::query_scalar(
        "WITH w AS (SELECT $1::date::timestamp AT TIME ZONE 'UTC' AS a, ($1::date+1)::timestamp AT TIME ZONE 'UTC' AS b)
        SELECT jsonb_build_object(
            'signups', (SELECT count(*) FROM users, w WHERE created_at>=a AND created_at<b),
            'verified_signups', (SELECT count(*) FROM users, w WHERE created_at>=a AND created_at<b AND email_verified),
            'viewers', (SELECT count(DISTINCT user_id) FROM xp_days WHERE day=$1 AND source='watch' AND minutes>0),
            'chatters', (SELECT count(DISTINCT author_id) FROM chat_messages, w WHERE created_at>=a AND created_at<b),
            'messages', (SELECT count(*) FROM chat_messages, w WHERE created_at>=a AND created_at<b),
            'follows', (SELECT count(*) FROM follows, w WHERE created_at>=a AND created_at<b),
            'faction_joins', (SELECT coalesce(jsonb_object_agg(faction,n),'{}') FROM (SELECT faction,count(*) n FROM faction_members, w WHERE joined_at>=a AND joined_at<b GROUP BY faction) f),
            'broadcasts', (SELECT count(*) FROM broadcasts, w WHERE started_at>=a AND started_at<b),
            'streamers', (SELECT count(DISTINCT owner_id) FROM broadcasts, w WHERE started_at<b AND coalesce(ended_at,now())>a),
            'broadcast_hours', (SELECT round(coalesce(sum(extract(epoch FROM least(coalesce(ended_at,now()),b)-greatest(started_at,a))),0)::numeric/3600,1) FROM broadcasts, w WHERE started_at<b AND coalesce(ended_at,now())>a),
            'watch_hours', (SELECT round(coalesce(sum(counted+trusted),0)::numeric/60,1) FROM integrity_snapshots, w WHERE taken_at>=a AND taken_at<b),
            'peak_viewers', (SELECT coalesce(max(n),0) FROM (SELECT sum(counted+trusted) n FROM integrity_snapshots, w WHERE taken_at>=a AND taken_at<b GROUP BY date_trunc('minute',taken_at)) p),
            'money', (SELECT coalesce(jsonb_object_agg(kind,jsonb_build_object('count',c,'valor',v,'usd_cents',u)),'{}') FROM (
                SELECT t.kind,count(DISTINCT t.id) c,
                    coalesce(sum(e.amount) FILTER (WHERE e.unit='valor' AND e.amount>0),0) v,
                    (coalesce(sum(e.amount) FILTER (WHERE e.unit='usd' AND e.amount>0),0)/10)::bigint u
                FROM ledger_transactions t JOIN ledger_entries e ON e.transaction_id=t.id, w
                WHERE t.created_at>=a AND t.created_at<b AND t.kind NOT LIKE 'adtest%' GROUP BY t.kind) m),
            'top_channels', (SELECT coalesce(jsonb_agg(jsonb_build_object('username',username,'watch_hours',round(minutes::numeric/60,1)) ORDER BY minutes DESC),'[]') FROM (
                SELECT u.username,sum(s.counted+s.trusted) minutes FROM integrity_snapshots s JOIN broadcasts b ON b.id=s.broadcast_id JOIN users u ON u.id=b.owner_id, w
                WHERE s.taken_at>=a AND s.taken_at<b GROUP BY u.username ORDER BY 2 DESC LIMIT 10) t),
            'top_categories', (SELECT coalesce(jsonb_agg(jsonb_build_object('category',category,'broadcast_hours',round(seconds::numeric/3600,1)) ORDER BY seconds DESC),'[]') FROM (
                SELECT k.name category,sum(extract(epoch FROM least(coalesce(k.ended_at,now()),w.b)-greatest(k.started_at,w.a))) seconds
                FROM broadcast_categories k, w
                WHERE k.started_at<w.b AND coalesce(k.ended_at,now())>w.a GROUP BY 1 ORDER BY 2 DESC LIMIT 10) k)
        )",
    )
    .bind(day)
    .fetch_one(db)
    .await?)
}

/// Streams from `from` (UTC midnight) to now, by the category actually streamed
/// (`broadcast_categories`): hours streamed, distinct streamers and broadcasts, watch hours and the
/// peak (Counted + Trusted sessions), and the same as totals. Computed over the whole range, so a
/// streamer live on three days counts once. SVER Plays channels are left out unless `plays`.
// ponytail: computed live from spans and minute snapshots; store weekly rollups if this gets slow.
pub async fn streams(db: &mut PgConnection, from: NaiveDate, plays: bool) -> Res<Value> {
    Ok(sqlx::query_scalar(
        "WITH w AS (SELECT $1::date::timestamp AT TIME ZONE 'UTC' AS a, now() AS b),
        spans AS (SELECT k.broadcast_id,k.name,k.genre,br.owner_id,greatest(k.started_at,w.a) s,least(coalesce(k.ended_at,now()),w.b) e
            FROM broadcast_categories k JOIN broadcasts br ON br.id=k.broadcast_id, w
            WHERE k.started_at<w.b AND coalesce(k.ended_at,now())>w.a
            AND ($2 OR NOT EXISTS(SELECT 1 FROM plays_runtime p WHERE p.enabled AND p.channel_id=br.owner_id))),
        watch AS (SELECT sp.name,date_trunc('minute',i.taken_at) m,sum(i.counted+i.trusted) n
            FROM spans sp JOIN integrity_snapshots i ON i.broadcast_id=sp.broadcast_id AND i.taken_at>=sp.s AND i.taken_at<sp.e GROUP BY 1,2),
        per AS (SELECT name,min(genre) genre,sum(extract(epoch FROM e-s)) seconds,count(DISTINCT owner_id) streamers,count(DISTINCT broadcast_id) broadcasts
            FROM spans WHERE e>s GROUP BY name)
        SELECT jsonb_build_object(
            'categories',(SELECT coalesce(jsonb_agg(jsonb_build_object('category',p.name,'genre',p.genre,'hours',round(p.seconds::numeric/3600,1),
                'streamers',p.streamers,'broadcasts',p.broadcasts,
                'watch_hours',coalesce((SELECT round(sum(n)::numeric/60,1) FROM watch WHERE watch.name=p.name),0),
                'peak_viewers',coalesce((SELECT max(n) FROM watch WHERE watch.name=p.name),0)) ORDER BY p.seconds DESC),'[]') FROM per p),
            'totals',jsonb_build_object(
                'hours',(SELECT round(coalesce(sum(extract(epoch FROM e-s)),0)::numeric/3600,1) FROM spans WHERE e>s),
                'streamers',(SELECT count(DISTINCT owner_id) FROM spans WHERE e>s),
                'broadcasts',(SELECT count(DISTINCT broadcast_id) FROM spans WHERE e>s),
                'categories',(SELECT count(*) FROM per WHERE name<>'Uncategorized'),
                'watch_hours',(SELECT round(coalesce(sum(n),0)::numeric/60,1) FROM watch),
                'peak_viewers',(SELECT coalesce(max(t),0) FROM (SELECT sum(n) t FROM watch GROUP BY m) x)))",
    )
    .bind(from)
    .bind(plays)
    .fetch_one(db)
    .await?)
}

/// Hourly: stores every finished day not yet kept, newest first, a few per pass.
pub async fn tick(app: &App) -> Res<()> {
    let today = Utc::now().date_naive();
    let first = today
        .checked_sub_days(Days::new(BACKFILL_DAYS))
        .unwrap_or(today);
    let missing: Vec<NaiveDate> = sqlx::query_scalar("SELECT d::date FROM generate_series($1::date,$2::date-1,'1 day') d WHERE NOT EXISTS(SELECT 1 FROM ravens_eye_days WHERE day=d::date) ORDER BY d DESC LIMIT 10")
        .bind(first).bind(today).fetch_all(&app.db).await?;
    for day in missing {
        let mut db = app.db.acquire().await?;
        let stats = compute(&mut db, day).await?;
        sqlx::query("INSERT INTO ravens_eye_days(day,stats) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(day)
            .bind(stats)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct Range {
    days: Option<u64>,
    #[serde(default)]
    plays: bool,
}
/// GET /api/admin/ravens-eye?days=30&plays=false: the stored days in range plus today so far, and
/// the range's streams by category (`streams`).
async fn view(
    State(app): State<App>,
    jar: CookieJar,
    Query(range): Query<Range>,
) -> Res<Json<Value>> {
    crate::safety::staff(&app, &jar).await?;
    let days = range.days.unwrap_or(30);
    if !(7..=BACKFILL_DAYS).contains(&days) {
        return Err(Fail::bad("Choose 7 to 90 days."));
    }
    let today = Utc::now().date_naive();
    let from = today.checked_sub_days(Days::new(days)).unwrap_or(today);
    let rows: Vec<(NaiveDate, Value)> = sqlx::query_as(
        "SELECT day,stats FROM ravens_eye_days WHERE day>=$1 AND day<$2 ORDER BY day",
    )
    .bind(from)
    .bind(today)
    .fetch_all(&app.db)
    .await?;
    let mut series: Vec<Value> = rows
        .into_iter()
        .map(|(day, stats)| json!({"day": day, "stats": stats, "partial": false}))
        .collect();
    let mut db = app.db.acquire().await?;
    let live = compute(&mut db, today).await?;
    series.push(json!({"day": today, "stats": live, "partial": true}));
    let streams = streams(&mut db, from, range.plays).await?;
    Ok(Json(
        json!({"days": series, "from": from, "streams": streams}),
    ))
}

pub fn routes() -> Router<App> {
    Router::new().route("/api/admin/ravens-eye", get(view))
}
