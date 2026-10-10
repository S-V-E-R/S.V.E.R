//! Restreaming (docs/LINKED_CHAT.md "Restreaming"): while a channel is live on S.V.E.R, each enabled
//! destination gets the same stream relayed with `ffmpeg -c copy` from the local media server.
//! Nothing is re-encoded or added to the video. Keys are sealed at rest and never returned.
use crate::{App, Error, Result, auth, security as sec};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tokio::process::{Child, Command};

pub const MAX_DESTINATIONS: i64 = 3;
/// A relay that stays up this long counts as live, and its failure count resets.
const LIVE_AFTER: Duration = Duration::from_secs(15);
/// Fast failures in a row before the destination shows "key rejected".
const REJECT_AFTER: u32 = 3;
const SERVER_HELP: &str = "Enter the platform's full rtmp:// or rtmps:// server address.";

/// Default ingest servers; Kick and custom destinations give their own (Kick's differs per account).
fn preset(platform: &str) -> Option<&'static str> {
    match platform {
        "twitch" => Some("rtmp://live.twitch.tv/app"),
        "youtube" => Some("rtmp://a.rtmp.youtube.com/live2"),
        _ => None,
    }
}

/// What happens after a relay exits: (status, seconds to wait, failures so far).
pub fn after_exit(ran: Duration, failures: u32) -> (&'static str, u64, u32) {
    let failures = if ran >= LIVE_AFTER { 1 } else { failures + 1 };
    if failures >= REJECT_AFTER {
        ("rejected", 300, failures)
    } else {
        ("reconnecting", 2u64.pow(failures).min(60), failures)
    }
}

/// An rtmp:// or rtmps:// server on a public address (no credentials, no private hosts).
async fn check_server(app: &App, server: &str) -> Result<String> {
    let parsed = url::Url::parse(server.trim()).map_err(|_| Error::bad(SERVER_HELP))?;
    if !matches!(parsed.scheme(), "rtmp" | "rtmps") {
        return Err(Error::bad(SERVER_HELP));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() || parsed.query().is_some() {
        return Err(Error::bad(
            "Put the stream key in the key field, not in the server address.",
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::bad(SERVER_HELP))?
        .trim_matches(['[', ']'])
        .to_string();
    if app.config.production {
        let port = parsed.port().unwrap_or(1935);
        let addrs: Vec<IpAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| Error::bad("That server address didn't resolve."))?
            .map(|a| a.ip())
            .collect();
        if addrs.is_empty() || addrs.iter().any(|ip| !crate::boards::public(*ip)) {
            return Err(Error::bad("Restreams can't go to private addresses."));
        }
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}
fn check_key(key: &str) -> Result<()> {
    if key.is_empty()
        || key.len() > 512
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.?=&".contains(c))
    {
        return Err(Error::bad(
            "Paste the stream key exactly as the platform shows it.",
        ));
    }
    Ok(())
}
async fn streamer(app: &App, jar: &CookieJar) -> Result<auth::User> {
    let (tx, user, session) = auth::session(app, jar, false).await?;
    tx.commit().await?;
    auth::authorize_streaming(&user, &session)?;
    Ok(user)
}
async fn list(app: &App, owner: &str) -> Result<Json<Value>> {
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'platform',platform,'label',label,'server',server,'enabled',enabled,'status',status,'detail',detail,'status_at',status_at) FROM restream_destinations WHERE owner_id=$1 ORDER BY created_at")
        .bind(owner)
        .fetch_all(&app.db)
        .await?;
    Ok(Json(
        json!({"destinations": rows, "max": MAX_DESTINATIONS, "available": source().is_some()}),
    ))
}
/// GET /api/me/restream
async fn mine(State(app): State<App>, jar: CookieJar) -> Result<Json<Value>> {
    let (tx, user, _) = auth::session(&app, &jar, false).await?;
    tx.commit().await?;
    list(&app, &user.id).await
}

#[derive(Deserialize)]
pub struct NewDestination {
    platform: String,
    #[serde(default)]
    server: Option<String>,
    key: String,
    #[serde(default)]
    label: Option<String>,
}
/// POST /api/me/restream: adds a destination (verified accounts with 2FA, like streaming).
async fn add(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewDestination>,
) -> Result<Json<Value>> {
    let user = streamer(&app, &jar).await?;
    if !matches!(
        input.platform.as_str(),
        "twitch" | "youtube" | "kick" | "custom"
    ) {
        return Err(Error::bad(
            "Choose Twitch, YouTube, Kick or a custom server.",
        ));
    }
    let server = match input
        .server
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => preset(&input.platform)
            .ok_or_else(|| Error::bad("Enter the platform's server address."))?
            .to_string(),
    };
    let server = check_server(&app, &server).await?;
    check_key(input.key.trim())?;
    let label: String = input
        .label
        .unwrap_or_default()
        .trim()
        .chars()
        .take(40)
        .collect();
    let mut tx = app.db.begin().await?;
    // Serializes adds per owner so two at once can't pass the limit.
    sqlx::query("SELECT 1 FROM users WHERE id=$1 FOR UPDATE")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM restream_destinations WHERE owner_id=$1")
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    if count >= MAX_DESTINATIONS {
        return Err(Error::bad("You can restream to up to 3 destinations."));
    }
    sqlx::query("INSERT INTO restream_destinations(id,owner_id,platform,label,server,key_sealed) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(uuid::Uuid::new_v4().to_string()).bind(&user.id).bind(&input.platform).bind(&label).bind(&server)
        .bind(sec::seal(&app, "restream", input.key.trim())?).execute(&mut *tx).await?;
    tx.commit().await?;
    list(&app, &user.id).await
}

#[derive(Deserialize)]
pub struct Change {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    server: Option<String>,
}
/// PATCH /api/me/restream/{id}: switch on or off, or replace the key or server. Any change clears
/// a "key rejected" status so the next stream tries again.
async fn change(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Change>,
) -> Result<Json<Value>> {
    let user = streamer(&app, &jar).await?;
    let server = match input.server.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(s) => Some(check_server(&app, s).await?),
        None => None,
    };
    let key = match input
        .key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        Some(k) => {
            check_key(k)?;
            Some(sec::seal(&app, "restream", k)?)
        }
        None => None,
    };
    let updated = sqlx::query("UPDATE restream_destinations SET enabled=coalesce($3,enabled),key_sealed=coalesce($4,key_sealed),server=coalesce($5,server),status=CASE WHEN status='rejected' THEN 'idle' ELSE status END,detail=CASE WHEN status='rejected' THEN NULL ELSE detail END WHERE id=$1 AND owner_id=$2")
        .bind(&id).bind(&user.id).bind(input.enabled).bind(key).bind(server).execute(&app.db).await?;
    if updated.rows_affected() == 0 {
        return Err(Error(StatusCode::NOT_FOUND, "Not found.", None));
    }
    forget(&id);
    list(&app, &user.id).await
}
/// DELETE /api/me/restream/{id}: deletes the destination and its key.
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (tx, user, _) = auth::session(&app, &jar, false).await?;
    tx.commit().await?;
    sqlx::query("DELETE FROM restream_destinations WHERE id=$1 AND owner_id=$2")
        .bind(&id)
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    forget(&id);
    list(&app, &user.id).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/restream", get(mine).post(add))
        .route("/api/me/restream/{id}", patch(change).delete(remove))
}

