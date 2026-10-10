//! Module 3 playback: public live state, playback URLs and heartbeat-based viewer counts.
use crate::{
    App,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, header},
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::net::SocketAddr;

/// Public playback bases. Each is optional; with neither, live state shows but nothing plays.
#[derive(Clone, Default)]
pub struct Config {
    /// HLS base including the SRS app (`https://media.example/rebuild`); serves `{base}/{id}.m3u8`.
    pub hls_url: Option<String>,
    /// SRS WHEP endpoint (`https://media.example/rtc/v1/whep`); `/?app=..&stream={id}` is appended.
    pub whep_url: Option<String>,
    /// CDN base in front of the HLS origin (`https://cdn.example/rebuild`); replaces `hls_url`.
    pub cdn_url: Option<String>,
    /// Bunny token authentication key; CDN URLs are signed for one stream's files.
    pub cdn_key: String,
    /// Shared secret the CDN sends to the origin as `X-Sver-Origin`; nginx refuses origin HLS
    /// without it, so the API's own probe (which reads `hls_url`) sends it too.
    pub origin_secret: Option<String>,
    /// Measured direct-WebRTC limits (viewers per broadcast, and across all broadcasts). Unset
    /// means no automatic switching; set privately from the load test (docs/LOAD_TEST.md).
    pub webrtc_per_broadcast: Option<i64>,
    pub webrtc_global: Option<i64>,
}
impl Config {
    pub fn from_env(production: bool) -> std::result::Result<Self, String> {
        let read = |name: &str| -> std::result::Result<Option<String>, String> {
            let value = std::env::var(name).unwrap_or_default();
            if value.is_empty() {
                return Ok(None);
            }
            let url = url::Url::parse(&value).map_err(|_| format!("Invalid {name}"))?;
            let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
            if !(url.scheme() == "https" || (!production && local && url.scheme() == "http"))
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(format!(
                    "{name} must be an HTTPS URL without query or credentials"
                ));
            }
            Ok(Some(value.trim_end_matches('/').to_string()))
        };
        let cdn_url = read("STREAM_CDN_URL")?;
        let cdn_key = std::env::var("STREAM_CDN_TOKEN_KEY").unwrap_or_default();
        if cdn_url.is_some() && cdn_key.is_empty() {
            return Err("STREAM_CDN_URL needs STREAM_CDN_TOKEN_KEY".into());
        }
        let limit = |name: &str| -> std::result::Result<Option<i64>, String> {
            match std::env::var(name).unwrap_or_default().as_str() {
                "" => Ok(None),
                v => v
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .map(Some)
                    .ok_or_else(|| format!("{name} must be a positive number")),
            }
        };
        Ok(Self {
            hls_url: read("STREAM_HLS_URL")?,
            whep_url: read("STREAM_WHEP_URL")?,
            cdn_url,
            cdn_key,
            origin_secret: std::env::var("STREAM_ORIGIN_SECRET")
                .ok()
                .filter(|v| !v.is_empty()),
            webrtc_per_broadcast: limit("STREAM_WEBRTC_PER_BROADCAST")?,
            webrtc_global: limit("STREAM_WEBRTC_GLOBAL")?,
        })
    }
}

/// Bunny directory token (docs: cdn/security/token-authentication/advanced) for one stream's
/// playlist and segments, which share the `{path}/{id}` prefix. Expiry rounds up to a six-hour
/// boundary so the URL stays the same between polls and a playing viewer never restarts.
/// ponytail: not bound to the viewer's IP (IPv4/IPv6 can differ between site and CDN), so a shared
/// URL works until it expires; bind the IP if restreaming abuse shows up.
pub fn cdn_hls(base: &str, key: &str, id: &str, now: i64) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use hmac::{Hmac, KeyInit, Mac};
    const WINDOW: i64 = 6 * 3600;
    let (origin, path) = match base
        .find("://")
        .and_then(|i| base[i + 3..].find('/').map(|j| i + 3 + j))
    {
        Some(i) => (&base[..i], &base[i..]),
        None => (base, ""),
    };
    let token_path = format!("{path}/{id}");
    let expires = (now.div_euclid(WINDOW) + 2) * WINDOW;
    let params = format!("token_ignore_params=true&token_path={token_path}");
    let mut mac =
        Hmac::<sha2::Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(format!("{token_path}{expires}{params}").as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let encoded = url::form_urlencoded::byte_serialize(token_path.as_bytes()).collect::<String>();
    format!(
        "{origin}/bcdn_token=HS256-{signature}&expires={expires}&token_ignore_params=true&token_path={encoded}{token_path}.m3u8"
    )
}

