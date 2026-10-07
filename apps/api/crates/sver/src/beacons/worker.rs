//! Durable Beacon jobs: one PROCESS job per Beacon (probe, Take It Down check, the one re-encode,
//! watermark, metadata strip, thumbnail) and one DELETE job that removes every stored copy.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(sqlx::FromRow)]
struct Job {
    id: String,
    beacon_id: String,
    kind: String,
    input: Value,
    attempts: i32,
}
/// A permanent failure is shown to the creator, who can retry; anything else retries on its own.
enum Stop {
    Failed(&'static str),
    Retry(Fail),
}
impl From<Fail> for Stop {
    fn from(fail: Fail) -> Self {
        Stop::Retry(fail)
    }
}
impl From<sqlx::Error> for Stop {
    fn from(error: sqlx::Error) -> Self {
        Stop::Retry(error.into())
    }
}
const MAX_ATTEMPTS: i32 = 6;

pub async fn run_one(app: &App) -> Res<bool> {
    let token = profiles::new_id();
    let job:Option<Job>=sqlx::query_as("UPDATE beacon_jobs SET lease_until=now()+interval '90 seconds',lease_token=$1,attempts=attempts+1 WHERE id=(SELECT id FROM beacon_jobs WHERE available_at<=now() AND (lease_until IS NULL OR lease_until<=now()) ORDER BY kind='PROCESS',available_at,id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id,beacon_id,kind,input,attempts")
        .bind(&token).fetch_optional(&app.db).await?;
    let Some(job) = job else { return Ok(false) };
    let result = {
        let work = async {
            match job.kind.as_str() {
                "PROCESS" => process(app, &job, &token).await.map(|_| true),
                "DELETE" => remove(app, &job, &token).await.map_err(Stop::Retry),
                _ => Err(Stop::Retry(Fail::internal())),
            }
        };
        tokio::pin!(work);
        let mut renew = tokio::time::interval(Duration::from_secs(20));
        let deadline = tokio::time::sleep(Duration::from_secs(1800));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                result=&mut work=>break result,
                _=&mut deadline=>break Err(Stop::Retry(Fail::unavailable("Beacon processing will retry."))),
                _=renew.tick()=>{
                    let changed=sqlx::query("UPDATE beacon_jobs SET lease_until=now()+interval '90 seconds' WHERE id=$1 AND lease_token=$2").bind(&job.id).bind(&token).execute(&app.db).await?.rows_affected();
                    if changed!=1 {break Err(Stop::Retry(Fail::unavailable("Beacon lease was replaced.")));}
                }
            }
        }
    };
    match result {
        Ok(true) => {
            sqlx::query("DELETE FROM beacon_jobs WHERE id=$1 AND lease_token=$2")
                .bind(&job.id)
                .bind(&token)
                .execute(&app.db)
                .await?;
        }
        // A bounded delete batch finished; continue promptly.
        Ok(false) => {
            sqlx::query("UPDATE beacon_jobs SET lease_until=NULL,lease_token=NULL,attempts=0,available_at=now() WHERE id=$1 AND lease_token=$2")
                .bind(&job.id).bind(&token).execute(&app.db).await?;
        }
        Err(Stop::Failed(reason)) => fail(app, &job, &token, reason).await?,
        Err(Stop::Retry(error)) => {
            if job.kind == "PROCESS" && job.attempts >= MAX_ATTEMPTS {
                fail(
                    app,
                    &job,
                    &token,
                    "Processing didn't finish. Try again in a few minutes.",
                )
                .await?;
            } else {
                sqlx::query("UPDATE beacon_jobs SET lease_until=NULL,lease_token=NULL,available_at=now()+make_interval(secs=>least(300,5*attempts)) WHERE id=$1 AND lease_token=$2").bind(&job.id).bind(&token).execute(&app.db).await?;
            }
            eprintln!(
                "beacon_event=job outcome=retry kind={} status={}",
                job.kind,
                error.status.as_u16()
            );
        }
    }
    Ok(true)
}
async fn fail(app: &App, job: &Job, token: &str, reason: &str) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let owned = sqlx::query("DELETE FROM beacon_jobs WHERE id=$1 AND lease_token=$2")
        .bind(&job.id)
        .bind(token)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if owned == 1 {
        // A failed Beacon gives its daily slot back; the creator sees why and can retry.
        sqlx::query("UPDATE beacons SET status='FAILED',failure=$2,quota=false,revision=revision+1 WHERE id=$1 AND status='PROCESSING'")
            .bind(&job.beacon_id)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    eprintln!("beacon_event=process outcome=failed");
    Ok(())
}
async fn fence(db: &mut PgConnection, job: &Job, token: &str) -> Res<()> {
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM beacon_jobs WHERE id=$1 AND lease_token=$2 AND lease_until>now())")
        .bind(&job.id).bind(token).fetch_one(db).await?;
    if !valid {
        return Err(Fail::unavailable("Beacon lease was replaced."));
    }
    Ok(())
}

