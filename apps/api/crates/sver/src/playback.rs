//! Module 3 playback: public live state, playback URLs and heartbeat-based viewer counts.
use crate::{
    App,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, Query, State},
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
        "is_owner": viewer.as_ref().is_some_and(|v| v.id == owner),
        "playback": {"webrtc": webrtc, "hls": hls, "preferred": preferred},
        "raid": crate::raids::for_viewers(&app, &b.id, viewer.as_ref().map(|v| v.id.as_str())).await?,
    })))
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
    Ok(Json(match outcome {
        None => json!({"recorded":false}),
        Some((level, needs_turnstile)) => {
            json!({"recorded":true,"level":level.name(),"needs_turnstile":needs_turnstile})
        }
    }))
}

#[derive(Deserialize, Default)]
struct DirectoryQuery {
    #[serde(default)]
    page: u32,
}
type DirectoryRow = (String, DateTime<Utc>, String, Option<String>, i64);

/// Public home shelves. MAGNet's weighted rotation remains Module 5.
async fn directory(
    State(app): State<App>,
    Query(query): Query<DirectoryQuery>,
) -> Res<Json<Value>> {
    if query.page > 100_000 {
        return Err(Fail::bad("Invalid page."));
    }
    let mut db = app.db.acquire().await?;
    let mut shelves = json!({"as_of": Utc::now()});
    for recent in [false, true] {
        // Page candidates by broadcast start, never viewer count. Recent channels are unique.
        let rows: Vec<DirectoryRow> = sqlx::query_as(
            "SELECT b.owner_id,b.started_at,coalesce(s.title,'Live stream'),c.name,
             (SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now() AND l.level IN ('counted','trusted'))
             FROM (SELECT DISTINCT ON (owner_id) id,owner_id,started_at,state,reconnect_deadline,end_reason FROM broadcasts ORDER BY owner_id,started_at DESC,id DESC) b
             LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id
             WHERE CASE WHEN $1 THEN b.state='ENDED' AND b.end_reason IS DISTINCT FROM 'revoked' ELSE b.state='LIVE' OR (b.state='RECONNECTING' AND b.reconnect_deadline>now()) END
             ORDER BY b.started_at DESC,b.owner_id LIMIT 25 OFFSET $2",
        ).bind(recent).bind(if recent { 0 } else { i64::from(query.page) * 24 }).fetch_all(&mut *db).await?;
        if !recent {
            shelves["has_more"] = json!(rows.len() > 24);
        }
        let ids: Vec<_> = rows.iter().take(24).map(|r| r.0.clone()).collect();
        let users = profiles::public_channels(&mut db, &ids).await?;
        let items: Vec<_> = rows.into_iter().take(24).filter_map(|(id, started_at, title, category, viewers)| {
            let user = users.iter().find(|u| u.id == id)?;
            Some(json!({"user": profiles::chip(&app, user), "banner": profiles::banner_json(&app,user.banner_key.as_deref()), "started_at":started_at,"title":title,"category":category,"viewers":viewers,"live":!recent}))
        }).collect();
        shelves[if recent { "recent" } else { "live" }] = json!(items);
    }
    Ok(Json(shelves))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/streams", get(directory))
        .route("/api/channels/{username}/live", get(live))
        .route("/api/channels/{username}/live/beat", post(beat))
}