#[derive(sqlx::FromRow)]
struct Live {
    id: String,
    public_id: String,
    state: String,
    delivery: String,
    started_at: DateTime<Utc>,
    title: String,
    category: Option<String>,
    viewers: i64,
}

/// One broadcast's delivery state for the switch.
#[derive(sqlx::FromRow, Clone, Debug, PartialEq)]
pub struct Delivery {
    pub id: String,
    pub cdn: bool,
    /// Unexpired playback leases at any level: excluded viewers still use bandwidth.
    pub audience: i64,
    pub over_since: Option<DateTime<Utc>>,
    pub under_since: Option<DateTime<Utc>>,
}
/// The transport policy (docs/LIVE_STREAMS.md "Playback and capacity"). Smallest audiences take the
/// global WebRTC budget first, so it moves the biggest streams (the most capacity per switch):
/// over the limit for 30 seconds moves a broadcast to
/// the CDN; at or under 70% of it for 120 seconds brings it back. Returns the changed rows.
pub fn decide(
    mut all: Vec<Delivery>,
    per_broadcast: i64,
    global: Option<i64>,
    now: DateTime<Utc>,
) -> Vec<Delivery> {
    all.sort_by(|a, b| a.audience.cmp(&b.audience).then(a.id.cmp(&b.id)));
    let (mut direct, mut changed) = (0, Vec::new());
    for row in all {
        let mut next = row.clone();
        let total = direct + row.audience;
        if row.cdn {
            let under = row.audience * 10 <= per_broadcast * 7
                && global.is_none_or(|g| total * 10 <= g * 7);
            next.under_since = under.then(|| row.under_since.unwrap_or(now));
            next.over_since = None;
            if next
                .under_since
                .is_some_and(|t| now - t >= chrono::Duration::seconds(120))
            {
                (next.cdn, next.under_since) = (false, None);
            }
        } else {
            let over = row.audience > per_broadcast || global.is_some_and(|g| total > g);
            next.over_since = over.then(|| row.over_since.unwrap_or(now));
            next.under_since = None;
            if next
                .over_since
                .is_some_and(|t| now - t >= chrono::Duration::seconds(30))
            {
                (next.cdn, next.over_since) = (true, None);
            }
        }
        if !next.cdn {
            direct += row.audience;
        }
        if next != row {
            changed.push(next);
        }
    }
    changed
}
/// Every 5 seconds: applies `decide` to live broadcasts when a WebRTC limit and the CDN are set.
pub async fn tick(app: &App) -> Res<()> {
    let p = &app.config.playback;
    let (Some(limit), true) = (
        p.webrtc_per_broadcast,
        p.cdn_url.is_some() || p.hls_url.is_some(),
    ) else {
        return Ok(());
    };
    let rows: Vec<Delivery> = sqlx::query_as("SELECT b.id, b.delivery='cdn' AS cdn, (SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now()) AS audience, b.delivery_over_since AS over_since, b.delivery_under_since AS under_since FROM broadcasts b WHERE b.state IN ('LIVE','RECONNECTING')")
        .fetch_all(&app.db)
        .await?;
    for row in decide(rows, limit, p.webrtc_global, Utc::now()) {
        sqlx::query("UPDATE broadcasts SET delivery=$2, delivery_over_since=$3, delivery_under_since=$4 WHERE id=$1")
            .bind(&row.id)
            .bind(if row.cdn { "cdn" } else { "webrtc" })
            .bind(row.over_since)
            .bind(row.under_since)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}

/// SQL expression: whether the user whose id is the SQL expression `id` has a public broadcast.
pub fn live_sql(id: &str) -> String {
    format!(
        "EXISTS(SELECT 1 FROM broadcasts lb WHERE lb.owner_id={id} AND lb.state IN ('LIVE','RECONNECTING'))"
    )
}
pub async fn is_live(db: &mut PgConnection, owner: &str) -> sqlx::Result<bool> {
    // The live expression uses the literal $1 placeholder; the owner is bound.
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT {}", live_sql("$1"))))
        .bind(owner)
        .fetch_one(db)
        .await
}

async fn owner_id(app: &App, name: &str) -> Res<String> {
    let mut conn = app.db.acquire().await?;
    Ok(profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id)
}

/// Only confirmed media (LIVE) or the reconnect grace (RECONNECTING) is public; STARTING is not.
#[derive(Deserialize)]
pub struct LiveQuery {
    transport: Option<String>,
}
pub async fn live(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    axum::extract::Query(query): axum::extract::Query<LiveQuery>,
) -> Res<Json<Value>> {
    let owner = owner_id(&app, &name).await?;
    let current: Option<Live> = sqlx::query_as("SELECT b.id,b.public_id,b.state,b.delivery,b.started_at,coalesce(s.title,u.username||'''s stream') AS title,c.name AS category,(SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now() AND l.level IN ('counted','trusted')) AS viewers FROM broadcasts b JOIN users u ON u.id=b.owner_id LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING')")
        .bind(&owner).fetch_optional(&app.db).await?;
    let Some(b) = current else {
        // An offline channel may host a live one; its page shows that stream.
        return Ok(Json(match crate::raids::hosting(&app, &owner).await? {
            Some(hosting) => json!({"live": false, "hosting": hosting}),
            None => json!({"live": false}),
        }));
    };
    let viewer = profiles::viewer(&app, &jar).await?;
    let mature: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT mature FROM stream_settings WHERE owner_id=$1),false)",
    )
    .bind(&owner)
    .fetch_one(&app.db)
    .await?;
    // Adults who chose "Don't warn me about mature streams" skip the warning screen.
    let skip_warning: bool = match &viewer {
        Some(v) => sqlx::query_scalar("SELECT skip_mature_warning AND coalesce(date_of_birth<=current_date-interval '18 years',false) FROM users WHERE id=$1")
            .bind(&v.id)
            .fetch_one(&app.db)
            .await?,
        None => false,
    };
    // Mature label: an under-18 account gets no playback (adults and guests see a warning first).
    if crate::streams::mature_blocked(
        &mut *app.db.acquire().await?,
        &owner,
        viewer.as_ref().map(|v| v.id.as_str()),
    )
    .await?
    {
        return Ok(Json(json!({
            "live": true, "broadcast_id": b.id, "state": b.state, "title": b.title,
            "category": b.category, "started_at": b.started_at, "viewers": b.viewers,
            "is_owner": false, "mature": true, "mature_blocked": true, "playback": null,
        })));
    }
    // A channel ban refuses signed-in playback (logged-out viewing cannot be prevented).
    if let Some(v) = &viewer
        && crate::moderation::banned(&app, &owner, &v.id).await?
    {
        return Ok(Json(json!({
            "live": true, "broadcast_id": b.id, "state": b.state, "title": b.title,
            "category": b.category, "started_at": b.started_at, "viewers": b.viewers,
            "is_owner": false, "banned": true, "playback": null,
        })));
    }
    let stream_app = app
        .config
        .streaming
        .as_ref()
        .map_or("rebuild", |c| c.app.as_str());
    let p = &app.config.playback;
    let webrtc = p
        .whep_url
        .as_ref()
        .map(|u| format!("{u}/?app={stream_app}&stream={}", b.public_id));
    let hls = match &p.cdn_url {
        Some(cdn) => Some(cdn_hls(
            cdn,
            &p.cdn_key,
            &b.public_id,
            Utc::now().timestamp(),
        )),
        None => p
            .hls_url
            .as_ref()
            .map(|u| format!("{u}/{}.m3u8", b.public_id)),
    };
    // Over the measured WebRTC limit the broadcast is on the CDN (`tick`): no WHEP URL is offered,
    // and open players move within one 15-second poll. Otherwise WebRTC first; the player falls
    // back to HLS on failure, and `?transport=hls` (latency tests, or a viewer who prefers it)
    // asks for HLS. Asking for HLS is always allowed.
    // The nginx playback gate refuses WHEP for a broadcast on the CDN (streams::authorize_playback).
    let webrtc = webrtc.filter(|_| b.delivery == "webrtc" || hls.is_none());
    let preferred = if webrtc.is_some() && query.transport.as_deref() != Some("hls") {
        "webrtc"
    } else {
        "hls"
    };
    Ok(Json(json!({
        "live": true, "broadcast_id": b.id, "state": b.state, "title": b.title,
        "category": b.category, "started_at": b.started_at, "viewers": b.viewers,
        "is_owner": viewer.as_ref().is_some_and(|v| v.id == owner), "mature": mature,
        "mature_warn": mature && !skip_warning,
        "playback": {"webrtc": webrtc, "hls": hls, "preferred": preferred},
        "raid": crate::raids::for_viewers(&app, &b.id, viewer.as_ref().map(|v| v.id.as_str())).await?,
    })))
}

