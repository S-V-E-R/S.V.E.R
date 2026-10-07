//! Module 9 (docs/BEACONS.md): short vertical videos that lead viewers to creators and their live
//! streams. Beacons are the platform's one re-encode: each is framed to 9:16, stripped of metadata
//! and marked with a watermark that moves from corner to corner. Everything is private storage
//! behind short-lived tickets, like recordings (docs/VODS_CLIPS.md).
mod config;
pub use config::{Config, Tuning};
mod counters;
mod create;
mod delivery;
mod feed;
pub use feed::search;
pub mod review;
pub mod watermark;
pub mod worker;
use crate::{
    App, auth, moderation,
    profiles::{self, Fail, Res},
    security,
};
use axum::Router;
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;

pub const DAILY_LIMIT: i64 = 10;
pub const MAX_UPLOAD_BYTES: u64 = 200 * 1024 * 1024;
pub const MIN_MS: i64 = 5000;
pub const MAX_MS: i64 = 60000;

#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct Beacon {
    pub id: String,
    pub owner_id: Option<String>,
    pub clipper_id: Option<String>,
    pub clip_id: Option<String>,
    pub broadcast_id: Option<String>,
    pub source: String,
    pub status: String,
    pub failure: Option<String>,
    pub publish: bool,
    #[serde(skip_serializing)]
    pub quota: bool,
    pub hidden: bool,
    pub title: String,
    pub category_id: Option<String>,
    pub category: Option<String>,
    pub genre: Option<String>,
    pub mature: bool,
    pub charity_name: Option<String>,
    pub charity_url: Option<String>,
    pub crop: Option<Value>,
    #[serde(skip_serializing)]
    pub seed: i64,
    pub duration_ms: i64,
    #[serde(skip_serializing)]
    pub hashes: Vec<String>,
    #[serde(skip_serializing)]
    pub upload_key: Option<String>,
    #[serde(skip_serializing)]
    pub upload_until: Option<DateTime<Utc>>,
    #[serde(skip_serializing)]
    pub mp4_key: Option<String>,
    #[serde(skip_serializing)]
    pub mp4_small_key: Option<String>,
    #[serde(skip_serializing)]
    pub clean_key: Option<String>,
    #[serde(skip_serializing)]
    pub thumbnail_key: Option<String>,
    pub revision: i64,
    pub views: i64,
    pub likes: i64,
    #[serde(skip_serializing)]
    pub request_key: String,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

pub fn routes() -> Router<App> {
    Router::new()
        .merge(create::routes())
        .merge(delivery::routes())
        .merge(feed::routes())
        .merge(counters::routes())
        .merge(review::routes())
}

pub async fn load(db: &mut PgConnection, id: &str) -> Res<Beacon> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(Fail::missing());
    }
    sqlx::query_as("SELECT * FROM beacons WHERE id=$1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(Fail::missing)
}

/// Anyone who has streamed on S.V.E.R at least once (a broadcast that was confirmed live).
pub async fn has_streamed(db: &mut PgConnection, user: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1 AND confirmed_live_at IS NOT NULL)",
    )
    .bind(user)
    .fetch_one(db)
    .await?)
}

/// Public playback: published, not hidden or held, from an eligible channel the viewer may see.
/// The creator can always preview their own Beacon while it exists.
pub async fn accessible(
    app: &App,
    beacon: &Beacon,
    user: Option<&auth::User>,
    age_ack: bool,
) -> Res<()> {
    let owner = beacon.owner_id.as_deref().ok_or_else(Fail::missing)?;
    if let Some(clip) = &beacon.clip_id
        && crate::videos::review::source_held(&mut *app.db.acquire().await?, clip).await?
    {
        return Err(Fail::missing());
    }
    let mine = user.is_some_and(|u| u.id == owner);
    if mine {
        if matches!(beacon.status.as_str(), "DELETING" | "DELETED" | "REMOVED") {
            return Err(Fail::missing());
        }
        return Ok(());
    }
    if beacon.status != "PUBLISHED" || beacon.hidden {
        return Err(Fail::missing());
    }
    let mut db = app.db.acquire().await?;
    let channel = profiles::channel_user_by_id(&mut db, owner)
        .await?
        .ok_or_else(Fail::missing)?;
    if !channel.eligible {
        return Err(Fail::missing());
    }
    if let Some(user) = user
        && (profiles::blocked_between(&mut db, owner, &user.id).await?
            || moderation::banned(app, owner, &user.id).await?)
    {
        return Err(Fail::denied("This Beacon is unavailable."));
    }
    if beacon.mature {
        if let Some(user) = user
            && !auth::is_adult(&mut db, &user.id).await?
        {
            return Err(Fail::denied("This Beacon is for viewers aged 18 and over."));
        }
        if !age_ack {
            return Err(Fail::denied("Confirm that you are 18 or older to watch."));
        }
    }
    Ok(())
}

