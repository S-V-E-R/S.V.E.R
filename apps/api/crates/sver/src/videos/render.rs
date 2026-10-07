//! The platform's one re-encode (docs/BEACONS.md): an approved clip cropped to 9:16, written as
//! watermarked 1080p and 720p public copies, a clean copy for its creator and a thumbnail, with
//! metadata stripped. Runs as a leased BEACON job.
use super::worker::{Job, fence, object, put};
use super::*;
use std::process::Stdio;
use tokio::process::Command;

/// Watermark corners inside the feed's safe area, clear of the right rail and bottom captions.
const CORNERS: [(&str, &str); 4] = [
    ("w*0.05", "h*0.07"),
    ("w*0.80-tw", "h*0.07"),
    ("w*0.80-tw", "h*0.78-th"),
    ("w*0.05", "h*0.78-th"),
];

/// The moving "@username · sver.tv" mark: it visits the corners in `order`, `period` seconds each.
pub(crate) fn watermark(username: &str, font: &str, order: &[usize], period: f64) -> String {
    let cycle = period * 4.0;
    let slot = format!("floor(mod(t\\,{cycle})/{period})");
    let pick = |axis: usize| {
        let at = |i: usize| {
            let corner = CORNERS[order[i] % 4];
            if axis == 0 { corner.0 } else { corner.1 }
        };
        format!(
            "if(eq({slot}\\,0)\\,{}\\,if(eq({slot}\\,1)\\,{}\\,if(eq({slot}\\,2)\\,{}\\,{})))",
            at(0),
            at(1),
            at(2),
            at(3)
        )
    };
    // A drive colon (Windows development) needs one escape inside the quoted value.
    let font = font.replace('\\', "/").replace(':', "\\:");
    format!(
        "drawtext=fontfile='{font}':text='@{username} · sver.tv':fontsize=h*0.022:fontcolor=white@0.55:shadowcolor=black@0.5:shadowx=2:shadowy=2:x={}:y={}",
        pick(0),
        pick(1)
    )
}