/// The temporary working directory is removed however processing ends.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, PartialEq)]
pub struct Probe {
    pub format: String,
    pub codec: String,
    pub duration_ms: i64,
    pub width: i64,
    pub height: i64,
    pub bytes: u64,
}
/// The server's own reading of a file. Type, length and size never come from the browser.
pub async fn probe(path: &Path) -> Result<Probe, &'static str> {
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "That file isn't a video we can read.")?
    .map_err(|_| "Video processing is unavailable. Try again shortly.")?;
    if !output.status.success() {
        return Err("That file isn't a video we can read.");
    }
    let info: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "That file isn't a video we can read.")?;
    parse_probe(&info)
}
pub fn parse_probe(info: &Value) -> Result<Probe, &'static str> {
    let unreadable = "That file isn't a video we can read.";
    let format = info["format"]["format_name"].as_str().ok_or(unreadable)?;
    let video = info["streams"]
        .as_array()
        .ok_or(unreadable)?
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .ok_or(unreadable)?;
    let codec = video["codec_name"].as_str().ok_or(unreadable)?;
    let mp4 = format.split(',').any(|f| f == "mov" || f == "mp4");
    let webm = format.split(',').any(|f| f == "webm");
    let allowed = if mp4 {
        matches!(codec, "h264" | "hevc" | "av1" | "vp9" | "mpeg4" | "prores")
    } else if webm {
        // Matroska and WebM share a demuxer; only WebM's own codecs pass.
        matches!(codec, "vp8" | "vp9" | "av1")
    } else {
        false
    };
    if !allowed {
        return Err("Use an MP4, MOV or WebM video.");
    }
    let seconds: f64 = info["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .ok_or(unreadable)?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(unreadable);
    }
    let mut width = video["width"].as_i64().ok_or(unreadable)?;
    let mut height = video["height"].as_i64().ok_or(unreadable)?;
    if !(16..=8192).contains(&width) || !(16..=8192).contains(&height) {
        return Err("That video's size isn't supported.");
    }
    // Phones store portrait video as landscape plus a rotation; FFmpeg applies it when decoding.
    let rotation = video["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| d["rotation"].as_i64())
        .or_else(|| {
            video["tags"]["rotate"]
                .as_str()
                .and_then(|r| r.parse().ok())
        })
        .unwrap_or(0);
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }
    Ok(Probe {
        format: if mp4 { "mp4" } else { "webm" }.into(),
        codec: codec.into(),
        duration_ms: (seconds * 1000.0).round() as i64,
        width,
        height,
        bytes: info["format"]["size"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
    })
}

