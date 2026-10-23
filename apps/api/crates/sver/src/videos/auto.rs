//! Automatic clip suggestions (docs/VODS_CLIPS.md "Automatic clip suggestions"). Two kinds of
//! moment, game-agnostic so they work for consoles on capture cards too:
//! - room moments: MAGNet's own signals (chat or follows well above the stream's normal), checked
//!   for every live stream rather than only when a lane features it;
//! - loud moments: a recorded segment's audio well above the stream's last 3 minutes.
//!
//! Each becomes a clip of the last 35 seconds that waits for the streamer's approval, through the
//! same path as MAGNet spotlights. Streamers switch it off in Studio → Videos & clips.
use super::spotlight::{live_source, suggest};
use super::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

const LENGTH_MS: i64 = 35_000;
/// Segments in a loudness baseline: about 3 minutes of 2-second segments.
const BASELINE: usize = 90;
/// Per recording: when its last segment was measured, and its recent levels (dBFS).
type Levels = HashMap<String, (Instant, VecDeque<f32>)>;

/// Every 10 seconds with the media pass: room moments for every live stream.
pub async fn tick(app: &App) -> Res<()> {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap();
        if last.is_some_and(|at| at.elapsed() < Duration::from_secs(10)) {
            return Ok(());
        }
        *last = Some(Instant::now());
    }
    let t = &app.config.magnet;
    let found = crate::magnet::candidates(&mut *app.db.acquire().await?, "global", t).await?;
    for (_, candidate, signals) in found {
        // A raid or the streamer's own flag isn't something that happened in the last 35 seconds.
        let signals = crate::magnet::Signals {
            raided_by: None,
            flagged: false,
            ..signals
        };
        let (score, reason) = signals.score(t);
        let Some(reason) = reason.filter(|_| score >= t.moment_ratio) else {
            continue;
        };
        for broadcast in &candidate.members {
            moment(app, broadcast, "room", &reason).await?;
        }
    }
    Ok(())
}

/// Called with each recorded segment (worker SEGMENT job). Never holds up the recording: a decode
/// problem only skips the check.
pub async fn segment(app: &App, video: &str, bytes: &[u8]) -> Res<()> {
    let row: Option<(String, Option<String>, bool)> = sqlx::query_as("SELECT v.broadcast_id,v.owner_id,coalesce(s.auto_clips,true) FROM videos v LEFT JOIN video_settings s ON s.owner_id=v.owner_id WHERE v.id=$1 AND v.kind='VOD' AND v.status='RECORDING'")
        .bind(video).fetch_optional(&app.db).await?;
    let Some((broadcast, Some(_), true)) = row else {
        return Ok(());
    };
    let Some(level) = loudness(bytes).await else {
        return Ok(());
    };
    let t = &app.config.videos.tuning;
    let spike = {
        static HISTORY: LazyLock<Mutex<Levels>> = LazyLock::new(Default::default);
        let mut history = HISTORY.lock().unwrap();
        // ponytail: baselines live in this process; after a restart a stream relearns for 3 minutes.
        history.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(600));
        let (at, levels) = history
            .entry(video.to_string())
            .or_insert_with(|| (Instant::now(), VecDeque::new()));
        *at = Instant::now();
        loud(levels, level, t.auto_loud_db, t.auto_loud_floor_db)
    };
    if spike {
        moment(app, &broadcast, "loud", "Loud moment").await?;
    }
    Ok(())
}

/// Whether `level` (dBFS) is a spike against the levels before it, then remembers it.
pub fn loud(levels: &mut VecDeque<f32>, level: f32, above: f32, floor: f32) -> bool {
    let spike = levels.len() >= BASELINE && level >= floor && level - median(levels) >= above;
    levels.push_back(level);
    if levels.len() > BASELINE {
        levels.pop_front();
    }
    spike
}
fn median(levels: &VecDeque<f32>) -> f32 {
    let mut sorted: Vec<f32> = levels.iter().copied().collect();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}

/// The loudest 400 ms of a segment's audio, as RMS in dBFS (8 kHz mono is plenty for loudness).
async fn loudness(bytes: &[u8]) -> Option<f32> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "mpegts",
            "-i",
            "pipe:0",
            "-map",
            "0:a:0",
            "-ac",
            "1",
            "-ar",
            "8000",
            "-f",
            "s16le",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input = bytes.to_vec();
    let feed = tokio::spawn(async move {
        let _ = stdin.write_all(&input).await;
    });
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    let _ = feed.await;
    output
        .status
        .success()
        .then(|| loudest(&output.stdout))
        .flatten()
}
/// Loudest 400 ms window (3200 samples at 8 kHz) of signed 16-bit PCM, in dBFS.
pub fn loudest(pcm: &[u8]) -> Option<f32> {
    let samples: Vec<f32> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
        .collect();
    samples
        .chunks(3200)
        .filter(|w| w.len() >= 800)
        .map(|w| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt())
        .map(|rms| 20.0 * rms.max(1e-5).log10())
        .max_by(f32::total_cmp)
}