/// GET /api/channels/{username}/overlay: public numbers for a creator's own OBS browser source.
/// Those load from local files, so any origin may read it; it uses no cookies and returns only
/// what the channel page already shows.
pub async fn overlay(State(app): State<App>, Path(name): Path<String>) -> Res<impl IntoResponse> {
    let owner = owner_id(&app, &name).await?;
    let mut db = app.db.acquire().await?;
    let stream: Option<(String, Option<String>, i64)> = sqlx::query_as("SELECT coalesce(s.title,u.username||'''s stream'),c.name,(SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now() AND l.level IN ('counted','trusted')) FROM broadcasts b JOIN users u ON u.id=b.owner_id LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING')")
        .bind(&owner).fetch_optional(&mut *db).await?;
    let (followers, _) = profiles::follower_counts(&mut db, &owner).await?;
    let stream = stream.map(
        |(title, category, viewers)| json!({"title":title,"category":category,"viewerCount":viewers}),
    );
    Ok((
        [
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Json(
            json!({"isLive":stream.is_some(),"stream":stream,"stats":{"followerCount":followers}}),
        ),
    ))
}

#[derive(Deserialize)]
pub struct Beat {
    broadcast_id: String,
    browser_id: String,
    /// Whether the page was visible since the last beat.
    #[serde(default)]
    visible: bool,
    /// The player's media time in seconds.
    #[serde(default)]
    media_time: Option<f64>,
    /// A Turnstile token, sent once when the server asks for the security check.
    #[serde(default)]
    turnstile: Option<String>,
    /// The raid that brought this viewer, from the raid link; counted only within 60 seconds.
    #[serde(default)]
    raid: Option<String>,
    /// The MAGNet Hype lane this viewer is watching from, for the streamer's feature history.
    #[serde(default)]
    magnet: Option<String>,
}

/// Sent every ten seconds by a player whose media is advancing. Signed-in viewers count once per
/// account, guests once per browser ID; never merged by IP. The owner's own preview never counts.
pub async fn beat(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Beat>,
) -> Res<Json<Value>> {
    let id = &input.browser_id;
    if !(16..=64).contains(&id.len()) || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err(Fail::bad("Invalid browser ID."));
    }
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("playback-beat:{ip}")], 240, 60).await?;
    let owner = owner_id(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    let key = match &viewer {
        Some(v) if v.id == owner => return Ok(Json(json!({"recorded":false}))),
        Some(v) if crate::moderation::banned(&app, &owner, &v.id).await? => {
            return Ok(Json(json!({"recorded":false})));
        }
        Some(v) => format!("u:{}", v.id),
        None => format!("b:{}", sec::digest(id)),
    };
    sec::reserve(&app, vec![format!("playback-viewer:{key}")], 12, 60).await?;
    let verified = match &viewer {
        Some(v) => {
            v.email_verified
                && sqlx::query_scalar::<_, bool>(
                    "SELECT coalesce((SELECT eligible FROM channel_users WHERE id=$1),false)",
                )
                .bind(&v.id)
                .fetch_one(&app.db)
                .await?
        }
        None => false,
    };
    let media_time = input.media_time.filter(|m| m.is_finite() && *m >= 0.0);
    let report = crate::integrity::Report {
        visible: input.visible,
        media_time,
        turnstile: input.turnstile.as_deref(),
    };
    let outcome = crate::integrity::record(
        &app,
        &input.broadcast_id,
        &owner,
        &key,
        viewer.is_some(),
        verified,
        ip,
        report,
    )
    .await?;
    if let (Some(_), Some(raid)) = (&outcome, &input.raid) {
        crate::raids::arrived(&app, &input.broadcast_id, &key, raid).await?;
    }
    if let (Some(_), Some(lane)) = (&outcome, &input.magnet) {
        // Only a session that started while that lane features this broadcast is MAGNet's: the
        // tag keeps it out of the stream's moment signals and explains its arrival to viewer
        // integrity, so a client can't claim it for any other session.
        sqlx::query("UPDATE playback_leases l SET magnet_lane=$3 WHERE l.broadcast_id=$1 AND l.viewer_key=$2 AND l.magnet_lane IS NULL AND EXISTS(SELECT 1 FROM magnet_features f WHERE f.lane=$3 AND f.broadcast_id=$1 AND f.ended_at IS NULL AND l.created_at>=f.started_at-interval '15 seconds')")
            .bind(&input.broadcast_id).bind(&key).bind(lane).execute(&app.db).await?;
    }
    Ok(Json(match outcome {
        None => json!({"recorded":false}),
        Some((level, needs_turnstile)) => {
            json!({"recorded":true,"level":level.name(),"needs_turnstile":needs_turnstile})
        }
    }))
}

