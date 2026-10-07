use super::*;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};

fn visibility(value: &str) -> Res<()> {
    if !matches!(value, "PUBLIC" | "SUBSCRIBERS" | "PRIVATE") {
        return Err(Fail::bad("Choose Public, Subscribers or Private."));
    }
    Ok(())
}
fn title(value: &str, limit: usize) -> Res<String> {
    let value = crate::text::clean(value, "title", false)?;
    if value.is_empty() || value.chars().count() > limit {
        return Err(Fail::bad(format!("Use a title of 1–{limit} characters.")));
    }
    Ok(value)
}
async fn owner(app: &App, jar: &CookieJar) -> Res<auth::User> {
    let user = profiles::signed_in(app, jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    Ok(user)
}
#[derive(Deserialize, Default)]
struct LibraryQuery {
    #[serde(default)]
    offset: i64,
    #[serde(default)]
    kind: String,
}
async fn mine(
    State(app): State<App>,
    jar: CookieJar,
    Query(query): Query<LibraryQuery>,
) -> Res<Json<Value>> {
    if !(0..=1_000_000).contains(&query.offset)
        || !matches!(
            query.kind.as_str(),
            "" | "ALL" | "PENDING" | "VOD" | "HIGHLIGHT" | "CLIP"
        )
    {
        return Err(Fail::bad("Choose a valid library page."));
    }
    let user = owner(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let settings = settings(&mut db, &user.id).await?;
    let tier = tiers::tier_of(&mut db, &user.id).await?;
    let mut videos:Vec<Video>=sqlx::query_as("SELECT * FROM videos WHERE owner_id=$1 AND status NOT IN ('DELETED','EXPIRED') AND ($2 IN ('','ALL') OR kind=$2 OR ($2='PENDING' AND approval='PENDING')) ORDER BY created_at DESC,id DESC LIMIT 101 OFFSET $3").bind(&user.id).bind(&query.kind).bind(query.offset).fetch_all(&mut *db).await?;
    let has_more = videos.len() > 100;
    videos.truncate(100);
    let editors: Vec<String> =
        sqlx::query_scalar("SELECT editor_id FROM video_editors WHERE owner_id=$1")
            .bind(&user.id)
            .fetch_all(&mut *db)
            .await?;
    let editors = profiles::public_channels(&mut db, &editors)
        .await?
        .iter()
        .map(|u| profiles::chip(&app, u))
        .collect::<Vec<_>>();
    let used:i64=sqlx::query_scalar("SELECT coalesce(sum(duration_ms),0)::bigint FROM videos WHERE owner_id=$1 AND kind='HIGHLIGHT' AND status NOT IN ('DELETED','EXPIRED','DELETING','FAILED')").bind(&user.id).fetch_one(&mut *db).await?;
    let assigned:Vec<Video>=sqlx::query_as("SELECT v.* FROM videos v JOIN video_editors e ON e.owner_id=v.owner_id WHERE e.editor_id=$1 AND v.kind<>'CLIP' AND v.status='READY' AND (v.expires_at IS NULL OR v.expires_at>now()) AND NOT EXISTS(SELECT 1 FROM video_holds WHERE video_id=v.id) ORDER BY v.created_at DESC LIMIT 100").bind(&user.id).fetch_all(&mut *db).await?;
    drop(db);
    let mut editing = Vec::new();
    for video in assigned {
        if downloadable(&app, &video, &user).await.is_ok() {
            editing.push(video);
        }
    }
    Ok(Json(
        json!({"settings":settings,"videos":videos,"has_more":has_more,"editors":editors,"editing":editing,"username":user.username,"configured":app.config.videos.storage.available(),"highlight_used_ms":used,"highlight_limit_ms":HIGHLIGHT_HOURS[tier as usize]*3_600_000,"retention_hours":tiers::VOD_HOURS[tier as usize]}),
    ))
}
async fn save_settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let user = owner(&app, &jar).await?;
    visibility(&input.visibility)?;
    if !matches!(
        input.clip_permission.as_str(),
        "SIGNED_IN" | "FOLLOWERS" | "SUBSCRIBERS" | "MODS" | "OFF"
    ) {
        return Err(Fail::bad("Choose who can clip."));
    }
    let mut tx = app.db.begin().await?;
    settings(&mut tx, &user.id).await?;
    sqlx::query("UPDATE video_settings SET recording=$2,visibility=$3,clip_permission=$4,clip_approval=$5,chat_replay=$6,mature=$7 WHERE owner_id=$1")
        .bind(&user.id).bind(input.recording).bind(input.visibility).bind(input.clip_permission).bind(input.clip_approval).bind(input.chat_replay).bind(input.mature).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize, Default)]
