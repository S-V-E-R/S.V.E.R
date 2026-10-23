//! Game detection (docs/LIVE_STREAMS.md "Game detection"): opt-in, suggest-only. The live still
//! the probe already makes is cropped to the layout's game area, turned into a CLIP image
//! embedding on one CPU thread, and matched against stills streamers labeled by choosing a
//! category. Frames never leave the server. A different game seen on three checks in a row
//! becomes a suggestion in Studio; nothing changes until the streamer accepts it.
use super::settings;
use crate::{App, Error, Result, auth};
use axum::{
    Json, Router,
    extract::State,
    routing::{post, put},
};
use axum_extra::extract::cookie::CookieJar;
use candle_core::{DType, Device, Module, Tensor};
use candle_transformers::models::clip::vision_model::{ClipVisionConfig, ClipVisionTransformer};
use image::{GrayImage, RgbImage, imageops::FilterType};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, LazyLock, Mutex, OnceLock},
    time::{Duration, Instant},
};

pub const DIM: usize = 512;
/// Neighbors consulted, and how many must agree on one game.
const K: usize = 5;
const VOTES: usize = 3;
/// Below this cosine similarity a neighbor is no evidence: a game nobody labeled yet stays unknown
/// instead of becoming the nearest known game. Calibrated on held-out games (detect tests).
const MIN_SIMILARITY: f32 = 0.85;
/// Checks in a row that must see the same other game before Studio suggests it.
pub const STREAK: i32 = 3;
const CHECK_EVERY: &str = "170 seconds";
/// Stills this soon after the streamer chose a category are trusted as examples of it.
const LABEL_MINUTES: i32 = 20;
pub const EXAMPLES_PER_GAME: i64 = 200;
/// Layout learning: this many recent stills, shrunk to this width.
const LAYOUT_STILLS: usize = 12;
pub const LAYOUT_WIDTH: u32 = 96;

struct Model {
    vision: ClipVisionTransformer,
    projection: candle_nn::Linear,
}

/// CLIP ViT-B/32 weights in Hugging Face safetensors layout (openai/clip-vit-base-patch32).
fn model_path() -> Option<String> {
    std::env::var("GAME_DETECTION_MODEL")
        .ok()
        .filter(|p| !p.is_empty())
}
pub fn available() -> bool {
    model_path().is_some()
}
fn model() -> Option<&'static Model> {
    static MODEL: OnceLock<Option<Model>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let loaded = model_path().map(|p| load(&p));
            if let Some(Err(e)) = &loaded {
                eprintln!("detect_event=model_load outcome=failed error={e}");
            }
            loaded.and_then(|m| m.ok())
        })
        .as_ref()
}
fn load(path: &str) -> candle_core::Result<Model> {
    let mut tensors = candle_core::safetensors::load(path, &Device::Cpu)?;
    // Only the image side is used; the text tower is dropped right away.
    tensors.retain(|k, _| k.starts_with("vision_model.") || k.starts_with("visual_projection."));
    let vb = candle_nn::VarBuilder::from_tensors(tensors, DType::F32, &Device::Cpu);
    let config = ClipVisionConfig::vit_base_patch32();
    Ok(Model {
        vision: ClipVisionTransformer::new(vb.pp("vision_model"), &config)?,
        projection: candle_nn::linear_no_bias(config.embed_dim, DIM, vb.pp("visual_projection"))?,
    })
}

/// One thread for every embedding on the server: SRS and the API keep the other cores.
static POOL: LazyLock<rayon::ThreadPool> = LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .thread_name(|_| "game-detect".into())
        .build()
        .expect("one-thread pool")
});