type DirectoryRow = (String, DateTime<Utc>, String, Option<String>, i64);

/// A circular queue advances one place every 30 seconds. Every faction stream gets a turn;
/// viewer counts only label the result. Module 5 can replace this with its full MAGNet scheduler.
pub async fn faction_streams(
    app: &App,
    db: &mut PgConnection,
    faction: &str,
    at: DateTime<Utc>,
) -> Res<Vec<Value>> {
    let rows:Vec<DirectoryRow>=sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH live AS (SELECT b.*,row_number() OVER(ORDER BY b.started_at,b.id)-1 AS pos,count(*) OVER() AS total FROM broadcasts b WHERE b.state='LIVE' AND {}=$1)
         SELECT b.owner_id,b.started_at,coalesce(s.title,'Live stream'),c.name,(SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now() AND l.level IN ('counted','trusted')) FROM live b LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id ORDER BY (b.pos-$2%b.total+b.total)%b.total LIMIT 12",
        crate::factions::membership_sql("b.owner_id"))))
        .bind(faction).bind(at.timestamp().div_euclid(30)).fetch_all(&mut *db).await?;
    let users =
        profiles::public_channels(db, &rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>())
            .await?;
    Ok(rows.into_iter().filter_map(|(id,started_at,title,category,viewers)| {
        users.iter().find(|u|u.id==id).map(|u|json!({"user":profiles::chip(app,u),"banner":profiles::banner_json(app,u.banner_key.as_deref()),"started_at":started_at,"title":title,"category":category,"viewers":viewers,"live":true}))
    }).take(6).collect())
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/live", get(live))
        .route("/api/channels/{username}/overlay", get(overlay))
        .route("/api/channels/{username}/live/beat", post(beat))
}

