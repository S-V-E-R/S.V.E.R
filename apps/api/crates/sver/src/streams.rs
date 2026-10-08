//! Module 3 stream credentials and persisted ingest lifecycle.
pub mod catalog;
use crate::{App, Error, Result, auth, profiles, security as sec};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration as Timeout,
};

#[derive(Clone)]
pub struct Config {
    pub ingest_url: String,
    /// Optional WHIP and SRT ingest (docs/DEVELOPER_PLATFORM.md §5): the public WHIP endpoint
    /// (`https://stream.example/rebuild/whip/`) and SRT address (`srt://stream.example:10081`).
    pub whip_url: Option<String>,
    pub srt_url: Option<String>,
    pub api_url: String,
    pub hook_secret: String,
    pub hook_ip: IpAddr,
    pub vhost: String,
    pub app: String,
}
impl Config {
    pub fn from_env() -> std::result::Result<Option<Self>, String> {
        let env = |name| std::env::var(name).unwrap_or_default();
        let names = [
            "STREAM_INGEST_URL",
            "STREAM_SRS_API_URL",
            "STREAM_HOOK_SECRET",
            "STREAM_HOOK_IP",
        ];
        let values: Vec<String> = names.iter().map(env).collect();
        if values.iter().all(String::is_empty) {
            return Ok(None);
        }
        let config = Self {
            ingest_url: values[0].trim_end_matches('/').into(),
            whip_url: std::env::var("STREAM_WHIP_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            srt_url: std::env::var("STREAM_SRT_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            api_url: values[1].trim_end_matches('/').into(),
            hook_secret: values[2].clone(),
            hook_ip: values[3]
                .parse()
                .map_err(|_| "STREAM_HOOK_IP must be the direct hook proxy IP")?,
            vhost: "__defaultVhost__".into(),
            app: "rebuild".into(),
        };
        let api = url::Url::parse(&config.api_url).map_err(|_| "Invalid STREAM_SRS_API_URL")?;
        let ingest =
            url::Url::parse(&config.ingest_url).map_err(|_| "Invalid STREAM_INGEST_URL")?;
        let private = api
            .host_str()
            .and_then(|h| h.parse::<IpAddr>().ok())
            .is_some_and(|ip| match ip {
                IpAddr::V4(v) => v.is_loopback() || v.is_private(),
                IpAddr::V6(v) => v.is_loopback() || v.is_unique_local(),
            });
        if api.scheme() != "http"
            || !private
            || api.path() != "/"
            || api.query().is_some()
            || api.fragment().is_some()
            || !api.username().is_empty()
            || api.password().is_some()
            || ingest.scheme() != "rtmp"
            || ingest.host_str().is_none()
            || ingest.path() != "/rebuild"
            || ingest.query().is_some()
            || ingest.fragment().is_some()
            || !ingest.username().is_empty()
            || ingest.password().is_some()
            || config.hook_secret.len() < 32
            || config.hook_secret.len() > 256
            || !config.hook_secret.bytes().all(|c| c.is_ascii_graphic())
        {
            return Err("Streaming needs a private HTTP SRS API origin, RTMP /rebuild ingest and a 32+ character hook secret".into());
        }
        Ok(Some(config))
    }
}
fn configured(app: &App) -> Result<&Config> {
    app.config.streaming.as_ref().ok_or_else(Error::unavailable)
}
fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
// Only paths emitted by our HLS config and the public WHEP endpoint may reach SRS.
fn playback_id(uri: &str) -> Option<String> {
    if uri.len() > 1024 {
        return None;
    }
    let uri: axum::http::Uri = uri.parse().ok()?;
    if uri.scheme().is_some() || uri.authority().is_some() {
        return None;
    }
    let id = if uri.path() == "/rebuild/whep/" {
        let params: Vec<_> = url::form_urlencoded::parse(uri.query()?.as_bytes()).collect();
        if params.len() != 2
            || params
                .iter()
                .filter(|(k, v)| k == "app" && v == "rebuild")
                .count()
                != 1
        {
            return None;
        }
        params.iter().find(|(k, _)| k == "stream")?.1.to_string()
    } else {
        let file = uri.path().strip_prefix("/rebuild/")?;
        if let Some(id) = file.strip_suffix(".m3u8") {
            id.to_string()
        } else {
            let parts: Vec<_> = file.strip_suffix(".ts")?.split('-').collect();
            if parts.len() != 3
                || !parts[1..].iter().all(|p| {
                    !p.is_empty() && p.len() <= 20 && p.bytes().all(|b| b.is_ascii_digit())
                })
            {
                return None;
            }
            parts[0].to_string()
        }
    };
    (id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(id)
}
/// Nginx auth_request gate. A stopped/revoked publisher must not leave replayable HLS files.
async fn authorize_playback(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    authorize_hook(&app, peer, &headers)?;
    let id = headers
        .get("x-original-uri")
        .and_then(|v| v.to_str().ok())
        .and_then(playback_id)
        .ok_or_else(|| Error::denied("Playback is unavailable."))?;
    let owner: Option<String> = sqlx::query_scalar("SELECT b.owner_id FROM broadcasts b JOIN stream_credentials c ON c.owner_id=b.owner_id AND c.public_id=b.public_id AND c.generation=b.generation WHERE b.public_id=$1 AND c.revoked_at IS NULL AND (b.state='LIVE' OR (b.state='RECONNECTING' AND b.reconnect_deadline>clock_timestamp()))")
        .bind(id).fetch_optional(&app.db).await?;
    if let Some(owner) = owner {
        let mut db = app.db.acquire().await?;
        if profiles::channel_user_by_id(&mut db, &owner)
            .await
            .map_err(|_| Error::unavailable())?
            .is_some_and(|u| u.eligible)
        {
            return Ok(StatusCode::NO_CONTENT);
        }
    }
    Err(Error::denied("Playback is unavailable."))
}
fn conflict(message: &'static str) -> Error {
    Error(StatusCode::CONFLICT, message, None)
}

pub async fn eligible(db: &mut PgConnection, user: &auth::User) -> Result<bool> {
    if user.deleted_at.is_some()
        || user.legacy_deletion_hold
        || !user.email_verified
        || !user.mfa_enabled
    {
        return Ok(false);
    }
    Ok(profiles::eligible_by_name(db, &user.username)
        .await
        .map_err(|_| Error::unavailable())?
        .is_some_and(|channel| channel.id == user.id))
}
async fn require_eligible(db: &mut PgConnection, user: &auth::User) -> Result<()> {
    if !eligible(db, user).await? {
        return Err(Error::denied(
            "Streaming requires a verified email, an authenticator and a channel in good standing.",
        ));
    }
    Ok(())
}
async fn settings(db: &mut PgConnection, user: &auth::User) -> Result<()> {
    sqlx::query("INSERT INTO stream_settings(owner_id,title) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(&user.id)
        .bind(format!("{}'s stream", user.username))
        .execute(db)
        .await?;
    Ok(())
}
#[derive(FromRow)]
struct Broadcast {
    id: String,
    public_id: String,
    generation: i64,
    state: String,
    server_id: String,
    service_id: String,
    client_id: String,
    startup_deadline: DateTime<Utc>,
    publisher_started_at: DateTime<Utc>,
    reconnect_deadline: Option<DateTime<Utc>>,
    recv_bytes: Option<i64>,
    checked_at: Option<DateTime<Utc>>,
}
async fn current(db: &mut PgConnection, owner: &str) -> Result<Option<Broadcast>> {
    Ok(
        sqlx::query_as("SELECT * FROM broadcasts WHERE owner_id=$1 AND state<>'ENDED'")
            .bind(owner)
            .fetch_optional(db)
            .await?,
    )
}
async fn clock(db: &mut PgConnection) -> Result<DateTime<Utc>> {
    Ok(sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(db)
        .await?)
}
async fn end(db: &mut PgConnection, owner: &str, reason: &str) -> Result<()> {
    sqlx::query("INSERT INTO stream_stop_jobs(server_id,service_id,client_id,public_id,owner_id) SELECT server_id,service_id,client_id,public_id,owner_id FROM broadcasts WHERE owner_id=$1 AND state<>'ENDED' ON CONFLICT DO NOTHING")
        .bind(owner).execute(&mut *db).await?;
    sqlx::query("UPDATE stream_publishers SET retired_at=coalesce(retired_at,clock_timestamp()) WHERE broadcast_id IN (SELECT id FROM broadcasts WHERE owner_id=$1 AND state<>'ENDED')")
        .bind(owner).execute(&mut *db).await?;
    sqlx::query("UPDATE broadcasts SET state='ENDED',ended_at=clock_timestamp(),end_reason=$2,reconnect_deadline=NULL WHERE owner_id=$1 AND state<>'ENDED'")
        .bind(owner).bind(reason).execute(&mut *db).await?;
    crate::videos::ended(db, owner)
        .await
        .map_err(|_| Error::internal())?;
    Ok(())
}
/// Public security interface. Takes the account lifecycle lock in the caller's transaction.
/// The durable stop request survives deletion of the account and API/SRS outages.
pub async fn revoke(db: &mut PgConnection, owner: &str) -> Result<()> {
    auth::stream_owner(db, owner).await?;
    sqlx::query("UPDATE stream_credentials SET revoked_at=coalesce(revoked_at,clock_timestamp()),secret_hash=NULL,secret_cipher=NULL WHERE owner_id=$1")
        .bind(owner).execute(&mut *db).await?;
    end(db, owner, "revoked").await
}
async fn expire(db: &mut PgConnection, owner: &str) -> Result<()> {
    if let Some(b) = current(db, owner).await? {
        let now = clock(db).await?;
        if b.state == "RECONNECTING" && b.reconnect_deadline.is_some_and(|at| at <= now) {
            end(db, owner, "reconnect_timeout").await?;
        } else if b.state == "STARTING" && b.startup_deadline <= now {
            end(db, owner, "startup_timeout").await?;
        }
    }
    Ok(())
}
async fn disconnect(db: &mut PgConnection, b: &Broadcast) -> Result<()> {
    if matches!(b.state.as_str(), "STARTING" | "LIVE") {
        sqlx::query("UPDATE broadcasts SET state='RECONNECTING',reconnect_deadline=clock_timestamp()+interval '60 seconds',health='{}' WHERE id=$1 AND state IN ('STARTING','LIVE')")
            .bind(&b.id).execute(&mut *db).await?;
        sqlx::query("UPDATE stream_publishers SET retired_at=clock_timestamp() WHERE server_id=$1 AND service_id=$2 AND client_id=$3 AND retired_at IS NULL")
            .bind(&b.server_id).bind(&b.service_id).bind(&b.client_id).execute(db).await?;
    }
    Ok(())
}
#[derive(Deserialize, Default)]
pub struct CategoryQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    include: String,
}
pub async fn categories(
    State(app): State<App>,
    Query(query): Query<CategoryQuery>,
) -> Result<Json<Value>> {
    if query.q.chars().count() > 80
        || query.include.len() > 80
        || query.q.chars().any(char::is_control)
    {
        return Err(Error::bad("Use a search of up to 80 characters."));
    }
    let tokens: Vec<_> = query.q.split_whitespace().map(str::to_lowercase).collect();
    let categories: Vec<Value> = sqlx::query_scalar("WITH choices AS (SELECT c.id,c.name,c.genre,coalesce((SELECT string_agg(g.name||' '||array_to_string(g.aliases,' '),' ') FROM game_catalog g WHERE g.category_id=c.id),'') AS aliases FROM stream_categories c WHERE c.active UNION ALL SELECT 'wikidata-'||lower(g.source_id),g.name,g.suggested_genre,array_to_string(g.aliases,' ') FROM game_catalog g WHERE g.category_id IS NULL AND g.suggested_genre IS NOT NULL AND NOT g.reviewed AND NOT EXISTS(SELECT 1 FROM stream_categories c WHERE lower(c.name)=lower(g.name))) SELECT jsonb_build_object('id',id,'name',name,'genre',genre) FROM choices WHERE id=$2 OR NOT EXISTS(SELECT 1 FROM unnest($1::text[]) t WHERE position(t IN lower(name||' '||aliases))=0) ORDER BY (id=$2) DESC,lower(name),id LIMIT 50")
        .bind(tokens).bind(query.include).fetch_all(&app.db).await?;
    Ok(Json(json!({"categories":categories})))
}
/// Confirmed advancing media and its current genre; callers cannot award to a client-picked genre.
pub async fn influence_context(
    db: &mut PgConnection,
    broadcast: &str,
) -> profiles::Res<Option<(String, String)>> {
    Ok(sqlx::query_as("SELECT b.owner_id,c.genre FROM broadcasts b JOIN stream_settings s ON s.owner_id=b.owner_id JOIN stream_categories c ON c.id=s.category_id WHERE b.id=$1 AND b.state='LIVE' AND b.observed_at>clock_timestamp()-interval '20 seconds'")
        .bind(broadcast).fetch_optional(db).await?)
}
pub async fn has_streamed(db: &mut PgConnection, owner: &str) -> profiles::Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1 AND confirmed_live_at IS NOT NULL)").bind(owner).fetch_one(db).await?)
}
pub async fn broadcast_active(
    db: &mut PgConnection,
    owner: &str,
    broadcast: &str,
) -> profiles::Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM broadcasts WHERE id=$1 AND owner_id=$2 AND state IN ('LIVE','RECONNECTING'))")
        .bind(broadcast).bind(owner).fetch_one(db).await?)
}
pub async fn live_broadcast(db: &mut PgConnection, owner: &str) -> profiles::Res<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM broadcasts WHERE owner_id=$1 AND state='LIVE'")
            .bind(owner)
            .fetch_optional(db)
            .await?,
    )
}
#[derive(FromRow)]
pub struct RecordingContext {
    pub broadcast_id: String,
    pub owner_id: String,
    pub title: String,
    pub category_id: Option<String>,
    pub category: Option<String>,
    pub genre: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Labeled mature at any point; its VOD and clips inherit it.
    pub mature: bool,
}
/// Mature label (docs/CHANNEL_ADDITIONS.md): a signed-in viewer under 18 can't watch or chat in a
/// channel labeled mature. Adults and signed-out visitors get a warning screen in the player.
pub async fn mature_blocked(
    db: &mut PgConnection,
    channel: &str,
    viewer: Option<&str>,
) -> profiles::Res<bool> {
    let Some(viewer) = viewer else {
        return Ok(false);
    };
    let labeled: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT mature FROM stream_settings WHERE owner_id=$1),false)",
    )
    .bind(channel)
    .fetch_one(&mut *db)
    .await?;
    Ok(labeled && viewer != channel && !auth::is_adult(db, viewer).await?)
}
/// Live channels labeled mature, when `viewer` is a signed-in under-18 account (otherwise none).
/// Lists drop them after their own ordering, so fair rotation is unchanged.
pub async fn mature_hidden(
    db: &mut PgConnection,
    viewer: Option<&str>,
) -> profiles::Res<std::collections::HashSet<String>> {
    let Some(viewer) = viewer else {
        return Ok(Default::default());
    };
    if auth::is_adult(&mut *db, viewer).await? {
        return Ok(Default::default());
    }
    Ok(sqlx::query_scalar("SELECT s.owner_id FROM stream_settings s WHERE s.mature AND s.owner_id<>$1 AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=s.owner_id AND b.state IN ('LIVE','RECONNECTING'))")
        .bind(viewer)
        .fetch_all(db)
        .await?
        .into_iter()
        .collect())
}
/// SRS media callbacks can resolve only an accepted publisher, including its final segments.
pub async fn recording_context(
    db: &mut PgConnection,
    server: &str,
    client: &str,
    stream: &str,
) -> profiles::Res<Option<RecordingContext>> {
    Ok(sqlx::query_as("SELECT b.id AS broadcast_id,b.owner_id,s.title,s.category_id,c.name AS category,c.genre,b.started_at,b.ended_at,b.mature FROM stream_publishers p JOIN broadcasts b ON b.id=p.broadcast_id JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id WHERE p.server_id=$1 AND p.client_id=$2 AND p.public_id=$3 AND (p.retired_at IS NULL OR p.retired_at>now()-interval '30 seconds') AND (b.ended_at IS NULL OR b.ended_at>now()-interval '30 seconds') ORDER BY p.retired_at NULLS FIRST LIMIT 1")
        .bind(server).bind(client).bind(stream).fetch_optional(db).await?)
}
pub async fn mine(State(app): State<App>, jar: CookieJar) -> Result<Json<Value>> {
    let (mut tx, user, _) = auth::session(&app, &jar, false).await?;
    settings(&mut tx, &user).await?;
    expire(&mut tx, &user.id).await?;
    let allowed = eligible(&mut tx, &user).await?;
    let metadata: Value = sqlx::query_scalar("SELECT jsonb_build_object('title',title,'category_id',category_id,'revision',revision,'mature',mature,'mature_locked',EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1 AND mature_locked AND state<>'ENDED')) FROM stream_settings WHERE owner_id=$1")
        .bind(&user.id).fetch_one(&mut *tx).await?;
    let credential: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('created_at',created_at,'revoked',revoked_at IS NOT NULL) FROM stream_credentials WHERE owner_id=$1")
        .bind(&user.id).fetch_optional(&mut *tx).await?;
    let broadcast: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'state',state,'started_at',started_at,'reconnect_deadline',reconnect_deadline,'ended_at',ended_at,'end_reason',end_reason,'observed_at',observed_at,'health',health) FROM broadcasts WHERE owner_id=$1 ORDER BY started_at DESC,id DESC LIMIT 1")
        .bind(&user.id).fetch_optional(&mut *tx).await?;
    let pending: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_stop_jobs WHERE owner_id=$1)")
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    let last = crate::integrity::last_broadcast(&mut tx, &user.id)
        .await
        .map_err(|_| Error::internal())?;
    tx.commit().await?;
    Ok(Json(
        json!({"configured":app.config.streaming.is_some(),"eligible":allowed,"settings":metadata,
        "credential":credential,"broadcast":broadcast,"disconnect_pending":pending,"last_broadcast":last}),
    ))
}
#[derive(Deserialize)]
pub struct Metadata {
    title: String,
    category_id: String,
    revision: i64,
    #[serde(default)]
    mature: Option<bool>,
}
pub async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Metadata>,
) -> Result<Json<Value>> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 140 || title.chars().any(char::is_control) {
        return Err(Error::bad(
            "Use a stream title with 1–140 characters and no control characters.",
        ));
    }
    let (mut tx, user, session) = auth::session(&app, &jar, false).await?;
    auth::authorize_streaming(&user, &session)?;
    require_eligible(&mut tx, &user).await?;
    settings(&mut tx, &user).await?;
    let category_id = catalog::select(&mut tx, &input.category_id)
        .await
        .map_err(|e| Error(e.status, "Choose an available stream category.", None))?;
    let active: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_categories WHERE id=$1 AND active)")
            .bind(&category_id)
            .fetch_one(&mut *tx)
            .await?;
    if !active {
        return Err(Error::bad("Choose an available stream category."));
    }
    let previous: Option<String> =
        sqlx::query_scalar("SELECT category_id FROM stream_settings WHERE owner_id=$1")
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    let locked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1 AND mature_locked AND state<>'ENDED')")
        .bind(&user.id).fetch_one(&mut *tx).await?;
    if locked && input.mature == Some(false) {
        return Err(Error::bad(
            "Staff labeled this broadcast mature; the label stays on until it ends.",
        ));
    }
    let revision: Option<i64> = sqlx::query_scalar("UPDATE stream_settings SET title=$2,category_id=$3,mature=coalesce($5,mature),revision=revision+1,updated_at=clock_timestamp() WHERE owner_id=$1 AND revision=$4 RETURNING revision")
        .bind(&user.id).bind(title).bind(&category_id).bind(input.revision).bind(input.mature).fetch_optional(&mut *tx).await?;
    let revision = revision
        .ok_or_else(|| conflict("This changed in another tab. Reload to see the latest."))?;
    // A broadcast labeled at any point stays labeled (its VOD and clips inherit it).
    sqlx::query("UPDATE broadcasts b SET mature=true FROM stream_settings s WHERE b.owner_id=$1 AND s.owner_id=$1 AND s.mature AND b.state<>'ENDED'")
        .bind(&user.id).execute(&mut *tx).await?;
    if previous.as_deref() != Some(category_id.as_str()) {
        let label: String = sqlx::query_scalar("SELECT name FROM stream_categories WHERE id=$1")
            .bind(&category_id)
            .fetch_one(&mut *tx)
            .await?;
        crate::videos::chapter(
            &mut tx,
            &user.id,
            "CATEGORY",
            Some(&revision.to_string()),
            &label,
        )
        .await
        .map_err(|_| Error::internal())?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"saved":true,"revision":revision,"category_id":category_id}),
    ))
}
#[derive(Deserialize)]
pub struct Proof {
    code: String,
}
#[derive(FromRow)]
struct Credential {
    public_id: String,
    generation: i64,
    secret_cipher: Option<String>,
    revoked_at: Option<DateTime<Utc>>,
}
async fn key_action(app: &App, jar: &CookieJar, code: &str, action: &str) -> Result<Value> {
    let config = configured(app)?;
    if code.len() > 64 {
        return Err(Error::bad("Invalid authenticator/recovery code."));
    }
    let (mut tx, user, session) = auth::sensitive(app, jar, code).await?;
    auth::authorize_streaming(&user, &session)?;
    require_eligible(&mut tx, &user).await?;
    settings(&mut tx, &user).await?;
    let previous: Option<Credential> = sqlx::query_as("SELECT public_id,generation,secret_cipher,revoked_at FROM stream_credentials WHERE owner_id=$1")
        .bind(&user.id).fetch_optional(&mut *tx).await?;
    let (public_id, secret) = if action == "reveal" {
        let Credential {
            public_id,
            generation,
            secret_cipher: cipher,
            revoked_at: revoked,
        } = previous.ok_or_else(|| Error::bad("Create a stream key first."))?;
        if revoked.is_some() {
            return Err(Error::bad("This key was revoked. Create a new key."));
        }
        let secret = sec::unseal(
            app,
            &format!("stream-key:{}:{generation}", user.id),
            &cipher.ok_or_else(Error::internal)?,
        )?;
        (public_id, secret)
    } else {
        if action == "create"
            && previous
                .as_ref()
                .is_some_and(|row| row.revoked_at.is_none())
        {
            return Err(conflict(
                "A stream key already exists. Reveal it or rotate it.",
            ));
        }
        revoke(&mut tx, &user.id).await?;
        let generation = previous.map_or(1, |row| row.generation + 1);
        let public_id = id();
        let secret = sec::token();
        let cipher = sec::seal(
            app,
            &format!("stream-key:{}:{generation}", user.id),
            &secret,
        )?;
        sqlx::query("INSERT INTO stream_credentials(owner_id,public_id,generation,secret_hash,secret_cipher) VALUES($1,$2,$3,$4,$5) ON CONFLICT(owner_id) DO UPDATE SET public_id=$2,generation=$3,secret_hash=$4,secret_cipher=$5,created_at=clock_timestamp(),revoked_at=NULL")
            .bind(&user.id).bind(&public_id).bind(generation).bind(sec::digest(&secret)).bind(cipher).execute(&mut *tx).await?;
        (public_id, secret)
    };
    tx.commit().await?;
    let _ = drain_stops(app, Some(&user.id)).await;
    let pending: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_stop_jobs WHERE owner_id=$1)")
            .bind(&user.id)
            .fetch_one(&app.db)
            .await?;
    // Never log these values, even on an SRS error.
    Ok(
        json!({"server":config.ingest_url,"key":format!("{public_id}?key={secret}"),
        // WHIP: the key is the bearer token, never in the URL. SRT: it rides in the stream ID.
        "whip":config.whip_url.as_ref().map(|u| json!({"url":format!("{u}?app={}&stream={public_id}",config.app),"token":secret})),
        "srt":config.srt_url.as_ref().map(|u| format!("{u}?streamid=#!::r={}/{public_id}?key={secret},m=publish",config.app)),
        "disconnect_pending":pending,"message":"Keep this key private. Rotation or Stop revokes the previous key."}),
    )
}
pub async fn create_key(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Proof>,
) -> Result<Json<Value>> {
    Ok(Json(key_action(&app, &jar, &input.code, "create").await?))
}
pub async fn reveal_key(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Proof>,
) -> Result<Json<Value>> {
    Ok(Json(key_action(&app, &jar, &input.code, "reveal").await?))
}
pub async fn rotate_key(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Proof>,
) -> Result<Json<Value>> {
    Ok(Json(key_action(&app, &jar, &input.code, "rotate").await?))
}
pub async fn stop(State(app): State<App>, jar: CookieJar) -> Result<Json<Value>> {
    let (mut tx, user, _) = auth::session(&app, &jar, false).await?;
    revoke(&mut tx, &user.id).await?;
    tx.commit().await?;
    let _ = drain_stops(&app, Some(&user.id)).await;
    let pending: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_stop_jobs WHERE owner_id=$1)")
            .bind(&user.id)
            .fetch_one(&app.db)
            .await?;
    Ok(Json(
        json!({"broadcast_closed":true,"key_revoked":true,"disconnect_pending":pending}),
    ))
}
/// Staff removals must wait for the media server to confirm that a revoked publisher is gone.
pub async fn confirm_stopped(app: &App, owner: &str) -> Result<bool> {
    drain_stops(app, Some(owner)).await?;
    Ok(!sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM stream_stop_jobs WHERE owner_id=$1)",
    )
    .bind(owner)
    .fetch_one(&app.db)
    .await?)
}

