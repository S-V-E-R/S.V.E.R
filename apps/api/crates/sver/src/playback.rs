//! Module 3 playback: public live state, playback URLs and heartbeat-based viewer counts.
use crate::{
    App,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
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
        Ok(Self {
            hls_url: read("STREAM_HLS_URL")?,
            whep_url: read("STREAM_WHEP_URL")?,
        })
    }
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
pub async fn live(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let owner = owner_id(&app, &name).await?;
    let current: Option<Live> = sqlx::query_as("SELECT b.id,b.public_id,b.state,b.started_at,coalesce(s.title,u.username||'''s stream') AS title,c.name AS category,(SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now()) AS viewers FROM broadcasts b JOIN users u ON u.id=b.owner_id LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING')")
        .bind(&owner).fetch_optional(&app.db).await?;
    let Some(b) = current else {
        return Ok(Json(json!({"live":false})));
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
    let hls = p
        .hls_url
        .as_ref()
        .map(|u| format!("{u}/{}.m3u8", b.public_id));
    // No automatic scale switching until both paths pass the media test: WebRTC first when
    // offered, and the player falls back to HLS on failure.
    let preferred = if webrtc.is_some() { "webrtc" } else { "hls" };
    Ok(Json(json!({
        "live": true, "broadcast_id": b.id, "state": b.state, "title": b.title,
        "category": b.category, "started_at": b.started_at, "viewers": b.viewers,
        "is_owner": viewer.is_some_and(|v| v.id == owner),
        "playback": {"webrtc": webrtc, "hls": hls, "preferred": preferred},
    })))
}

#[derive(Deserialize)]
pub struct Beat {
    broadcast_id: String,
    browser_id: String,
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
        Some(v) if v.id == owner => return Ok(Json(json!({"counted":false}))),
        Some(v) if crate::moderation::banned(&app, &owner, &v.id).await? => {
            return Ok(Json(json!({"counted":false})));
        }
        Some(v) => format!("u:{}", v.id),
        None => format!("b:{}", sec::digest(id)),
    };
    sec::reserve(&app, vec![format!("playback-viewer:{key}")], 12, 60).await?;
    let counted = sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at) SELECT id,$2,now()+interval '30 seconds' FROM broadcasts WHERE id=$1 AND owner_id=$3 AND state IN ('LIVE','RECONNECTING') ON CONFLICT(broadcast_id,viewer_key) DO UPDATE SET expires_at=EXCLUDED.expires_at")
        .bind(&input.broadcast_id).bind(&key).bind(&owner).execute(&app.db).await?.rows_affected() == 1;
    Ok(Json(json!({"counted":counted})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/live", get(live))
        .route("/api/channels/{username}/live/beat", post(beat))
}