#[cfg(test)]
mod cdn_tests {
    #[test]
    fn signed_url_matches_bunny_directory_tokens() {
        // Expected value from the legacy signer (same algorithm, same inputs).
        let url = super::cdn_hls(
            "https://cdn.example/rebuild",
            "example-key",
            "abc",
            1_000_000_000,
        );
        assert_eq!(
            url,
            "https://cdn.example/bcdn_token=HS256-WdByBiFlvZtzVQxRRq9KW0QDxyWuflom-74Mllaz6Yk&expires=1000036800&token_ignore_params=true&token_path=%2Frebuild%2Fabc/rebuild/abc.m3u8"
        );
        // Stable within the window, so polling never restarts playback; at least six hours left.
        assert_eq!(
            url,
            super::cdn_hls(
                "https://cdn.example/rebuild",
                "example-key",
                "abc",
                1_000_000_000 + 1500
            )
        );
        let expires: i64 = url
            .split("expires=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(expires - 1_000_000_000 >= 6 * 3600);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: &str, cdn: bool, audience: i64) -> Delivery {
        Delivery {
            id: id.into(),
            cdn,
            audience,
            over_since: None,
            under_since: None,
        }
    }
    #[test]
    fn switches_with_hysteresis_and_the_global_budget() {
        let t0 = Utc::now();
        let secs = |s| t0 + chrono::Duration::seconds(s);
        // Over the limit: noted, then moved after 30 seconds.
        let noted = decide(vec![row("a", false, 71)], 70, None, t0);
        assert_eq!(noted[0].over_since, Some(t0));
        assert!(!noted[0].cdn);
        assert!(
            decide(noted.clone(), 70, None, secs(29)).is_empty(),
            "still waiting"
        );
        let moved = decide(noted, 70, None, secs(30));
        assert!(moved[0].cdn);
        // A dip below the limit resets the clock.
        let dip = decide(
            vec![Delivery {
                over_since: Some(t0),
                ..row("a", false, 70)
            }],
            70,
            None,
            secs(40),
        );
        assert_eq!(dip[0].over_since, None);
        // Back only at or under 70% (49 of 70) for 120 seconds.
        assert!(decide(vec![row("a", true, 50)], 70, None, t0).is_empty());
        let under = decide(vec![row("a", true, 49)], 70, None, t0);
        assert!(decide(under.clone(), 70, None, secs(119)).is_empty());
        assert!(!decide(under, 70, None, secs(120))[0].cdn);
        // The global budget moves the biggest stream first.
        let global = decide(
            vec![row("small", false, 30), row("big", false, 60)],
            70,
            Some(80),
            t0,
        );
        assert_eq!(
            global.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["big"]
        );
        let after = decide(
            vec![
                row("small", false, 30),
                Delivery {
                    over_since: Some(t0),
                    ..row("big", false, 60)
                },
            ],
            70,
            Some(80),
            secs(30),
        );
        assert!(after[0].cdn && after[0].id == "big");
    }
}