/// The unit-length CLIP embedding of an image (CPU-bound: call from a blocking thread).
pub fn embed(image: &RgbImage) -> Option<Vec<f32>> {
    let model = model()?;
    let pixels = preprocess(image);
    POOL.install(|| {
        let input = Tensor::from_vec(pixels, (1, 3, 224, 224), &Device::Cpu)?;
        let features = model
            .projection
            .forward(&model.vision.forward(&input)?)?
            .flatten_all()?
            .to_vec1::<f32>()?;
        candle_core::Result::Ok(normalize(features))
    })
    .map_err(|e| eprintln!("detect_event=embed outcome=failed error={e}"))
    .ok()
}
/// CLIP's own preparation: shortest side to 224, center 224×224, per-channel normalization.
fn preprocess(image: &RgbImage) -> Vec<f32> {
    const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
    const STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];
    let (w, h) = image.dimensions();
    let scale = 224.0 / w.min(h) as f32;
    let (sw, sh) = (
        ((w as f32 * scale).round() as u32).max(224),
        ((h as f32 * scale).round() as u32).max(224),
    );
    let resized = image::imageops::resize(image, sw, sh, FilterType::CatmullRom);
    let (x0, y0) = ((sw - 224) / 2, (sh - 224) / 2);
    let mut out = vec![0f32; 3 * 224 * 224];
    for y in 0..224 {
        for x in 0..224 {
            let p = resized.get_pixel(x0 + x, y0 + y).0;
            for c in 0..3 {
                out[c * 224 * 224 + (y * 224 + x) as usize] =
                    (f32::from(p[c]) / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    out
}
fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v
        .iter()
        .map(|x| x * x)
        .sum::<f32>()
        .sqrt()
        .max(f32::EPSILON);
    v.iter_mut().for_each(|x| *x /= norm);
    v
}
pub fn to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn from_bytes(bytes: &[u8]) -> Option<Vec<f32>> {
    (bytes.len() == DIM * 4).then(|| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    })
}

/// A still shrunk and greyed for layout learning.
pub fn layout_still(image: &RgbImage) -> GrayImage {
    let (w, h) = image.dimensions();
    image::imageops::grayscale(&image::imageops::resize(
        image,
        LAYOUT_WIDTH,
        (h * LAYOUT_WIDTH / w.max(1)).max(1),
        FilterType::Triangle,
    ))
}
/// The game area of a layout as fractions [x0,y0,x1,y1]: the biggest region that keeps changing
/// across stills, since webcam frames, panels and goal bars stay put. None until it is clear
/// (fewer than 6 stills, or nothing large moving), and the whole frame is used meanwhile.
pub fn game_area(stills: &[GrayImage]) -> Option<[f32; 4]> {
    let (w, h) = stills.first()?.dimensions();
    if stills.len() < 6 || stills.iter().any(|s| s.dimensions() != (w, h)) {
        return None;
    }
    let n = stills.len() as f32;
    let moving: Vec<bool> = (0..(w * h) as usize)
        .map(|i| {
            let values = stills.iter().map(|s| f32::from(s.as_raw()[i]));
            let mean = values.clone().sum::<f32>() / n;
            (values.map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt() > 12.0
        })
        .collect();
    let at = |x: u32, y: u32| moving[(y * w + x) as usize];
    let (x0, x1) =
        longest_run((0..w).map(|x| (0..h).filter(|&y| at(x, y)).count() * 2 > h as usize))?;
    let (y0, y1) = longest_run(
        (0..h).map(|y| (x0..x1).filter(|&x| at(x, y)).count() * 2 > (x1 - x0) as usize),
    )?;
    // A game smaller than a quarter of the frame is more likely a webcam: keep the whole frame.
    ((x1 - x0) * (y1 - y0) * 4 >= w * h).then(|| {
        [
            x0 as f32 / w as f32,
            y0 as f32 / h as f32,
            x1 as f32 / w as f32,
            y1 as f32 / h as f32,
        ]
    })
}
fn longest_run(on: impl Iterator<Item = bool>) -> Option<(u32, u32)> {
    let (mut best, mut start) = (None::<(u32, u32)>, None);
    for (i, v) in on.chain(std::iter::once(false)).enumerate() {
        let i = i as u32;
        match (v, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if best.is_none_or(|(a, b)| i - s > b - a) {
                    best = Some((s, i));
                }
                start = None;
            }
            _ => {}
        }
    }
    best
}
pub fn crop(image: &RgbImage, area: Option<&[f32]>) -> RgbImage {
    let (w, h) = image.dimensions();
    let Some(&[x0, y0, x1, y1]) = area else {
        return image.clone();
    };
    let px = |f: f32, max: u32| ((f.clamp(0.0, 1.0) * max as f32) as u32).min(max);
    let (x0, y0, x1, y1) = (px(x0, w), px(y0, h), px(x1, w), px(y1, h));
    if x1 <= x0 + 16 || y1 <= y0 + 16 {
        return image.clone();
    }
    image::imageops::crop_imm(image, x0, y0, x1 - x0, y1 - y0).to_image()
}
/// Loading and black screens carry no game: skip them without touching the streak.
pub fn blank(image: &RgbImage) -> bool {
    let gray = image::imageops::grayscale(image);
    let n = gray.len() as f32;
    let mean = gray.iter().map(|&v| f32::from(v)).sum::<f32>() / n;
    let var = gray
        .iter()
        .map(|&v| (f32::from(v) - mean).powi(2))
        .sum::<f32>()
        / n;
    var.sqrt() < 8.0
}

/// The game most of the closest examples agree on, if they are close enough to count.
pub fn vote(gallery: &[(String, Vec<f32>)], embedding: &[f32]) -> Option<String> {
    let mut near: Vec<(f32, &str)> = gallery
        .iter()
        .map(|(game, e)| (dot(e, embedding), game.as_str()))
        .filter(|(s, _)| *s >= MIN_SIMILARITY)
        .collect();
    near.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (_, game) in near.iter().take(K) {
        *counts.entry(game).or_default() += 1;
    }
    counts
        .into_iter()
        .find(|&(_, n)| n >= VOTES)
        .map(|(game, _)| game.to_string())
}
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The next (guess, streak) after one check. A frame matching the category, or a game the
/// streamer dismissed, clears it; an unknown frame (menus, a game nobody labeled) keeps it.
pub fn next_streak(
    seen: Option<&str>,
    category: Option<&str>,
    dismissed: &[String],
    guess: Option<&str>,
    streak: i32,
) -> (Option<String>, i32) {
    match seen {
        None => (guess.map(str::to_string), streak),
        Some(g) if Some(g) == category || dismissed.iter().any(|d| d == g) => (None, 0),
        Some(g) if Some(g) == guess => (Some(g.to_string()), streak + 1),
        Some(g) => (Some(g.to_string()), 1),
    }
}

/// Labeled examples: (category id, embedding).
type Gallery = Arc<Vec<(String, Vec<f32>)>>;
/// Per channel: when its last still arrived, and its recent layout stills.
type Recent = HashMap<String, (Instant, VecDeque<GrayImage>)>;

/// ponytail: the whole gallery in memory, brute-force cosine, reloaded every 5 minutes. Fine to
/// about 50k examples (100 MB, a few ms per check); past that, pgvector or a per-genre shortlist.
async fn gallery(app: &App) -> Result<Gallery> {
    static CACHE: Mutex<Option<(Instant, Gallery)>> = Mutex::new(None);
    if let Some((at, g)) = CACHE.lock().unwrap().as_ref()
        && at.elapsed() < Duration::from_secs(300)
    {
        return Ok(g.clone());
    }
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as("SELECT e.category_id,e.embedding FROM game_examples e JOIN stream_categories c ON c.id=e.category_id AND c.active")
        .fetch_all(&app.db)
        .await?;
    let g = Arc::new(
        rows.into_iter()
            .filter_map(|(c, b)| Some((c, from_bytes(&b)?)))
            .collect::<Vec<_>>(),
    );
    *CACHE.lock().unwrap() = Some((Instant::now(), g.clone()));
    Ok(g)
}

/// ponytail: recent stills per channel live in this process; a restart relearns the layout from
/// the saved area. Entries for channels that stopped sending stills are dropped after 10 minutes.
fn learn_layout(owner: &str, image: &RgbImage) -> Option<[f32; 4]> {
    static RECENT: LazyLock<Mutex<Recent>> = LazyLock::new(Default::default);
    let small = layout_still(image);
    let mut recent = RECENT.lock().unwrap();
    recent.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(600));
    let (at, stills) = recent
        .entry(owner.to_string())
        .or_insert_with(|| (Instant::now(), VecDeque::new()));
    *at = Instant::now();
    stills.push_back(small);
    if stills.len() > LAYOUT_STILLS {
        stills.pop_front();
    }
    game_area(stills.make_contiguous())
}

