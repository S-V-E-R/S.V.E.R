//! Take It Down frame matching (docs/TAKE_IT_DOWN.md "Identical copies"), owned by the media module.
//!
//! Every sampled video frame and every removed image gets a 64-bit difference hash: the image is
//! shrunk to 9×8 greys and each bit says whether a pixel is brighter than its right neighbour. A
//! re-encoded, resized or recompressed copy lands within a few bits of the original; a crop or an
//! edit does not. A near match is a likely copy, not a proven one, so it files a report and hides
//! the content until staff decide. Nothing is removed automatically.
//!
//! Sources are `video:<id>` (recordings, Highlights and clips), `beacon:<id>` and, for removed
//! media only, `image:<root>`.
use crate::profiles::{Fail, Res};
use image::{DynamicImage, GrayImage, imageops::FilterType};
use sqlx::PgConnection;
use std::{path::Path, process::Stdio};
use tokio::{io::AsyncWriteExt, process::Command};

/// FFmpeg hands over frames at this size; [`dhash`] shrinks them the same way it shrinks images.
const WIDTH: u32 = 72;
const HEIGHT: u32 = 64;
/// Frames kept per file: a 60-second Beacon or clip at one frame a second fits with room to spare.
const MAX_FRAMES: usize = 600;

/// The difference hash of a greyscale picture of any size.
pub fn dhash(gray: &GrayImage) -> i64 {
    let small = image::imageops::resize(gray, 9, 8, FilterType::Triangle);
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            hash = (hash << 1) | u64::from(small.get_pixel(x, y)[0] > small.get_pixel(x + 1, y)[0]);
        }
    }
    hash as i64
}
/// Blank, faded and gradient frames hash to (nearly) all zeros or all ones and would match each
/// other everywhere: a stream's black opening frame is not a copy of anything.
pub fn informative(hash: i64) -> bool {
    (8..=56).contains(&hash.count_ones())
}
/// An image's hash, or None when the picture is too plain to identify anything.
pub fn image_hash(image: &DynamicImage) -> Option<i64> {
    let gray = image
        .resize_exact(WIDTH, HEIGHT, FilterType::Triangle)
        .to_luma8();
    Some(dhash(&gray)).filter(|h| informative(*h))
}
fn parse(raw: &[u8]) -> Vec<i64> {
    let size = (WIDTH * HEIGHT) as usize;
    let mut hashes: Vec<i64> = raw
        .chunks_exact(size)
        .take(MAX_FRAMES)
        .filter_map(|frame| GrayImage::from_raw(WIDTH, HEIGHT, frame.to_vec()))
        .map(|frame| dhash(&frame))
        .filter(|h| informative(*h))
        .collect();
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}
/// Keyframes only: decoding them is cheap, and a live segment or a clip has one at least every
/// couple of seconds. Files are thinned to a frame a second; a recording segment is a few seconds.
fn args(input: &str, format: Option<&str>, per_second: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-skip_frame",
        "nokey",
    ]
    .map(String::from)
    .to_vec();
    if let Some(format) = format {
        args.extend(["-f".into(), format.into()]);
    }
    args.extend(["-i".into(), input.into(), "-map".into(), "0:v:0".into()]);
    let thin = if per_second {
        r"select='isnan(prev_selected_t)+gte(t-prev_selected_t\,1)',"
    } else {
        ""
    };
    args.extend([
        "-vf".into(),
        format!("{thin}scale={WIDTH}:{HEIGHT},format=gray"),
        "-fps_mode".into(),
        "passthrough".into(),
        "-frames:v".into(),
        MAX_FRAMES.to_string(),
        "-f".into(),
        "rawvideo".into(),
        "pipe:1".into(),
    ]);
    args
}
async fn run(args: Vec<String>, input: Option<&[u8]>) -> Res<Vec<i64>> {
    let mut child = Command::new("ffmpeg")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Fail::unavailable("Media processing is unavailable."))?;
    let stdin = child.stdin.take();
    let feed = async {
        // FFmpeg may stop reading once it has its frames; a broken pipe then is fine.
        if let (Some(mut stdin), Some(bytes)) = (stdin, input) {
            let _ = stdin.write_all(bytes).await;
        }
    };
    let (_, output) = tokio::join!(feed, child.wait_with_output());
    let output = output.map_err(|_| Fail::internal())?;
    // Video FFmpeg can't decode has no frames to compare; that is not a reason to retry forever.
    if !output.status.success() {
        eprintln!("frames_event=decode outcome=failed");
    }
    Ok(parse(&output.stdout))
}
/// Hashes of a video held in memory (`mpegts` segments, fragmented `mp4` clips).
pub async fn video_hashes(bytes: &[u8], format: &str) -> Res<Vec<i64>> {
    run(
        args("pipe:0", Some(format), format != "mpegts"),
        Some(bytes),
    )
    .await
}
/// Hashes of a local video file, at most one frame a second.
pub async fn file_hashes(path: &Path) -> Res<Vec<i64>> {
    run(args(&path.to_string_lossy(), None, true), None).await
}
/// Whether a live recording segment is one of the sampled ones: it covers a multiple of `every`.
pub fn sampled(start_ms: i64, duration_ms: i64, every: i64) -> bool {
    let next = (start_ms + every - 1).div_euclid(every) * every;
    next < start_ms + duration_ms
}

