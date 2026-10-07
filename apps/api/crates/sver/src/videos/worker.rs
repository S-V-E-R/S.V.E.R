use super::*;
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(sqlx::FromRow)]
struct Job {
    id: String,
    video_id: String,
    kind: String,
    object_key: Option<String>,
    payload: Option<Vec<u8>>,
    input: Value,
    lease_token: String,
}
async fn put(app: &App, key: &str, bytes: Vec<u8>, kind: &str) -> Res<()> {
    app.config
        .videos
        .storage
        .put_typed(&app.http, key, bytes, kind, "private, no-store")
        .await
}
async fn object(app: &App, key: &str) -> Res<Vec<u8>> {
    app.config
        .videos
        .storage
        .get(&app.http, key)
        .await?
        .ok_or_else(|| Fail::unavailable("Recording storage is catching up."))
}
/// Deterministic destination keys and a fenced database publish make retries idempotent.
pub async fn run_one(app: &App, segments: bool) -> Res<bool> {
    let token = profiles::new_id();
    let job:Option<Job>=sqlx::query_as("UPDATE video_jobs SET lease_until=now()+interval '90 seconds',lease_token=$1,attempts=attempts+1 WHERE id=(SELECT id FROM video_jobs WHERE (kind='SEGMENT')=$2 AND available_at<=now() AND (lease_until IS NULL OR lease_until<=now()) ORDER BY available_at,id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id,video_id,kind,object_key,payload,input,lease_token")
        .bind(&token).bind(segments).fetch_optional(&app.db).await?;
    let Some(job) = job else { return Ok(false) };
    let result = {
        let work = process(app, &job);
        tokio::pin!(work);
        let mut renew = tokio::time::interval(Duration::from_secs(20));
        let deadline = tokio::time::sleep(Duration::from_secs(3600));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                result=&mut work=>break result,
                _=&mut deadline=>break Err(Fail::unavailable("Media work will retry.")),
                _=renew.tick()=>{
                    let changed=sqlx::query("UPDATE video_jobs SET lease_until=now()+interval '90 seconds' WHERE id=$1 AND lease_token=$2").bind(&job.id).bind(&token).execute(&app.db).await?.rows_affected();
                    if changed!=1 {break Err(Fail::unavailable("Media lease was replaced."));}
                }
            }
        }
    };
    if matches!(result, Ok(true)) {
        sqlx::query("DELETE FROM video_jobs WHERE id=$1 AND lease_token=$2")
            .bind(&job.id)
            .bind(&token)
            .execute(&app.db)
            .await?;
    } else if matches!(result, Ok(false)) {
        sqlx::query("UPDATE video_jobs SET lease_until=NULL,lease_token=NULL,attempts=0,available_at=now() WHERE id=$1 AND lease_token=$2")
            .bind(&job.id).bind(&token).execute(&app.db).await?;
    } else {
        sqlx::query("UPDATE video_jobs SET lease_until=NULL,lease_token=NULL,available_at=now()+make_interval(secs=>least(300,5*attempts)) WHERE id=$1 AND lease_token=$2").bind(&job.id).bind(&token).execute(&app.db).await?;
        eprintln!("video_event=job outcome=retry kind={}", job.kind);
    }
    result?;
    Ok(true)
}
async fn fence(db: &mut PgConnection, job: &Job) -> Res<()> {
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_jobs WHERE id=$1 AND lease_token=$2 AND lease_until>now())")
        .bind(&job.id).bind(&job.lease_token).fetch_one(db).await?;
    if !valid {
        return Err(Fail::unavailable("Media lease was replaced."));
    }
    Ok(())
}
async fn process(app: &App, job: &Job) -> Res<bool> {
    match job.kind.as_str() {
        "SEGMENT" => {
            let key = job.object_key.as_deref().ok_or_else(Fail::internal)?;
            let bytes = job.payload.as_ref().ok_or_else(Fail::internal)?;
            put(app, key, bytes.clone(), "video/mp2t").await?;
            let mut tx = app.db.begin().await?;
            fence(&mut tx, job).await?;
            sqlx::query("UPDATE video_objects SET ready=true WHERE key=$1")
                .bind(key)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(true)
        }
        "THUMBNAIL" => thumbnail(app, job).await.map(|_| true),
        "ASSEMBLE" => assemble(app, job).await.map(|_| true),
        "DOWNLOAD" => mp4(app, job, true).await.map(|_| true),
        "DELETE" => remove(app, job).await,
        _ => Err(Fail::internal()),
    }
}
#[derive(sqlx::FromRow)]
struct Source {
    id: String,
    object_key: String,
    duration_ms: i64,
    wall_start: DateTime<Utc>,
    discontinuity: bool,
}
async fn sources(app: &App, id: &str, cut: bool) -> Res<Vec<Source>> {
    let sql = if cut {
        "SELECT s.id,s.object_key,s.duration_ms,s.wall_start,s.discontinuity FROM video_copy_sources c JOIN video_segments s ON s.id=c.segment_id JOIN video_objects o ON o.key=s.object_key WHERE c.video_id=$1 AND o.ready ORDER BY c.position"
    } else {
        "SELECT s.id,s.object_key,s.duration_ms,s.wall_start,s.discontinuity FROM video_segments s JOIN video_objects o ON o.key=s.object_key WHERE s.video_id=$1 AND o.ready ORDER BY s.start_ms"
    };
    Ok(sqlx::query_as(sql).bind(id).fetch_all(&app.db).await?)
}
async fn assemble(app: &App, job: &Job) -> Res<()> {
    let video = load(&mut *app.db.acquire().await?, &job.video_id).await?;
    if video.status != "PROCESSING" {
        return Ok(());
    }
    if video.kind == "CLIP" {
        return mp4(app, job, false).await;
    }
    let sources = sources(app, &video.id, true).await?;
    if sources.is_empty() {
        return Err(Fail::unavailable("Source segments are not ready."));
    }
    let mut offset = 0;
    for source in &sources {
        let key = format!("{}/{}.ts", video.id, source.id);
        let bytes = object(app, &source.object_key).await?;
        sqlx::query("INSERT INTO video_objects(key,video_id,content_type,bytes) VALUES($1,$2,'video/mp2t',$3) ON CONFLICT DO NOTHING").bind(&key).bind(&video.id).bind(bytes.len() as i64).execute(&app.db).await?;
        put(app, &key, bytes, "video/mp2t").await?;
        let mut tx = app.db.begin().await?;
        fence(&mut tx, job).await?;
        sqlx::query("INSERT INTO video_segments(id,video_id,object_key,source_identity,start_ms,duration_ms,wall_start,discontinuity) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(video_id,source_identity) DO NOTHING")
            .bind(profiles::new_id()).bind(&video.id).bind(&key).bind(&source.id).bind(offset).bind(source.duration_ms).bind(source.wall_start).bind(offset>0&&source.discontinuity).execute(&mut *tx).await?;
        sqlx::query("UPDATE video_objects SET ready=true WHERE key=$1")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        offset += source.duration_ms;
    }
    finish_cut(app, job, None).await
}
async fn finish_cut(app: &App, job: &Job, artifact: Option<&str>) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let status: String = sqlx::query_scalar("SELECT status FROM videos WHERE id=$1 FOR UPDATE")
        .bind(&job.video_id)
        .fetch_one(&mut *tx)
        .await?;
    fence(&mut tx, job).await?;
    if status != "PROCESSING" {
        return Ok(());
    }
    sqlx::query("UPDATE videos SET status='READY',mp4_key=$2 WHERE id=$1")
        .bind(&job.video_id)
        .bind(artifact)
        .execute(&mut *tx)
        .await?;
    if let Some(artifact) = artifact {
        sqlx::query(
            "UPDATE video_objects SET delete_after=NULL,ready=true WHERE video_id=$1 AND key=$2",
        )
        .bind(&job.video_id)
        .bind(artifact)
        .execute(&mut *tx)
        .await?;
    }
    // Thumbnail is generated while its original segments are still pinned.
    enqueue(
        &mut tx,
        &job.video_id,
        "THUMBNAIL",
        Some(&format!("{}:thumb", job.video_id)),
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
/// FFmpeg copies the codecs into one MP4, streamed directly into native S3 multipart upload.
async fn mp4(app: &App, job: &Job, download: bool) -> Res<()> {
    let video = load(&mut *app.db.acquire().await?, &job.video_id).await?;
    if !matches!(video.status.as_str(), "READY" | "PROCESSING") {
        return Ok(());
    }
    let source = sources(app, &job.video_id, !download).await?;
    if source.is_empty() {
        return Err(Fail::unavailable("Source segments are not ready."));
    }
    let key = format!("{}/{}.mp4", job.video_id, job.lease_token);
    let storage = &app.config.videos.storage;
    if let (Some(key), Some(upload)) = (
        job.input["upload_key"].as_str(),
        job.input["upload_id"].as_str(),
    ) {
        storage.abort_mp4(&app.http, key, upload).await?;
    }
    sqlx::query("INSERT INTO video_objects(key,video_id,content_type,delete_after) VALUES($1,$2,'video/mp4',now()+interval '1 day') ON CONFLICT DO NOTHING").bind(&key).bind(&job.video_id).execute(&app.db).await?;
    let upload = storage.begin_mp4(&app.http, &key).await?;
    sqlx::query("UPDATE video_jobs SET input=input || $3 WHERE id=$1 AND lease_token=$2")
        .bind(&job.id)
        .bind(&job.lease_token)
        .bind(json!({"upload_key":key,"upload_id":upload}))
        .execute(&app.db)
        .await?;
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-fflags",
            "+genpts",
            "-dts_delta_threshold",
            "1",
            "-f",
            "mpegts",
            "-i",
            "pipe:0",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-c",
            "copy",
            "-bsf:a",
            "aac_adtstoasc",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-f",
            "mp4",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Fail::unavailable("Media processing is unavailable."))?;
    let mut input = child.stdin.take().ok_or_else(Fail::internal)?;
    let output = child.stdout.take().ok_or_else(Fail::internal)?;
    let feed = async {
        for source in source {
            input
                .write_all(&object(app, &source.object_key).await?)
                .await
                .map_err(|_| Fail::unavailable("Media assembly will retry."))?;
        }
        input.shutdown().await.map_err(|_| Fail::internal())?;
        drop(input);
        Res::Ok(())
    };
    let (_, bytes) = tokio::try_join!(
        feed,
        storage.finish_mp4(&app.http, &key, upload.as_deref(), output)
    )?;
    if !child.wait().await.map_err(|_| Fail::internal())?.success() {
        return Err(Fail::unavailable("Media assembly will retry."));
    }
    let mut tx = app.db.begin().await?;
    fence(&mut tx, job).await?;
    sqlx::query("UPDATE video_objects SET ready=true,bytes=$2 WHERE key=$1")
        .bind(&key)
        .bind(bytes as i64)
        .execute(&mut *tx)
        .await?;
    if download {
        sqlx::query("UPDATE videos SET download_key=$2,download_until=now()+interval '1 day' WHERE id=$1 AND status='READY'").bind(&job.video_id).bind(&key).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    if download {
        Ok(())
    } else {
        finish_cut(app, job, Some(&key)).await
    }
}
async fn thumbnail(app: &App, job: &Job) -> Res<()> {
    let video = load(&mut *app.db.acquire().await?, &job.video_id).await?;
    if !matches!(video.status.as_str(), "READY" | "RECORDING") {
        return Ok(());
    }
    let source = sources(app, &video.id, video.kind == "CLIP").await?;
    let wanted = job.input["offset_ms"].as_i64().unwrap_or(0).max(0);
    let mut end = 0;
    let source = source
        .iter()
        .find(|s| {
            end += s.duration_ms;
            end > wanted
        })
        .or_else(|| source.last())
        .ok_or_else(|| Fail::unavailable("Thumbnail source is not ready."))?;
    let bytes = object(app, &source.object_key).await?;
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-skip_frame",
            "nokey",
            "-f",
            "mpegts",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-vf",
            "scale=640:-2",
            "-c:v",
            "libwebp",
            "-f",
            "image2pipe",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Fail::unavailable("Thumbnail processing is unavailable."))?;
    let mut stdin = child.stdin.take().ok_or_else(Fail::internal)?;
    let feed = async {
        let _ = stdin.write_all(&bytes).await;
        drop(stdin);
    };
    let (_, output) = tokio::join!(feed, child.wait_with_output());
    let output = output.map_err(|_| Fail::internal())?;
    if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > 1024 * 1024 {
        return Err(Fail::unavailable("Thumbnail will retry."));
    }
    let key = format!("{}/{}.webp", video.id, job.lease_token);
    sqlx::query("INSERT INTO video_objects(key,video_id,content_type,bytes,delete_after) VALUES($1,$2,'image/webp',$3,now()+interval '1 day') ON CONFLICT DO NOTHING").bind(&key).bind(&video.id).bind(output.stdout.len() as i64).execute(&app.db).await?;
    put(app, &key, output.stdout, "image/webp").await?;
    let mut tx = app.db.begin().await?;
    fence(&mut tx, job).await?;
    sqlx::query("UPDATE video_objects SET delete_after=now() WHERE key=(SELECT thumbnail_key FROM videos WHERE id=$1)").bind(&video.id).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE videos SET thumbnail_key=$2 WHERE id=$1 AND status IN ('READY','RECORDING')",
    )
    .bind(&video.id)
    .bind(&key)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE video_objects SET ready=true,delete_after=NULL WHERE key=$1")
        .bind(key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM video_copy_sources WHERE video_id=$1 AND EXISTS(SELECT 1 FROM videos WHERE id=$1 AND status='READY')").bind(&video.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
async fn remove(app: &App, job: &Job) -> Res<bool> {
    let mut tx = app.db.begin().await?;
    fence(&mut tx, job).await?;
    sqlx::query("SELECT id FROM videos WHERE id=$1 FOR UPDATE")
        .bind(&job.video_id)
        .execute(&mut *tx)
        .await?;
    let legal:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_holds WHERE video_id=$1 AND kind='TAKE_DOWN' AND permanent)").bind(&job.video_id).fetch_one(&mut *tx).await?;
    if held(&mut tx, &job.video_id).await? && !legal {
        return Err(Fail::unavailable("Recording is held for review."));
    }
    let pinned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_copy_sources c JOIN video_segments s ON s.id=c.segment_id WHERE s.video_id=$1 AND c.video_id<>$1) OR EXISTS(SELECT 1 FROM video_jobs WHERE video_id=$1 AND id<>$2 AND lease_until>now())").bind(&job.video_id).bind(&job.id).fetch_one(&mut *tx).await?;
    if pinned {
        return Err(Fail::unavailable("A saved cut is finishing."));
    }
    let uploads: Vec<Value> = sqlx::query_scalar(
        "SELECT input FROM video_jobs WHERE video_id=$1 AND input ? 'upload_id'",
    )
    .bind(&job.video_id)
    .fetch_all(&mut *tx)
    .await?;
    for upload in uploads {
        if let (Some(key), Some(id)) = (upload["upload_key"].as_str(), upload["upload_id"].as_str())
        {
            app.config
                .videos
                .storage
                .abort_mp4(&app.http, key, id)
                .await?;
        }
    }
    // Checkpoint bounded batches so a long VOD or a storage outage cannot restart
    // hundreds of thousands of already completed deletes on every retry.
    let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM video_objects WHERE video_id=$1 AND deleted_at IS NULL ORDER BY key LIMIT 100")
        .bind(&job.video_id)
        .fetch_all(&mut *tx)
        .await?;
    for key in keys {
        app.config.videos.storage.delete(&app.http, &key).await?;
        sqlx::query("UPDATE video_objects SET deleted_at=now(),ready=false,delete_after=now()+interval '1 hour' WHERE key=$1")
            .bind(key).execute(&mut *tx).await?;
    }
    let remaining: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM video_objects WHERE video_id=$1 AND deleted_at IS NULL)",
    )
    .bind(&job.video_id)
    .fetch_one(&mut *tx)
    .await?;
    if remaining {
        tx.commit().await?;
        return Ok(false);
    }
    crate::media::removal::purge_urls(
        app,
        &[
            format!("{}/videos/{}", app.config.origin, job.video_id),
            format!("{}/clips/{}", app.config.origin, job.video_id),
        ],
    )
    .await?;
    sqlx::query("DELETE FROM video_jobs WHERE video_id=$1 AND id<>$2")
        .bind(&job.video_id)
        .bind(&job.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM video_copy_sources WHERE video_id=$1")
        .bind(&job.video_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM video_segments WHERE video_id=$1")
        .bind(&job.video_id)
        .execute(&mut *tx)
        .await?;
    // Retain ownership tombstones briefly, then delete each key again. This also
    // collects a delayed response from a canceled or expired storage writer.
    sqlx::query("DELETE FROM video_chat WHERE video_id=$1")
        .bind(&job.video_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE videos SET status=$2,thumbnail_key=NULL,mp4_key=NULL,download_key=NULL WHERE id=$1",
    )
    .bind(&job.video_id)
    .bind(if job.input["expiry"] == true {
        "EXPIRED"
    } else {
        "DELETED"
    })
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}
pub async fn maintain(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let ready:Vec<String>=sqlx::query_scalar("UPDATE videos v SET status='READY' WHERE kind='VOD' AND status='RECORDING' AND ended_at<now()-interval '10 seconds' AND NOT EXISTS(SELECT 1 FROM video_jobs j WHERE j.video_id=v.id AND kind='SEGMENT') RETURNING id").fetch_all(&mut *tx).await?;
    for id in ready {
        enqueue(
            &mut tx,
            &id,
            "THUMBNAIL",
            Some(&format!("{id}:thumb")),
            json!({}),
        )
        .await?;
    }
    let due:Vec<String>=sqlx::query_scalar("SELECT id FROM videos v WHERE status='READY' AND (expires_at<=now() OR (kind='VOD' AND NOT recording)) AND NOT EXISTS(SELECT 1 FROM video_holds WHERE video_id=v.id) ORDER BY expires_at LIMIT 100 FOR UPDATE SKIP LOCKED").fetch_all(&mut *tx).await?;
    for id in due {
        request_delete(&mut tx, &id, true).await?;
    }
    // A recording-off broadcast owns only its rolling clipping buffer; pending cuts pin sources.
    sqlx::query("UPDATE video_objects o SET delete_after=now() FROM video_segments s JOIN videos v ON v.id=s.video_id WHERE o.key=s.object_key AND v.kind='VOD' AND NOT v.recording AND s.start_ms+s.duration_ms<v.duration_ms-120000 AND NOT EXISTS(SELECT 1 FROM video_copy_sources WHERE segment_id=s.id) AND NOT EXISTS(SELECT 1 FROM video_holds WHERE video_id=v.id)").execute(&mut *tx).await?;
    tx.commit().await?;
    // Filter before LIMIT so held media or stalled uploads cannot starve unrelated cleanup. A
    // segment upload pins only its own object; cuts and thumbnails, which read the recording, pin it all.
    // (Any lease used to pin the whole video, so a live recording-off broadcast never trimmed.)
    // Recheck under the video lock below, since a hold or a new cut can arrive meanwhile.
    let keys:Vec<(String,String)>=sqlx::query_as("SELECT o.key,o.video_id FROM video_objects o JOIN videos v ON v.id=o.video_id WHERE o.delete_after<=now() AND (v.status IN ('DELETED','EXPIRED') OR NOT EXISTS(SELECT 1 FROM video_holds WHERE video_id=o.video_id)) AND NOT EXISTS(SELECT 1 FROM video_copy_sources c JOIN video_segments s ON s.id=c.segment_id WHERE s.object_key=o.key) AND NOT EXISTS(SELECT 1 FROM video_jobs j WHERE j.video_id=o.video_id AND ((kind='SEGMENT' AND object_key=o.key) OR (kind<>'SEGMENT' AND lease_until>now()))) ORDER BY o.delete_after LIMIT 100").fetch_all(&app.db).await?;
    for (key, video) in keys {
        let mut tx = app.db.begin().await?;
        // Cut creation and review holds take the same row lock before pinning a source.
        sqlx::query("SELECT id FROM videos WHERE id=$1 FOR UPDATE")
            .bind(&video)
            .execute(&mut *tx)
            .await?;
        let safe:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_objects o WHERE o.key=$1 AND delete_after<=now() AND (EXISTS(SELECT 1 FROM videos v WHERE v.id=o.video_id AND v.status IN ('DELETED','EXPIRED')) OR NOT EXISTS(SELECT 1 FROM video_holds WHERE video_id=o.video_id)) AND NOT EXISTS(SELECT 1 FROM video_copy_sources c JOIN video_segments s ON s.id=c.segment_id WHERE s.object_key=o.key) AND NOT EXISTS(SELECT 1 FROM video_jobs j WHERE j.video_id=o.video_id AND ((kind='SEGMENT' AND object_key=o.key) OR (kind<>'SEGMENT' AND lease_until>now()))))").bind(&key).fetch_one(&mut *tx).await?;
        if !safe {
            continue;
        }
        app.config.videos.storage.delete(&app.http, &key).await?;
        sqlx::query("DELETE FROM video_segments WHERE object_key=$1")
            .bind(&key)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM video_objects WHERE key=$1")
            .bind(&key)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    Ok(())
}