struct WatchQuery {
    #[serde(default)]
    age_ack: bool,
}
async fn page(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<WatchQuery>,
) -> Res<Json<Value>> {
    let (video, user) = watched(&app, &jar, &id, query.age_ack).await?;
    let owner = video.owner_id.as_deref().ok_or_else(Fail::missing)?;
    let mut db = app.db.acquire().await?;
    let channel = profiles::channel_user_by_id(&mut db, owner)
        .await?
        .ok_or_else(Fail::missing)?;
    let chapters:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'offset_ms',offset_ms,'label',label,'source',source) FROM video_chapters WHERE video_id=$1 ORDER BY offset_ms,id").bind(&id).fetch_all(&mut *db).await?;
    let settings = settings(&mut db, owner).await?;
    let can_manage = match &user {
        Some(user) => moderation::role_of(&app, owner, user).await?.is_some(),
        None => false,
    };
    let can_download = match &user {
        Some(user) => editor(&app, owner, user).await?,
        None => false,
    };
    let ticket = ticket(&app, &video, user.as_ref(), "play", query.age_ack)?;
    let playback = if video.kind == "CLIP" {
        format!("/api/videos/{id}/file?ticket={ticket}")
    } else {
        format!("/api/videos/{id}/playlist?ticket={ticket}")
    };
    let is_owner = user.as_ref().is_some_and(|u| u.id == owner);
    let can_delete = is_owner
        || (video.kind == "CLIP"
            && (can_manage
                || user
                    .as_ref()
                    .is_some_and(|u| Some(&u.id) == video.clipper_id.as_ref())));
    Ok(Json(
        json!({"video":video,"channel":profiles::chip(&app,&channel),"live":crate::playback::is_live(&mut db,owner).await?,"chapters":chapters,"signed_in":user.is_some(),"is_owner":is_owner,"can_manage":can_manage,"can_delete":can_delete,"can_download":can_download,"can_highlight":is_owner,"chat_replay":settings.chat_replay,"clip_permission":settings.clip_permission,"playback":playback,"thumbnail":video.thumbnail_key.as_ref().map(|_|format!("/api/videos/{id}/thumbnail?ticket={ticket}"))}),
    ))
}
#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    popular: bool,
    #[serde(default)]
    offset: i64,
}
async fn channel(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(query): Query<Listing>,
) -> Res<Json<Value>> {
    if !(0..=1_000_000).contains(&query.offset) {
        return Err(Fail::bad("Choose a valid videos page."));
    }
    let mut db = app.db.acquire().await?;
    let channel = profiles::eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let user = profiles::viewer(&app, &jar).await?;
    let rows:Vec<Video>=sqlx::query_as("WITH ranked AS (SELECT v.*,row_number() OVER (PARTITION BY kind ORDER BY CASE WHEN $3 THEN views ELSE 0 END DESC,created_at DESC,id DESC) AS position FROM videos v WHERE owner_id=$1 AND status='READY' AND recording AND (expires_at IS NULL OR expires_at>now())) SELECT * FROM ranked WHERE position>$2 AND position<=$2+21 ORDER BY kind,position")
        .bind(&channel.id).bind(query.offset).bind(query.popular).fetch_all(&mut *db).await?;
    drop(db);
    let mut videos = Vec::new();
    let mut seen = std::collections::HashMap::<String, usize>::new();
    let mut has_more = false;
    for video in rows {
        let count = seen.entry(video.kind.clone()).or_default();
        *count += 1;
        if *count > 20 {
            has_more = true;
            continue;
        }
        if accessible(&app, &video, user.as_ref(), true).await.is_ok() {
            let thumbnail = if video.mature {
                None
            } else {
                video
                    .thumbnail_key
                    .as_ref()
                    .map(|_| {
                        ticket(&app, &video, user.as_ref(), "play", false).map(|token| {
                            format!("/api/videos/{}/thumbnail?ticket={token}", video.id)
                        })
                    })
                    .transpose()?
            };
            videos.push(json!({"video":video,"thumbnail":thumbnail}));
        }
    }
    Ok(Json(json!({"videos":videos,"has_more":has_more})))
}
async fn latest(State(app): State<App>) -> Res<Json<Value>> {
    let rows:Vec<Video>=sqlx::query_as("SELECT v.* FROM videos v WHERE kind='CLIP' AND status='READY' AND approval='APPROVED' AND visibility='PUBLIC' AND NOT mature ORDER BY created_at DESC LIMIT 24").fetch_all(&app.db).await?;
    let mut clips = Vec::new();
    for video in rows {
        if accessible(&app, &video, None, false).await.is_ok() {
            let owner = profiles::channel_user_by_id(
                &mut *app.db.acquire().await?,
                video.owner_id.as_deref().unwrap_or(""),
            )
            .await?
            .ok_or_else(Fail::missing)?;
            let token = ticket(&app, &video, None, "play", false)?;
            clips.push(json!({"video":video,"channel":profiles::chip(&app,&owner),"thumbnail":video.thumbnail_key.as_ref().map(|_|format!("/api/videos/{}/thumbnail?ticket={token}",video.id))}));
        }
        if clips.len() == 8 {
            break;
        }
    }
    Ok(Json(json!({"clips":clips})))
}
#[derive(Deserialize)]
struct Cut {
    kind: String,
    title: String,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    request_id: String,
    #[serde(default)]
    age_ack: bool,
}
#[derive(sqlx::FromRow)]
struct CutSegment {
    id: String,
    start_ms: i64,
    duration_ms: i64,
}
async fn cut(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Cut>,
) -> Res<Json<Value>> {
    let user = owner(&app, &jar).await?;
    if !matches!(input.kind.as_str(), "CLIP" | "HIGHLIGHT")
        || uuid::Uuid::parse_str(&input.request_id).is_err()
    {
        return Err(Fail::bad("Invalid cut request."));
    }
    let source = load(&mut *app.db.acquire().await?, &id).await?;
    accessible(&app, &source, Some(&user), input.age_ack).await?;
    let channel = source.owner_id.as_deref().ok_or_else(Fail::missing)?;
    if source.kind == "CLIP" {
        return Err(Fail::bad("Cut a live stream, past broadcast or Highlight."));
    }
    let title = title(&input.title, if input.kind == "CLIP" { 100 } else { 140 })?;
    moderation::check_clip(&app, channel, &user, &title).await?;
    let privileged = moderation::role_of(&app, channel, &user).await?.is_some();
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,81))")
        .bind(channel)
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(80742)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT id FROM videos WHERE id=$1 FOR UPDATE")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    let source = load(&mut tx, &id).await?;
    accessible(&app, &source, Some(&user), input.age_ack).await?;
    if !matches!(source.status.as_str(), "RECORDING" | "READY")
        || held(&mut tx, &id).await?
        || source.expires_at.is_some_and(|t| t <= Utc::now())
    {
        return Err(Fail::missing());
    }
    let prior: Option<String> =
        sqlx::query_scalar("SELECT id FROM videos WHERE clipper_id=$1 AND request_key=$2")
            .bind(&user.id)
            .bind(&input.request_id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(id) = prior {
        return Ok(Json(json!({"id":id,"queued":true})));
    }
    let settings = settings(&mut tx, channel).await?;
    if settings.copyright_restricted {
        return Err(Fail::denied(
            "Recording and clipping are restricted on this channel.",
        ));
    }
    if input.kind == "HIGHLIGHT" {
        if user.id != channel || !source.recording {
            return Err(Fail::denied(
                "Only the streamer can save a recorded Highlight.",
            ));
        }
    } else {
        let allowed = match settings.clip_permission.as_str() {
            "SIGNED_IN" => true,
            "MODS" => privileged,
            "OFF" => false,
            "FOLLOWERS" => privileged || profiles::follows(&mut tx, &user.id, channel).await?,
            "SUBSCRIBERS" => {
                privileged
                    || crate::subs::active_tier(&app, channel, &user.id)
                        .await?
                        .is_some()
            }
            _ => false,
        };
        if !allowed {
            return Err(Fail::denied(
                "Clipping isn't available to you on this channel.",
            ));
        }
        profiles::rate(
            &app,
            format!("video-clip-user:{}", user.id),
            app.config.videos.tuning.clips_per_viewer_hour,
            3600,
        )
        .await?;
        profiles::rate(
            &app,
            format!("video-clip-channel:{channel}"),
            app.config.videos.tuning.clips_per_channel_hour,
            3600,
        )
        .await?;
    }
    let end = input.end_ms.unwrap_or(source.duration_ms);
    let start = input.start_ms.unwrap_or((end - 30000).max(0));
    if start < 0
        || end <= start
        || end > source.duration_ms
        || (input.kind == "CLIP"
            && (!(5000..=60000).contains(&(end - start))
                || (source.status == "RECORDING" && start < source.duration_ms - 120000)))
    {
        return Err(Fail::bad(
            "Clips must be 5–60 seconds within the available video; live clips use the last two minutes.",
        ));
    }
    let segments:Vec<CutSegment>=sqlx::query_as("SELECT s.id,s.start_ms,s.duration_ms FROM video_segments s JOIN video_objects o ON o.key=s.object_key WHERE s.video_id=$1 AND o.ready AND s.start_ms<$3 AND s.start_ms+s.duration_ms>$2 ORDER BY s.start_ms")
        .bind(&id).bind(start).bind(end).fetch_all(&mut *tx).await?;
    if segments
        .last()
        .is_none_or(|segment| segment.start_ms + segment.duration_ms < end)
    {
        return Err(Fail::unavailable(
            "That section is still processing. Try again shortly.",
        ));
    }
    let first = segments
        .first()
        .ok_or_else(|| Fail::bad("Those segments are no longer available."))?
        .start_ms;
    let segments: Vec<_> = segments
        .into_iter()
        .filter(|s| input.kind != "CLIP" || s.start_ms + s.duration_ms - first <= 60000)
        .collect();
    let last = segments
        .last()
        .ok_or_else(|| Fail::bad("Choose a shorter cut."))?;
    let end = last.start_ms + last.duration_ms;
    let duration = end - first;
    if first > start
        || duration <= 0
        || (input.kind == "CLIP" && duration < 5000)
        || segments
            .windows(2)
            .any(|w| w[0].start_ms + w[0].duration_ms != w[1].start_ms)
    {
        return Err(Fail::bad(
            "That section is incomplete. Choose another range.",
        ));
    }
    if input.kind == "HIGHLIGHT" {
        let used:i64=sqlx::query_scalar("SELECT coalesce(sum(duration_ms),0)::bigint FROM videos WHERE owner_id=$1 AND kind='HIGHLIGHT' AND status NOT IN ('DELETED','EXPIRED','DELETING','FAILED')").bind(channel).fetch_one(&mut *tx).await?;
        let tier = tiers::tier_of(&mut tx, channel).await?;
        if used + duration > HIGHLIGHT_HOURS[tier as usize] * 3_600_000 {
            return Err(Fail::bad(
                "Your Highlight storage is full. Delete a Highlight to save another.",
            ));
        }
    }
    let new_id = profiles::new_id();
    sqlx::query("INSERT INTO videos(id,owner_id,clipper_id,broadcast_id,parent_id,kind,status,approval,visibility,mature,title,category_id,category,genre,faction,started_at,ended_at,duration_ms,source_start_ms,source_end_ms,request_key) VALUES($1,$2,$3,$4,$5,$6,'PROCESSING',$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)")
        .bind(&new_id).bind(channel).bind(&user.id).bind(&source.broadcast_id).bind(&id).bind(&input.kind).bind(if input.kind=="CLIP"&&settings.clip_approval&&!privileged{"PENDING"}else{"APPROVED"}).bind(&source.visibility).bind(source.mature).bind(title).bind(&source.category_id).bind(&source.category).bind(&source.genre).bind(&source.faction).bind(source.started_at+chrono::Duration::milliseconds(first)).bind(source.started_at+chrono::Duration::milliseconds(end)).bind(duration).bind(first).bind(end).bind(input.request_id).execute(&mut *tx).await?;
    for (position, segment) in segments.iter().enumerate() {
        sqlx::query(
            "INSERT INTO video_copy_sources(video_id,position,segment_id) VALUES($1,$2,$3)",
        )
        .bind(&new_id)
        .bind(position as i32)
        .bind(&segment.id)
        .execute(&mut *tx)
        .await?;
    }
    copy_replay(
        &mut tx,
        &source.id,
        &new_id,
        channel,
        first,
        end,
        source.kind == "VOD",
    )
    .await?;
    enqueue(
        &mut tx,
        &new_id,
        "ASSEMBLE",
        Some(&format!("{new_id}:assemble")),
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":new_id,"queued":true,"start_ms":first,"end_ms":end}),
    ))
}
#[derive(Deserialize)]
struct Edit {
    title: String,
    visibility: String,
}
async fn edit(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Edit>,
) -> Res<Json<Value>> {
    let user = owner(&app, &jar).await?;
    let video = load(&mut *app.db.acquire().await?, &id).await?;
    if video.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::denied("Only the streamer can edit this recording."));
    }
    visibility(&input.visibility)?;
    let title = title(&input.title, if video.kind == "CLIP" { 100 } else { 140 })?;
    moderation::check_clip(&app, &user.id, &user, &title).await?;
    let mut tx = app.db.begin().await?;
    // Serialize restrictive visibility cascades with cuts, including grandchildren.
    sqlx::query("SELECT pg_advisory_xact_lock(80742)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE videos SET title=$2,visibility=$3,revision=revision+1 WHERE id=$1")
        .bind(&id)
        .bind(title)
        .bind(&input.visibility)
        .execute(&mut *tx)
        .await?;
    if input.visibility != "PUBLIC" {
        sqlx::query("WITH RECURSIVE children AS (SELECT id FROM videos WHERE parent_id=$1 UNION ALL SELECT v.id FROM videos v JOIN children c ON v.parent_id=c.id) UPDATE videos SET visibility=CASE WHEN visibility='PRIVATE' THEN visibility ELSE $2 END,revision=revision+1 WHERE id IN (SELECT id FROM children)").bind(&id).bind(input.visibility).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = owner(&app, &jar).await?;
    let video = load(&mut *app.db.acquire().await?, &id).await?;
    let allowed = video.owner_id.as_deref() == Some(&user.id)
        || (video.kind == "CLIP"
            && (video.clipper_id.as_deref() == Some(&user.id)
                || moderation::role_of(&app, video.owner_id.as_deref().unwrap_or(""), &user)
                    .await?
                    .is_some()));
    if !allowed {
        return Err(Fail::denied("You can't delete this recording."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT id FROM videos WHERE id=$1 FOR UPDATE")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    request_delete(&mut tx, &id, false).await?;
    tx.commit().await?;
    Ok(Json(json!({"queued":true})))
}
#[derive(Deserialize)]
struct Approval {
    approve: bool,
    reason: String,
}
async fn approve(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Approval>,
) -> Res<Json<Value>> {
    let user = owner(&app, &jar).await?;
    let video = load(&mut *app.db.acquire().await?, &id).await?;
    let channel = video.owner_id.as_deref().ok_or_else(Fail::missing)?;
    if video.kind != "CLIP" || moderation::role_of(&app, channel, &user).await?.is_none() {
        return Err(Fail::denied("You can't review this clip."));
    }
    let reason = crate::text::clean(&input.reason, "reason", false)?;
    if reason.is_empty() || reason.len() > 500 {
        return Err(Fail::bad("Give a review reason of up to 500 characters."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE videos SET approval=$2,beacon_approved=$3,revision=revision+1 WHERE id=$1")
        .bind(&id)
        .bind(if input.approve {
            "APPROVED"
        } else {
            "REJECTED"
        })
        .bind(input.approve && user.id == channel)
        .execute(&mut *tx)
        .await?;
    crate::safety::audit(
        &mut tx,
        Some(&user.id),
        if input.approve {
            "approve_clip"
        } else {
            "reject_clip"
        },
        "clip",
        &id,
        &[],
        &reason,
        json!({}),
        false,
    )
    .await?;
    if !input.approve {
        request_delete(&mut tx, &id, false).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .merge(super::delivery::routes())
        .merge(super::manage::routes())
        .merge(super::views::routes())
        .merge(super::sharing::routes())
        .merge(super::copyright::routes())
        .merge(super::review::routes())
        .merge(super::beacons::routes())
        .route("/api/me/videos", get(mine).put(save_settings))
        .route("/api/channels/{name}/videos", get(channel))
        .route("/api/clips/latest", get(latest))
        .route("/api/videos/{id}", get(page).patch(edit).delete(remove))
        .route("/api/videos/{id}/cuts", post(cut))
        .route("/api/videos/{id}/approval", post(approve))
        .route("/api/internal/srs/segment", post(super::recording::hook))
}