pub async fn watched(
    app: &App,
    jar: &CookieJar,
    id: &str,
    age_ack: bool,
) -> Res<(Beacon, Option<auth::User>)> {
    let user = profiles::viewer(app, jar).await?;
    let beacon = load(&mut *app.db.acquire().await?, id).await?;
    accessible(app, &beacon, user.as_ref(), age_ack).await?;
    Ok((beacon, user))
}

#[derive(Serialize, Deserialize)]
pub struct Ticket {
    pub beacon: String,
    pub revision: i64,
    pub subject: Option<String>,
    pub scope: String,
    pub until: i64,
    pub age_ack: bool,
}
pub fn ticket(
    app: &App,
    beacon: &Beacon,
    user: Option<&auth::User>,
    scope: &str,
    age_ack: bool,
) -> Res<String> {
    let value = Ticket {
        beacon: beacon.id.clone(),
        revision: beacon.revision,
        subject: user.map(|u| u.id.clone()),
        scope: scope.into(),
        until: Utc::now().timestamp() + 300,
        age_ack,
    };
    Ok(url::form_urlencoded::byte_serialize(
        security::seal(
            app,
            "beacon-ticket",
            &serde_json::to_string(&value).map_err(|_| Fail::internal())?,
        )?
        .as_bytes(),
    )
    .collect())
}
pub async fn verify_ticket(
    app: &App,
    jar: &CookieJar,
    id: &str,
    raw: &str,
) -> Res<(Beacon, Ticket)> {
    if raw.len() > 4096 {
        return Err(Fail::denied("Playback link expired. Reload the player."));
    }
    let ticket: Ticket = serde_json::from_str(
        &security::unseal(app, "beacon-ticket", raw)
            .map_err(|_| Fail::denied("Invalid playback link."))?,
    )
    .map_err(|_| Fail::denied("Invalid playback link."))?;
    if ticket.beacon != id || ticket.until <= Utc::now().timestamp() {
        return Err(Fail::denied("Playback link expired. Reload the player."));
    }
    let user = profiles::viewer(app, jar).await?;
    if ticket
        .subject
        .as_deref()
        .is_some_and(|s| user.as_ref().is_none_or(|u| u.id != s))
    {
        return Err(Fail::denied("Sign in again to watch."));
    }
    let beacon = load(&mut *app.db.acquire().await?, id).await?;
    if beacon.revision != ticket.revision {
        return Err(Fail::denied(
            "Playback permission changed. Reload the player.",
        ));
    }
    match ticket.scope.as_str() {
        "play" => accessible(app, &beacon, user.as_ref(), ticket.age_ack).await?,
        // The clean copy is reachable only by its creator.
        "clean" => {
            let user = user
                .as_ref()
                .ok_or_else(|| Fail::denied("Sign in to download."))?;
            if beacon.owner_id.as_deref() != Some(&user.id)
                || !matches!(beacon.status.as_str(), "READY" | "PUBLISHED")
            {
                return Err(Fail::missing());
            }
        }
        "review" => {
            crate::safety::staff_write(app, jar).await?;
            if matches!(beacon.status.as_str(), "DELETED" | "DELETING") {
                return Err(Fail::missing());
            }
        }
        _ => return Err(Fail::denied("Invalid playback link.")),
    }
    Ok((beacon, ticket))
}

pub async fn enqueue(db: &mut PgConnection, beacon: &str, kind: &str, input: Value) -> Res<()> {
    sqlx::query("INSERT INTO beacon_jobs(id,beacon_id,kind,input) VALUES($1,$2,$3,$4) ON CONFLICT(beacon_id,kind) DO UPDATE SET input=EXCLUDED.input,available_at=now(),attempts=0 WHERE beacon_jobs.lease_until IS NULL OR beacon_jobs.lease_until<=now()")
        .bind(profiles::new_id())
        .bind(beacon)
        .bind(kind)
        .bind(input)
        .execute(db)
        .await?;
    Ok(())
}

/// Deleting removes every rendition, the thumbnail, the clean copy and any upload, then purges the CDN.
pub async fn request_delete(db: &mut PgConnection, id: &str, legal: bool) -> Res<()> {
    sqlx::query("UPDATE beacons SET status='DELETING',revision=revision+1 WHERE id=$1 AND status NOT IN ('DELETING','DELETED')")
        .bind(id)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM beacon_jobs WHERE beacon_id=$1 AND kind='PROCESS' AND (lease_until IS NULL OR lease_until<=now())")
        .bind(id)
        .execute(&mut *db)
        .await?;
    enqueue(db, id, "DELETE", json!({"legal":legal})).await
}

