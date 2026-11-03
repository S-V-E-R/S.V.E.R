//! Viewer analytics for streamers (docs/ANALYTICS.md): visits and where they came from, how long
//! they stay, which discovery surfaces showed the stream and how often that turned into a visit,
//! and whether Live Now turns and MAGNet features brought more arrivals. Streamers see totals
//! only, never a viewer. Nothing here feeds discovery, MAGNet, factions or money.
use crate::{
    App,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::SocketAddr;

/// Sources a player may claim. `magnet_channel` and `raid` are decided by the server.
const CLAIMABLE: [&str; 15] = [
    "home",
    "magnet",
    "search",
    "browse",
    "following",
    "alert",
    "suggestion",
    "host",
    "beacon",
    "clip",
    "video",
    "faction",
    "channel",
    "external",
    "direct",
];
/// Discovery surfaces that count impressions, and the source a click from each one carries.
const SURFACES: [&str; 7] = [
    "home",
    "browse",
    "search",
    "following",
    "faction",
    "magnet",
    "suggestion",
];
/// A visit ends when heartbeats stop for this long (the playback lease's own lapse).
const VISIT_GAP_SECONDS: i64 = 30;
/// Kept this long (both tables); the Studio offers 7, 30 or 90 days.
const KEEP_DAYS: i64 = 90;

/// The source a heartbeat's claim maps to: anything unknown is `other`, a missing claim `direct`.
pub fn source(claimed: Option<&str>) -> &'static str {
    match claimed {
        None => "direct",
        Some(c) => CLAIMABLE
            .iter()
            .find(|s| **s == c)
            .copied()
            .unwrap_or("other"),
    }
}
/// A referring site's host name, normalized; None unless it looks like one.
pub fn referrer_host(raw: Option<&str>) -> Option<String> {
    let host = raw?.trim().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    (!host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
    .then(|| host.to_string())
}

/// One recorded heartbeat (docs/ANALYTICS.md "Visits"): extends the viewer's open visit on this
/// broadcast by the time since its last beat (at most 15 s), or starts a new one with `source`.
pub async fn beat(
    app: &App,
    broadcast: &str,
    channel: &str,
    key: &str,
    source: &str,
    referrer: Option<&str>,
    counted: bool,
) -> Res<()> {
    // ponytail: one extra write per heartbeat; past a few thousand concurrent viewers, batch
    // these in memory and flush every minute.
    sqlx::query("WITH open AS (UPDATE watch_visits SET seconds=seconds+least(extract(epoch FROM now()-last_at),15),last_at=now(),counted=counted OR $6
            WHERE id=(SELECT id FROM watch_visits WHERE broadcast_id=$1 AND viewer_key=$3 AND last_at>now()-make_interval(secs=>$7) ORDER BY last_at DESC LIMIT 1) RETURNING id)
        INSERT INTO watch_visits(broadcast_id,channel_id,viewer_key,source,referrer_host,follower,counted)
        SELECT $1,$2,$3,$4,CASE WHEN $4='external' THEN $5 END,
            $3 LIKE 'u:%' AND EXISTS(SELECT 1 FROM follows WHERE follower_id=substr($3,3) AND following_id=$2),$6
        WHERE NOT EXISTS(SELECT 1 FROM open)")
        .bind(broadcast).bind(channel).bind(key).bind(source).bind(referrer).bind(counted).bind(VISIT_GAP_SECONDS as f64)
        .execute(&app.db).await?;
    Ok(())
}

/// Removes an account's visits as a viewer (account deletion).
pub async fn forget_viewer(db: &mut sqlx::PgConnection, user: &str) -> Res<()> {
    sqlx::query("DELETE FROM watch_visits WHERE viewer_key='u:'||$1")
        .bind(user)
        .execute(db)
        .await?;
    Ok(())
}

/// With the jobs loop: drop rows older than 90 days.
pub async fn tick(app: &App) -> Res<()> {
    sqlx::query("DELETE FROM watch_visits WHERE started_at<now()-make_interval(days=>$1)")
        .bind(KEEP_DAYS as i32)
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM discovery_impressions WHERE hour<now()-make_interval(days=>$1)")
        .bind(KEEP_DAYS as i32)
        .execute(&app.db)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct Seen {
    surface: String,
    broadcasts: Vec<String>,
}
/// POST /api/discovery/seen: live cards that were on a viewer's screen for a second. The page
/// sends each card once per visit to that page; a viewer's own stream isn't counted.
async fn seen(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<Seen>,
) -> Res<Json<Value>> {
    if !SURFACES.contains(&input.surface.as_str()) {
        return Err(Fail::field("surface", "Unknown surface."));
    }
    if input.broadcasts.len() > 48 || input.broadcasts.iter().any(|b| b.len() > 64) {
        return Err(Fail::bad("Too many cards."));
    }
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("discovery-seen:{ip}")], 60, 60).await?;
    let viewer = profiles::viewer(&app, &jar).await?.map(|u| u.id);
    sqlx::query("INSERT INTO discovery_impressions(broadcast_id,surface,hour,shown) SELECT id,$2,date_trunc('hour',now()),1 FROM broadcasts WHERE id=ANY($1) AND state IN ('LIVE','RECONNECTING') AND owner_id IS DISTINCT FROM $3
        ON CONFLICT(broadcast_id,surface,hour) DO UPDATE SET shown=discovery_impressions.shown+1")
        .bind(&input.broadcasts).bind(&input.surface).bind(viewer)
        .execute(&app.db).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct Range {
    #[serde(default)]
    days: Option<i64>,
}
/// GET /api/me/analytics?days=7|30|90: the signed-in streamer's viewer analytics.
async fn mine(
    State(app): State<App>,
    jar: CookieJar,
    Query(range): Query<Range>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let days = range.days.unwrap_or(30);
    if ![7, 30, 90].contains(&days) {
        return Err(Fail::field("days", "Choose 7, 30 or 90 days."));
    }
    let from = Utc::now() - Duration::days(days);
    report(&app, &user.id, from).await.map(Json)
}

/// The analytics for one channel since `from`. Only visits viewer integrity counted are used.
pub async fn report(app: &App, channel: &str, from: DateTime<Utc>) -> Res<Value> {
    let mut db = app.db.acquire().await?;
    let totals: Value = sqlx::query_scalar("WITH v AS (SELECT * FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2),
        firsts AS (SELECT viewer_key,min(started_at) AS first FROM watch_visits WHERE channel_id=$1 AND counted GROUP BY 1)
        SELECT jsonb_build_object(
          'visits',(SELECT count(*) FROM v),
          'viewers',(SELECT count(DISTINCT viewer_key) FROM v),
          'watch_hours',round(coalesce((SELECT sum(seconds) FROM v),0)::numeric/3600,1),
          'avg_minutes',round(coalesce((SELECT avg(seconds) FROM v),0)::numeric/60,1),
          'median_minutes',round(coalesce((SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY seconds) FROM v),0)::numeric/60,1),
          'new_viewers',(SELECT count(*) FROM firsts WHERE first>=$2),
          'returning_viewers',(SELECT count(DISTINCT v.viewer_key) FROM v JOIN firsts f USING(viewer_key) WHERE f.first<$2),
          'follower_watch_share',coalesce((SELECT round((sum(seconds) FILTER (WHERE follower)/nullif(sum(seconds),0))::numeric,3) FROM v),0),
          'signed_in_share',coalesce((SELECT round((count(*) FILTER (WHERE viewer_key LIKE 'u:%'))::numeric/nullif(count(*),0),3) FROM v),0),
          'new_follows',(SELECT count(*) FROM follows WHERE following_id=$1 AND created_at>=$2),
          'chatters',(SELECT count(DISTINCT author_id) FROM chat_messages WHERE channel_id=$1 AND squad_id IS NULL AND created_at>=$2),
          'live_hours',round(coalesce((SELECT sum(extract(epoch FROM coalesce(ended_at,now())-greatest(started_at,$2))) FROM broadcasts WHERE owner_id=$1 AND coalesce(ended_at,now())>$2 AND started_at IS NOT NULL),0)::numeric/3600,1),
          'peak_viewers',coalesce((SELECT max(s.counted+s.trusted) FROM integrity_snapshots s JOIN broadcasts b ON b.id=s.broadcast_id WHERE b.owner_id=$1 AND s.taken_at>=$2),0),
          'avg_viewers',coalesce((SELECT round(avg(s.counted+s.trusted)::numeric,1) FROM integrity_snapshots s JOIN broadcasts b ON b.id=s.broadcast_id WHERE b.owner_id=$1 AND s.taken_at>=$2),0))")
        .bind(channel).bind(from).fetch_one(&mut *db).await?;
    // A follow "came from" a visit when that signed-in viewer followed during it or within 10 minutes.
    let sources: Vec<Value> = sqlx::query_scalar("WITH v AS (SELECT * FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2)
        SELECT jsonb_build_object('source',source,'visits',count(*),'viewers',count(DISTINCT viewer_key),
          'watch_hours',round((sum(seconds)/3600)::numeric,1),'avg_minutes',round((avg(seconds)/60)::numeric,1),
          'stayed_5_share',round((count(*) FILTER (WHERE seconds>=300))::numeric/count(*),3),
          'follows',count(DISTINCT viewer_key) FILTER (WHERE NOT follower AND EXISTS(SELECT 1 FROM follows f WHERE 'u:'||f.follower_id=v.viewer_key AND f.following_id=$1 AND f.created_at BETWEEN v.started_at AND v.last_at+interval '10 minutes')))
        FROM v GROUP BY source ORDER BY sum(seconds) DESC")
        .bind(channel).bind(from).fetch_all(&mut *db).await?;
    let lengths: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('bucket',b,'visits',n) FROM (SELECT CASE WHEN seconds<60 THEN 0 WHEN seconds<300 THEN 1 WHEN seconds<900 THEN 2 WHEN seconds<3600 THEN 3 ELSE 4 END AS b,count(*) AS n FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2 GROUP BY 1) x ORDER BY b")
        .bind(channel).bind(from).fetch_all(&mut *db).await?;
    // Impressions per surface, and visits that came from that surface (the click-through).
    let exposure: Vec<Value> = sqlx::query_scalar("WITH shown AS (SELECT i.surface,sum(i.shown) AS shown FROM discovery_impressions i JOIN broadcasts b ON b.id=i.broadcast_id WHERE b.owner_id=$1 AND i.hour>=date_trunc('hour',$2::timestamptz) GROUP BY 1),
        clicked AS (SELECT source AS surface,count(*) AS visits FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2 AND source=ANY($3) GROUP BY 1)
        SELECT jsonb_build_object('surface',surface,'shown',coalesce(shown,0),'visits',coalesce(visits,0),
          'conversion',CASE WHEN coalesce(shown,0)>0 THEN round(coalesce(visits,0)::numeric/shown,3) END)
        FROM shown FULL JOIN clicked USING(surface) ORDER BY coalesce(shown,0) DESC")
        .bind(channel).bind(from).bind(&SURFACES[..]).fetch_all(&mut *db).await?;
    // MAGNet: viewers who watched inside a MAGNet channel, and how many then opened the channel.
    let magnet: Value = sqlx::query_scalar("WITH v AS (SELECT * FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2)
        SELECT jsonb_build_object('viewers',(SELECT count(DISTINCT viewer_key) FROM v WHERE source='magnet_channel'),
          'watch_hours',round(coalesce((SELECT sum(seconds) FROM v WHERE source='magnet_channel'),0)::numeric/3600,1),
          'opened_channel',(SELECT count(DISTINCT a.viewer_key) FROM v a WHERE a.source='magnet' AND EXISTS(SELECT 1 FROM v m WHERE m.source='magnet_channel' AND m.viewer_key=a.viewer_key AND m.started_at<=a.started_at)))")
        .bind(channel).bind(from).fetch_one(&mut *db).await?;
    // Did exposure bring more people? Arrivals per live hour while on top of Live Now or featured
    // in MAGNet, against the rest of the channel's live time in the range.
    let (live_now, featured) = (
        crate::discovery::turn_windows(&mut db, channel, from).await?,
        crate::magnet::feature_windows(&mut db, channel, from).await?,
    );
    let lift: Value = sqlx::query_scalar("WITH w AS (SELECT 'live_now' AS kind,s,e FROM unnest($3::timestamptz[],$4::timestamptz[]) AS t(s,e)
            UNION ALL SELECT 'magnet',s,e FROM unnest($5::timestamptz[],$6::timestamptz[]) AS t(s,e)),
        v AS (SELECT started_at FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2),
        live AS (SELECT coalesce(sum(extract(epoch FROM coalesce(ended_at,now())-greatest(started_at,$2))),0)/3600 AS hours FROM broadcasts WHERE owner_id=$1 AND coalesce(ended_at,now())>$2 AND started_at IS NOT NULL),
        k AS (SELECT kind,sum(extract(epoch FROM e-s))/3600 AS hours,(SELECT count(*) FROM v WHERE EXISTS(SELECT 1 FROM w x WHERE x.kind=w.kind AND v.started_at>=x.s AND v.started_at<x.e)) AS arrivals FROM w GROUP BY kind),
        inside AS (SELECT count(*) AS arrivals FROM v WHERE EXISTS(SELECT 1 FROM w WHERE v.started_at>=w.s AND v.started_at<w.e)),
        exposed AS (SELECT coalesce(sum(hours),0) AS hours FROM k)
        SELECT jsonb_build_object(
          'live_now',(SELECT jsonb_build_object('hours',round(hours::numeric,2),'arrivals',arrivals,'per_hour',round((arrivals/nullif(hours,0))::numeric,1)) FROM k WHERE kind='live_now'),
          'magnet',(SELECT jsonb_build_object('hours',round(hours::numeric,2),'arrivals',arrivals,'per_hour',round((arrivals/nullif(hours,0))::numeric,1)) FROM k WHERE kind='magnet'),
          'otherwise',(SELECT jsonb_build_object('hours',round(greatest(live.hours-exposed.hours,0)::numeric,2),'arrivals',(SELECT count(*) FROM v)-inside.arrivals,
             'per_hour',round((((SELECT count(*) FROM v)-inside.arrivals)/nullif(greatest(live.hours-exposed.hours,0),0))::numeric,1)) FROM live,exposed,inside))")
        .bind(channel).bind(from)
        .bind(live_now.iter().map(|w| w.0).collect::<Vec<_>>()).bind(live_now.iter().map(|w| w.1).collect::<Vec<_>>())
        .bind(featured.iter().map(|w| w.0).collect::<Vec<_>>()).bind(featured.iter().map(|w| w.1).collect::<Vec<_>>())
        .fetch_one(&mut *db).await?;
    let referrers: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('host',referrer_host,'visits',count(*)) FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2 AND referrer_host IS NOT NULL GROUP BY referrer_host ORDER BY count(*) DESC LIMIT 10")
        .bind(channel).bind(from).fetch_all(&mut *db).await?;
    let daily: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('day',d,'watch_hours',round((sum(seconds)/3600)::numeric,1),'viewers',count(DISTINCT viewer_key),
          'discovery_hours',round((coalesce(sum(seconds) FILTER (WHERE source IN ('home','magnet','magnet_channel','browse','search','suggestion','faction')),0)/3600)::numeric,1))
        FROM (SELECT date_trunc('day',started_at) AS d,* FROM watch_visits WHERE channel_id=$1 AND counted AND started_at>=$2) x GROUP BY d ORDER BY d DESC")
        .bind(channel).bind(from).fetch_all(&mut *db).await?;
    let broadcasts: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',b.id,'started_at',b.started_at,'ended_at',b.ended_at,
          'category',(SELECT name FROM broadcast_categories c WHERE c.broadcast_id=b.id ORDER BY c.started_at LIMIT 1),
          'viewers',(SELECT count(DISTINCT viewer_key) FROM watch_visits v WHERE v.broadcast_id=b.id AND v.counted),
          'watch_hours',round(coalesce((SELECT sum(seconds) FROM watch_visits v WHERE v.broadcast_id=b.id AND v.counted),0)::numeric/3600,1),
          'avg_minutes',round(coalesce((SELECT avg(seconds) FROM watch_visits v WHERE v.broadcast_id=b.id AND v.counted),0)::numeric/60,1),
          'peak_viewers',coalesce((SELECT max(counted+trusted) FROM integrity_snapshots s WHERE s.broadcast_id=b.id),0),
          'new_follows',(SELECT count(*) FROM follows f WHERE f.following_id=b.owner_id AND f.created_at BETWEEN b.started_at AND coalesce(b.ended_at,now())),
          'top_source',(SELECT source FROM watch_visits v WHERE v.broadcast_id=b.id AND v.counted GROUP BY source ORDER BY sum(seconds) DESC LIMIT 1))
        FROM broadcasts b WHERE b.owner_id=$1 AND b.started_at IS NOT NULL AND coalesce(b.ended_at,now())>=$2 ORDER BY b.started_at DESC LIMIT 30")
        .bind(channel).bind(from).fetch_all(&mut *db).await?;
    Ok(
        json!({"from":from,"totals":totals,"sources":sources,"lengths":lengths,"exposure":exposure,
        "magnet":magnet,"lift":lift,"referrers":referrers,"daily":daily,"broadcasts":broadcasts}),
    )
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/discovery/seen", post(seen))
        .route("/api/me/analytics", get(mine))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sources_and_referrers() {
        assert_eq!(source(None), "direct");
        assert_eq!(source(Some("home")), "home");
        assert_eq!(
            source(Some("raid")),
            "other",
            "raids are decided by the server"
        );
        assert_eq!(source(Some("magnet_channel")), "other");
        assert_eq!(
            referrer_host(Some("WWW.Discord.com")).as_deref(),
            Some("discord.com")
        );
        assert_eq!(referrer_host(Some("x.com/path")), None);
        assert_eq!(referrer_host(Some("localhost")), None);
    }
}