/// Stores a source's frames and returns the removed media they match, if any. The caller hides
/// its own content and files the report with [`flag`] in the same transaction.
pub async fn record(
    db: &mut PgConnection,
    source: &str,
    hashes: &[i64],
    distance: i32,
) -> Res<Option<String>> {
    sqlx::query("INSERT INTO frame_sources(source) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(source)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT INTO frame_hashes(source,hash) SELECT $1,unnest($2::bigint[]) ON CONFLICT DO NOTHING")
        .bind(source)
        .bind(hashes)
        .execute(&mut *db)
        .await?;
    Ok(sqlx::query_scalar("SELECT r.source FROM removed_frames r JOIN unnest($1::bigint[]) h(hash) ON bit_count((r.hash # h.hash)::bit(64))<=$2 WHERE r.source<>$3 LIMIT 1")
        .bind(hashes).bind(distance).bind(source).fetch_optional(db).await?)
}
pub async fn indexed(db: &mut PgConnection, source: &str) -> Res<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM frame_sources WHERE source=$1)")
            .bind(source)
            .fetch_one(db)
            .await?,
    )
}
/// A legal removal: the source's frames become removed media that new and existing content is
/// compared with. The scan of existing content runs in [`tick`].
pub async fn removed(db: &mut PgConnection, source: &str) -> Res<()> {
    sqlx::query("INSERT INTO removed_frames(source,hash) SELECT source,hash FROM frame_hashes WHERE source=$1 ON CONFLICT DO NOTHING")
        .bind(source)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn removed_hashes(db: &mut PgConnection, source: &str, hashes: &[i64]) -> Res<()> {
    sqlx::query("INSERT INTO removed_frames(source,hash) SELECT $1,unnest($2::bigint[]) ON CONFLICT DO NOTHING")
        .bind(source)
        .bind(hashes)
        .execute(db)
        .await?;
    Ok(())
}
/// Deleted content leaves the comparison set (its removed frames, if any, stay).
pub async fn forget(db: &mut PgConnection, source: &str) -> Res<()> {
    sqlx::query("DELETE FROM frame_hashes WHERE source=$1")
        .bind(source)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM frame_sources WHERE source=$1")
        .bind(source)
        .execute(db)
        .await?;
    Ok(())
}

/// Files a staff report on content whose frames match removed media and hides it meanwhile:
/// recordings and clips get a report hold, a Beacon leaves every feed, and a recording that is
/// still live also puts the live stream in the queue. Staff remove it or dismiss the report.
pub async fn flag(db: &mut PgConnection, source: &str, removed: &str) -> Res<()> {
    let Some((kind, id)) = source.split_once(':') else {
        return Ok(());
    };
    let request: Option<String> = sqlx::query_scalar("SELECT r.number FROM take_down_requests r WHERE EXISTS(SELECT 1 FROM take_down_media m WHERE m.request_id=r.id AND 'image:'||m.root=$1) OR EXISTS(SELECT 1 FROM take_down_targets t WHERE t.request_id=r.id AND CASE WHEN t.kind='beacon' THEN 'beacon:' ELSE 'video:' END||t.target_id=$1) ORDER BY r.id LIMIT 1")
        .bind(removed).fetch_optional(&mut *db).await?;
    let note = match request {
        Some(number) => format!("Frames closely match media removed under Take It Down request {number}. Compare before deciding."),
        None => "Frames closely match media removed under a Take It Down request. Compare before deciding.".into(),
    };
    let filed = match kind {
        "video" => {
            let Ok(video) = crate::videos::load(db, id).await else {
                return Ok(());
            };
            if !matches!(video.status.as_str(), "RECORDING" | "PROCESSING" | "READY") {
                return Ok(());
            }
            let kind = video.kind.to_ascii_lowercase();
            let report =
                crate::safety::system_report(db, &kind, id, &note, serde_json::json!({})).await?;
            if let Some(report) = &report {
                crate::videos::review::hold(db, id, "REPORT", report, false).await?;
            }
            let live = match video.broadcast_id.as_deref() {
                Some(broadcast) if video.status == "RECORDING" => {
                    crate::safety::system_report(
                        db,
                        "live_stream",
                        broadcast,
                        &note,
                        serde_json::json!({}),
                    )
                    .await?
                }
                _ => None,
            };
            report.is_some() || live.is_some()
        }
        "beacon" => {
            if crate::beacons::review::target(db, id).await?.is_none() {
                return Ok(());
            }
            let previous = crate::beacons::review::hide(db, id).await?;
            crate::safety::system_report(
                db,
                "beacon",
                id,
                &note,
                serde_json::json!({"hidden":previous}),
            )
            .await?
            .is_some()
        }
        _ => false,
    };
    if !filed {
        return Ok(());
    }
    eprintln!("frames_event=match outcome=reported kind={kind}");
    // One alert covers a batch of matches: skip it while an earlier one is still undelivered.
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM staff_push_jobs WHERE delivered_at IS NULL)",
    )
    .fetch_one(&mut *db)
    .await?;
    if pending {
        return Ok(());
    }
    for user in crate::safety::take_down::staff_ids(db).await? {
        crate::staff_push::enqueue(db, &user).await?;
    }
    Ok(())
}