/// A 9:16 crop window in source pixels from the creator's fractions (x, y, width of the frame).
pub fn crop_pixels(crop: &Value, width: i64, height: i64) -> Option<(i64, i64, i64, i64)> {
    let x = crop["x"].as_f64()?;
    let y = crop["y"].as_f64()?;
    let w = crop["width"].as_f64()?;
    if ![x, y, w]
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        || w <= 0.0
    {
        return None;
    }
    let even = |v: f64| ((v / 2.0).floor() as i64 * 2).max(2);
    let mut cw = even(w * width as f64);
    let mut ch = even(cw as f64 * 16.0 / 9.0);
    if ch > height {
        ch = even(height as f64);
        cw = even(ch as f64 * 9.0 / 16.0);
    }
    cw = cw.min(width - width % 2);
    let cx = ((x * width as f64) as i64).clamp(0, width - cw);
    let cy = ((y * height as f64) as i64).clamp(0, height - ch);
    Some((cw, ch, cx, cy))
}

/// The filter graph: frame to 9:16 (crop, or pad without stretching), then a clean branch and a
/// watermarked branch split into the 1080×1920 and 720×1280 public renditions.
pub fn graph(frame: Option<(i64, i64, i64, i64)>, watermark: &str) -> String {
    let fit = match frame {
        Some((w, h, x, y)) => format!("crop={w}:{h}:{x}:{y},scale=1080:1920"),
        None => "scale=1080:1920:force_original_aspect_ratio=decrease,pad=1080:1920:(ow-iw)/2:(oh-ih)/2:color=black".into(),
    };
    format!(
        "[0:v:0]{fit},setsar=1,format=yuv420p,split=2[clean][mark];[mark]{watermark},split=2[hd][wide];[wide]scale=720:1280[sd]"
    )
}
fn encode(args: &mut Vec<String>, label: &str, crf: &str, audio: bool, out: &Path) {
    for value in [
        "-map",
        label,
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        crf,
        "-profile:v",
        "high",
        "-pix_fmt",
        "yuv420p",
        "-fpsmax",
        "60",
        "-g",
        "60",
        // SEI units carry encoder settings; drop them with every other kind of metadata.
        "-bsf:v",
        "filter_units=remove_types=6",
    ] {
        args.push(value.into());
    }
    if audio {
        for value in [
            "-map", "0:a:0", "-c:a", "aac", "-b:a", "128k", "-ac", "2", "-ar", "48000",
        ] {
            args.push(value.into());
        }
    }
    for value in [
        "-t",
        "60",
        "-map_metadata",
        "-1",
        "-map_chapters",
        "-1",
        "-fflags",
        "+bitexact",
        "-flags:v",
        "+bitexact",
        "-flags:a",
        "+bitexact",
        "-movflags",
        "+faststart",
        "-f",
        "mp4",
    ] {
        args.push(value.into());
    }
    args.push(out.to_string_lossy().into_owned());
}
async fn run(args: &[String], seconds: u64) -> Result<(), Stop> {
    let mut child = Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Fail::unavailable("Video processing is unavailable."))?;
    let mut stderr = child.stderr.take().ok_or_else(Fail::internal)?;
    let mut log = Vec::new();
    let finished = tokio::time::timeout(Duration::from_secs(seconds), async {
        use tokio::io::AsyncReadExt;
        let _ = (&mut stderr).take(4096).read_to_end(&mut log).await;
        child.wait().await
    })
    .await;
    match finished {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(_)) => {
            eprintln!(
                "beacon_event=ffmpeg outcome=error detail={:?}",
                String::from_utf8_lossy(&log).lines().next().unwrap_or("")
            );
            Err(Stop::Failed(
                "That video couldn't be processed. Try another file.",
            ))
        }
        _ => Err(Stop::Retry(Fail::unavailable(
            "Video processing will retry.",
        ))),
    }
}
async fn has_audio(path: &Path) -> bool {
    Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
}
fn sha256_file(path: &Path) -> Res<String> {
    let bytes = std::fs::read(path).map_err(|_| Fail::internal())?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
/// Ranged reads keep each storage request bounded, whatever the upload size.
async fn fetch(app: &App, key: &str, path: &Path) -> Result<String, Stop> {
    let storage = &app.config.videos.storage;
    let length = storage
        .head(&app.http, key)
        .await?
        .ok_or(Stop::Failed("The source video is no longer available."))?;
    if length == 0 {
        return Err(Stop::Failed("That file is empty."));
    }
    if length > MAX_UPLOAD_BYTES {
        return Err(Stop::Failed("Videos can be up to 200 MB."));
    }
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|_| Fail::internal())?;
    let mut hash = Sha256::new();
    let mut at = 0;
    while at < length {
        let end = (at + 8 * 1024 * 1024 - 1).min(length - 1);
        let bytes = storage.range(&app.http, key, at, end).await?;
        hash.update(&bytes);
        file.write_all(&bytes).await.map_err(|_| Fail::internal())?;
        at = end + 1;
    }
    file.flush().await.map_err(|_| Fail::internal())?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
async fn store(app: &App, beacon: &str, key: &str, path: &Path, kind: &str) -> Res<u64> {
    sqlx::query("INSERT INTO beacon_objects(key,beacon_id,content_type) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(key).bind(beacon).bind(kind).execute(&app.db).await?;
    let storage = &app.config.videos.storage;
    let bytes = if kind == "video/mp4" {
        let upload = storage.begin_mp4(&app.http, key).await?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| Fail::internal())?;
        match storage
            .finish_mp4(&app.http, key, upload.as_deref(), file)
            .await
        {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(upload) = upload {
                    let _ = storage.abort_mp4(&app.http, key, &upload).await;
                }
                return Err(error);
            }
        }
    } else {
        let bytes = tokio::fs::read(path).await.map_err(|_| Fail::internal())?;
        let length = bytes.len() as u64;
        storage
            .put_typed(&app.http, key, bytes, kind, "private, no-store")
            .await?;
        length
    };
    sqlx::query("UPDATE beacon_objects SET bytes=$2 WHERE key=$1")
        .bind(key)
        .bind(bytes as i64)
        .execute(&app.db)
        .await?;
    Ok(bytes)
}
/// Exact copies of removed media never publish (docs/TAKE_IT_DOWN.md): the source bytes are
/// checked against the blocklist and against media held for a removal request.
async fn blocked(db: &mut PgConnection, hashes: &[String]) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blocked_media_hashes WHERE hash=ANY($1)) OR EXISTS(SELECT 1 FROM media_removal_holds h JOIN media_fingerprints f USING(root) WHERE f.hashes && $1) OR EXISTS(SELECT 1 FROM beacons b WHERE b.hashes && $1 AND b.hidden AND b.status NOT IN ('DELETING','DELETED'))")
        .bind(hashes).fetch_one(db).await?)
}

