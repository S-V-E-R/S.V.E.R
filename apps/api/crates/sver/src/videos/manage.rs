use super::*;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post, put},
};

async fn owned(app: &App, jar: &CookieJar, id: &str) -> Res<(Video, auth::User)> {
    let user = profiles::signed_in(app, jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let video = load(&mut *app.db.acquire().await?, id).await?;
    if video.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::denied("Only the streamer can edit this recording."));
    }
    Ok((video, user))
}
#[derive(Deserialize)]
struct Editor {
    username: String,
    enabled: bool,
}
async fn editor_set(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Editor>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    profiles::ensure_unrestricted(&mut tx, &user.id).await?;
    let target = profiles::eligible_by_name(&mut tx, input.username.trim())
        .await?
        .ok_or_else(Fail::channel_missing)?;
    if target.id == user.id {
        return Err(Fail::bad("You already own your recordings."));
    }
    if input.enabled {
        if profiles::blocked_between(&mut tx, &user.id, &target.id).await? {
            return Err(Fail::denied("This editor is unavailable."));
        }
        sqlx::query(
            "INSERT INTO video_editors(owner_id,editor_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(&user.id)
        .bind(&target.id)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query("DELETE FROM video_editors WHERE owner_id=$1 AND editor_id=$2")
            .bind(&user.id)
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
    }
    crate::safety::audit(
        &mut tx,
        Some(&user.id),
        if input.enabled {
            "appoint_video_editor"
        } else {
            "remove_video_editor"
        },
        "profile",
        &target.id,
        &[],
        "Recording download permission",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
struct Marker {
    label: Option<String>,
}
pub async fn marker(
    db: &mut PgConnection,
    owner: &str,
    label: &str,
    source_id: Option<&str>,
) -> Res<()> {
    let label = crate::text::clean(label, "label", false)?;
    let label = if label.is_empty() { "Marker" } else { &label };
    if label.chars().count() > 100 {
        return Err(Fail::bad("Use a marker label of up to 100 characters."));
    }
    chapter(db, owner, "MARKER", source_id, label).await
}
async fn add_marker(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Marker>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let channel = profiles::eligible_by_name(&mut *app.db.acquire().await?, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    if moderation::role_of(&app, &channel.id, &user)
        .await?
        .is_none()
    {
        return Err(Fail::denied("Only the streamer and mods can add markers."));
    }
    let mut tx = app.db.begin().await?;
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM videos WHERE owner_id=$1 AND kind='VOD' AND status='RECORDING' AND recording AND ended_at IS NULL)").bind(&channel.id).fetch_one(&mut *tx).await?;
    if !active {
        return Err(Fail::bad("There is no live recording to mark."));
    }
    marker(
        &mut tx,
        &channel.id,
        input.label.as_deref().unwrap_or(""),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
struct Chapter {
    id: String,
    label: Option<String>,
    merge_next: Option<bool>,
}
async fn chapter_edit(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Chapter>,
) -> Res<Json<Value>> {
    owned(&app, &jar, &id).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT id FROM videos WHERE id=$1 FOR UPDATE")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    let at: Option<i64> =
        sqlx::query_scalar("SELECT offset_ms FROM video_chapters WHERE video_id=$1 AND id=$2")
            .bind(&id)
            .bind(&input.id)
            .fetch_optional(&mut *tx)
            .await?;
    let at = at.ok_or_else(Fail::missing)?;
    if input.merge_next == Some(true) {
        sqlx::query("DELETE FROM video_chapters WHERE id=(SELECT id FROM video_chapters WHERE video_id=$1 AND (offset_ms,id)>($2,$3) ORDER BY offset_ms,id LIMIT 1)").bind(&id).bind(at).bind(&input.id).execute(&mut *tx).await?;
    }
    if let Some(label) = input.label {
        let label = crate::text::clean(&label, "label", false)?;
        if label.is_empty() || label.chars().count() > 100 {
            return Err(Fail::bad("Use a chapter label of 1–100 characters."));
        }
        sqlx::query("UPDATE video_chapters SET label=$3 WHERE video_id=$1 AND id=$2")
            .bind(&id)
            .bind(&input.id)
            .bind(label)
            .execute(&mut *tx)
            .await?;
    } else if input.merge_next != Some(true) {
        sqlx::query("DELETE FROM video_chapters WHERE video_id=$1 AND id=$2")
            .bind(&id)
            .bind(&input.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
struct Frame {
    offset_ms: i64,
}
async fn thumbnail(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Frame>,
) -> Res<Json<Value>> {
    let (video, _) = owned(&app, &jar, &id).await?;
    if video.kind == "CLIP"
        || input.offset_ms < 0
        || input.offset_ms >= video.duration_ms
        || !matches!(video.status.as_str(), "READY" | "RECORDING")
    {
        return Err(Fail::bad("Choose a frame in the recording."));
    }
    enqueue(
        &mut *app.db.acquire().await?,
        &id,
        "THUMBNAIL",
        Some(&format!("{id}:thumb")),
        json!({"offset_ms":input.offset_ms}),
    )
    .await?;
    Ok(Json(json!({"queued":true})))
}
#[derive(Deserialize, Default)]
struct Age {
    #[serde(default)]
    age_ack: bool,
}
async fn live(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(query): Query<Age>,
) -> Res<Json<Value>> {
    let channel = profiles::eligible_by_name(&mut *app.db.acquire().await?, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let video:Option<Video>=sqlx::query_as("SELECT * FROM videos WHERE owner_id=$1 AND kind='VOD' AND status='RECORDING' AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1").bind(&channel.id).fetch_optional(&app.db).await?;
    let Some(video) = video else {
        return Ok(Json(json!({"recording":null})));
    };
    let user = profiles::viewer(&app, &jar).await?;
    accessible(&app, &video, user.as_ref(), query.age_ack).await?;
    let start:Option<i64>=sqlx::query_scalar("SELECT min(s.start_ms) FROM video_segments s JOIN video_objects o ON o.key=s.object_key WHERE s.video_id=$1 AND o.ready").bind(&video.id).fetch_one(&app.db).await?;
    Ok(Json(
        json!({"recording":{"id":video.id,"enabled":video.recording,"duration_ms":video.duration_ms,"available_from_ms":start}}),
    ))
}
#[derive(Deserialize)]
struct Replay {
    at_ms: i64,
    #[serde(default)]
    age_ack: bool,
}
#[derive(sqlx::FromRow)]
struct ReplayMessage {
    message_id: String,
    author_id: Option<String>,
    offset_ms: i64,
    message: Value,
}
async fn replay(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<Replay>,
) -> Res<Json<Value>> {
    let (video, user) = watched(&app, &jar, &id, query.age_ack).await?;
    let owner = video.owner_id.as_deref().ok_or_else(Fail::missing)?;
    let mut db = app.db.acquire().await?;
    if !settings(&mut db, owner).await?.chat_replay {
        return Ok(Json(json!({"enabled":false,"messages":[]})));
    };
    if query.at_ms < 0 || query.at_ms > video.duration_ms + 5000 {
        return Err(Fail::bad("Choose a time in the video."));
    }
    let start = (query.at_ms - 30000).max(0);
    let rows: Vec<ReplayMessage> = if video.kind == "VOD" {
        let sql = format!(
            "WITH chat AS ({}) SELECT c.id AS message_id,c.author_id,s.start_ms+(extract(epoch FROM c.created_at-s.wall_start)*1000)::bigint AS offset_ms,c.message FROM video_segments s JOIN chat c ON c.channel_id=$2 AND c.created_at>=s.wall_start AND c.created_at<s.wall_start+make_interval(secs=>s.duration_ms::float8/1000) WHERE s.video_id=$1 AND s.start_ms<=$4 AND s.start_ms+s.duration_ms>=$3 ORDER BY c.created_at DESC,c.id DESC LIMIT 200",
            crate::chat::replay_source_sql()
        );
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(&id)
            .bind(owner)
            .bind(start)
            .bind(query.at_ms)
            .fetch_all(&mut *db)
            .await?
    } else {
        sqlx::query_as("SELECT message_id,author_id,offset_ms,message FROM video_chat WHERE video_id=$1 AND offset_ms BETWEEN $2 AND $3 ORDER BY offset_ms DESC,message_id DESC LIMIT 200").bind(&id).bind(start).bind(query.at_ms).fetch_all(&mut *db).await?
    };
    let redacted = crate::chat::replay_redacted(
        &mut db,
        &rows
            .iter()
            .map(|r| r.message_id.clone())
            .collect::<Vec<_>>(),
    )
    .await?;
    let mut messages = Vec::new();
    for mut row in rows.into_iter().rev() {
        if row.offset_ms < start
            || row.offset_ms > query.at_ms
            || redacted.contains(&row.message_id)
        {
            continue;
        }
        let Some(author) = row.author_id else {
            continue;
        };
        let Some(channel) = profiles::channel_user_by_id(&mut db, &author).await? else {
            continue;
        };
        if !channel.eligible {
            continue;
        }
        if let Some(user) = &user
            && profiles::blocked_between(&mut db, &user.id, &author).await?
        {
            continue;
        }
        row.message["author"] = profiles::chip(&app, &channel);
        row.message["offset_ms"] = json!(row.offset_ms);
        messages.push(row.message);
    }
    Ok(Json(json!({"enabled":true,"messages":messages})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/videos/editors", put(editor_set))
        .route("/api/channels/{name}/recording", get(live))
        .route("/api/channels/{name}/marker", post(add_marker))
        .route("/api/videos/{id}/chapters", put(chapter_edit))
        .route("/api/videos/{id}/thumbnail", post(thumbnail))
        .route("/api/videos/{id}/chat", get(replay))
}