#[derive(sqlx::FromRow)]
struct Check {
    owner_id: String,
    category_id: Option<String>,
    detect_area: Option<Vec<f32>>,
    due: bool,
    fresh_label: bool,
    detect_guess: Option<String>,
    detect_streak: i32,
    detect_dismissed: Vec<String>,
}
/// Called with each live still (about once a minute). Learns the layout every time; checks the
/// game about every 3 minutes, for channels that switched detection on.
pub async fn still(app: &App, broadcast: &str, webp: &[u8]) -> Result<()> {
    if !available() {
        return Ok(());
    }
    let check: Option<Check> = sqlx::query_as("SELECT b.owner_id,s.category_id,s.detect_area,coalesce(b.detect_at<now()-$2::interval,true) AS due,coalesce(s.category_at>now()-make_interval(mins=>$3),false) AS fresh_label,b.detect_guess,b.detect_streak,b.detect_dismissed FROM broadcasts b JOIN stream_settings s ON s.owner_id=b.owner_id AND s.game_detection WHERE b.id=$1 AND b.state='LIVE'")
        .bind(broadcast).bind(CHECK_EVERY).bind(LABEL_MINUTES).fetch_optional(&app.db).await?;
    let Some(check) = check else {
        return Ok(());
    };
    let Ok(image) = image::load_from_memory(webp).map(|i| i.to_rgb8()) else {
        return Ok(());
    };
    let mut area = check.detect_area.clone();
    if let Some(learned) = learn_layout(&check.owner_id, &image)
        && area.as_deref().is_none_or(|a| moved(a, &learned))
    {
        sqlx::query("UPDATE stream_settings SET detect_area=$2 WHERE owner_id=$1")
            .bind(&check.owner_id)
            .bind(learned.to_vec())
            .execute(&app.db)
            .await?;
        area = Some(learned.to_vec());
    }
    if !check.due {
        return Ok(());
    }
    let game = crop(&image, area.as_deref());
    let embedding = if blank(&game) {
        None
    } else {
        tokio::task::spawn_blocking(move || embed(&game))
            .await
            .map_err(|_| Error::internal())?
    };
    let Some(embedding) = embedding else {
        sqlx::query("UPDATE broadcasts SET detect_at=now() WHERE id=$1")
            .bind(broadcast)
            .execute(&app.db)
            .await?;
        return Ok(());
    };
    let seen = vote(&gallery(app).await?, &embedding);
    let category = check.category_id.as_deref();
    // Learning: a still soon after the streamer chose the category is an example of it, unless it
    // clearly shows another game (they forgot to switch, or switched games before the category).
    if let Some(category) = category
        && check.fresh_label
        && seen.as_deref().is_none_or(|s| s == category)
    {
        add_example(&app.db, &check.owner_id, category, &embedding).await?;
    }
    let (guess, streak) = next_streak(
        seen.as_deref(),
        category,
        &check.detect_dismissed,
        check.detect_guess.as_deref(),
        check.detect_streak,
    );
    sqlx::query(
        "UPDATE broadcasts SET detect_at=now(),detect_guess=$2,detect_streak=$3 WHERE id=$1",
    )
    .bind(broadcast)
    .bind(guess)
    .bind(streak)
    .execute(&app.db)
    .await?;
    Ok(())
}
fn moved(saved: &[f32], learned: &[f32; 4]) -> bool {
    saved.len() != 4 || saved.iter().zip(learned).any(|(a, b)| (a - b).abs() > 0.03)
}
/// Stores one labeled example and keeps only the newest per channel and game.
pub async fn add_example(
    db: &sqlx::PgPool,
    owner: &str,
    category: &str,
    embedding: &[f32],
) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("INSERT INTO game_examples(category_id,owner_id,embedding) VALUES($1,$2,$3)")
        .bind(category)
        .bind(owner)
        .bind(to_bytes(embedding))
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM game_examples WHERE id IN (SELECT id FROM game_examples WHERE owner_id=$1 AND category_id=$2 ORDER BY created_at DESC,id DESC OFFSET $3)")
        .bind(owner).bind(category).bind(EXAMPLES_PER_GAME).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// A new category clears any pending suggestion and starts a fresh labeling window.