async fn process(app: &App, job: &Job, token: &str) -> Result<(), Stop> {
    let beacon = load(&mut *app.db.acquire().await?, &job.beacon_id).await?;
    if beacon.status != "PROCESSING" {
        return Ok(());
    }
    let owner = beacon
        .owner_id
        .as_deref()
        .ok_or(Stop::Failed("This channel is unavailable."))?;
    let channel = profiles::channel_user_by_id(&mut *app.db.acquire().await?, owner)
        .await?
        .filter(|c| c.eligible)
        .ok_or(Stop::Failed("This channel is unavailable."))?;
    let source_key = if beacon.source == "CLIP" {
        let clip_id = beacon
            .clip_id
            .as_deref()
            .ok_or(Stop::Failed("The clip is no longer available."))?;
        let mut db = app.db.acquire().await?;
        let clip = crate::videos::load(&mut db, clip_id)
            .await
            .map_err(|_| Stop::Failed("The clip is no longer available."))?;
        if clip.kind != "CLIP"
            || clip.status != "READY"
            || clip.approval != "APPROVED"
            || crate::videos::held(&mut db, clip_id).await?
        {
            return Err(Stop::Failed("The clip is no longer available."));
        }
        clip.mp4_key
            .clone()
            .ok_or(Stop::Failed("The clip is no longer available."))?
    } else {
        beacon
            .upload_key
            .clone()
            .ok_or(Stop::Failed("Upload the video first."))?
    };
    let dir = std::env::temp_dir().join(format!("sver-beacon-{}-{token}", beacon.id));
    std::fs::create_dir_all(&dir).map_err(|_| Fail::internal())?;
    let scratch = Scratch(dir);
    let source = scratch.0.join("source");
    let source_hash = fetch(app, &source_key, &source).await?;
    let probe = probe(&source).await.map_err(Stop::Failed)?;
    if beacon.source == "UPLOAD" && probe.bytes > MAX_UPLOAD_BYTES {
        return Err(Stop::Failed("Videos can be up to 200 MB."));
    }
    // A clip's cut can run a frame long; uploads must already fit.
    let slack = if beacon.source == "CLIP" { 1000 } else { 0 };
    if probe.duration_ms < MIN_MS || probe.duration_ms > MAX_MS + slack {
        return Err(Stop::Failed("Beacons are 5–60 seconds long."));
    }
    if blocked(
        &mut *app.db.acquire().await?,
        std::slice::from_ref(&source_hash),
    )
    .await?
    {
        eprintln!("beacon_event=blocklist outcome=match");
        return Err(Stop::Failed("This video can't be published."));
    }
    let frame = match &beacon.crop {
        Some(crop) => Some(
            crop_pixels(crop, probe.width, probe.height)
                .ok_or(Stop::Failed("Choose a crop inside the video."))?,
        ),
        None => None,
    };
    let duration = probe.duration_ms.min(MAX_MS);
    let tuning = &app.config.beacons.tuning;
    let steps = watermark::plan(
        beacon.seed,
        duration,
        tuning.watermark_min_ms,
        tuning.watermark_max_ms,
    );
    let mark = watermark::filter(&channel.username, &app.config.beacons.font_file, &steps);
    let (hd, sd, clean, thumb) = (
        scratch.0.join("hd.mp4"),
        scratch.0.join("sd.mp4"),
        scratch.0.join("clean.mp4"),
        scratch.0.join("thumb.webp"),
    );
    let audio = has_audio(&source).await;
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(source.to_string_lossy().into_owned());
    args.push("-filter_complex".into());
    args.push(graph(frame, &mark));
    encode(&mut args, "[hd]", "21", audio, &hd);
    encode(&mut args, "[sd]", "23", audio, &sd);
    encode(&mut args, "[clean]", "20", audio, &clean);
    run(&args, 900).await?;
    let at = format!("{:.3}", (duration as f64 / 2000.0).min(1.0));
    let thumb_args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-ss",
        &at,
        "-i",
        &hd.to_string_lossy(),
        "-frames:v",
        "1",
        "-vf",
        "scale=540:-2",
        "-map_metadata",
        "-1",
        "-c:v",
        "libwebp",
        "-f",
        "image2",
        &thumb.to_string_lossy(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    run(&thumb_args, 120).await?;
    let mut hashes = vec![source_hash];
    for path in [&hd, &sd, &clean] {
        hashes.push(sha256_file(path)?);
    }
    let prefix = format!("beacons/{}/{token}", beacon.id);
    let keys = (
        format!("{prefix}-hd.mp4"),
        format!("{prefix}-sd.mp4"),
        format!("{prefix}-clean.mp4"),
        format!("{prefix}-thumb.webp"),
    );
    store(app, &beacon.id, &keys.0, &hd, "video/mp4").await?;
    store(app, &beacon.id, &keys.1, &sd, "video/mp4").await?;
    store(app, &beacon.id, &keys.2, &clean, "video/mp4").await?;
    store(app, &beacon.id, &keys.3, &thumb, "image/webp").await?;
    drop(scratch);
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT id FROM beacons WHERE id=$1 FOR UPDATE")
        .bind(&beacon.id)
        .execute(&mut *tx)
        .await?;
    fence(&mut tx, job, token).await?;
    // Removal holds registered while this job ran are rechecked under the lock.
    if blocked(&mut tx, &hashes).await? {
        tx.rollback().await?;
        return Err(Stop::Failed("This video can't be published."));
    }
    let published = sqlx::query("UPDATE beacons SET status=CASE WHEN publish THEN 'PUBLISHED' ELSE 'READY' END,published_at=CASE WHEN publish THEN now() END,failure=NULL,duration_ms=$2,hashes=$3,mp4_key=$4,mp4_small_key=$5,clean_key=$6,thumbnail_key=$7,revision=revision+1 WHERE id=$1 AND status='PROCESSING'")
        .bind(&beacon.id).bind(duration).bind(&hashes).bind(&keys.0).bind(&keys.1).bind(&keys.2).bind(&keys.3)
        .execute(&mut *tx).await?.rows_affected();
    if published == 1 {
        sqlx::query("UPDATE beacon_objects SET ready=true WHERE key=ANY($1)")
            .bind([&keys.0, &keys.1, &keys.2, &keys.3])
            .execute(&mut *tx)
            .await?;
        // The upload has served its purpose; the clean copy is the creator's original from now on.
        if let Some(upload) = &beacon.upload_key {
            sqlx::query("UPDATE beacons SET upload_key=NULL WHERE id=$1")
                .bind(&beacon.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE beacon_objects SET ready=false WHERE key=$1")
                .bind(upload)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    if published == 1
        && let Some(upload) = &beacon.upload_key
        && app
            .config
            .videos
            .storage
            .delete(&app.http, upload)
            .await
            .is_ok()
    {
        sqlx::query("UPDATE beacon_objects SET deleted_at=now() WHERE key=$1")
            .bind(upload)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}

async fn remove(app: &App, job: &Job, token: &str) -> Res<bool> {
    let mut tx = app.db.begin().await?;
    fence(&mut tx, job, token).await?;
    sqlx::query("SELECT id FROM beacons WHERE id=$1 FOR UPDATE")
        .bind(&job.beacon_id)
        .execute(&mut *tx)
        .await?;
    // A processing job that already holds its lease finishes its writes first.
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM beacon_jobs WHERE beacon_id=$1 AND kind='PROCESS' AND lease_until>now())")
        .bind(&job.beacon_id).fetch_one(&mut *tx).await?;
    if busy {
        return Err(Fail::unavailable("Beacon processing is finishing."));
    }
    sqlx::query("DELETE FROM beacon_jobs WHERE beacon_id=$1 AND kind='PROCESS'")
        .bind(&job.beacon_id)
        .execute(&mut *tx)
        .await?;
    let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM beacon_objects WHERE beacon_id=$1 AND deleted_at IS NULL ORDER BY key LIMIT 100")
        .bind(&job.beacon_id)
        .fetch_all(&mut *tx)
        .await?;
    for key in keys {
        app.config.videos.storage.delete(&app.http, &key).await?;
        sqlx::query("UPDATE beacon_objects SET deleted_at=now(),ready=false WHERE key=$1")
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    let remaining: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM beacon_objects WHERE beacon_id=$1 AND deleted_at IS NULL)",
    )
    .bind(&job.beacon_id)
    .fetch_one(&mut *tx)
    .await?;
    if remaining {
        tx.commit().await?;
        return Ok(false);
    }
    crate::media::removal::purge_urls(
        app,
        &[format!("{}/beacons/{}", app.config.origin, job.beacon_id)],
    )
    .await?;
    if job.input["legal"] == true {
        // A legal removal blocks every exact copy from publishing again, anywhere.
        sqlx::query("INSERT INTO blocked_media_hashes(hash) SELECT unnest(hashes) FROM beacons WHERE id=$1 ON CONFLICT DO NOTHING")
            .bind(&job.beacon_id)
            .execute(&mut *tx)
            .await?;
    }
    for table in ["beacon_likes", "beacon_playback", "beacon_events"] {
        // Only fixed table literals are interpolated; the id is bound.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE beacon_id=$1"
        )))
        .bind(&job.beacon_id)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE beacons SET status='DELETED',hidden=true,mp4_key=NULL,mp4_small_key=NULL,clean_key=NULL,thumbnail_key=NULL,upload_key=NULL,crop=NULL,revision=revision+1 WHERE id=$1")
        .bind(&job.beacon_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Housekeeping: live joins from Live now taps, abandoned uploads and old view leases.
pub async fn maintain(app: &App) -> Res<()> {
    sqlx::query("INSERT INTO beacon_events(beacon_id,viewer_key,kind,owner_id,day,created_at) SELECT e.beacon_id,e.viewer_key,'LIVE_JOIN',e.owner_id,e.day,now() FROM beacon_events e WHERE e.kind='LIVE_TAP' AND e.created_at>now()-interval '30 minutes' AND EXISTS(SELECT 1 FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id WHERE b.owner_id=e.owner_id AND l.viewer_key=e.viewer_key AND l.level IN ('counted','trusted') AND l.created_at<=e.created_at+interval '10 minutes' AND l.expires_at>=e.created_at) ON CONFLICT DO NOTHING")
        .execute(&app.db).await?;
    let abandoned: Vec<String> = sqlx::query_scalar("UPDATE beacons SET quota=false,failure='The upload was not completed.' WHERE status='DRAFT' AND upload_until<now()-interval '1 hour' RETURNING id")
        .fetch_all(&app.db)
        .await?;
    for id in abandoned {
        request_delete(&mut *app.db.acquire().await?, &id, false).await?;
    }
    sqlx::query("DELETE FROM beacon_playback WHERE day<current_date-2")
        .execute(&app.db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crops_stay_9_by_16_and_inside_the_frame() {
        let (w, h, x, y) =
            crop_pixels(&json!({"x":0.5,"y":0.0,"width":0.3164}), 1920, 1080).unwrap();
        assert_eq!((w % 2, h % 2), (0, 0));
        assert!((w as f64 / h as f64 - 9.0 / 16.0).abs() < 0.01);
        assert!(x + w <= 1920 && y + h <= 1080);
        // A crop wider than the frame allows is narrowed to full height.
        let (w, h, x, _) = crop_pixels(&json!({"x":0.9,"y":0.2,"width":1.0}), 1920, 1080).unwrap();
        assert_eq!(h, 1080);
        assert!(x + w <= 1920);
        assert!(crop_pixels(&json!({"x":-1,"y":0,"width":0.3}), 1920, 1080).is_none());
        assert!(crop_pixels(&json!({"x":0,"y":0,"width":0}), 1920, 1080).is_none());
    }
    #[test]
    fn probes_trust_the_container_not_the_name() {
        let ok = json!({"format":{"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"12.5","size":"1000"},"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"side_data_list":[{"rotation":-90}]}]});
        let probe = parse_probe(&ok).unwrap();
        assert_eq!(
            (probe.width, probe.height, probe.duration_ms),
            (1080, 1920, 12500)
        );
        let mkv = json!({"format":{"format_name":"matroska,webm","duration":"9"},"streams":[{"codec_type":"video","codec_name":"h264","width":640,"height":360}]});
        assert!(parse_probe(&mkv).is_err());
        let image = json!({"format":{"format_name":"png_pipe","duration":"1"},"streams":[{"codec_type":"video","codec_name":"png","width":64,"height":64}]});
        assert!(parse_probe(&image).is_err());
        let audio = json!({"format":{"format_name":"mov,mp4","duration":"9"},"streams":[{"codec_type":"audio","codec_name":"aac"}]});
        assert!(parse_probe(&audio).is_err());
    }
}