/// Suggests a clip of the last 35 seconds for one moment, within the streamer's setting and the
/// hourly cap. Each moment is its own short transaction.
pub async fn moment(app: &App, broadcast: &str, kind: &str, title: &str) -> Res<()> {
    let t = &app.config.videos.tuning;
    let mut tx = app.db.begin().await?;
    let owner: Option<String> = sqlx::query_scalar("SELECT b.owner_id FROM broadcasts b LEFT JOIN video_settings s ON s.owner_id=b.owner_id WHERE b.id=$1 AND coalesce(s.auto_clips,true)")
        .bind(broadcast).fetch_optional(&mut *tx).await?;
    let Some(owner) = owner else {
        return Ok(());
    };
    // Only a recording still in progress: live_source skips ended ones.
    let Some(source) = live_source(&mut tx, &owner).await? else {
        return Ok(());
    };
    let recent: i64 = sqlx::query_scalar("SELECT count(*) FROM videos WHERE parent_id=$1 AND kind='CLIP' AND request_key LIKE 'auto:%' AND created_at>now()-interval '1 hour'")
        .bind(&source.id).fetch_one(&mut *tx).await?;
    if recent >= t.auto_clips_per_hour {
        return Ok(());
    }
    let key = format!("auto:{kind}:{}", source.duration_ms);
    if suggest(
        &mut tx,
        &owner,
        &source,
        &key,
        title,
        LENGTH_MS,
        t.auto_gap_seconds * 1000,
    )
    .await?
    .is_some()
    {
        eprintln!("auto_clip_event=suggested kind={kind}");
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spike_needs_a_baseline_a_jump_and_real_volume() {
        let mut levels = VecDeque::new();
        // No baseline yet: even a jump isn't a moment.
        for _ in 0..BASELINE - 1 {
            assert!(!loud(&mut levels, -40.0, 15.0, -30.0));
        }
        assert!(!loud(&mut levels, -10.0, 15.0, -30.0), "still learning");
        // 3 minutes of -40 dBFS: +15 dB to -25 is a moment, +10 is not.
        let mut levels: VecDeque<f32> = std::iter::repeat_n(-40.0, BASELINE).collect();
        assert!(!loud(&mut levels, -30.0, 15.0, -30.0));
        assert!(loud(&mut levels, -25.0, 15.0, -30.0));
        // A quiet stream jumping 15 dB but still under the floor is not.
        let mut quiet: VecDeque<f32> = std::iter::repeat_n(-60.0, BASELINE).collect();
        assert!(!loud(&mut quiet, -40.0, 15.0, -30.0));
        // The baseline keeps only the last 90 segments.
        assert_eq!(levels.len(), BASELINE);
    }

    #[test]
    fn loudest_window_in_dbfs() {
        let pcm = |amplitude: i16, n: usize| -> Vec<u8> {
            (0..n)
                .flat_map(|i| (if i % 2 == 0 { amplitude } else { -amplitude }).to_le_bytes())
                .collect()
        };
        let mut both = pcm(328, 3200); // about -40 dBFS
        both.extend(pcm(3277, 3200)); // about -20 dBFS
        let level = loudest(&both).unwrap();
        assert!((level + 20.0).abs() < 0.1, "{level}");
        assert!(
            loudest(&pcm(0, 3200)).unwrap() <= -99.0,
            "silence floors at -100"
        );
        assert_eq!(loudest(&[]), None);
    }

    #[tokio::test]
    #[ignore = "requires ffmpeg; run explicitly with --ignored"]
    async fn measures_a_real_segment() {
        // A 2-second H.264/AAC transport stream: ffmpeg's sine is 1/8 scale, halved here, so its
        // RMS is 20·log10(0.0625/√2) = -27.1 dBFS.
        let out = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:s=320x180:r=30:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:d=2",
                "-af",
                "volume=0.5",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-f",
                "mpegts",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        let level = loudness(&out.stdout).await.unwrap();
        assert!((level + 27.1).abs() < 1.0, "{level}");
        assert_eq!(loudness(b"not a segment").await, None);
    }
}