/// Compares newly removed frames with every stored frame and flags the matches.
// ponytail: a sequential XOR scan of frame_hashes per removal batch, fine while removals are rare
// and the table holds a few million rows; past that, index 16-bit bands (multi-index hashing).
pub async fn tick(app: &crate::App) -> Res<()> {
    let distance = app.config.videos.tuning.frame_distance;
    let mut tx = app.db.begin().await?;
    let batch: Vec<(String, i64)> = sqlx::query_as("SELECT source,hash FROM removed_frames WHERE NOT scanned ORDER BY source,hash LIMIT 500 FOR UPDATE SKIP LOCKED")
        .fetch_all(&mut *tx)
        .await?;
    if batch.is_empty() {
        return Ok(());
    }
    let (sources, hashes): (Vec<String>, Vec<i64>) = batch.into_iter().unzip();
    let matches: Vec<(String, String)> = sqlx::query_as("SELECT DISTINCT ON (f.source) f.source,r.source FROM unnest($1::text[],$2::bigint[]) r(source,hash) JOIN frame_hashes f ON bit_count((f.hash # r.hash)::bit(64))<=$3 WHERE f.source<>r.source ORDER BY f.source,r.source")
        .bind(&sources).bind(&hashes).bind(distance).fetch_all(&mut *tx).await?;
    for (source, removed) in matches {
        flag(&mut tx, &source, &removed).await?;
    }
    sqlx::query("UPDATE removed_frames SET scanned=true FROM unnest($1::text[],$2::bigint[]) b(source,hash) WHERE removed_frames.source=b.source AND removed_frames.hash=b.hash")
        .bind(&sources)
        .bind(&hashes)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};
    use std::io::Cursor;

    fn picture(seed: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(640, 360, |x, y| {
            let v = ((x * seed / 7 + y * 3) ^ (x / 40 * 29 + y / 30 * 53 * seed)) % 256;
            image::Rgb([v as u8, (v / 2) as u8, (255 - v) as u8])
        }))
    }
    fn distance(a: i64, b: i64) -> u32 {
        (a ^ b).count_ones()
    }
    #[test]
    fn recompressed_and_resized_copies_stay_close() {
        let original = picture(5);
        let hash = image_hash(&original).expect("a detailed picture is informative");
        let mut jpeg = Vec::new();
        original
            .resize_exact(320, 180, FilterType::Lanczos3)
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();
        let copy = image::load_from_memory(&jpeg).unwrap();
        assert!(distance(hash, image_hash(&copy).unwrap()) <= 4);
        assert!(distance(hash, image_hash(&picture(11)).unwrap()) > 12);
    }
    #[test]
    fn blank_frames_identify_nothing() {
        let black = DynamicImage::ImageRgb8(RgbImage::new(640, 360));
        assert_eq!(image_hash(&black), None);
        let fade = DynamicImage::ImageRgb8(RgbImage::from_fn(640, 360, |x, _| {
            image::Rgb([(x * 255 / 640) as u8; 3])
        }));
        assert_eq!(image_hash(&fade), None);
    }
    #[test]
    fn live_sampling_takes_one_segment_per_interval() {
        let starts: Vec<i64> = (0..10).map(|i| i * 1000).collect();
        let picked: Vec<i64> = starts
            .into_iter()
            .filter(|s| sampled(*s, 1000, 5000))
            .collect();
        assert_eq!(picked, [0, 5000]);
        assert!(sampled(4500, 1000, 5000));
        assert!(!sampled(5001, 1000, 5000));
    }
    #[tokio::test]
    #[ignore = "requires ffmpeg; run explicitly with --ignored"]
    async fn video_frames_match_the_still_they_show() {
        let dir = std::env::temp_dir().join(format!("sver-frames-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let still = dir.join("still.png");
        picture(5).save(&still).unwrap();
        let video = dir.join("video.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-loop",
                "1",
                "-i",
            ])
            .arg(&still)
            .args([
                "-t",
                "3",
                "-r",
                "30",
                "-g",
                "30",
                "-vf",
                "scale=1280:720",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "frag_keyframe+empty_moov",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let frames = file_hashes(&video).await.unwrap();
        let removed = image_hash(&picture(5)).unwrap();
        assert!(!frames.is_empty() && frames.len() <= 3);
        assert!(frames.iter().any(|f| distance(*f, removed) <= 6));
        let bytes = std::fs::read(&video).unwrap();
        assert!(
            video_hashes(b"not a video", "mpegts")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(!video_hashes(&bytes, "mp4").await.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
