//! Automatic clip suggestions (docs/VODS_CLIPS.md "Automatic clip suggestions"). Two kinds of
//! moment, game-agnostic so they work for consoles on capture cards too:
//! - room moments: MAGNet's own signals (chat or follows well above the stream's normal), checked
//!   for every live stream rather than only when a lane features it;
//! - loud moments: a recorded segment's audio well above the stream's last 3 minutes;
//! - MW4 wins: Call of Duty: Modern Warfare 4's own VICTORY and ROUND WIN screens, recognized with
//!   the game detection model (CLIP) against labeled examples, so the clip is the fight that won it.
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

#[derive(sqlx::FromRow)]
struct Recording {
    broadcast_id: String,
    owner_id: Option<String>,
    auto_clips: bool,
    category: Option<String>,
    detect_area: Option<Vec<f32>>,
}
/// Called with each recorded segment (worker SEGMENT job). Never holds up the recording: a decode
/// problem only skips the check.
pub async fn segment(app: &App, video: &str, bytes: &[u8]) -> Res<()> {
    let row: Option<Recording> = sqlx::query_as("SELECT v.broadcast_id,v.owner_id,coalesce(s.auto_clips,true) AS auto_clips,c.name AS category,ss.detect_area FROM videos v LEFT JOIN video_settings s ON s.owner_id=v.owner_id LEFT JOIN stream_settings ss ON ss.owner_id=v.owner_id LEFT JOIN stream_categories c ON c.id=ss.category_id WHERE v.id=$1 AND v.kind='VOD' AND v.status='RECORDING'")
        .bind(video).fetch_optional(&app.db).await?;
    let Some(Recording {
        broadcast_id: broadcast,
        owner_id: Some(_),
        auto_clips,
        category,
        detect_area: area,
    }) = row
    else {
        return Ok(());
    };
    // A win is a MAGNet moment (docs/MAGNET.md "What counts as a moment") whether or not clips are suggested.
    if category.as_deref() == Some(MW4)
        && let Some(title) = win_screen(bytes, area).await
    {
        crate::magnet::game_moment(app, &broadcast, title).await?;
        if auto_clips {
            moment(app, &broadcast, "win", title).await?;
        }
    }
    if !auto_clips {
        return Ok(());
    }
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

const MW4: &str = "Call of Duty: Modern Warfare 4";
/// Labeled MW4 screens cut from Joe's recordings (the `mw4_screens` test writes it): per example a
/// label byte (0 anything else, 1 VICTORY, 2 ROUND WIN) and a 512-float CLIP embedding of the
/// game area. Embeddings, not pictures. Losses, mid-match scoreboards, killcams and menus are
/// labeled "other" so they outvote a near miss.
static SCREENS: &[u8] = include_bytes!("mw4_screens.bin");
const RECORD: usize = 1 + 4 * crate::streams::detect::DIM;
fn screens() -> &'static [(String, Vec<f32>)] {
    static PARSED: LazyLock<Vec<(String, Vec<f32>)>> = LazyLock::new(|| {
        SCREENS
            .as_chunks::<RECORD>()
            .0
            .iter()
            .filter_map(|r| {
                let label = ["other", "victory", "round_win"].get(usize::from(r[0]))?;
                Some((
                    label.to_string(),
                    crate::streams::detect::from_bytes(&r[1..])?,
                ))
            })
            .collect()
    });
    &PARSED
}
/// "Victory" or "Round win" when the segment's first frame is one of MW4's win screens. The frame
/// is the 640-pixel still the probe makes, cropped to the channel's learned game area if any.
async fn win_screen(bytes: &[u8], area: Option<Vec<f32>>) -> Option<&'static str> {
    use crate::streams::detect;
    if !detect::available() || screens().is_empty() {
        return None;
    }
    let still = crate::probe::snapshot(bytes.to_vec()).await?;
    let image = image::load_from_memory(&still).ok()?.to_rgb8();
    let game = detect::crop(&image, area.as_deref());
    // CLIP sees an end-of-match board but not the one word on it; losses are drawn in red.
    if red_share(&game) >= 0.05 {
        return None;
    }
    // ponytail: one embedding per 2-second segment per MW4 stream on the shared one-thread pool;
    // past a dozen MW4 streams at once, check every other segment.
    let embedding = tokio::task::spawn_blocking(move || detect::embed(&game))
        .await
        .ok()??;
    match detect::vote(screens(), &embedding)?.as_str() {
        "victory" => Some("Victory"),
        "round_win" => Some("Round win"),
        _ => None,
    }
}

