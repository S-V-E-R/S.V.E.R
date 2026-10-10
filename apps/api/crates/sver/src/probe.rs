//! Measured OBS settings for Studio (docs/LIVE_STREAMS.md "Metadata, categories and OBS health"),
//! read from the stream's own HLS output without re-encoding anything:
//! - keyframe interval: SRS cuts a segment only at a keyframe once a second has passed
//!   (`hls_fragment 1`, `hls_wait_keyframe on`), so the median segment length is the keyframe
//!   interval whenever it is a second or more;
//! - B-frames: video frames are reordered exactly when a PES timestamp's DTS differs from its PTS.
use crate::{App, profiles::Res};
use serde_json::json;

/// Above this, Studio warns (the OBS baseline is a one-second keyframe interval).
const KEYFRAME_WARNING_SECONDS: f64 = 2.0;
/// Bounded downloads: a playlist and one segment of our own one-second output.
const PLAYLIST_LIMIT: usize = 64 * 1024;
const SEGMENT_LIMIT: usize = 16 * 1024 * 1024;

/// `#EXTINF` durations and segment URIs, in playlist order.
pub fn segments(playlist: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    let mut duration = None;
    for line in playlist.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            duration = rest
                .split(',')
                .next()
                .and_then(|d| d.trim().parse::<f64>().ok());
        } else if !line.is_empty()
            && !line.starts_with('#')
            && let Some(d) = duration.take().filter(|d| d.is_finite() && *d > 0.0)
        {
            out.push((d, line.to_string()));
        }
    }
    out
}
/// The median segment length, rounded to a tenth of a second.
pub fn keyframe_seconds(segments: &[(f64, String)]) -> Option<f64> {
    let mut lengths: Vec<f64> = segments.iter().map(|s| s.0).collect();
    if lengths.is_empty() {
        return None;
    }
    lengths.sort_by(f64::total_cmp);
    Some((lengths[lengths.len() / 2] * 10.0).round() / 10.0)
}

fn timestamp(b: &[u8]) -> u64 {
    ((u64::from(b[0]) >> 1) & 7) << 30
        | u64::from(b[1]) << 22
        | (u64::from(b[2]) >> 1) << 15
        | u64::from(b[3]) << 7
        | u64::from(b[4]) >> 1
}
/// Whether an MPEG-TS segment's H.264 video is reordered (B-frames); None without video PES.
pub fn b_frames(ts: &[u8]) -> Option<bool> {
    let (mut pmt, mut video, mut seen) = (None, None, false);
    for packet in ts.as_chunks::<188>().0 {
        if packet[0] != 0x47 {
            return None;
        }
        let start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let mut i = 4;
        match (packet[3] >> 4) & 3 {
            1 => {}
            3 => i += 1 + usize::from(packet[4]),
            _ => continue,
        }
        if !start || i >= 188 {
            continue;
        }
        let payload = &packet[i..];
        if pid == 0 || Some(pid) == pmt {
            // PSI: skip the pointer field; the table follows.
            let Some(t) = payload.get(1 + usize::from(payload[0])..) else {
                continue;
            };
            if t.len() < 12 {
                continue;
            }
            if pid == 0 {
                pmt = Some((u16::from(t[10] & 0x1f) << 8) | u16::from(t[11]));
                continue;
            }
            let section = (usize::from(t[1] & 0x0f) << 8) | usize::from(t[2]);
            let end = (3 + section).saturating_sub(4).min(t.len());
            let mut j = 12 + ((usize::from(t[10] & 0x0f) << 8) | usize::from(t[11]));
            while j + 5 <= end {
                if t[j] == 0x1b && video.is_none() {
                    video = Some((u16::from(t[j + 1] & 0x1f) << 8) | u16::from(t[j + 2]));
                }
                j += 5 + ((usize::from(t[j + 3] & 0x0f) << 8) | usize::from(t[j + 4]));
            }
        } else if Some(pid) == video && payload.len() >= 19 && payload[..3] == [0, 0, 1] {
            seen = true;
            if payload[7] >> 6 == 3 && timestamp(&payload[9..14]) != timestamp(&payload[14..19]) {
                return Some(true);
            }
        }
    }
    seen.then_some(false)
}

