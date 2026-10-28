//! Clip uploads from S.V.E.R Desktop (docs/DEVELOPER_PLATFORM.md "Desktop clip upload"): the
//! streamer creates the clip, PUTs the MP4 straight to private storage, then marks it complete;
//! the UPLOAD job (worker.rs) checks and remuxes it before it becomes a clip on their channel.
use super::*;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::HeaderMap,
    routing::{get, post, put},
};

pub const MAX_UPLOAD_BYTES: u64 = 120_000_000;
pub const MAX_UPLOAD_MS: i64 = 5 * 60_000;

/// The streamer: S.V.E.R Desktop's token with `clips:upload`, or else the signed-in person.
async fn streamer(app: &App, headers: &HeaderMap, jar: &CookieJar) -> Res<auth::User> {
    let user = crate::devapps::actor(app, headers, jar, crate::devapps::CLIPS_UPLOAD.0).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    Ok(user)
}
async fn uploaded(app: &App, headers: &HeaderMap, jar: &CookieJar, id: &str) -> Res<Video> {
    let user = streamer(app, headers, jar).await?;
    let video = load(&mut *app.db.acquire().await?, id).await?;
    if video.owner_id.as_deref() != Some(&user.id)
        || video.parent_id.is_some()
        || video.kind != "CLIP"
        || matches!(video.status.as_str(), "DELETING" | "DELETED" | "EXPIRED")
    {
        return Err(Fail::missing());
    }
    Ok(video)
}
fn upload_key(id: &str) -> String {
    format!("{id}/upload")
}
/// Where the file goes: a signed URL straight to private object storage, or in development a
/// signed local endpoint that writes to the same private store.
fn target(app: &App, id: &str) -> Res<Value> {
    if let crate::media::Storage::S3(s3) = &app.config.videos.storage {
        return Ok(
            json!({"method":"PUT","url":s3.presign_put(&upload_key(id), 3600)?,"max_bytes":MAX_UPLOAD_BYTES}),
        );
    }
    let sealed = security::seal(
        app,
        "clip-upload",
        &json!({"id":id,"until":Utc::now().timestamp()+3600}).to_string(),
    )?;
    let token: String = url::form_urlencoded::byte_serialize(sealed.as_bytes()).collect();
    Ok(
        json!({"method":"PUT","url":format!("/api/videos/{id}/upload?ticket={token}"),"max_bytes":MAX_UPLOAD_BYTES}),
    )
}
#[derive(Deserialize)]
struct Create {
    title: String,
    request_id: String,
    visibility: Option<String>,
}
/// POST /api/me/clips: a new clip waiting for its file, with where to PUT it. Retrying with the
/// same request_id returns the same clip.
async fn create(
    State(app): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<Create>,
) -> Res<Json<Value>> {
    let user = streamer(&app, &headers, &jar).await?;
    if uuid::Uuid::parse_str(&input.request_id).is_err() {
        return Err(Fail::bad("Invalid upload request."));
    }
    if let Some(value) = &input.visibility {
        super::routes::visibility(value)?;
    }
    let title = super::routes::title(&input.title, 100)?;
    crate::switches::guard(&mut *app.db.acquire().await?, "clipping").await?;
    if !app.config.videos.storage.available() {
        return Err(Fail::unavailable("Clip uploads aren't available yet."));
    }
    moderation::check_clip(&app, &user.id, &user, &title).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,81))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let prior: Option<(String, String)> =
        sqlx::query_as("SELECT id,status FROM videos WHERE clipper_id=$1 AND request_key=$2")
            .bind(&user.id)
            .bind(&input.request_id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((id, status)) = prior {
        let upload = (status == "UPLOADING")
            .then(|| target(&app, &id))
            .transpose()?;
        return Ok(Json(json!({"id":id,"status":status,"upload":upload})));
    }
    let settings = settings(&mut tx, &user.id).await?;
    if settings.copyright_restricted {
        return Err(Fail::denied(
            "Recording and clipping are restricted on this channel.",
        ));
    }
    profiles::rate(
        &app,
        format!("video-upload:{}", user.id),
        app.config.videos.tuning.clips_per_viewer_hour,
        3600,
    )
    .await?;
    let faction = crate::factions::membership(&mut tx, &user.id).await?;
    let id = profiles::new_id();
    sqlx::query("INSERT INTO videos(id,owner_id,clipper_id,kind,status,visibility,mature,title,faction,started_at,request_key) VALUES($1,$2,$2,'CLIP','UPLOADING',$3,$4,$5,$6,now(),$7)")
        .bind(&id).bind(&user.id).bind(input.visibility.as_ref().unwrap_or(&settings.visibility)).bind(settings.mature).bind(title).bind(faction).bind(&input.request_id)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO video_objects(key,video_id,content_type) VALUES($1,$2,'video/mp4')")
        .bind(upload_key(&id))
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":id,"status":"UPLOADING","upload":target(&app,&id)?}),
    ))
}
#[derive(Deserialize)]
struct Link {
    ticket: String,
}
/// PUT /api/videos/{id}/upload (local storage only; production uploads go straight to the bucket).
/// The sealed ticket is the permission, so S.V.E.R Desktop needs no cookie here.
async fn local_upload(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(link): Query<Link>,
    body: Bytes,
) -> Res<Json<Value>> {
    if !matches!(
        app.config.videos.storage,
        crate::media::Storage::Filesystem(_)
    ) {
        return Err(Fail::missing());
    }
    let ticket: Value = serde_json::from_str(
        &security::unseal(&app, "clip-upload", &link.ticket)
            .map_err(|_| Fail::denied("Upload link expired."))?,
    )
    .map_err(|_| Fail::denied("Upload link expired."))?;
    if ticket["id"] != id
        || ticket["until"]
            .as_i64()
            .is_none_or(|t| t <= Utc::now().timestamp())
    {
        return Err(Fail::denied("Upload link expired."));
    }
    if load(&mut *app.db.acquire().await?, &id).await?.status != "UPLOADING" {
        return Err(Fail::bad("This upload is already complete."));
    }
    app.config
        .videos
        .storage
        .put_typed(
            &app.http,
            &upload_key(&id),
            body.to_vec(),
            "video/mp4",
            "private, no-store",
        )
        .await?;
    Ok(Json(json!({"uploaded":true})))
}
/// POST /api/me/clips/{id}/complete: the file is in storage; the server takes it from here.
async fn complete(
    State(app): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let video = uploaded(&app, &headers, &jar, &id).await?;
    if video.status != "UPLOADING" {
        return Ok(Json(json!({"id":id,"status":video.status})));
    }
    let key = upload_key(&id);
    let bytes = match app.config.videos.storage.head(&app.http, &key).await? {
        None | Some(0) => return Err(Fail::bad("Upload the video first.")),
        Some(bytes) if bytes > MAX_UPLOAD_BYTES => {
            return Err(Fail::bad("Clips can be up to 120 MB."));
        }
        Some(bytes) => bytes,
    };
    let mut tx = app.db.begin().await?;
    let started = sqlx::query(
        "UPDATE videos SET status='PROCESSING',revision=revision+1 WHERE id=$1 AND status='UPLOADING'",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if started == 1 {
        sqlx::query("UPDATE video_objects SET bytes=$2,ready=true WHERE key=$1")
            .bind(&key)
            .bind(bytes as i64)
            .execute(&mut *tx)
            .await?;
        enqueue(&mut tx, &id, "UPLOAD", Some(&key), json!({})).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"id":id,"status":"PROCESSING"})))
}
/// GET /api/me/clips/{id}: where an upload stands, the reason when it failed, and its page when ready.
async fn status(
    State(app): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let video = uploaded(&app, &headers, &jar, &id).await?;
    let url = (video.status == "READY").then(|| format!("{}/clips/{id}", app.config.origin));
    Ok(Json(
        json!({"id":id,"status":video.status,"failure":video.failure,"url":url}),
    ))
}