#[derive(Deserialize)]
pub struct Hook {
    action: String,
    server_id: String,
    service_id: String,
    client_id: String,
    vhost: String,
    app: String,
    stream: String,
    #[serde(default)]
    param: String,
}
pub fn authorize_hook(app: &App, peer: SocketAddr, headers: &HeaderMap) -> Result<()> {
    let config = configured(app)?;
    let supplied = headers
        .get("x-srs-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Constant work over fixed-size digests; never trust forwarded client-IP headers here.
    let a = sec::digest(supplied);
    let b = sec::digest(&config.hook_secret);
    let mismatch = a
        .bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    if peer.ip() != config.hook_ip || supplied.len() > 256 || mismatch != 0 {
        return Err(Error::denied("Invalid media callback."));
    }
    Ok(())
}
async fn srs_get(app: &App, path: &str) -> Result<Value> {
    let mut response = app
        .http
        .get(format!("{}{path}", configured(app)?.api_url))
        .timeout(Timeout::from_secs(3))
        .send()
        .await
        .map_err(|_| Error::unavailable())?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 2_000_000) {
        return Err(Error::unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::unavailable())? {
        if bytes.len() + chunk.len() > 2_000_000 {
            return Err(Error::unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::unavailable())?;
    if value["code"] != 0
        || !value["server"].as_str().is_some_and(identifier)
        || !value["service"].as_str().is_some_and(identifier)
    {
        return Err(Error::unavailable());
    }
    Ok(value)
}
pub async fn hook(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(action): Path<String>,
    Json(input): Json<Hook>,
) -> Result<Json<Value>> {
    authorize_hook(&app, peer, &headers)?;
    let config = configured(&app)?;
    if !matches!(action.as_str(), "publish" | "unpublish")
        || input.action != format!("on_{action}")
        || input.vhost != config.vhost
        || input.app != config.app
        || ![
            &input.server_id,
            &input.service_id,
            &input.client_id,
            &input.stream,
        ]
        .into_iter()
        .all(|s| identifier(s))
        || input.param.len() > 1024
    {
        return Err(Error::denied("Invalid media callback."));
    }
    if action == "publish" {
        let instance = srs_get(&app, "/api/v1/versions").await?;
        if instance["server"] != input.server_id || instance["service"] != input.service_id {
            return Err(Error::denied("Retired media instance."));
        }
    }
    let owner: Option<String> = sqlx::query_scalar("SELECT owner_id FROM stream_credentials WHERE public_id=$1 UNION SELECT owner_id FROM stream_publishers WHERE server_id=$2 AND service_id=$3 AND client_id=$4 AND public_id=$1")
        .bind(&input.stream).bind(&input.server_id).bind(&input.service_id).bind(&input.client_id).fetch_optional(&app.db).await?;
    let Some(owner) = owner else {
        return if action == "unpublish" {
            Ok(Json(json!({"code":0})))
        } else {
            Err(Error::denied("Invalid publishing credential."))
        };
    };
    let mut tx = app.db.begin().await?;
    let user = auth::stream_owner(&mut tx, &owner)
        .await?
        .ok_or_else(Error::auth)?;
    if action == "publish" {
        require_eligible(&mut tx, &user).await?;
        let keys: Vec<_> =
            url::form_urlencoded::parse(input.param.trim_start_matches('?').as_bytes())
                .filter(|(k, _)| k == "key")
                .map(|(_, v)| v.into_owned())
                .collect();
        if keys.len() != 1 || keys[0].len() != 43 {
            return Err(Error::denied("Invalid publishing credential."));
        }
        let generation: Option<i64> = sqlx::query_scalar("SELECT generation FROM stream_credentials WHERE owner_id=$1 AND public_id=$2 AND secret_hash=$3 AND revoked_at IS NULL")
            .bind(&owner).bind(&input.stream).bind(sec::digest(&keys[0])).fetch_optional(&mut *tx).await?;
        let generation =
            generation.ok_or_else(|| Error::denied("Invalid publishing credential."))?;
        let ready: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_settings s JOIN stream_categories c ON c.id=s.category_id WHERE s.owner_id=$1 AND c.active)")
            .bind(&owner).fetch_one(&mut *tx).await?;
        if !ready {
            return Err(Error::denied(
                "Set your stream title and category in Studio first.",
            ));
        }
        expire(&mut tx, &owner).await?;
        let seen: Option<Option<DateTime<Utc>>> = sqlx::query_scalar("SELECT retired_at FROM stream_publishers WHERE server_id=$1 AND service_id=$2 AND client_id=$3")
            .bind(&input.server_id).bind(&input.service_id).bind(&input.client_id).fetch_optional(&mut *tx).await?;
        if seen.flatten().is_some() {
            return Err(Error::denied("Retired publisher."));
        }
        let mut open = current(&mut tx, &owner).await?;
        if let Some(b) = &open {
            if b.generation != generation || b.public_id != input.stream {
                return Err(Error::denied("Invalid publishing generation."));
            }
            if b.server_id == input.server_id
                && b.service_id == input.service_id
                && b.client_id == input.client_id
                && b.state != "RECONNECTING"
            {
                tx.commit().await?;
                return Ok(Json(json!({"code":0})));
            }
            if b.state != "RECONNECTING" {
                if b.server_id == input.server_id && b.service_id == input.service_id {
                    return Err(conflict("Another publisher is already connected."));
                }
                // A verified new SRS process proves the old connection no longer exists.
                disconnect(&mut tx, b).await?;
                open = current(&mut tx, &owner).await?;
            }
        }
        if seen.is_some() {
            return Err(Error::denied("Retired publisher."));
        }
        let now = clock(&mut tx).await?;
        let broadcast_id = if let Some(b) = open {
            sqlx::query("UPDATE broadcasts SET state='STARTING',server_id=$2,service_id=$3,client_id=$4,startup_deadline=$5,publisher_started_at=clock_timestamp(),reconnect_deadline=NULL,checked_at=NULL,observed_at=NULL,recv_bytes=NULL,health='{}' WHERE id=$1")
                .bind(&b.id).bind(&input.server_id).bind(&input.service_id).bind(&input.client_id).bind(now+Duration::seconds(15)).execute(&mut *tx).await?;
            b.id
        } else {
            let bid = id();
            sqlx::query("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,mature) VALUES($1,$2,$3,$4,'STARTING',$5,$6,$7,$8,$8,$9,coalesce((SELECT mature FROM stream_settings WHERE owner_id=$2),false))")
                .bind(&bid).bind(&owner).bind(&input.stream).bind(generation).bind(&input.server_id).bind(&input.service_id).bind(&input.client_id).bind(now).bind(now+Duration::seconds(15)).execute(&mut *tx).await?;
            bid
        };
        sqlx::query("INSERT INTO stream_publishers(server_id,service_id,client_id,owner_id,broadcast_id,public_id) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(&input.server_id).bind(&input.service_id).bind(&input.client_id).bind(&owner).bind(broadcast_id).bind(&input.stream).execute(&mut *tx).await?;
    } else {
        // Tombstone first, including an out-of-order unpublish before publish.
        sqlx::query("INSERT INTO stream_publishers(server_id,service_id,client_id,owner_id,public_id,retired_at) VALUES($1,$2,$3,$4,$5,clock_timestamp()) ON CONFLICT(server_id,service_id,client_id) DO UPDATE SET retired_at=coalesce(stream_publishers.retired_at,EXCLUDED.retired_at)")
            .bind(&input.server_id).bind(&input.service_id).bind(&input.client_id).bind(&owner).bind(&input.stream).execute(&mut *tx).await?;
        if let Some(b) = current(&mut tx, &owner).await?
            && b.server_id == input.server_id
            && b.service_id == input.service_id
            && b.client_id == input.client_id
        {
            disconnect(&mut tx, &b).await?;
        }
    }
    tx.commit().await?;
    Ok(Json(json!({"code":0})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/internal/streams/playback", get(authorize_playback))
        .route("/api/categories", get(categories))
        .route("/api/me/stream", get(mine).patch(save))
        .route("/api/me/stream/health", get(mine))
        .route("/api/me/stream/key", post(create_key))
        .route("/api/me/stream/key/reveal", post(reveal_key))
        .route("/api/me/stream/key/rotate", post(rotate_key))
        .route("/api/me/stream/stop", post(stop))
        .route("/api/internal/srs/{action}", post(hook))
}

#[derive(FromRow)]
struct StopJob {
    server_id: String,
    service_id: String,
    client_id: String,
    public_id: String,
    attempts: i32,
}
async fn inventory(app: &App) -> Result<Value> {
    // SRS redirects the slashless collection URL. Keep redirects disabled on
    // the shared HTTP client and address the canonical control endpoint.
    let value = srs_get(app, "/api/v1/streams/?start=0&count=1000").await?;
    // ponytail: one small SRS host; fail on a truncated inventory rather than
    // treating unlisted publishers as disconnected. Page this before 1000 streams.
    if !value["streams"]
        .as_array()
        .is_some_and(|rows| rows.len() < 1000)
    {
        return Err(Error::unavailable());
    }
    Ok(value)
}
fn publisher<'a>(
    inventory: &'a Value,
    config: &Config,
    public_id: &str,
    client_id: &str,
) -> Option<&'a Value> {
    inventory["streams"].as_array()?.iter().find(|stream| {
        stream["name"] == public_id
            && stream["app"] == config.app
            && stream["publish"]["active"] == true
            && stream["publish"]["cid"] == client_id
    })
}
async fn drain_stops(app: &App, owner: Option<&str>) -> Result<()> {
    let Some(config) = app.config.streaming.as_ref() else {
        return Ok(());
    };
    for _ in 0..if owner.is_some() { 1 } else { 20 } {
        let mut tx = app.db.begin().await?;
        let job: Option<StopJob>=sqlx::query_as("SELECT * FROM stream_stop_jobs WHERE available_at<=clock_timestamp() AND ($1::text IS NULL OR owner_id=$1) ORDER BY available_at FOR UPDATE SKIP LOCKED LIMIT 1")
            .bind(owner).fetch_optional(&mut *tx).await?;
        let Some(job) = job else {
            return Ok(());
        };
        let stopped = match inventory(app).await {
            Ok(state) if state["server"] != job.server_id || state["service"] != job.service_id => {
                true
            }
            Ok(state) if publisher(&state, config, &job.public_id, &job.client_id).is_none() => {
                true
            }
            Ok(_) => {
                // The instance and owned stream must match before addressing a client.
                match app
                    .http
                    .delete(format!(
                        "{}/api/v1/clients/{}",
                        config.api_url, job.client_id
                    ))
                    .timeout(Timeout::from_secs(3))
                    .send()
                    .await
                {
                    Ok(response) if response.status().is_success() => {
                        let accepted = response.json::<Value>().await.is_ok_and(|v| {
                            v["code"] == 0
                                && v["server"] == job.server_id
                                && v["service"] == job.service_id
                        });
                        // An acknowledged kick is not proof that media has stopped.
                        accepted
                            && inventory(app).await.is_ok_and(|v| {
                                v["server"] != job.server_id
                                    || v["service"] != job.service_id
                                    || publisher(&v, config, &job.public_id, &job.client_id)
                                        .is_none()
                            })
                    }
                    _ => false,
                }
            }
            Err(_) => false,
        };
        if stopped {
            sqlx::query("DELETE FROM stream_stop_jobs WHERE server_id=$1 AND service_id=$2 AND client_id=$3")
                .bind(&job.server_id).bind(&job.service_id).bind(&job.client_id).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE stream_stop_jobs SET attempts=attempts+1,available_at=clock_timestamp()+make_interval(secs=>$4) WHERE server_id=$1 AND service_id=$2 AND client_id=$3")
                .bind(&job.server_id).bind(&job.service_id).bind(&job.client_id)
                .bind((2_i32.saturating_pow(job.attempts.min(5) as u32)*5).min(60) as f64).execute(&mut *tx).await?;
            eprintln!("stream_event=disconnect outcome=retry");
        }
        tx.commit().await?;
        if !stopped {
            break;
        }
    }
    Ok(())
}
/// Media maintenance runs independently from mail, every five seconds. Account locks
/// serialize callbacks, revocation and expiry; wall-clock deadlines survive restarts.
pub async fn tick(app: &App) -> Result<()> {
    let Some(config) = app.config.streaming.as_ref() else {
        return Ok(());
    };
    let snapshot_started: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&app.db)
        .await?;
    let snapshot = inventory(app).await.ok();
    let owners: Vec<String> = sqlx::query_scalar("SELECT owner_id FROM broadcasts WHERE state<>'ENDED' ORDER BY checked_at NULLS FIRST,id LIMIT 100")
        .fetch_all(&app.db).await?;
    for owner in owners {
        let mut tx = app.db.begin().await?;
        // Checkpoints can grant FK-backed rewards; acquire their barrier before account locks.
        crate::factions::lock(&mut tx)
            .await
            .map_err(|_| Error::internal())?;
        let Some(user) = auth::stream_owner(&mut tx, &owner).await? else {
            continue;
        };
        expire(&mut tx, &owner).await?;
        if !eligible(&mut tx, &user).await? {
            revoke(&mut tx, &owner).await?;
        } else if let Some(b) = current(&mut tx, &owner).await? {
            if let Some(state) = &snapshot
                && b.publisher_started_at <= snapshot_started
            {
                let stream = (state["server"] == b.server_id && state["service"] == b.service_id)
                    .then(|| publisher(state, config, &b.public_id, &b.client_id))
                    .flatten();
                if let Some(stream) = stream {
                    if b.state != "RECONNECTING" {
                        let bytes = stream["recv_bytes"].as_i64().unwrap_or(0);
                        let video = stream["video"]["codec"].as_str();
                        let audio = stream["audio"]["codec"].as_str();
                        let compatible = video.is_some_and(|c| {
                            c.eq_ignore_ascii_case("h264") || c.eq_ignore_ascii_case("avc")
                        }) && audio.is_some_and(|c| c.eq_ignore_ascii_case("aac"));
                        let fresh = b.recv_bytes.is_some_and(|previous| bytes > previous);
                        if compatible && fresh && b.state == "LIVE" {
                            let at = clock(&mut tx).await?;
                            let elapsed = b
                                .checked_at
                                .map_or(0, |last| (at - last).num_milliseconds());
                            crate::factions::stream(app, &mut tx, &b.id, elapsed, at)
                                .await
                                .map_err(|_| Error::internal())?;
                        }
                        let kbps = stream["kbps"]["recv_30s"]
                            .as_f64()
                            .filter(|n| n.is_finite() && *n >= 0.0);
                        let health = json!({"video_codec":video,"audio_codec":audio,"width":stream["video"]["width"].as_u64(),
                            "height":stream["video"]["height"].as_u64(),"input_kbps":kbps,
                            "codec_warning":video.is_some()&&!compatible,
                            "bitrate_warning":kbps.is_some_and(|n|n>8000.0),"bitrate_warning_provisional":true});
                        sqlx::query("UPDATE broadcasts SET confirmed_live_at=CASE WHEN $2 AND $3 THEN coalesce(confirmed_live_at,clock_timestamp()) ELSE confirmed_live_at END,state=CASE WHEN $2 AND $3 THEN 'LIVE' ELSE state END,recv_bytes=$4,health=$5::jsonb||jsonb_strip_nulls(jsonb_build_object('keyframe_seconds',health->'keyframe_seconds','b_frames',health->'b_frames','keyframe_warning',health->'keyframe_warning','probed_at',health->'probed_at')),observed_at=CASE WHEN $3 THEN clock_timestamp() ELSE observed_at END WHERE id=$1 AND state IN ('STARTING','LIVE')")
                            .bind(&b.id).bind(compatible).bind(fresh).bind(bytes).bind(health).execute(&mut *tx).await?;
                    }
                } else if b.state != "STARTING" {
                    // The on_publish response can still be in flight when the
                    // inventory is sampled. Startup has its own 15-second deadline.
                    disconnect(&mut tx, &b).await?;
                }
            }
            sqlx::query("UPDATE broadcasts SET checked_at=clock_timestamp() WHERE id=$1")
                .bind(&b.id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
    }
    if let Some(state) = snapshot {
        // Catch a publish accepted just after a rotation/stop's first SRS check.
        // Only previously authenticated publishers in this rebuild are eligible for a kick.
        for stream in state["streams"].as_array().ok_or_else(Error::unavailable)? {
            if stream["app"] != config.app || stream["publish"]["active"] != true {
                continue;
            }
            let Some(client) = stream["publish"]["cid"].as_str() else {
                continue;
            };
            let Some(public_id) = stream["name"].as_str() else {
                continue;
            };
            let owner: Option<String> = sqlx::query_scalar("SELECT p.owner_id FROM stream_publishers p LEFT JOIN broadcasts b ON b.id=p.broadcast_id WHERE p.server_id=$1 AND p.service_id=$2 AND p.client_id=$3 AND p.public_id=$4 AND (p.retired_at IS NOT NULL OR b.state='ENDED')")
                .bind(state["server"].as_str()).bind(state["service"].as_str()).bind(client).bind(public_id).fetch_optional(&app.db).await?;
            if let Some(owner) = owner {
                let mut tx = app.db.begin().await?;
                if auth::stream_owner(&mut tx, &owner).await?.is_some() {
                    sqlx::query("INSERT INTO stream_stop_jobs(server_id,service_id,client_id,public_id,owner_id) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
                        .bind(state["server"].as_str()).bind(state["service"].as_str()).bind(client).bind(public_id).bind(owner).execute(&mut *tx).await?;
                }
                tx.commit().await?;
            }
        }
    }
    drain_stops(app, None).await
}