// ---- The relay supervisor ----

struct Relay {
    child: Child,
    broadcast: String,
    started: Instant,
}
#[derive(Default)]
struct Supervisor {
    relays: HashMap<String, Relay>,
    /// Destination → (don't start before, failures in a row).
    waits: HashMap<String, (Instant, u32)>,
}
static SUPERVISOR: LazyLock<Mutex<Supervisor>> = LazyLock::new(Mutex::default);

/// Where relays pull from: `RESTREAM_SOURCE`, the media server's local RTMP app
/// (`rtmp://127.0.0.1:1936/rebuild`). Unset turns restreaming off.
pub fn source() -> Option<String> {
    std::env::var("RESTREAM_SOURCE")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
}
/// The most relays at once (`RESTREAM_MAX`), from the bandwidth budget in LOAD_TEST.md: each relay
/// sends one stream's bitrate. ponytail: a count, not measured Mbps; meter bytes if bitrates vary a lot.
fn capacity() -> usize {
    std::env::var("RESTREAM_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
}
/// Stops a destination's relay (it was changed or deleted); the next tick restarts it if wanted.
fn forget(id: &str) {
    let mut s = SUPERVISOR.lock().unwrap_or_else(|e| e.into_inner());
    s.relays.remove(id); // kill_on_drop ends the process
    s.waits.remove(id);
}
async fn status(app: &App, id: &str, status: &str, detail: Option<&str>) -> Result<()> {
    sqlx::query("UPDATE restream_destinations SET status=$2,detail=$3,status_at=now() WHERE id=$1 AND (status,detail) IS DISTINCT FROM ($2,$3)")
        .bind(id)
        .bind(status)
        .bind(detail)
        .execute(&app.db)
        .await?;
    Ok(())
}
type Wanted = (String, String, String, String, String);

/// Every few seconds: start relays for live channels' enabled destinations, notice exits, back off,
/// and stop relays that are no longer wanted. `source` is where relays pull from.
pub async fn tick(app: &App, source: &str) -> Result<()> {
    let wanted: Vec<Wanted> = sqlx::query_as("SELECT d.id,d.server,d.key_sealed,b.id,b.public_id FROM restream_destinations d
        JOIN users u ON u.id=d.owner_id AND u.email_verified AND u.mfa_enabled AND u.deleted_at IS NULL
        JOIN broadcasts b ON b.owner_id=d.owner_id AND b.state='LIVE'
        WHERE d.enabled AND NOT EXISTS(SELECT 1 FROM feature_switches WHERE name='restreaming' AND off) ORDER BY d.created_at")
        .fetch_all(&app.db)
        .await?;
    let mut updates: Vec<(String, &'static str, Option<&'static str>)> = Vec::new();
    {
        let mut s = SUPERVISOR.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        // Exits, and relays no longer wanted (stream ended, switched off, deleted).
        let ids: Vec<String> = s.relays.keys().cloned().collect();
        for id in ids {
            let broadcast = wanted.iter().find(|w| w.0 == id).map(|w| w.3.clone());
            let relay = s.relays.get_mut(&id).expect("listed");
            if broadcast.as_deref() != Some(relay.broadcast.as_str()) {
                s.relays.remove(&id);
                s.waits.remove(&id);
                continue;
            }
            match relay.child.try_wait() {
                Ok(None) => {
                    if relay.started.elapsed() >= LIVE_AFTER {
                        s.waits.remove(&id);
                        updates.push((id, "live", None));
                    }
                }
                _ => {
                    let ran = relay.started.elapsed();
                    s.relays.remove(&id);
                    let failures = s.waits.get(&id).map_or(0, |w| w.1);
                    let (state, wait, failures) = after_exit(ran, failures);
                    s.waits
                        .insert(id.clone(), (now + Duration::from_secs(wait), failures));
                    let detail = if state == "rejected" {
                        "The platform keeps refusing the stream. Check the stream key and server, then save them again."
                    } else {
                        "Reconnecting to the platform."
                    };
                    updates.push((id, state, Some(detail)));
                }
            }
        }
        // Starts, within capacity.
        for (id, server, sealed, broadcast, public_id) in &wanted {
            if s.relays.contains_key(id) || s.waits.get(id).is_some_and(|w| w.0 > now) {
                continue;
            }
            if s.relays.len() >= capacity() {
                updates.push((id.clone(), "waiting", Some("Restreaming is at capacity right now. This destination starts when space frees up; your S.V.E.R stream isn't affected.")));
                continue;
            }
            let Ok(key) = sec::unseal(app, "restream", sealed) else {
                updates.push((id.clone(), "rejected", Some("Save the stream key again.")));
                continue;
            };
            // Output goes nowhere: ffmpeg's messages can contain the destination key.
            let child = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "quiet", "-nostdin"])
                .args(["-rw_timeout", "15000000", "-i"])
                .arg(format!("{source}/{public_id}"))
                .args(["-map", "0", "-c", "copy", "-f", "flv"])
                .args(["-flvflags", "no_duration_filesize"])
                .arg(format!("{server}/{key}"))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn();
            match child {
                Ok(child) => {
                    let relay = Relay {
                        child,
                        broadcast: broadcast.clone(),
                        started: now,
                    };
                    s.relays.insert(id.clone(), relay);
                    updates.push((id.clone(), "starting", None));
                }
                Err(_) => updates.push((
                    id.clone(),
                    "reconnecting",
                    Some("Couldn't start the relay; retrying."),
                )),
            }
        }
    }
    for (id, state, detail) in updates {
        status(app, &id, state, detail).await?;
    }
    // Destinations of channels that aren't live go back to idle (a rejection stays visible).
    let ids: Vec<&str> = wanted.iter().map(|w| w.0.as_str()).collect();
    sqlx::query("UPDATE restream_destinations SET status='idle',detail=NULL,status_at=now() WHERE status NOT IN ('idle','rejected') AND id<>ALL($1)")
        .bind(ids)
        .execute(&app.db)
        .await?;
    Ok(())
}
/// Running relays (for tests and the operations check).
pub fn running() -> usize {
    SUPERVISOR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .relays
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fast_failures_become_a_rejection() {
        let quick = Duration::from_secs(1);
        assert_eq!(after_exit(quick, 0), ("reconnecting", 2, 1));
        assert_eq!(after_exit(quick, 1), ("reconnecting", 4, 2));
        assert_eq!(after_exit(quick, 2), ("rejected", 300, 3));
        // A relay that was up for a while just dropped; it reconnects quickly.
        assert_eq!(
            after_exit(Duration::from_secs(600), 2),
            ("reconnecting", 2, 1)
        );
    }
}