/// Account deletion: every Beacon the account owns is deleted.
pub async fn erase(db: &mut PgConnection, owner: &str) -> Res<()> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM beacons WHERE owner_id=$1 AND status NOT IN ('DELETING','DELETED')",
    )
    .bind(owner)
    .fetch_all(&mut *db)
    .await?;
    for id in ids {
        request_delete(db, &id, false).await?;
    }
    // Their likes elsewhere leave with them, and the counters follow.
    sqlx::query("UPDATE beacons SET likes=greatest(likes-1,0) WHERE id IN (SELECT beacon_id FROM beacon_likes WHERE user_id=$1)")
        .bind(owner)
        .execute(&mut *db)
        .await?;
    for statement in [
        "DELETE FROM beacon_likes WHERE user_id=$1",
        "DELETE FROM beacon_playback WHERE viewer_key='u:'||$1",
        "DELETE FROM beacon_events WHERE viewer_key='u:'||$1",
        "DELETE FROM beacon_mutes WHERE user_id=$1 OR muted_id=$1",
    ] {
        sqlx::query(statement).bind(owner).execute(&mut *db).await?;
    }
    Ok(())
}

/// When a recording or clip is removed, every Beacon made from it goes too (Take It Down and
/// copyright removals reach every copy).
pub async fn remove_made_from(db: &mut PgConnection, videos: &[String], legal: bool) -> Res<()> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM beacons WHERE clip_id=ANY($1) AND status NOT IN ('DELETING','DELETED') FOR UPDATE")
        .bind(videos)
        .fetch_all(&mut *db)
        .await?;
    for id in ids {
        request_delete(db, &id, legal).await?;
    }
    Ok(())
}

/// Public JSON for one Beacon in a feed, shelf or page; `viewer` decides `liked` and the ticket.
pub async fn present(
    app: &App,
    db: &mut PgConnection,
    beacons: Vec<Beacon>,
    viewer: Option<&auth::User>,
    age_ack: bool,
) -> Res<Vec<Value>> {
    let owners: Vec<String> = beacons
        .iter()
        .filter_map(|b| b.owner_id.clone())
        .chain(beacons.iter().filter_map(|b| b.clipper_id.clone()))
        .collect();
    let channels = profiles::public_channels(db, &owners).await?;
    let live: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM unnest($1::text[]) AS o(id) WHERE {}",
        crate::playback::live_sql("o.id")
    )))
    .bind(&owners)
    .fetch_all(&mut *db)
    .await?;
    let ids: Vec<String> = beacons.iter().map(|b| b.id.clone()).collect();
    let liked: Vec<String> = match viewer {
        Some(user) => {
            sqlx::query_scalar(
                "SELECT beacon_id FROM beacon_likes WHERE user_id=$1 AND beacon_id=ANY($2)",
            )
            .bind(&user.id)
            .bind(&ids)
            .fetch_all(&mut *db)
            .await?
        }
        None => vec![],
    };
    let mut out = Vec::new();
    for beacon in beacons {
        let Some(channel) = channels
            .iter()
            .find(|c| Some(&c.id) == beacon.owner_id.as_ref())
        else {
            continue;
        };
        let mut chip = profiles::chip(app, channel);
        chip["live"] = json!(live.contains(&channel.id));
        let clipper = beacon
            .clipper_id
            .as_ref()
            .filter(|c| Some(*c) != beacon.owner_id.as_ref())
            .and_then(|c| channels.iter().find(|u| &u.id == c))
            .map(|u| profiles::chip(app, u));
        let token = ticket(app, &beacon, viewer, "play", age_ack)?;
        let ready = matches!(beacon.status.as_str(), "READY" | "PUBLISHED");
        let id = beacon.id.clone();
        let playback = ready.then(|| {
            json!({
                "hd": format!("/api/beacons/{id}/file?q=hd&ticket={token}"),
                "sd": format!("/api/beacons/{id}/file?q=sd&ticket={token}"),
            })
        });
        // A mature Beacon's still never shows before the viewer passes the 18+ gate.
        let thumbnail = beacon
            .thumbnail_key
            .as_ref()
            .filter(|_| ready && (!beacon.mature || age_ack))
            .map(|_| format!("/api/beacons/{id}/thumbnail?ticket={token}"));
        let charity = beacon
            .charity_name
            .as_ref()
            .map(|name| json!({"name":name,"url":beacon.charity_url}));
        out.push(json!({
            "beacon": beacon,
            "channel": chip,
            "clipper": clipper,
            "liked": liked.contains(&id),
            "playback": playback,
            "thumbnail": thumbnail,
            "charity": charity,
        }));
    }
    Ok(out)
}
