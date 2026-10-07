//! Creator side: turning an approved clip into a Beacon, the direct-to-storage upload flow, and
//! publishing, retrying, editing, deleting and the clean download.
use super::*;
use axum::{
    Json,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    routing::{get, post, put},
};

async fn creator(app: &App, jar: &CookieJar) -> Res<auth::User> {
    let user = profiles::signed_in(app, jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    Ok(user)
}
async fn owned(app: &App, jar: &CookieJar, id: &str) -> Res<(Beacon, auth::User)> {
    let user = creator(app, jar).await?;
    let beacon = load(&mut *app.db.acquire().await?, id).await?;
    if beacon.owner_id.as_deref() != Some(&user.id)
        || matches!(beacon.status.as_str(), "DELETING" | "DELETED")
    {
        return Err(Fail::missing());
    }
    Ok((beacon, user))
}
fn title(value: &str) -> Res<String> {
    let value = crate::text::clean(value, "title", false)?;
    if value.is_empty() || value.chars().count() > 120 {
        return Err(Fail::bad("Use a title of 1–120 characters."));
    }
    Ok(value)
}
/// Crop fractions: the window's left edge, top edge and width, each within the frame.
fn crop(value: &Value) -> Res<Value> {
    let part = |key: &str| {
        value[key]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
    };
    match (part("x"), part("y"), part("width")) {
        (Some(x), Some(y), Some(width)) if width >= 0.05 && x + width <= 1.0001 => {
            Ok(json!({"x":x,"y":y,"width":width}))
        }
        _ => Err(Fail::bad("Choose a crop inside the video.")),
    }
}
async fn used_today(db: &mut PgConnection, owner: &str) -> Res<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM beacons WHERE owner_id=$1 AND quota AND created_at>now()-interval '1 day'",
    )
    .bind(owner)
    .fetch_one(db)
    .await?)
}
async fn stats(db: &mut PgConnection, ids: &[String]) -> Res<Vec<(String, String, i64)>> {
    Ok(sqlx::query_as("SELECT beacon_id,kind,count(*) FROM beacon_events WHERE beacon_id=ANY($1) AND kind<>'LIVE_TAP' GROUP BY beacon_id,kind")
        .bind(ids)
        .fetch_all(db)
        .await?)
}

/// GET /api/me/beacons: Creator Studio's Beacons page.
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let eligible = has_streamed(&mut db, &user.id).await?;
    let used = used_today(&mut db, &user.id).await?;
    let crop: Option<Value> = sqlx::query_scalar("SELECT crop FROM beacon_crops WHERE owner_id=$1")
        .bind(&user.id)
        .fetch_optional(&mut *db)
        .await?;
    let beacons: Vec<Beacon> = sqlx::query_as("SELECT * FROM beacons WHERE owner_id=$1 AND status NOT IN ('DELETING','DELETED') ORDER BY created_at DESC LIMIT 100")
        .bind(&user.id)
        .fetch_all(&mut *db)
        .await?;
    let ids: Vec<String> = beacons.iter().map(|b| b.id.clone()).collect();
    let counts = stats(&mut db, &ids).await?;
    let mut items = present(&app, &mut db, beacons, Some(&user), true).await?;
    for item in &mut items {
        let id = item["beacon"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let count = |kind: &str| {
            counts
                .iter()
                .find(|(b, k, _)| *b == id && k == kind)
                .map_or(0, |c| c.2)
        };
        item["stats"] = json!({"completions":count("COMPLETION"),"follows":count("FOLLOW"),"live_joins":count("LIVE_JOIN")});
    }
    // Approved clips of this channel, newest first, with a short-lived preview for the crop tool.
    let clips: Vec<crate::videos::Video> = sqlx::query_as("SELECT v.* FROM videos v WHERE v.owner_id=$1 AND v.kind='CLIP' AND v.status='READY' AND v.approval='APPROVED' AND v.mp4_key IS NOT NULL AND NOT EXISTS(SELECT 1 FROM video_holds WHERE video_id=v.id) ORDER BY v.created_at DESC LIMIT 60")
        .bind(&user.id)
        .fetch_all(&mut *db)
        .await?;
    let clippers: Vec<String> = clips.iter().filter_map(|c| c.clipper_id.clone()).collect();
    let clippers = profiles::public_channels(&mut db, &clippers).await?;
    let mut sources = Vec::new();
    for clip in clips {
        let token = crate::videos::ticket(&app, &clip, Some(&user), "play", true)?;
        let id = clip.id.clone();
        let clipper = clippers
            .iter()
            .find(|c| Some(&c.id) == clip.clipper_id.as_ref() && c.id != user.id)
            .map(|c| profiles::chip(&app, c));
        let thumbnail = clip
            .thumbnail_key
            .as_ref()
            .map(|_| format!("/api/videos/{id}/thumbnail?ticket={token}"));
        sources.push(json!({"id":id,"title":clip.title,"duration_ms":clip.duration_ms,"category":clip.category,"mature":clip.mature,"created_at":clip.created_at,"clipper":clipper,"file":format!("/api/videos/{id}/file?ticket={token}"),"thumbnail":thumbnail}));
    }
    Ok(Json(json!({
        "eligible": eligible,
        "configured": app.config.videos.storage.available(),
        "uploads": app.config.beacons.uploads,
        "daily_limit": DAILY_LIMIT,
        "remaining": (DAILY_LIMIT - used).max(0),
        "last_crop": crop,
        "beacons": items,
        "clips": sources,
    })))
}