async fn fetch(app: &App, url: &str, limit: usize) -> Option<Vec<u8>> {
    let mut request = app.http.get(url);
    if let Some(secret) = &app.config.playback.origin_secret {
        request = request.header("x-sver-origin", secret);
    }
    let mut response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > limit {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

/// Every 30 seconds per live broadcast, measure its HLS output and merge the result into health.
pub async fn tick(app: &App) -> Res<()> {
    let Some(base) = app.config.playback.hls_url.clone() else {
        return Ok(());
    };
    // ponytail: one playlist and one segment fetch per live stream every 30 seconds; sample
    // fewer streams per pass if hundreds broadcast at once.
    let due: Vec<(String, String)> = sqlx::query_as("SELECT id,public_id FROM broadcasts WHERE state='LIVE' AND coalesce((health->>'probed_at')::timestamptz<now()-interval '30 seconds',true) LIMIT 20")
        .fetch_all(&app.db)
        .await?;
    for (id, public_id) in due {
        let playlist_url = format!("{base}/{public_id}.m3u8");
        let playlist = fetch(app, &playlist_url, PLAYLIST_LIMIT)
            .await
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let list = segments(&playlist);
        let keyframes = keyframe_seconds(&list);
        let mut reordered = None;
        if let Some((_, uri)) = list.last()
            && let Ok(url) = url::Url::parse(&playlist_url).and_then(|p| p.join(uri))
            && let Some(ts) = fetch(app, url.as_str(), SEGMENT_LIMIT).await
        {
            reordered = b_frames(&ts);
            thumbnail(app, &id, ts).await?;
        }
        let measured = json!({"keyframe_seconds":keyframes,"b_frames":reordered,
            "keyframe_warning":keyframes.is_some_and(|k|k>KEYFRAME_WARNING_SECONDS),"probed_at":chrono::Utc::now()});
        sqlx::query("UPDATE broadcasts SET health=health||$2 WHERE id=$1 AND state='LIVE'")
            .bind(&id)
            .bind(measured)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}

/// Decodes one frame of a segment to a 640-pixel-wide WebP. The stream itself is never altered.
pub async fn snapshot(ts: Vec<u8>) -> Option<Vec<u8>> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
        ])
        .args([
            "-vf",
            "scale=640:-2",
            "-c:v",
            "libwebp",
            "-quality",
            "70",
            "-f",
            // The WebP muxer needs seeking to finish its RIFF header; stdout cannot seek.
            "image2pipe",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    // ffmpeg may stop reading after the first frame; a broken pipe then is fine.
    let feed = tokio::spawn(async move {
        let _ = stdin.write_all(&ts).await;
    });
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    let _ = feed.await;
    (output.status.success() && output.stdout.len() > 32 && output.stdout.len() < 2_000_000)
        .then_some(output.stdout)
}
/// About once a minute per live stream: a fresh still for discovery cards. Each still gets a new
/// key (immutable caching) and the previous one is deleted.
async fn thumbnail(app: &App, broadcast: &str, ts: Vec<u8>) -> Res<()> {
    let due: bool = sqlx::query_scalar("SELECT coalesce(thumbnail_at<now()-interval '55 seconds',true) FROM broadcasts WHERE id=$1 AND state='LIVE'")
        .bind(broadcast)
        .fetch_optional(&app.db)
        .await?
        .unwrap_or(false);
    if !due || !app.config.media.storage.available() {
        return Ok(());
    }
    let Some(image) = snapshot(ts).await else {
        return Ok(());
    };
    let key = format!("thumbs/{broadcast}/{}.webp", chrono::Utc::now().timestamp());
    if app
        .config
        .media
        .storage
        .put(&app.http, &key, image)
        .await
        .is_err()
    {
        return Ok(());
    }
    let previous: Option<Option<String>> = sqlx::query_scalar("UPDATE broadcasts b SET thumbnail_key=$2,thumbnail_at=now() FROM (SELECT thumbnail_key FROM broadcasts WHERE id=$1 FOR UPDATE) old WHERE b.id=$1 AND b.state='LIVE' RETURNING old.thumbnail_key")
        .bind(broadcast)
        .bind(&key)
        .fetch_optional(&app.db)
        .await?;
    match previous {
        // The stream ended meanwhile: don't keep a still for it.
        None => {
            let _ = app.config.media.storage.delete(&app.http, &key).await;
        }
        Some(Some(old)) => {
            let _ = app.config.media.storage.delete(&app.http, &old).await;
        }
        Some(None) => {}
    }
    Ok(())
}
/// Ended streams keep no still (also covers a stopped or removed stream).
pub async fn sweep_thumbnails(app: &App) -> Res<()> {
    let keys: Vec<(String, String)> = sqlx::query_as("SELECT id,thumbnail_key FROM broadcasts WHERE state='ENDED' AND thumbnail_key IS NOT NULL LIMIT 50")
        .fetch_all(&app.db)
        .await?;
    for (id, key) in keys {
        if app
            .config
            .media
            .storage
            .delete(&app.http, &key)
            .await
            .is_ok()
        {
            sqlx::query(
                "UPDATE broadcasts SET thumbnail_key=NULL WHERE id=$1 AND thumbnail_key=$2",
            )
            .bind(&id)
            .bind(&key)
            .execute(&app.db)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires ffmpeg with libwebp; run explicitly with --ignored"]
    async fn snapshot_encodes_large_webp_to_pipe() {
        // A detailed frame exceeds FFmpeg's output buffer, exposing muxers that need to seek.
        let mut input = b"P6\n640 576\n255\n".to_vec();
        let mut seed = 1u32;
        for _ in 0..640 * 576 * 3 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            input.push((seed >> 24) as u8);
        }
        let webp = snapshot(input).await.expect("ffmpeg snapshot");
        assert!(webp.len() > 32 * 1024, "must exceed the output buffer");
        assert_eq!(
            u32::from_le_bytes(webp[4..8].try_into().unwrap()) as usize,
            webp.len() - 8,
            "WebP RIFF length must be complete on a non-seekable pipe"
        );
        let decoded = image::load_from_memory(&webp).expect("valid WebP");
        assert_eq!((decoded.width(), decoded.height()), (640, 576));
    }

    fn packet(pid: u16, start: bool, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![
            0x47,
            (if start { 0x40 } else { 0 }) | (pid >> 8) as u8,
            pid as u8,
            0x10,
        ];
        p.extend_from_slice(payload);
        p.resize(188, 0xff);
        p
    }
    fn ts_bytes(value: u64, marker: u8) -> [u8; 5] {
        [
            (marker << 4) | (((value >> 30) as u8 & 7) << 1) | 1,
            (value >> 22) as u8,
            ((value >> 14) as u8 & 0xfe) | 1,
            (value >> 7) as u8,
            ((value << 1) as u8) | 1,
        ]
    }
    /// PAT -> PMT on 0x1000 -> H.264 on 0x100, then one PES per (pts, dts) frame.
    fn segment(frames: &[(u64, Option<u64>)]) -> Vec<u8> {
        let mut out = packet(
            0,
            true,
            &[
                0, 0x00, 0xb0, 13, 0, 1, 0xc1, 0, 0, 0, 1, 0xf0, 0x00, 0, 0, 0, 0,
            ],
        );
        out.extend(packet(
            0x1000,
            true,
            &[
                0, 0x02, 0xb0, 18, 0, 1, 0xc1, 0, 0, 0xe1, 0x00, 0xf0, 0, 0x1b, 0xe1, 0x00, 0xf0,
                0, 0, 0, 0, 0,
            ],
        ));
        for (pts, dts) in frames {
            let mut pes = vec![0, 0, 1, 0xe0, 0, 0, 0x80];
            match dts {
                Some(dts) => {
                    pes.extend([0xc0, 10]);
                    pes.extend(ts_bytes(*pts, 3));
                    pes.extend(ts_bytes(*dts, 1));
                }
                None => {
                    pes.extend([0x80, 5]);
                    pes.extend(ts_bytes(*pts, 2));
                    pes.extend([0; 5]);
                }
            }
            out.extend(packet(0x100, true, &pes));
        }
        out
    }

    #[test]
    fn measures_keyframes_and_b_frames() {
        let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.004,\nrebuild/a-1.ts\n#EXTINF:3.98,\nrebuild/a-2.ts\n#EXTINF:4.0,\nrebuild/a-3.ts\n";
        let list = segments(playlist);
        assert_eq!(list.len(), 3);
        assert_eq!(list[2].1, "rebuild/a-3.ts");
        assert_eq!(keyframe_seconds(&list), Some(4.0));
        assert_eq!(keyframe_seconds(&segments("#EXTM3U\n")), None);
        // Equal or absent DTS: no reordering. A DTS different from its PTS: B-frames.
        assert_eq!(
            b_frames(&segment(&[(90_000, Some(90_000)), (93_000, None)])),
            Some(false)
        );
        assert_eq!(
            b_frames(&segment(&[(90_000, Some(90_000)), (99_000, Some(93_000))])),
            Some(true)
        );
        assert_eq!(
            b_frames(&segment(&[(8_589_934_000, Some(8_589_933_000))])),
            Some(true)
        );
        assert_eq!(b_frames(&segment(&[])), None, "no video");
        assert_eq!(b_frames(&[0u8; 188]), None, "not TS");
    }
}
