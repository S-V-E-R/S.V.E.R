//! Module 8: private recordings, independent cuts and durable media work.
mod config;
pub use config::Config;
pub(crate) mod delivery;
mod manage;
pub mod recording;
mod routes;
pub mod worker;
pub use manage::marker;
pub mod copyright;
pub mod review;
mod sharing;
mod spotlight;
mod views;
use crate::{
    App, auth, moderation,
    profiles::{self, Fail, Res},
    security, tiers,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
pub use routes::routes;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub use spotlight::spotlight;
use sqlx::PgConnection;

pub const HIGHLIGHT_HOURS: [i64; 4] = [10, 25, 50, 100];
pub const MAX_SEGMENT_BYTES: usize = 16 * 1024 * 1024;
#[derive(sqlx::FromRow, Serialize)]
pub struct Video {
    pub id: String,
    pub owner_id: Option<String>,
    pub clipper_id: Option<String>,
    pub broadcast_id: Option<String>,
    pub parent_id: Option<String>,
    pub kind: String,
    pub status: String,
    pub approval: String,
    pub visibility: String,
    pub recording: bool,
    pub mature: bool,
    pub title: String,
    pub category_id: Option<String>,
    pub category: Option<String>,
    pub genre: Option<String>,
    pub faction: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub duration_ms: i64,
    pub source_start_ms: i64,
    pub source_end_ms: Option<i64>,
    pub revision: i64,
    pub views: i64,
    #[serde(skip_serializing)]
    pub thumbnail_key: Option<String>,
    #[serde(skip_serializing)]
    pub mp4_key: Option<String>,
    #[serde(skip_serializing)]
    pub download_key: Option<String>,
    pub download_until: Option<DateTime<Utc>>,
    pub beacon_approved: bool,
}
#[derive(sqlx::FromRow, Serialize, Deserialize)]
pub struct Settings {
    pub recording: bool,
    pub visibility: String,
    pub clip_permission: String,
    pub clip_approval: bool,
    pub chat_replay: bool,
    pub mature: bool,
    #[serde(default, skip_deserializing)]
    pub copyright_restricted: bool,
}
pub async fn settings(db: &mut PgConnection, owner: &str) -> Res<Settings> {
    sqlx::query("INSERT INTO video_settings(owner_id) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(owner)
        .execute(&mut *db)
        .await?;
    Ok(
        sqlx::query_as("SELECT * FROM video_settings WHERE owner_id=$1")
            .bind(owner)
            .fetch_one(db)
            .await?,
    )
}
pub async fn load(db: &mut PgConnection, id: &str) -> Res<Video> {
    sqlx::query_as("SELECT * FROM videos WHERE id=$1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(Fail::missing)
}
pub async fn editor(app: &App, owner: &str, user: &auth::User) -> Res<bool> {
    if user.id == owner {
        return Ok(true);
    }
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM video_editors WHERE owner_id=$1 AND editor_id=$2)",
    )
    .bind(owner)
    .bind(&user.id)
    .fetch_one(&app.db)
    .await?)
}
pub async fn held(db: &mut PgConnection, id: &str) -> Res<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_holds WHERE video_id=$1)")
            .bind(id)
            .fetch_one(db)
            .await?,
    )
}
pub async fn accessible(
    app: &App,
    video: &Video,
    user: Option<&auth::User>,
    age_ack: bool,
) -> Res<()> {
    access(app, video, user, age_ack, false).await
}
pub async fn downloadable(app: &App, video: &Video, user: &auth::User) -> Res<()> {
    if video.kind == "CLIP"
        || video.status != "READY"
        || !editor(
            app,
            video.owner_id.as_deref().ok_or_else(Fail::missing)?,
            user,
        )
        .await?
    {
        return Err(Fail::denied(
            "Only the streamer and editors can download recordings.",
        ));
    }
    access(app, video, Some(user), true, true).await
}
async fn access(
    app: &App,
    video: &Video,
    user: Option<&auth::User>,
    age_ack: bool,
    download: bool,
) -> Res<()> {
    let owner = video.owner_id.as_deref().ok_or_else(Fail::missing)?;
    if !matches!(video.status.as_str(), "RECORDING" | "PROCESSING" | "READY")
        || video.expires_at.is_some_and(|at| at <= Utc::now())
    {
        return Err(Fail::missing());
    }
    let mut db = app.db.acquire().await?;
    if held(&mut db, &video.id).await? {
        return Err(Fail::missing());
    }
    let channel = profiles::channel_user_by_id(&mut db, owner)
        .await?
        .ok_or_else(Fail::missing)?;
    if !channel.eligible {
        return Err(Fail::missing());
    }
    let privileged = download
        || match user {
            Some(user) => moderation::role_of(app, owner, user).await?.is_some(),
            None => false,
        };
    if let Some(user) = user
        && (profiles::blocked_between(&mut db, owner, &user.id).await?
            || moderation::banned(app, owner, &user.id).await?)
    {
        return Err(Fail::denied("This recording is unavailable."));
    }
    if video.approval != "APPROVED"
        && !privileged
        && !user.is_some_and(|u| video.clipper_id.as_deref() == Some(&u.id))
    {
        return Err(Fail::missing());
    }
    if video.visibility == "PRIVATE" && !privileged {
        return Err(Fail::denied("This recording is private."));
    }
    if video.visibility == "SUBSCRIBERS" && !privileged {
        let user = user.ok_or_else(|| Fail::denied("Subscribe to watch this recording."))?;
        if crate::subs::active_tier(app, owner, &user.id)
            .await?
            .is_none()
        {
            return Err(Fail::denied("Subscribe to watch this recording."));
        }
    }
    if video.mature {
        if let Some(user) = user
            && !crate::auth::is_adult(&mut db, &user.id).await?
        {
            return Err(Fail::denied(
                "This recording is for viewers aged 18 and over.",
            ));
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
) -> Res<(Video, Option<auth::User>)> {
    let user = profiles::viewer(app, jar).await?;
    let video = load(&mut *app.db.acquire().await?, id).await?;
    accessible(app, &video, user.as_ref(), age_ack).await?;
    Ok((video, user))
}
#[derive(Serialize, Deserialize)]
pub struct Ticket {
    pub video: String,
    pub revision: i64,
    pub subject: Option<String>,
    pub scope: String,
    pub until: i64,
    pub age_ack: bool,
}
pub fn ticket(
    app: &App,
    video: &Video,
    user: Option<&auth::User>,
    scope: &str,
    age_ack: bool,
) -> Res<String> {
    let value = Ticket {
        video: video.id.clone(),
        revision: video.revision,
        subject: user.map(|u| u.id.clone()),
        scope: scope.into(),
        until: Utc::now().timestamp() + 300,
        age_ack,
    };
    Ok(url::form_urlencoded::byte_serialize(
        security::seal(
            app,
            "video-ticket",
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
) -> Res<(Video, Ticket)> {
    if raw.len() > 4096 {
        return Err(Fail::denied("Playback link expired. Reload the player."));
    }
    let ticket: Ticket = serde_json::from_str(
        &security::unseal(app, "video-ticket", raw)
            .map_err(|_| Fail::denied("Invalid playback link."))?,
    )
    .map_err(|_| Fail::denied("Invalid playback link."))?;
    if ticket.video != id || ticket.until <= Utc::now().timestamp() {
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
    let video = load(&mut *app.db.acquire().await?, id).await?;
    if video.revision != ticket.revision {
        return Err(Fail::denied(
            "Playback permission changed. Reload the player.",
        ));
    }
    if ticket.scope == "download" {
        let user = user
            .as_ref()
            .ok_or_else(|| Fail::denied("Sign in to download."))?;
        downloadable(app, &video, user).await?;
    } else if ticket.scope == "play" {
        accessible(app, &video, user.as_ref(), ticket.age_ack).await?;
    } else if ticket.scope == "review" {
        crate::safety::staff_write(app, jar).await?;
        if matches!(video.status.as_str(), "DELETED" | "EXPIRED") {
            return Err(Fail::missing());
        }
    } else {
        return Err(Fail::denied("Invalid playback link."));
    }
    Ok((video, ticket))
}
pub async fn enqueue(
    db: &mut PgConnection,
    video: &str,
    kind: &str,
    object: Option<&str>,
    input: Value,
) -> Res<()> {
    sqlx::query("INSERT INTO video_jobs(id,video_id,kind,object_key,input) VALUES($1,$2,$3,$4,$5) ON CONFLICT(kind,object_key) DO NOTHING")
        .bind(profiles::new_id()).bind(video).bind(kind).bind(object).bind(input).execute(db).await?;
    Ok(())
}
pub async fn chapter(
    db: &mut PgConnection,
    owner: &str,
    source: &str,
    source_id: Option<&str>,
    label: &str,
) -> Res<()> {
    sqlx::query("INSERT INTO video_chapters(id,video_id,offset_ms,label,source,source_id) SELECT $1,id,duration_ms,$3,$4,$5 FROM videos WHERE owner_id=$2 AND kind='VOD' AND status='RECORDING' AND recording AND ended_at IS NULL ON CONFLICT(video_id,source,source_id) DO NOTHING")
        .bind(profiles::new_id()).bind(owner).bind(label).bind(source).bind(source_id).execute(db).await?;
    Ok(())
}
pub async fn request_delete(db: &mut PgConnection, id: &str, expiry: bool) -> Res<()> {
    if held(db, id).await? {
        return Err(Fail::denied("This recording is held for a review."));
    }
    sqlx::query("UPDATE videos SET status='DELETING',revision=revision+1 WHERE id=$1 AND status NOT IN ('EXPIRED','DELETED','DELETING')").bind(id).execute(&mut *db).await?;
    enqueue(db, id, "DELETE", Some(id), json!({"expiry":expiry})).await
}
pub async fn erase(db: &mut PgConnection, owner: &str) -> Res<()> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM videos WHERE owner_id=$1 AND status NOT IN ('DELETED','EXPIRED')",
    )
    .bind(owner)
    .fetch_all(&mut *db)
    .await?;
    for id in ids {
        if !held(db, &id).await? {
            request_delete(db, &id, false).await?;
        }
    }
    sqlx::query("DELETE FROM video_chat WHERE author_id=$1")
        .bind(owner)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn extend_retention(db: &mut PgConnection, owner: &str, tier: i16) -> Res<()> {
    let hours = tiers::VOD_HOURS[tier as usize];
    sqlx::query("UPDATE videos SET retention_hours=$2,expires_at=CASE WHEN ended_at IS NULL THEN NULL ELSE ended_at+make_interval(hours=>$2) END WHERE owner_id=$1 AND kind='VOD' AND retention_hours<$2 AND status IN ('READY','RECORDING') AND (expires_at IS NULL OR expires_at>now())")
        .bind(owner).bind(hours as i32).execute(db).await?;
    Ok(())
}
pub async fn ended(db: &mut PgConnection, owner: &str) -> Res<()> {
    sqlx::query("UPDATE videos SET ended_at=coalesce(ended_at,clock_timestamp()),expires_at=coalesce(ended_at,clock_timestamp())+make_interval(hours=>retention_hours) WHERE owner_id=$1 AND kind='VOD' AND status='RECORDING'")
        .bind(owner).execute(db).await?;
    Ok(())
}
pub async fn redact_messages(db: &mut PgConnection, ids: &[String]) -> Res<()> {
    sqlx::query("DELETE FROM video_chat WHERE message_id=ANY($1)")
        .bind(ids)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn copy_replay(
    db: &mut PgConnection,
    source: &str,
    destination: &str,
    owner: &str,
    start: i64,
    end: i64,
    vod: bool,
) -> Res<()> {
    if vod {
        let query = format!(
            "WITH chat AS ({}) INSERT INTO video_chat(video_id,message_id,author_id,offset_ms,message) SELECT $2,c.id,c.author_id,s.start_ms-$4+(extract(epoch FROM c.created_at-s.wall_start)*1000)::bigint,c.message FROM video_segments s JOIN chat c ON c.channel_id=$3 AND c.created_at>=s.wall_start AND c.created_at<s.wall_start+make_interval(secs=>s.duration_ms::float8/1000) WHERE s.video_id=$1 AND s.start_ms>=$4 AND s.start_ms<$5 ON CONFLICT DO NOTHING",
            crate::chat::replay_source_sql()
        );
        sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(source)
            .bind(destination)
            .bind(owner)
            .bind(start)
            .bind(end)
            .execute(db)
            .await?;
    } else {
        sqlx::query("INSERT INTO video_chat(video_id,message_id,author_id,offset_ms,message) SELECT $2,message_id,author_id,offset_ms-$3,message FROM video_chat WHERE video_id=$1 AND offset_ms>=$3 AND offset_ms<$4 ON CONFLICT DO NOTHING").bind(source).bind(destination).bind(start).bind(end).execute(db).await?;
    }
    Ok(())
}