#[derive(Deserialize)]
struct Create {
    source: String,
    clip_id: Option<String>,
    crop: Option<Value>,
    title: String,
    category_id: Option<String>,
    #[serde(default = "yes")]
    publish: bool,
    request_id: String,
}
fn yes() -> bool {
    true
}
/// POST /api/me/beacons
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Create>,
) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    if !matches!(input.source.as_str(), "CLIP" | "UPLOAD")
        || uuid::Uuid::parse_str(&input.request_id).is_err()
    {
        return Err(Fail::bad("Invalid Beacon request."));
    }
    if !app.config.videos.storage.available() {
        return Err(Fail::unavailable("Beacons aren't available yet."));
    }
    if input.source == "UPLOAD" && !app.config.beacons.uploads {
        return Err(Fail::denied(
            "Uploads open soon. Make a Beacon from one of your clips.",
        ));
    }
    let title = title(&input.title)?;
    moderation::check_clip(&app, &user.id, &user, &title).await?;
    let crop = input.crop.as_ref().map(crop).transpose()?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,91))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let prior: Option<String> =
        sqlx::query_scalar("SELECT id FROM beacons WHERE owner_id=$1 AND request_key=$2")
            .bind(&user.id)
            .bind(&input.request_id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(id) = prior {
        return Ok(Json(json!({"id":id,"queued":true})));
    }
    if !has_streamed(&mut tx, &user.id).await? {
        return Err(Fail::denied(
            "Go live on S.V.E.R once, then you can post Beacons.",
        ));
    }
    let settings = crate::videos::settings(&mut tx, &user.id).await?;
    if settings.copyright_restricted {
        return Err(Fail::denied(
            "Posting is restricted on this channel after repeated copyright removals.",
        ));
    }
    if used_today(&mut tx, &user.id).await? >= DAILY_LIMIT {
        return Err(Fail::bad("You can post up to 10 Beacons a day."));
    }
    let id = profiles::new_id();
    let seed = i64::from_le_bytes(
        uuid::Uuid::new_v4().as_bytes()[..8]
            .try_into()
            .map_err(|_| Fail::internal())?,
    );
    let mut upload = None;
    if input.source == "CLIP" {
        let clip_id = input
            .clip_id
            .as_deref()
            .ok_or_else(|| Fail::bad("Choose a clip."))?;
        let crop = crop.ok_or_else(|| Fail::bad("Choose where to crop the clip."))?;
        sqlx::query("SELECT id FROM videos WHERE id=$1 FOR UPDATE")
            .bind(clip_id)
            .execute(&mut *tx)
            .await?;
        let clip = crate::videos::load(&mut tx, clip_id).await?;
        if clip.kind != "CLIP"
            || clip.owner_id.as_deref() != Some(&user.id)
            || clip.status != "READY"
            || clip.approval != "APPROVED"
            || clip.mp4_key.is_none()
            || crate::videos::held(&mut tx, clip_id).await?
        {
            return Err(Fail::bad("Choose one of your channel's approved clips."));
        }
        let charity: Option<(String, String)> = sqlx::query_as(
            "SELECT charity_name,donate_url FROM charity_streams WHERE broadcast_id=$1 LIMIT 1",
        )
        .bind(&clip.broadcast_id)
        .fetch_optional(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO beacons(id,owner_id,clipper_id,clip_id,broadcast_id,source,status,publish,title,category_id,category,genre,mature,charity_name,charity_url,crop,seed,request_key) VALUES($1,$2,$3,$4,$5,'CLIP','PROCESSING',$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
            .bind(&id).bind(&user.id).bind(&clip.clipper_id).bind(clip_id).bind(&clip.broadcast_id).bind(input.publish).bind(&title)
            .bind(&clip.category_id).bind(&clip.category).bind(&clip.genre).bind(clip.mature)
            .bind(charity.as_ref().map(|c| &c.0)).bind(charity.as_ref().map(|c| &c.1)).bind(&crop).bind(seed).bind(&input.request_id)
            .execute(&mut *tx).await?;
        // Choosing a clip for a Beacon is the streamer's approval for it; it's logged like any review.
        sqlx::query("UPDATE videos SET beacon_approved=true WHERE id=$1")
            .bind(clip_id)
            .execute(&mut *tx)
            .await?;
        crate::safety::audit(
            &mut tx,
            Some(&user.id),
            "approve_clip_beacon",
            "clip",
            clip_id,
            &[],
            "Made a Beacon from this clip",
            json!({"beacon":id}),
            false,
        )
        .await?;
        sqlx::query("INSERT INTO beacon_crops(owner_id,crop) VALUES($1,$2) ON CONFLICT(owner_id) DO UPDATE SET crop=EXCLUDED.crop")
            .bind(&user.id).bind(&crop).execute(&mut *tx).await?;
        enqueue(&mut tx, &id, "PROCESS", json!({})).await?;
    } else {
        let category = input
            .category_id
            .as_deref()
            .ok_or_else(|| Fail::bad("Choose a category."))?;
        let category = crate::streams::catalog::select(&mut tx, category)
            .await
            .map_err(|_| Fail::bad("Choose an available category."))?;
        let (name, genre): (String, String) =
            sqlx::query_as("SELECT name,genre FROM stream_categories WHERE id=$1 AND active")
                .bind(&category)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| Fail::bad("Choose an available category."))?;
        let key = format!("beacons/{id}/upload");
        sqlx::query("INSERT INTO beacons(id,owner_id,source,status,publish,title,category_id,category,genre,mature,crop,seed,upload_key,upload_until,request_key) VALUES($1,$2,'UPLOAD','DRAFT',$3,$4,$5,$6,$7,$8,$9,$10,$11,now()+interval '1 hour',$12)")
            .bind(&id).bind(&user.id).bind(input.publish).bind(&title).bind(&category).bind(name).bind(genre)
            .bind(settings.mature).bind(&crop).bind(seed).bind(&key).bind(&input.request_id)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO beacon_objects(key,beacon_id,content_type) VALUES($1,$2,'application/octet-stream')")
            .bind(&key).bind(&id).execute(&mut *tx).await?;
        upload = Some(upload_target(&app, &id, &user, &key)?);
    }
    tx.commit().await?;
    Ok(Json(
        json!({"id":id,"queued":upload.is_none(),"upload":upload}),
    ))
}
/// Where the browser sends the file: a signed URL straight to private object storage, or in
/// development a signed local endpoint that writes to the same private store.
fn upload_target(app: &App, id: &str, user: &auth::User, key: &str) -> Res<Value> {
    if let crate::media::Storage::S3(s3) = &app.config.videos.storage {
        return Ok(
            json!({"method":"PUT","url":s3.presign_put(key, 3600)?,"max_bytes":MAX_UPLOAD_BYTES}),
        );
    }
    let sealed = security::seal(
        app,
        "beacon-upload",
        &json!({"id":id,"user":user.id,"until":Utc::now().timestamp()+3600}).to_string(),
    )?;
    let token: String = url::form_urlencoded::byte_serialize(sealed.as_bytes()).collect();
    Ok(
        json!({"method":"PUT","url":format!("/api/beacons/{id}/upload?ticket={token}"),"max_bytes":MAX_UPLOAD_BYTES}),
    )
}
#[derive(Deserialize)]
struct Link {
    ticket: String,
}
/// PUT /api/beacons/{id}/upload (local storage only; production uploads go straight to the bucket).
async fn local_upload(
    State(app): State<App>,
    jar: CookieJar,
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
    let user = creator(&app, &jar).await?;
    let ticket: Value = serde_json::from_str(
        &security::unseal(&app, "beacon-upload", &link.ticket)
            .map_err(|_| Fail::denied("Upload link expired."))?,
    )
    .map_err(|_| Fail::denied("Upload link expired."))?;
    if ticket["id"] != id
        || ticket["user"] != user.id
        || ticket["until"]
            .as_i64()
            .is_none_or(|t| t <= Utc::now().timestamp())
    {
        return Err(Fail::denied("Upload link expired."));
    }
    let beacon = load(&mut *app.db.acquire().await?, &id).await?;
    let key = beacon
        .upload_key
        .filter(|_| beacon.status == "DRAFT")
        .ok_or_else(|| Fail::bad("This upload is already complete."))?;
    app.config
        .videos
        .storage
        .put_typed(
            &app.http,
            &key,
            body.to_vec(),
            "application/octet-stream",
            "private, no-store",
        )
        .await?;
    Ok(Json(json!({"uploaded":true})))
}
/// POST /api/beacons/{id}/complete: the creator marks the upload done; the server takes it from here.
async fn complete(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let (beacon, _) = owned(&app, &jar, &id).await?;
    if beacon.status != "DRAFT" {
        return Ok(Json(json!({"queued":beacon.status=="PROCESSING"})));
    }
    let key = beacon.upload_key.as_deref().ok_or_else(Fail::missing)?;
    let size = app.config.videos.storage.head(&app.http, key).await?;
    let mut tx = app.db.begin().await?;
    match size {
        None | Some(0) => return Err(Fail::bad("Upload the video first.")),
        Some(bytes) if bytes > MAX_UPLOAD_BYTES => {
            sqlx::query("UPDATE beacons SET status='FAILED',failure='Videos can be up to 200 MB.',quota=false,revision=revision+1 WHERE id=$1 AND status='DRAFT'")
                .bind(&id).execute(&mut *tx).await?;
        }
        Some(bytes) => {
            sqlx::query("UPDATE beacon_objects SET bytes=$2,ready=true WHERE key=$1")
                .bind(key)
                .bind(bytes as i64)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE beacons SET status='PROCESSING',upload_until=NULL,revision=revision+1 WHERE id=$1 AND status='DRAFT'")
                .bind(&id).execute(&mut *tx).await?;
            enqueue(&mut tx, &id, "PROCESS", json!({})).await?;
        }
    }
    tx.commit().await?;
    Ok(Json(json!({"queued":true})))
}
/// POST /api/beacons/{id}/publish: a Ready Beacon goes into the feed.
async fn publish(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let (beacon, _) = owned(&app, &jar, &id).await?;
    if beacon.status != "READY" {
        return Err(Fail::bad("Only a ready Beacon can be published."));
    }
    sqlx::query("UPDATE beacons SET status='PUBLISHED',publish=true,published_at=now(),revision=revision+1 WHERE id=$1 AND status='READY'")
        .bind(&id)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"published":true})))
}
/// POST /api/beacons/{id}/retry: after a failure the creator can try again, within the daily limit.
async fn retry(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    let (beacon, user) = owned(&app, &jar, &id).await?;
    if beacon.status != "FAILED" {
        return Err(Fail::bad("Only a failed Beacon can be retried."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,91))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    if used_today(&mut tx, &user.id).await? >= DAILY_LIMIT {
        return Err(Fail::bad("You can post up to 10 Beacons a day."));
    }
    let status = if beacon.source == "UPLOAD" && beacon.upload_key.is_none() {
        return Err(Fail::bad("Upload the video again in a new Beacon."));
    } else {
        "PROCESSING"
    };
    sqlx::query("UPDATE beacons SET status=$2,failure=NULL,quota=true,revision=revision+1 WHERE id=$1 AND status='FAILED'")
        .bind(&id)
        .bind(status)
        .execute(&mut *tx)
        .await?;
    enqueue(&mut tx, &id, "PROCESS", json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({"queued":true})))
}
#[derive(Deserialize)]
struct Edit {
    title: String,
}
async fn edit(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Edit>,
) -> Res<Json<Value>> {
    let (_, user) = owned(&app, &jar, &id).await?;
    let title = title(&input.title)?;
    moderation::check_clip(&app, &user.id, &user, &title).await?;
    sqlx::query("UPDATE beacons SET title=$2 WHERE id=$1")
        .bind(&id)
        .bind(title)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"saved":true})))
}
/// DELETE /api/beacons/{id}: every rendition, the thumbnail and the clean copy go, and the CDN is purged.
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    let beacon = load(&mut *app.db.acquire().await?, &id).await?;
    if beacon.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::denied("You can't delete this Beacon."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT id FROM beacons WHERE id=$1 FOR UPDATE")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    request_delete(&mut tx, &id, false).await?;
    tx.commit().await?;
    Ok(Json(json!({"queued":true})))
}
/// POST /api/beacons/{id}/download: a short-lived link to the creator's clean copy.
async fn download(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let (beacon, user) = owned(&app, &jar, &id).await?;
    if beacon.clean_key.is_none() || !matches!(beacon.status.as_str(), "READY" | "PUBLISHED") {
        return Err(Fail::bad(
            "The clean copy is ready once processing finishes.",
        ));
    }
    let token = ticket(&app, &beacon, Some(&user), "clean", true)?;
    Ok(Json(
        json!({"url":format!("/api/beacons/{id}/file?q=clean&ticket={token}")}),
    ))
}
pub fn routes() -> axum::Router<App> {
    axum::Router::new()
        .route("/api/me/beacons", get(mine).post(create))
        .route(
            "/api/beacons/{id}/upload",
            put(local_upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES as usize + 1)),
        )
        .route("/api/beacons/{id}/complete", post(complete))
        .route("/api/beacons/{id}/publish", post(publish))
        .route("/api/beacons/{id}/retry", post(retry))
        .route("/api/beacons/{id}/download", post(download))
        .route(
            "/api/beacons/{id}",
            get(super::feed::page).patch(edit).delete(remove),
        )
}
