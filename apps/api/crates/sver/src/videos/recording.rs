use super::*;
use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::HeaderMap,
};
use std::{net::SocketAddr, time::Duration};

#[derive(Deserialize)]
pub struct SegmentHook {
    action: String,
    server_id: String,
    client_id: String,
    vhost: String,
    app: String,
    stream: String,
    url: String,
    duration: f64,
    seq_no: i64,
}
fn segment_path(input: &SegmentHook, stream_app: &str) -> Res<String> {
    let prefix = format!("{stream_app}/{}-", input.stream);
    if input.action != "on_hls"
        || input.app != stream_app
        || input.seq_no < 0
        || !input.duration.is_finite()
        || !(0.001..=30.0).contains(&input.duration)
        || input.stream.is_empty()
        || input.stream.len() > 64
        || !input
            .stream
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || !input.url.starts_with(&prefix)
        || !input.url.ends_with(".ts")
    {
        return Err(Fail::bad("Invalid segment callback."));
    }
    let middle = &input.url[prefix.len()..input.url.len() - 3];
    if middle.is_empty()
        || middle.len() > 80
        || !middle.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    {
        return Err(Fail::bad("Invalid segment path."));
    }
    Ok(input.url.clone())
}
/// The internal callback durably spools one bounded segment before acknowledging SRS.
/// Object copies and all FFmpeg work run on leased jobs; user requests never wait on them.
pub async fn hook(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<SegmentHook>,
) -> Res<Json<Value>> {
    crate::streams::authorize_hook(&app, peer, &headers)?;
    if !app.config.videos.storage.available() {
        return Err(Fail::unavailable("Recording storage is unavailable."));
    }
    let streaming = app.config.streaming.as_ref().ok_or_else(Fail::internal)?;
    if input.vhost != streaming.vhost {
        return Err(Fail::denied("Invalid segment callback."));
    }
    let path = segment_path(&input, &streaming.app)?;
    let context = crate::streams::recording_context(
        &mut *app.db.acquire().await?,
        &input.server_id,
        &input.client_id,
        &input.stream,
    )
    .await?
    .ok_or_else(|| Fail::denied("Unknown recording publisher."))?;
    // The 24/7 Plays channel would fill storage: no recording and no clipping buffer.
    if crate::plays::is_channel(&mut *app.db.acquire().await?, &context.owner_id).await? {
        return Ok(Json(json!({"code":0})));
    }
    let source = format!("{}:{}:{}", input.server_id, input.client_id, path);
    let known:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_segments WHERE source_identity=$1 AND video_id IN (SELECT id FROM videos WHERE broadcast_id=$2 AND kind='VOD'))").bind(&source).bind(&context.broadcast_id).fetch_one(&app.db).await?;
    if known {
        return Ok(Json(json!({"code":0})));
    }
    let queued:i64=sqlx::query_scalar("SELECT coalesce(sum(octet_length(payload)),0)::bigint FROM video_jobs WHERE payload IS NOT NULL").fetch_one(&app.db).await?;
    if queued >= app.config.videos.tuning.queued_segment_bytes {
        return Err(Fail::unavailable("Recording storage is catching up."));
    }
    let url = format!("{}/{}", app.config.videos.segment_base, path);
    let mut response = app
        .http
        .get(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map_err(|_| Fail::unavailable("Segment copy will retry."))?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|n| n > MAX_SEGMENT_BYTES as u64)
    {
        return Err(Fail::unavailable("Segment copy will retry."));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Fail::unavailable("Segment copy will retry."))?
    {
        if bytes.len() + chunk.len() > MAX_SEGMENT_BYTES {
            return Err(Fail::bad("Segment exceeds the recording limit."));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty()
        || bytes.len() % 188 != 0
        || bytes.as_chunks::<188>().0.iter().any(|p| p[0] != 0x47)
    {
        return Err(Fail::bad("Invalid transport-stream segment."));
    }
    let duration = (input.duration * 1000.0).round() as i64;
    let wall_start = Utc::now() - chrono::Duration::milliseconds(duration);
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,80))")
        .bind(&context.broadcast_id)
        .execute(&mut *tx)
        .await?;
    let settings = settings(&mut tx, &context.owner_id).await?;
    if settings.copyright_restricted {
        return Ok(Json(json!({"code":0})));
    }
    let tier = tiers::tier_of(&mut tx, &context.owner_id).await?;
    let faction = crate::factions::membership(&mut tx, &context.owner_id).await?;
    let id = profiles::new_id();
    sqlx::query("INSERT INTO videos(id,owner_id,broadcast_id,kind,status,visibility,recording,mature,title,category_id,category,genre,faction,started_at,ended_at,expires_at,retention_hours) VALUES($1,$2,$3,'VOD','RECORDING',$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13+make_interval(hours=>$14),$14) ON CONFLICT(broadcast_id) WHERE kind='VOD' DO NOTHING")
        .bind(&id).bind(&context.owner_id).bind(&context.broadcast_id).bind(&settings.visibility).bind(settings.recording).bind(settings.mature).bind(&context.title).bind(&context.category_id).bind(&context.category).bind(&context.genre).bind(faction).bind(wall_start.max(context.started_at)).bind(context.ended_at).bind(tiers::VOD_HOURS[tier as usize] as i32).execute(&mut *tx).await?;
    let video: Video =
        sqlx::query_as("SELECT * FROM videos WHERE broadcast_id=$1 AND kind='VOD' FOR UPDATE")
            .bind(&context.broadcast_id)
            .fetch_one(&mut *tx)
            .await?;
    if video.status != "RECORDING" {
        return Ok(Json(json!({"code":0})));
    }
    let duplicate: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM video_segments WHERE video_id=$1 AND source_identity=$2)",
    )
    .bind(&video.id)
    .bind(&source)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate {
        return Ok(Json(json!({"code":0})));
    }
    // Serialize the final queue-budget check with enqueueing across all broadcasts.
    sqlx::query("SELECT pg_advisory_xact_lock(80741)")
        .execute(&mut *tx)
        .await?;
    let queued:i64=sqlx::query_scalar("SELECT coalesce(sum(octet_length(payload)),0)::bigint FROM video_jobs WHERE payload IS NOT NULL").fetch_one(&mut *tx).await?;
    if queued + bytes.len() as i64 > app.config.videos.tuning.queued_segment_bytes {
        return Err(Fail::unavailable("Recording storage is catching up."));
    }
    let last:Option<(String,DateTime<Utc>,i64)>=sqlx::query_as("SELECT source_identity,wall_start,duration_ms FROM video_segments WHERE video_id=$1 ORDER BY start_ms DESC LIMIT 1").bind(&video.id).fetch_optional(&mut *tx).await?;
    let discontinuity = last.as_ref().is_some_and(|(last, _, _)| {
        !last.starts_with(&format!("{}:{}:", input.server_id, input.client_id))
    });
    let wall_start = match &last {
        Some((_, at, duration)) if !discontinuity => {
            *at + chrono::Duration::milliseconds(*duration)
        }
        None => video.started_at,
        _ => wall_start,
    };
    let key = format!("{}/{}.ts", video.id, profiles::new_id());
    sqlx::query(
        "INSERT INTO video_objects(key,video_id,content_type,bytes) VALUES($1,$2,'video/mp2t',$3)",
    )
    .bind(&key)
    .bind(&video.id)
    .bind(bytes.len() as i64)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO video_segments(id,video_id,object_key,source_identity,start_ms,duration_ms,wall_start,discontinuity) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(profiles::new_id()).bind(&video.id).bind(&key).bind(source).bind(video.duration_ms).bind(duration).bind(wall_start).bind(discontinuity).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO video_jobs(id,video_id,kind,object_key,payload) VALUES($1,$2,'SEGMENT',$3,$4)",
    )
    .bind(profiles::new_id())
    .bind(&video.id)
    .bind(key)
    .bind(bytes)
    .execute(&mut *tx)
    .await?;
    if video.duration_ms == 0 {
        chapter(
            &mut tx,
            &context.owner_id,
            "CATEGORY",
            Some(&context.broadcast_id),
            context.category.as_deref().unwrap_or("Stream started"),
        )
        .await?;
    }
    sqlx::query("UPDATE videos SET duration_ms=duration_ms+$2 WHERE id=$1")
        .bind(&video.id)
        .bind(duration)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"code":0})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_our_bounded_segment_paths_are_fetchable() {
        let mut input = SegmentHook {
            action: "on_hls".into(),
            server_id: "server".into(),
            client_id: "client".into(),
            vhost: "__defaultVhost__".into(),
            app: "rebuild".into(),
            stream: "abc".into(),
            url: "rebuild/abc-1234-1.ts".into(),
            duration: 1.0,
            seq_no: 1,
        };
        assert!(segment_path(&input, "rebuild").is_ok());
        for url in [
            "https://example.test/rebuild/abc-1.ts",
            "rebuild/abc-../secret.ts",
            "rebuild/other-123-1.ts",
            "rebuild/abc-123-1.ts?key=secret",
        ] {
            input.url = url.into();
            assert!(segment_path(&input, "rebuild").is_err());
        }
        input.url = "rebuild/abc-1234-1.ts".into();
        input.duration = f64::NAN;
        assert!(segment_path(&input, "rebuild").is_err());
    }
}