pub(super) async fn beacon(app: &App, job: &Job) -> Res<()> {
    let video = load(&mut *app.db.acquire().await?, &job.video_id).await?;
    if video.status != "PROCESSING" {
        return Ok(());
    }
    match render(app, job, &video).await {
        Ok(()) => Ok(()),
        // After repeated failures the creator sees why and can try again; the job ends.
        Err(_) if job.attempts >= 3 => {
            let mut tx = app.db.begin().await?;
            fence(&mut tx, job).await?;
            sqlx::query("UPDATE videos SET status='FAILED' WHERE id=$1 AND status='PROCESSING'")
                .bind(&video.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE beacons SET failure='The video could not be processed. Try again, or choose another clip.' WHERE video_id=$1")
                .bind(&video.id).execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// A private scratch directory, removed whatever happens.
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn ffmpeg(args: &[String]) -> Res<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|_| Fail::unavailable("Beacon processing is unavailable."))?;
    if output.status.success() {
        return Ok(());
    }
    // The last FFmpeg error line, for operators; it names no user data beyond scratch paths.
    let error = String::from_utf8_lossy(&output.stderr);
    eprintln!(
        "beacon_event=ffmpeg_failed error={:?}",
        error
            .lines()
            .collect::<Vec<_>>()
            .join(" | ")
            .chars()
            .take(600)
            .collect::<String>()
    );
    Err(Fail::unavailable("Beacon processing will retry."))
}

async fn render(app: &App, job: &Job, video: &Video) -> Res<()> {
    let owner = video.owner_id.as_deref().ok_or_else(Fail::missing)?;
    let (source, crop, username): (Option<String>, f32, String) = sqlx::query_as("SELECT s.mp4_key,b.crop,c.username FROM beacons b JOIN videos s ON s.id=b.source_id JOIN channel_users c ON c.id=$2 WHERE b.video_id=$1")
        .bind(&video.id).bind(owner).fetch_one(&app.db).await?;
    let source = object(app, &source.ok_or_else(Fail::missing)?).await?;
    let dir = Scratch(std::env::temp_dir().join(format!("sver-beacon-{}", job.lease_token)));
    std::fs::create_dir_all(&dir.0).map_err(|_| Fail::internal())?;
    let path = |name: &str| dir.0.join(name).to_string_lossy().into_owned();
    tokio::fs::write(path("in.mp4"), source)
        .await
        .map_err(|_| Fail::internal())?;
    let order: Vec<usize> = job.input["order"]
        .as_array()
        .map(|o| {
            o.iter()
                .filter_map(|v| v.as_u64())
                .map(|v| v as usize % 4)
                .collect()
        })
        .filter(|o: &Vec<usize>| o.len() == 4)
        .unwrap_or_else(|| vec![0, 1, 2, 3]);
    let period = job.input["period"]
        .as_f64()
        .unwrap_or(app.config.videos.tuning.beacon_mark_seconds);
    let mark = watermark(&username, &app.config.videos.beacon_font, &order, period);
    // Pad rather than stretch whatever the crop leaves.
    let fit = |w: u32, h: u32| {
        format!(
            "scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1"
        )
    };
    let graph = format!(
        "[0:v]crop=w='min(iw,ih*9/16)':h='min(ih,iw*16/9)':x='(iw-ow)*{crop}':y='(ih-oh)*{crop}',split=3[a][b][c];[a]{},{mark}[hd];[b]{},{mark}[sd];[c]{}[clean]",
        fit(1080, 1920),
        fit(720, 1280),
        fit(1080, 1920)
    );
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        path("in.mp4"),
        "-filter_complex".into(),
        graph,
    ];
    for (label, crf, file) in [
        ("[hd]", "21", "hd.mp4"),
        ("[sd]", "23", "sd.mp4"),
        ("[clean]", "21", "clean.mp4"),
    ] {
        for arg in [
            "-map",
            label,
            "-map",
            "0:a:0?",
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
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ] {
            args.push(arg.into());
        }
        args.push(path(file));
    }
    ffmpeg(&args).await?;
    let thumbnail: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-ss",
        "1",
        "-i",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([path("clean.mp4")])
    .chain(
        [
            "-frames:v",
            "1",
            "-vf",
            "scale=540:-2",
            "-c:v",
            "libwebp",
            "-map_metadata",
            "-1",
        ]
        .iter()
        .map(|s| s.to_string()),
    )
    .chain([path("thumb.webp")])
    .collect();
    ffmpeg(&thumbnail).await?;
    let mut keys = Vec::new();
    for (file, kind) in [
        ("hd.mp4", "video/mp4"),
        ("sd.mp4", "video/mp4"),
        ("clean.mp4", "video/mp4"),
        ("thumb.webp", "image/webp"),
    ] {
        let bytes = tokio::fs::read(path(file))
            .await
            .map_err(|_| Fail::internal())?;
        let key = format!("{}/{}-{file}", video.id, job.lease_token);
        sqlx::query("INSERT INTO video_objects(key,video_id,content_type,bytes,delete_after) VALUES($1,$2,$3,$4,now()+interval '1 day') ON CONFLICT DO NOTHING")
            .bind(&key).bind(&video.id).bind(kind).bind(bytes.len() as i64).execute(&app.db).await?;
        put(app, &key, bytes, kind).await?;
        keys.push(key);
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT id FROM videos WHERE id=$1 FOR UPDATE")
        .bind(&video.id)
        .execute(&mut *tx)
        .await?;
    fence(&mut tx, job).await?;
    sqlx::query("UPDATE video_objects SET ready=true,delete_after=NULL WHERE key=ANY($1)")
        .bind(&keys)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE beacons SET hd_key=$2,sd_key=$3,clean_key=$4,failure=NULL WHERE video_id=$1",
    )
    .bind(&video.id)
    .bind(&keys[0])
    .bind(&keys[1])
    .bind(&keys[2])
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE videos SET status='READY',mp4_key=$2,thumbnail_key=$3,revision=revision+1 WHERE id=$1 AND status='PROCESSING'")
        .bind(&video.id).bind(&keys[0]).bind(&keys[3]).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::watermark;
    #[test]
    fn the_mark_visits_every_corner_in_the_given_order() {
        let mark = watermark("Joe", "C:\\Fonts\\a.ttf", &[2, 0, 3, 1], 3.0);
        assert!(mark.contains("text='@Joe · sver.tv'"));
        assert!(mark.contains("fontfile='C\\:/Fonts/a.ttf'"));
        // First slot is corner 2 (right, bottom), the second corner 0 (left, top).
        let x = mark.split(":x=").nth(1).unwrap();
        let first = x.find("w*0.80-tw").unwrap();
        let second = x.find("w*0.05").unwrap();
        assert!(first < second);
        assert!(mark.contains("mod(t\\,12)/3"));
    }
}