pub async fn category_changed(db: &mut PgConnection, owner: &str) -> Result<()> {
    sqlx::query("UPDATE stream_settings SET category_at=now() WHERE owner_id=$1")
        .bind(owner)
        .execute(&mut *db)
        .await?;
    sqlx::query("UPDATE broadcasts SET detect_guess=NULL,detect_streak=0 WHERE owner_id=$1 AND state<>'ENDED' AND detect_guess IS NOT NULL")
        .bind(owner).execute(&mut *db).await?;
    Ok(())
}

/// Studio's view: whether detection can run, whether it's on, and the current suggestion.
pub async fn studio(db: &mut PgConnection, owner: &str) -> Result<Value> {
    let on: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT game_detection FROM stream_settings WHERE owner_id=$1),false)",
    )
    .bind(owner)
    .fetch_one(&mut *db)
    .await?;
    let category: Option<String> = sqlx::query_scalar("SELECT c.name FROM stream_settings s JOIN stream_categories c ON c.id=s.category_id WHERE s.owner_id=$1")
        .bind(owner).fetch_optional(&mut *db).await?;
    // `mature`: a mature-rated game switches the Mature label on when accepted, as in the picker.
    let suggestion: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('category_id',c.id,'name',c.name,'mature',EXISTS(SELECT 1 FROM game_catalog g WHERE g.category_id=c.id AND g.mature)) FROM broadcasts b JOIN stream_settings s ON s.owner_id=b.owner_id AND s.game_detection JOIN stream_categories c ON c.id=b.detect_guess AND c.active WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') AND b.detect_streak>=$2 AND b.detect_guess IS DISTINCT FROM s.category_id ORDER BY b.started_at DESC LIMIT 1")
        .bind(owner).bind(STREAK).fetch_optional(&mut *db).await?;
    Ok(json!({"available": available(), "on": on, "category": category, "suggestion": suggestion}))
}