/// Share of clearly red pixels in the top 40% of the game area. MW4 draws DEFEAT and ROUND LOSS
/// in red-orange and wins in teal: on Joe's recordings wins were at most 3%, losses 7% to 10%.
pub fn red_share(game: &image::RgbImage) -> f32 {
    let (w, h) = game.dimensions();
    let rows = (h * 2 / 5).max(1);
    let red = (0..rows)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, b] = game.get_pixel(x, y).0.map(i32::from);
            r > g + 40 && r > b + 40 && r > 90
        })
        .count();
    red as f32 / (w * rows).max(1) as f32
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

    /// MW4 win screens across sessions (docs/VODS_CLIPS.md): frames named
    /// `<victory|roundwin|other>_<session>_<seconds>.png` (full 640-pixel stills) in
    /// SVER_SCREEN_FRAMES, GAME_DETECTION_MODEL set. Session A's examples judge session B's
    /// frames and the reverse; with SVER_SCREEN_WRITE set, every example is written there.
    #[test]
    #[ignore = "needs GAME_DETECTION_MODEL and SVER_SCREEN_FRAMES"]
    fn mw4_screens() {
        use crate::streams::detect;
        // The game area learned for the recordings' layout (game detection, October 22).
        const AREA: [f32; 4] = [0.302_083_34, 0.1, 0.9375, 0.975];
        let dir = std::env::var("SVER_SCREEN_FRAMES").unwrap();
        let mut rows: Vec<(String, String, Vec<f32>, String)> = Vec::new();
        let mut reds = Vec::new();
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        for path in paths {
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            let mut parts = stem.splitn(3, '_');
            let label = match parts.next().unwrap() {
                "roundwin" => "round_win",
                l => l,
            }
            .to_string();
            let session = parts.next().unwrap().to_string();
            let image = image::open(&path).unwrap().to_rgb8();
            let game = detect::crop(&image, Some(&AREA));
            if label != "other" {
                assert!(red_share(&game) < 0.05, "{stem} is a win drawn in red");
            }
            reds.push(red_share(&game) >= 0.05);
            rows.push((label, session, detect::embed(&game).unwrap(), stem));
        }
        let mut wins_found = 0;
        let mut wins_total = 0;
        let mut false_wins = 0;
        for (examples, tested) in [("A", "B"), ("B", "A")] {
            let gallery: Vec<(String, Vec<f32>)> = rows
                .iter()
                .filter(|r| r.1 == examples)
                .map(|r| (r.0.clone(), r.2.clone()))
                .collect();
            for ((label, session, embedding, name), red) in
                rows.iter().zip(&reds).filter(|r| r.0.1 == tested)
            {
                let seen = match detect::vote(&gallery, embedding) {
                    Some(_) if *red => "other (red)".to_string(),
                    Some(seen) => seen,
                    None => "unknown".into(),
                };
                if label != "other" {
                    wins_total += 1;
                    wins_found += usize::from(&seen == label);
                    println!("{session} {label:9} -> {seen:9} {name}");
                } else if seen == "victory" || seen == "round_win" {
                    false_wins += 1;
                    println!("{session} other     -> {seen:9} {name} (false win)");
                }
            }
        }
        println!("wins found {wins_found}/{wins_total}, false wins {false_wins}");
        if let Ok(out) = std::env::var("SVER_SCREEN_WRITE") {
            let bytes: Vec<u8> = rows
                .iter()
                .flat_map(|(label, _, e, _)| {
                    let code = match label.as_str() {
                        "victory" => 1u8,
                        "round_win" => 2,
                        _ => 0,
                    };
                    std::iter::once(code).chain(detect::to_bytes(e))
                })
                .collect();
            std::fs::write(out, bytes).unwrap();
        }
        assert_eq!(false_wins, 0);
    }

    /// The live path on real segments: SVER_SCREEN_SEGMENTS holds victory.ts, roundwin.ts,
    /// defeat.ts and gameplay.ts cut from an MW4 recording with the layout below.
    #[tokio::test]
    #[ignore = "needs ffmpeg, GAME_DETECTION_MODEL and SVER_SCREEN_SEGMENTS"]
    async fn win_screens_on_real_segments() {
        let dir = std::path::PathBuf::from(std::env::var("SVER_SCREEN_SEGMENTS").unwrap());
        let area = Some(vec![0.302_083_34, 0.1, 0.9375, 0.975]);
        let seen = |name: &str| {
            let bytes = std::fs::read(dir.join(name)).unwrap();
            let area = area.clone();
            async move { win_screen(&bytes, area).await }
        };
        assert_eq!(seen("victory.ts").await, Some("Victory"));
        assert!(seen("roundwin.ts").await.is_some());
        assert_eq!(seen("defeat.ts").await, None);
        assert_eq!(seen("gameplay.ts").await, None);
    }

    #[test]
    fn the_shipped_screens_parse() {
        let all = screens();
        assert_eq!(
            all.len() * (1 + 4 * crate::streams::detect::DIM),
            SCREENS.len()
        );
        for label in ["victory", "round_win", "other"] {
            assert!(all.iter().any(|(l, _)| l == label), "{label}");
        }
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
