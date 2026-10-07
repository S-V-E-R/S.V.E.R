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
        Ok(Self {
            hls_url: read("STREAM_HLS_URL")?,
            whep_url: read("STREAM_WHEP_URL")?,
            cdn_url,
            cdn_key,
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
    started_at: DateTime<Utc>,
    title: String,
    category: Option<String>,
    viewers: i64,
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
    let current: Option<Live> = sqlx::query_as("SELECT b.id,b.public_id,b.state,b.started_at,coalesce(s.title,u.username||'''s stream') AS title,c.name AS category,(SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now() AND l.level IN ('counted','trusted')) AS viewers FROM broadcasts b JOIN users u ON u.id=b.owner_id LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING')")
        .bind(&owner).fetch_optional(&app.db).await?;
    let Some(b) = current else {
        // An offline channel may host a live one; its page shows that stream.
        return Ok(Json(match crate::raids::hosting(&app, &owner).await? {
            Some(hosting) => json!({"live": false, "hosting": hosting}),
            None => json!({"live": false}),
        }));
    };
    let viewer = profiles::viewer(&app, &jar).await?;
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
    // No automatic scale switching until the load test measures the WebRTC limit: WebRTC first
    // when offered, the player falls back to HLS on failure, and `?transport=hls` (latency tests,
    // or a viewer who prefers it) asks for HLS. Asking for HLS is always allowed.
    let preferred = if webrtc.is_some() && query.transport.as_deref() != Some("hls") {
        "webrtc"
    } else {
        "hls"
    };
    Ok(Json(json!({
        "live": true, "broadcast_id": b.id, "state": b.state, "title": b.title,
        "category": b.category, "started_at": b.started_at, "viewers": b.viewers,
        "is_owner": viewer.as_ref().is_some_and(|v| v.id == owner),
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