#[derive(Deserialize)]
struct Switch {
    on: bool,
}
/// PUT /api/me/stream/detection: opt in or out. Opting out deletes the channel's examples.
async fn switch(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Switch>,
) -> Result<Json<Value>> {
    let (mut tx, user, session) = auth::session(&app, &jar, false).await?;
    auth::authorize_streaming(&user, &session)?;
    settings(&mut tx, &user).await?;
    sqlx::query("UPDATE stream_settings SET game_detection=$2,detect_area=CASE WHEN $2 THEN detect_area END,updated_at=now() WHERE owner_id=$1")
        .bind(&user.id).bind(input.on).execute(&mut *tx).await?;
    if !input.on {
        sqlx::query("DELETE FROM game_examples WHERE owner_id=$1")
            .bind(&user.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE broadcasts SET detect_guess=NULL,detect_streak=0,detect_dismissed='{}' WHERE owner_id=$1 AND state<>'ENDED'")
            .bind(&user.id).execute(&mut *tx).await?;
    }
    let body = studio(&mut tx, &user.id).await?;
    tx.commit().await?;
    Ok(Json(body))
}
#[derive(Deserialize)]
struct Dismiss {
    category_id: String,
}
/// POST /api/me/stream/detection/dismiss: "not this game" for the rest of the broadcast.
async fn dismiss(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Dismiss>,
) -> Result<Json<Value>> {
    if input.category_id.is_empty() || input.category_id.len() > 200 {
        return Err(Error::bad("Choose a suggestion to dismiss."));
    }
    let (mut tx, user, session) = auth::session(&app, &jar, false).await?;
    auth::authorize_streaming(&user, &session)?;
    sqlx::query("UPDATE broadcasts SET detect_dismissed=array_append(detect_dismissed,$2),detect_guess=NULL,detect_streak=0 WHERE owner_id=$1 AND state<>'ENDED' AND NOT $2=ANY(detect_dismissed) AND cardinality(detect_dismissed)<50")
        .bind(&user.id).bind(&input.category_id).execute(&mut *tx).await?;
    let body = studio(&mut tx, &user.id).await?;
    tx.commit().await?;
    Ok(Json(body))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/stream/detection", put(switch))
        .route("/api/me/stream/detection/dismiss", post(dismiss))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn still(seed: u32) -> GrayImage {
        // A 100×40 layout: a static frame, a 10×10 "webcam" that changes, and a 70×30 game.
        GrayImage::from_fn(100, 40, |x, y| {
            let n = (x * 31 + y * 17 + seed * 101) % 255;
            if (25..95).contains(&x) && (5..35).contains(&y) {
                image::Luma([n as u8])
            } else if x < 10 && y < 10 {
                image::Luma([(n / 2) as u8])
            } else {
                image::Luma([40])
            }
        })
    }

    #[test]
    fn game_area_finds_the_moving_region_and_ignores_the_webcam() {
        let stills: Vec<_> = (0..12).map(still).collect();
        let [x0, y0, x1, y1] = game_area(&stills).unwrap();
        assert_eq!((x0, x1), (0.25, 0.95));
        assert_eq!((y0, y1), (5.0 / 40.0, 35.0 / 40.0));
        assert!(game_area(&stills[..5]).is_none(), "needs at least 6 stills");
        let still_frame: Vec<_> = (0..12).map(|_| still(0)).collect();
        assert!(game_area(&still_frame).is_none(), "nothing moves");
    }

    #[test]
    fn vote_needs_close_agreeing_neighbors() {
        let unit = |i: usize| {
            let mut v = vec![0.0; DIM];
            v[i] = 1.0;
            v
        };
        let near = |i: usize, j: usize| {
            normalize(
                (0..DIM)
                    .map(|k| match k {
                        _ if k == i => 1.0,
                        _ if k == j => 0.2,
                        _ => 0.0,
                    })
                    .collect(),
            )
        };
        let gallery: Vec<(String, Vec<f32>)> = (0..4)
            .map(|j| ("mw4".to_string(), near(0, j + 2)))
            .chain((0..4).map(|j| ("lol".to_string(), near(1, j + 2))))
            .collect();
        assert_eq!(vote(&gallery, &unit(0)).as_deref(), Some("mw4"));
        assert_eq!(vote(&gallery, &unit(1)).as_deref(), Some("lol"));
        assert_eq!(
            vote(&gallery, &unit(9)),
            None,
            "an unknown game stays unknown"
        );
        assert_eq!(
            vote(&gallery[..2], &unit(0)),
            None,
            "two examples can't outvote"
        );
    }

    #[test]
    fn streak_counts_one_other_game_in_a_row() {
        let none: Vec<String> = vec![];
        let s = |g: &str| Some(g.to_string());
        assert_eq!(
            next_streak(Some("fn"), Some("mw4"), &none, None, 0),
            (s("fn"), 1)
        );
        assert_eq!(
            next_streak(Some("fn"), Some("mw4"), &none, Some("fn"), 2),
            (s("fn"), 3)
        );
        assert_eq!(
            next_streak(Some("lol"), Some("mw4"), &none, Some("fn"), 2),
            (s("lol"), 1)
        );
        assert_eq!(
            next_streak(None, Some("mw4"), &none, Some("fn"), 2),
            (s("fn"), 2),
            "menus keep it"
        );
        assert_eq!(
            next_streak(Some("mw4"), Some("mw4"), &none, Some("fn"), 2),
            (None, 0)
        );
        assert_eq!(
            next_streak(Some("fn"), Some("mw4"), &["fn".into()], Some("fn"), 2),
            (None, 0)
        );
    }

    #[test]
    fn embeddings_round_trip_and_crop_falls_back_to_the_frame() {
        let e = normalize((0..DIM).map(|i| i as f32).collect());
        assert_eq!(from_bytes(&to_bytes(&e)).unwrap(), e);
        assert!(from_bytes(&[0; 12]).is_none());
        let image = RgbImage::new(640, 268);
        assert_eq!(crop(&image, None).dimensions(), (640, 268));
        assert_eq!(
            crop(&image, Some(&[0.25, 0.0, 0.75, 1.0])).dimensions(),
            (320, 268)
        );
        assert_eq!(
            crop(&image, Some(&[0.5, 0.5, 0.5, 0.5])).dimensions(),
            (640, 268)
        );
        assert!(blank(&image));
    }

    /// Accuracy on real recordings (docs/LIVE_STREAMS.md "Game detection"): frames named
    /// `<game>_<seconds>.png` in SVER_DETECT_FRAMES, GAME_DETECTION_MODEL set. Per game, the first
    /// half is the gallery and the second half the test; then each game is hidden from the gallery
    /// to count how often a game nobody labeled would be mistaken for a labeled one.
    #[test]
    #[ignore = "needs GAME_DETECTION_MODEL and SVER_DETECT_FRAMES"]
    fn recordings() {
        let dir = std::env::var("SVER_DETECT_FRAMES").unwrap();
        let mut frames: Vec<(String, u32, std::path::PathBuf)> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| {
                let path = e.ok()?.path();
                let stem = path.file_stem()?.to_str()?.to_string();
                let (game, t) = stem.split_once('_')?;
                Some((game.to_string(), t.parse().ok()?, path))
            })
            .collect();
        frames.sort();
        let mut games: Vec<String> = frames.iter().map(|f| f.0.clone()).collect();
        games.dedup();
        let of = |game: &str| -> Vec<usize> {
            (0..frames.len()).filter(|&i| frames[i].0 == game).collect()
        };
        let images: Vec<RgbImage> = frames
            .iter()
            .map(|(_, _, p)| image::open(p).unwrap().to_rgb8())
            .collect();
        // Each game's layout is learned from 12 of its gallery stills, as a live channel would.
        let mut areas = HashMap::new();
        for g in &games {
            let idx = of(g);
            let gallery = &idx[..idx.len() / 2];
            let stills: Vec<GrayImage> = gallery
                .iter()
                .step_by((gallery.len() / 12).max(1))
                .take(12)
                .map(|&i| layout_still(&images[i]))
                .collect();
            areas.insert(g.clone(), game_area(&stills));
            println!("{g:10} game area {:?}", areas[g]);
        }
        let started = Instant::now();
        let embedded: Vec<Vec<f32>> = (0..images.len())
            .map(|i| {
                embed(&crop(
                    &images[i],
                    areas[&frames[i].0].as_ref().map(|a| &a[..]),
                ))
                .unwrap()
            })
            .collect();
        println!(
            "{} ms per still",
            started.elapsed().as_millis() / images.len() as u128
        );
        let gallery_of = |skip: Option<&str>| -> Vec<(String, Vec<f32>)> {
            games
                .iter()
                .filter(|g| Some(g.as_str()) != skip)
                .flat_map(|g| {
                    let idx = of(g);
                    idx[..idx.len() / 2]
                        .iter()
                        .map(|&i| (g.clone(), embedded[i].clone()))
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let all = gallery_of(None);
        let (mut right, mut total, mut mistaken) = (0, 0, 0);
        for g in &games {
            let idx = of(g);
            let test = &idx[idx.len() / 2..];
            let votes: Vec<Option<String>> =
                test.iter().map(|&i| vote(&all, &embedded[i])).collect();
            let r = votes.iter().filter(|v| v.as_deref() == Some(g)).count();
            let unknown = votes.iter().filter(|v| v.is_none()).count();
            let without = gallery_of(Some(g));
            let m = test
                .iter()
                .filter(|&&i| vote(&without, &embedded[i]).is_some())
                .count();
            let best_other = test
                .iter()
                .map(|&i| {
                    without
                        .iter()
                        .map(|(_, e)| dot(e, &embedded[i]))
                        .fold(f32::MIN, f32::max)
                })
                .fold(f32::MIN, f32::max);
            println!(
                "{g:10} right {r}/{} unknown {unknown} | hidden: mistaken {m}/{}, closest other game {best_other:.3}",
                test.len(),
                test.len()
            );
            (right, total, mistaken) = (right + r, total + test.len(), mistaken + m);
        }
        println!(
            "total right {right}/{total}; a hidden game mistaken for another {mistaken} times"
        );
        assert!(right * 100 >= total * 90);
    }
}