/// What ffprobe must find in an uploaded clip: an MP4 with H.264 video and AAC or Opus audio (or
/// none), 1 second to 5 minutes long. Returns the length in milliseconds.
pub fn check(info: &Value) -> Result<i64, &'static str> {
    let unreadable = "That file isn't an MP4 clip we can read.";
    let format = info["format"]["format_name"].as_str().ok_or(unreadable)?;
    if !format.split(',').any(|f| f == "mp4" || f == "mov") {
        return Err("Upload an MP4 file.");
    }
    let streams = info["streams"].as_array().ok_or(unreadable)?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .ok_or(unreadable)?;
    if video["codec_name"] != "h264" {
        return Err("Clips need H.264 video.");
    }
    let size = |key: &str| {
        video[key]
            .as_i64()
            .is_some_and(|v| (16..=4096).contains(&v))
    };
    if !size("width") || !size("height") {
        return Err("That video's size isn't supported.");
    }
    if streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .any(|s| !matches!(s["codec_name"].as_str(), Some("aac" | "opus")))
    {
        return Err("Clips need AAC or Opus audio.");
    }
    let seconds: f64 = info["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .ok_or(unreadable)?;
    let ms = (seconds * 1000.0).round();
    if !ms.is_finite() || !(1000.0..=MAX_UPLOAD_MS as f64).contains(&ms) {
        return Err("Clips can be 1 second to 5 minutes long.");
    }
    Ok(ms as i64)
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/clips", post(create))
        .route("/api/me/clips/{id}", get(status))
        .route("/api/me/clips/{id}/complete", post(complete))
        .route(
            "/api/videos/{id}/upload",
            put(local_upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES as usize + 1)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn probe(format: &str, video: &str, audio: Option<&str>, duration: &str) -> Value {
        let mut streams =
            vec![json!({"codec_type":"video","codec_name":video,"width":1920,"height":1080})];
        if let Some(audio) = audio {
            streams.push(json!({"codec_type":"audio","codec_name":audio}));
        }
        json!({"format":{"format_name":format,"duration":duration},"streams":streams})
    }
    #[test]
    fn only_short_h264_mp4_clips_pass() {
        let mp4 = "mov,mp4,m4a,3gp,3g2,mj2";
        assert_eq!(check(&probe(mp4, "h264", Some("aac"), "30.5")), Ok(30500));
        assert_eq!(check(&probe(mp4, "h264", Some("opus"), "1")), Ok(1000));
        assert_eq!(check(&probe(mp4, "h264", None, "300")), Ok(300_000));
        for bad in [
            probe("matroska,webm", "h264", Some("aac"), "30"),
            probe(mp4, "hevc", Some("aac"), "30"),
            probe(mp4, "h264", Some("mp3"), "30"),
            probe(mp4, "h264", Some("aac"), "300.6"),
            probe(mp4, "h264", Some("aac"), "0.4"),
            probe(mp4, "h264", Some("aac"), "nan"),
            json!({"format":{"format_name":mp4,"duration":"30"},"streams":[]}),
        ] {
            assert!(check(&bad).is_err(), "{bad}");
        }
    }
}
