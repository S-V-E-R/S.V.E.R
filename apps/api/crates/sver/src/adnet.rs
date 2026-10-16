//! S.V.E.R's own network of approved ads (docs/ADVERTISERS.md), build step 1: the rate card,
//! advertiser accounts, ads and their review, quotes sold only against forecast inventory
//! (admission control), Stripe checkout, cancellation, withdrawal and refunds. Serving and
//! impression verification are build step 2; ads.rs keeps the separate test ledger.
use crate::{
    App, media,
    profiles::{self, Fail, Res},
    safety, security as sec, stripe, switches, text,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// Stored widths of ad pictures (16:9 and 9:16); the largest is also the minimum upload width.
pub const LANDSCAPE_WIDTHS: &[u32] = &[640, 1280];
pub const PORTRAIT_WIDTHS: &[u32] = &[360, 720];
const PLACEMENTS: [&str; 3] = ["break", "feed", "site"];
/// Only this share of the forecast is ever sold, so delivery can absorb a slow week.
const SELLABLE: f64 = 0.7;
const MIN_IMPRESSIONS: i64 = 1000;
const MAX_IMPRESSIONS: i64 = 10_000_000;
const MAX_DAYS: i64 = 90;
const LEAD_DAYS: i64 = 180;
/// Inventory is held for an unpaid checkout this long (Stripe's minimum session is 30 minutes).
const CHECKOUT_MINUTES: i64 = 31;
/// A paid campaign whose ad is never approved is refunded in full after this many days.
const UNAPPROVED_DAYS: i64 = 30;
const VIDEO_MIN_MS: i64 = 5000;
const VIDEO_MAX_MS: i64 = 30_500;
/// Under nginx's 12 MB request limit with room for the form fields.
const UPLOAD_LIMIT: usize = 11 * 1024 * 1024;

fn formats(placement: &str) -> &'static [&'static str] {
    match placement {
        "break" => &["video_landscape"],
        "feed" => &["video_portrait", "image_portrait"],
        _ => &["image_landscape"],
    }
}
fn today() -> NaiveDate {
    Utc::now().date_naive()
}
/// Whole cents for `impressions` at `cpm` cents per 1,000, rounded up.
pub fn price(impressions: i64, cpm: i64) -> i64 {
    (impressions * cpm + 999) / 1000
}
/// The undelivered part of a campaign's payment, rounded down so a refund never exceeds it.
pub fn undelivered_refund(amount: i64, impressions: i64, delivered: i64) -> i64 {
    amount * (impressions - delivered).max(0) / impressions.max(1)
}

// ---- Advertisers ----

async fn eligible(db: &mut PgConnection, user: &crate::auth::User) -> Res<Option<&'static str>> {
    if !user.email_verified {
        return Ok(Some("Verify your email address to advertise."));
    }
    if !user.mfa_enabled {
        return Ok(Some("Turn on an authenticator to advertise."));
    }
    if !crate::support::age(db, &user.id)
        .await?
        .is_some_and(|a| a >= 18)
    {
        return Ok(Some("Advertising is for adults (18 or older)."));
    }
    Ok(None)
}
async fn advertiser_user(app: &App, jar: &CookieJar) -> Res<crate::auth::User> {
    let user = profiles::signed_in(app, jar).await?;
    if let Some(reason) = eligible(&mut *app.db.acquire().await?, &user).await? {
        return Err(Fail::denied(reason));
    }
    Ok(user)
}
async fn advertiser(app: &App, jar: &CookieJar) -> Res<crate::auth::User> {
    let user = advertiser_user(app, jar).await?;
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM advertisers WHERE user_id=$1)")
            .bind(&user.id)
            .fetch_one(&app.db)
            .await?;
    if !known {
        return Err(Fail::conflict("Add your business details first."));
    }
    Ok(user)
}

#[derive(Deserialize)]
struct Account {
    name: String,
    website: String,
    country: String,
}
/// PUT /api/advertise/account: the business behind the ads.
async fn account(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Account>,
) -> Res<Json<Value>> {
    let user = advertiser_user(&app, &jar).await?;
    let name = text::plain(&input.name, "name", 1, 80, 0, true)?;
    let website = text::website_url(&input.website, "website")?;
    let country = input.country.trim().to_ascii_uppercase();
    if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Fail::field("country", "Choose a country."));
    }
    sqlx::query("INSERT INTO advertisers(user_id,name,website,country) VALUES($1,$2,$3,$4) ON CONFLICT(user_id) DO UPDATE SET name=$2,website=$3,country=$4,updated_at=now()")
        .bind(&user.id).bind(&name).bind(&website).bind(&country)
        .execute(&app.db).await?;
    Ok(Json(json!({"saved": true})))
}

// ---- Ads ----

#[derive(sqlx::FromRow)]
struct Creative {
    id: String,
    advertiser_id: String,
    format: String,
    stored: String,
    duration_ms: Option<i32>,
    title: String,
    link: String,
    status: String,
    teen_safe: bool,
    review_note: String,
    created_at: chrono::DateTime<Utc>,
}
const CREATIVE: &str = "SELECT id,advertiser_id,format,stored,duration_ms,title,link,status,teen_safe,review_note,created_at FROM ad_creatives";
fn creative_json(app: &App, c: &Creative) -> Value {
    let prefix = media::stored_prefix(&c.stored);
    let (picture, video) = if c.format.starts_with("video") {
        (
            profiles::media_url(app, &format!("{prefix}/poster.webp")),
            Some(profiles::media_url(app, &format!("{prefix}/video.mp4"))),
        )
    } else {
        let largest = c.stored.split_once('@').map_or("1280", |(_, w)| w);
        (
            profiles::media_url(app, &format!("{prefix}/{largest}.webp")),
            None,
        )
    };
    json!({"id": c.id, "format": c.format, "title": c.title, "link": c.link, "status": c.status,
        "teen_safe": c.teen_safe, "review_note": c.review_note, "duration_ms": c.duration_ms,
        "picture": picture, "video": video, "created_at": c.created_at})
}

/// Removes the scratch directory however processing ends.
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn ffmpeg(args: &[String]) -> Res<()> {
    let run = tokio::process::Command::new("ffmpeg")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status();
    match tokio::time::timeout(std::time::Duration::from_secs(120), run).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        _ => Err(Fail::field("file", "That video couldn't be processed.")),
    }
}
/// Normalizes an uploaded video ad: 5–30 s at 16:9 or 9:16, re-encoded to H.264/AAC at 720p
/// with the metadata stripped, plus a poster frame. Type and length come from the server's own
/// probe, never the browser.
async fn video(bytes: Vec<u8>, portrait: bool) -> Res<(media::Processed, i64)> {
    use sha2::{Digest, Sha256};
    let dir = std::env::temp_dir().join(format!("sver-ad-{}", profiles::new_id()));
    std::fs::create_dir_all(&dir).map_err(|_| Fail::internal())?;
    let scratch = Scratch(dir);
    let (source, out, poster) = (
        scratch.0.join("source"),
        scratch.0.join("ad.mp4"),
        scratch.0.join("poster.webp"),
    );
    tokio::fs::write(&source, &bytes)
        .await
        .map_err(|_| Fail::internal())?;
    let probe = crate::beacons::worker::probe(&source)
        .await
        .map_err(|m| Fail::field("file", m))?;
    if !(VIDEO_MIN_MS..=VIDEO_MAX_MS).contains(&probe.duration_ms) {
        return Err(Fail::field("file", "Video ads are 5 to 30 seconds long."));
    }
    let (across, down, w, h) = if portrait {
        (9, 16, 720, 1280)
    } else {
        (16, 9, 1280, 720)
    };
    let off_ratio = ((probe.width * down - probe.height * across) as f64).abs()
        > (probe.height * across) as f64 * 0.02;
    if probe.width < w || off_ratio {
        return Err(Fail::field(
            "file",
            if portrait {
                "Use a 9:16 video at least 720 by 1280 pixels."
            } else {
                "Use a 16:9 video at least 1280 by 720 pixels."
            },
        ));
    }
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"]
        .map(String::from)
        .to_vec();
    args.push(path(&source));
    args.extend(
        [
            "-t",
            "30",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-vf",
            &format!("scale={w}:{h}:flags=lanczos,setsar=1,fps=30"),
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-maxrate",
            "3M",
            "-bufsize",
            "6M",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-ac",
            "2",
            "-ar",
            "48000",
            "-map_metadata",
            "-1",
            "-movflags",
            "+faststart",
        ]
        .map(String::from),
    );
    args.push(path(&out));
    ffmpeg(&args).await?;
    let mut frame: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-ss",
        "1",
        "-i",
    ]
    .map(String::from)
    .to_vec();
    frame.push(path(&out));
    frame.extend(["-frames:v", "1", "-c:v", "libwebp"].map(String::from));
    frame.push(path(&poster));
    ffmpeg(&frame).await?;
    let (clip, still) = (
        tokio::fs::read(&out).await.map_err(|_| Fail::internal())?,
        tokio::fs::read(&poster)
            .await
            .map_err(|_| Fail::internal())?,
    );
    let decoded = image::load_from_memory(&still).map_err(|_| Fail::internal())?;
    let hash: String = Sha256::digest(&clip)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let prefix = format!("ads/{hash}");
    let fingerprints = media::removal::fingerprints(&still, &decoded);
    Ok((
        media::Processed {
            stored: prefix.clone(),
            variants: vec![
                media::Variant {
                    key: format!("{prefix}/video.mp4"),
                    bytes: clip,
                    width: w as u32,
                    height: h as u32,
                },
                media::Variant {
                    key: format!("{prefix}/poster.webp"),
                    bytes: still,
                    width: w as u32,
                    height: h as u32,
                },
            ],
            fingerprints,
        },
        probe.duration_ms.min(30_000),
    ))
}

/// POST /api/advertise/creatives (multipart: format, title, link, file): a new ad, waiting for
/// staff review.
async fn upload(
    State(app): State<App>,
    jar: CookieJar,
    mut multipart: Multipart,
) -> Res<Json<Value>> {
    let user = advertiser(&app, &jar).await?;
    let mut file = None;
    let mut fields = std::collections::HashMap::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| Fail::bad("The upload could not be read."))?
    {
        let name = field.name().unwrap_or_default().to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|_| Fail::bad("The upload could not be read."))?;
        if name == "file" {
            file = Some(bytes.to_vec());
        } else if bytes.len() <= 4096 {
            fields.insert(name, String::from_utf8_lossy(&bytes).trim().to_string());
        }
    }
    let field = |name: &str| fields.get(name).cloned().unwrap_or_default();
    let format = field("format");
    if !PLACEMENTS
        .iter()
        .any(|p| formats(p).contains(&format.as_str()))
    {
        return Err(Fail::field("format", "Choose the kind of ad."));
    }
    let title = text::plain(&field("title"), "title", 1, 60, 0, true)?;
    let link = text::website_url(&field("link"), "link")?;
    let file = file
        .filter(|f| !f.is_empty())
        .ok_or_else(|| Fail::field("file", "Choose the ad's file."))?;
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Uploads aren't available yet."));
    }
    profiles::rate(&app, format!("ad-upload:{}", user.id), 20, 3600).await?;
    let (processed, kind, duration) = match format.as_str() {
        "image_landscape" | "image_portrait" => {
            let kind = if format == "image_landscape" {
                media::Kind::AdLandscape
            } else {
                media::Kind::AdPortrait
            };
            if file.len() > kind.max_bytes() {
                return Err(Fail::field("file", "Ad pictures can be up to 5 MB."));
            }
            (media::process_async(file, kind, None).await?, kind, None)
        }
        _ => {
            if file.len() > media::Kind::AdVideo.max_bytes() {
                return Err(Fail::field("file", "Ad videos can be up to 11 MB."));
            }
            let (processed, duration) = video(file, format == "video_portrait").await?;
            (processed, media::Kind::AdVideo, Some(duration as i32))
        }
    };
    media::store(&app, &processed).await?;
    let id = profiles::new_id();
    let mut tx = app.db.begin().await?;
    media::record(&mut tx, &user.id, kind, &processed).await?;
    sqlx::query("INSERT INTO ad_creatives(id,advertiser_id,format,stored,duration_ms,title,link) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(&id).bind(&user.id).bind(&format).bind(&processed.stored).bind(duration).bind(&title).bind(&link)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    let creative: Creative = sqlx::query_as(sqlx::AssertSqlSafe(format!("{CREATIVE} WHERE id=$1")))
        .bind(&id)
        .fetch_one(&app.db)
        .await?;
    Ok(Json(json!({"creative": creative_json(&app, &creative)})))
}

// ---- Inventory and quotes ----

#[derive(sqlx::FromRow)]
struct Card {
    placement: String,
    cpm_cents: Option<i32>,
    min_spend_cents: i32,
    daily_estimate: i32,
}
async fn card(db: &mut PgConnection, placement: &str) -> Res<Card> {
    sqlx::query_as("SELECT placement,cpm_cents,min_spend_cents,daily_estimate FROM ad_rate_card WHERE placement=$1")
        .bind(placement)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| Fail::field("placement", "Choose where the ad runs."))
}

#[derive(Deserialize, Clone)]
pub struct Want {
    placement: String,
    impressions: i64,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default = "all")]
    audience: String,
}
fn all() -> String {
    "all".into()
}
async fn check(db: &mut PgConnection, want: &mut Want) -> Res<Card> {
    let card = card(db, &want.placement).await?;
    if !(MIN_IMPRESSIONS..=MAX_IMPRESSIONS).contains(&want.impressions) {
        return Err(Fail::field(
            "impressions",
            "Buy 1,000 to 10,000,000 impressions.",
        ));
    }
    let today = today();
    if want.starts_on < today
        || want.starts_on > today + Duration::days(LEAD_DAYS)
        || want.ends_on < want.starts_on
        || (want.ends_on - want.starts_on).num_days() >= MAX_DAYS
    {
        return Err(Fail::field(
            "dates",
            "Start today or later (within 180 days) and run up to 90 days.",
        ));
    }
    if !matches!(want.audience.as_str(), "all" | "adult") {
        return Err(Fail::field("audience", "Choose who sees the ad."));
    }
    want.genres.sort();
    want.genres.dedup();
    want.languages.sort();
    want.languages.dedup();
    if want.genres.len() > 20 || want.languages.len() > 10 {
        return Err(Fail::field(
            "targeting",
            "Choose fewer genres or languages.",
        ));
    }
    for genre in &want.genres {
        crate::factions::validate_genre(db, genre).await?;
    }
    if want
        .languages
        .iter()
        .any(|l| !crate::streams::LANGUAGES.contains(&l.as_str()))
    {
        return Err(Fail::field("languages", "Choose languages from the list."));
    }
    Ok(card)
}
/// The forecast of verified impressions a day for this targeting. Until delivery history exists
/// (build step 2), it's staff's starting estimate for the placement, narrowed by the share of the
/// last 14 days' broadcast time in the chosen genres and languages, and by the share of adults.
// ponytail: genre, language and age shares are treated as independent and use each channel's
// current category; replace with measured per-targeting delivery once impressions are recorded.
async fn forecast(db: &mut PgConnection, card: &Card, want: &Want) -> Res<f64> {
    let mut daily = f64::from(card.daily_estimate);
    let share = |matched: Option<f64>| matched.unwrap_or(0.0).clamp(0.0, 1.0);
    const MINUTES: &str = "extract(epoch FROM least(coalesce(b.ended_at,now()),now())-greatest(b.started_at,now()-interval '14 days'))";
    if !want.genres.is_empty() {
        let sql = format!(
            "SELECT (sum({MINUTES}) FILTER (WHERE c.genre=ANY($1)) / nullif(sum({MINUTES}),0))::float8 FROM broadcasts b LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id WHERE b.started_at IS NOT NULL AND coalesce(b.ended_at,now())>now()-interval '14 days'"
        );
        let matched: Option<f64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(&want.genres)
            .fetch_one(&mut *db)
            .await?;
        daily *= share(matched);
    }
    if !want.languages.is_empty() {
        let sql = format!(
            "SELECT (sum({MINUTES}) FILTER (WHERE s.language=ANY($1)) / nullif(sum({MINUTES}),0))::float8 FROM broadcasts b LEFT JOIN stream_settings s ON s.owner_id=b.owner_id WHERE b.started_at IS NOT NULL AND coalesce(b.ended_at,now())>now()-interval '14 days'"
        );
        let matched: Option<f64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(&want.languages)
            .fetch_one(&mut *db)
            .await?;
        daily *= share(matched);
    }
    if want.audience == "adult" {
        let matched: Option<f64> = sqlx::query_scalar("SELECT (count(*) FILTER (WHERE date_of_birth<=current_date-interval '18 years')::float8 / nullif(count(*),0))::float8 FROM users WHERE date_of_birth IS NOT NULL AND deleted_at IS NULL")
            .fetch_one(&mut *db)
            .await?;
        daily *= share(matched);
    }
    Ok(daily)
}
/// Impressions still owed by every live, paid or held campaign in the placement:
/// (remaining, first day, last day). An overdue campaign's whole remainder falls on today.
async fn commitments(
    db: &mut PgConnection,
    placement: &str,
    except: Option<&str>,
) -> Res<Vec<(i64, NaiveDate, NaiveDate)>> {
    let rows: Vec<(i64, NaiveDate, NaiveDate)> = sqlx::query_as("SELECT (impressions-delivered)::bigint,starts_on,ends_on FROM ad_campaigns WHERE placement=$1 AND (status IN ('paid','running') OR (status='checkout' AND hold_until>now())) AND id IS DISTINCT FROM $2")
        .bind(placement)
        .bind(except)
        .fetch_all(db)
        .await?;
    let today = today();
    Ok(rows
        .into_iter()
        .map(|(left, from, to)| (left.max(0), from.max(today), to.max(today)))
        .collect())
}
/// Impressions already promised for days `from..=to`, spreading each campaign evenly over its
/// days. Every promise counts against any targeting.
// ponytail: commitments ignore targeting (conservative: never oversells, may undersell niche
// targeting); split them by genre/language once delivery is measured per targeting.
fn committed(promises: &[(i64, NaiveDate, NaiveDate)], from: NaiveDate, to: NaiveDate) -> f64 {
    promises
        .iter()
        .map(|(left, start, end)| {
            let days = (*end - *start).num_days() + 1;
            let overlap = ((to.min(*end) - from.max(*start)).num_days() + 1).max(0);
            *left as f64 * overlap as f64 / days as f64
        })
        .sum()
}
/// Impressions that can still be sold for `from..=to` at `daily` forecast impressions a day.
pub fn sellable(
    daily: f64,
    promises: &[(i64, NaiveDate, NaiveDate)],
    from: NaiveDate,
    to: NaiveDate,
) -> i64 {
    let from = from.max(today());
    if to < from {
        return 0;
    }
    let days = (to - from).num_days() + 1;
    (daily * days as f64 * SELLABLE - committed(promises, from, to))
        .floor()
        .max(0.0) as i64
}
/// The quote: price, what fits and, when it doesn't, the soonest start that fits the full amount.
async fn quote_for(db: &mut PgConnection, want: &mut Want, except: Option<&str>) -> Res<Value> {
    let card = check(db, want).await?;
    let Some(cpm) = card.cpm_cents else {
        return Err(Fail::unavailable("This placement isn't for sale yet."));
    };
    let daily = forecast(db, &card, want).await?;
    let promises = commitments(db, &want.placement, except).await?;
    let available = sellable(daily, &promises, want.starts_on, want.ends_on);
    let span = want.ends_on - want.starts_on;
    let soonest = (available < want.impressions)
        .then(|| {
            (0..=LEAD_DAYS)
                .map(|d| today() + Duration::days(d))
                .find(|start| sellable(daily, &promises, *start, *start + span) >= want.impressions)
        })
        .flatten();
    let amount = price(want.impressions, cpm.into());
    Ok(json!({
        "placement": card.placement, "impressions": want.impressions, "cpm_cents": cpm,
        "amount_cents": amount.max(card.min_spend_cents.into()), "min_spend_cents": card.min_spend_cents,
        "below_minimum": amount < i64::from(card.min_spend_cents),
        "available": available, "fits": available >= want.impressions,
        "soonest_start": soonest, "starts_on": want.starts_on, "ends_on": want.ends_on,
    }))
}

/// POST /api/advertise/quote
async fn quote(
    State(app): State<App>,
    jar: CookieJar,
    Json(mut want): Json<Want>,
) -> Res<Json<Value>> {
    advertiser(&app, &jar).await?;
    Ok(Json(
        quote_for(&mut *app.db.acquire().await?, &mut want, None).await?,
    ))
}

// ---- Campaigns ----

#[derive(sqlx::FromRow)]
struct Campaign {
    id: String,
    advertiser_id: Option<String>,
    creative_id: Option<String>,
    name: String,
    placement: String,
    genres: Vec<String>,
    languages: Vec<String>,
    audience: String,
    impressions: i32,
    delivered: i32,
    cpm_cents: i32,
    amount_cents: i32,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
    status: String,
    ending: Option<String>,
    refund_cents: i32,
    checkout_id: Option<String>,
    payment_intent: Option<String>,
    hold_until: Option<chrono::DateTime<Utc>>,
    paid_at: Option<chrono::DateTime<Utc>>,
    created_at: chrono::DateTime<Utc>,
}
const CAMPAIGN: &str = "SELECT id,advertiser_id,creative_id,name,placement,genres,languages,audience,impressions,delivered,cpm_cents,amount_cents,starts_on,ends_on,status,ending,refund_cents,checkout_id,payment_intent,hold_until,paid_at,created_at FROM ad_campaigns";
async fn load(db: &mut PgConnection, id: &str, lock: bool) -> Res<Campaign> {
    let sql = format!(
        "{CAMPAIGN} WHERE id=$1{}",
        if lock { " FOR UPDATE" } else { "" }
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(Fail::missing)
}
fn can_withdraw(c: &Campaign) -> bool {
    matches!(c.status.as_str(), "paid" | "running")
        && today() > c.ends_on
        && c.delivered < c.impressions
}
fn can_cancel(c: &Campaign) -> bool {
    c.status == "checkout" || (c.status == "paid" && c.delivered == 0)
}
fn campaign_json(c: &Campaign, creative: Option<&Value>) -> Value {
    json!({"id": c.id, "name": c.name, "placement": c.placement, "genres": c.genres,
        "languages": c.languages, "audience": c.audience, "impressions": c.impressions,
        "delivered": c.delivered, "cpm_cents": c.cpm_cents, "amount_cents": c.amount_cents,
        "refund_cents": c.refund_cents, "starts_on": c.starts_on, "ends_on": c.ends_on,
        "status": c.status, "ending": c.ending, "creative_id": c.creative_id,
        "creative": creative, "hold_until": c.hold_until, "paid_at": c.paid_at,
        "created_at": c.created_at, "can_cancel": can_cancel(c), "can_withdraw": can_withdraw(c)})
}
async fn own_creative(
    db: &mut PgConnection,
    user: &str,
    id: &str,
    placement: &str,
) -> Res<Creative> {
    let creative: Creative = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{CREATIVE} WHERE id=$1 AND advertiser_id=$2"
    )))
    .bind(id)
    .bind(user)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| Fail::field("creative_id", "Choose one of your ads."))?;
    if creative.status == "retired" || !formats(placement).contains(&creative.format.as_str()) {
        return Err(Fail::field(
            "creative_id",
            "That ad doesn't fit this placement.",
        ));
    }
    Ok(creative)
}

#[derive(Deserialize)]
struct NewCampaign {
    name: String,
    creative_id: String,
    #[serde(flatten)]
    want: Want,
}
/// POST /api/advertise/campaigns: reserves the quoted inventory for a checkout (31 minutes) and
/// starts Stripe Checkout. Refused when the forecast can't cover it.
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewCampaign>,
) -> Res<Json<Value>> {
    let user = advertiser(&app, &jar).await?;
    let name = text::plain(&input.name, "name", 1, 60, 0, true)?;
    let mut want = input.want;
    sec::reserve(&app, vec![format!("ad-checkout:{}", user.id)], 10, 3600).await?;
    let mut tx = app.db.begin().await?;
    switches::guard(&mut tx, "ad_sales").await?;
    let creative = own_creative(&mut tx, &user.id, &input.creative_id, &want.placement).await?;
    // One sale at a time per placement, so two checkouts can't both take the last impressions.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ad_inventory:'||$1,0))")
        .bind(&want.placement)
        .execute(&mut *tx)
        .await?;
    let quote = quote_for(&mut tx, &mut want, None).await?;
    if quote["fits"] != true {
        return Err(Fail::conflict(match quote["soonest_start"].as_str() {
            Some(start) => format!(
                "Only {} impressions are available for those dates. The full amount fits starting {start}.",
                quote["available"]
            ),
            None => format!(
                "Only {} impressions are available for those dates.",
                quote["available"]
            ),
        }));
    }
    let amount = quote["amount_cents"].as_i64().ok_or_else(Fail::internal)?;
    let cpm = quote["cpm_cents"].as_i64().ok_or_else(Fail::internal)?;
    let id = profiles::new_id();
    let origin = &app.config.origin;
    let expires = Utc::now().timestamp() + CHECKOUT_MINUTES * 60;
    let session = stripe::post(
        &app.http,
        &app.config.stripe,
        "checkout/sessions",
        &[
            ("mode", "payment".into()),
            ("line_items[0][quantity]", "1".into()),
            ("line_items[0][price_data][currency]", "usd".into()),
            ("line_items[0][price_data][unit_amount]", amount.to_string()),
            (
                "line_items[0][price_data][product_data][name]",
                format!(
                    "S.V.E.R ad campaign: {name} ({} verified impressions)",
                    want.impressions
                ),
            ),
            ("client_reference_id", user.id.clone()),
            ("metadata[sver_kind]", "ad".into()),
            ("payment_intent_data[metadata][sver_user]", user.id.clone()),
            ("payment_intent_data[receipt_email]", user.email.clone()),
            ("expires_at", expires.to_string()),
            ("success_url", format!("{origin}/advertise?campaign={id}")),
            ("cancel_url", format!("{origin}/advertise")),
        ],
        Some(&format!("ad-checkout-{id}")),
    )
    .await?;
    let (Some(session_id), Some(url)) = (session["id"].as_str(), session["url"].as_str()) else {
        return Err(Fail::unavailable("Stripe sent an unexpected response."));
    };
    sqlx::query("INSERT INTO ad_campaigns(id,advertiser_id,creative_id,name,placement,genres,languages,audience,impressions,cpm_cents,amount_cents,starts_on,ends_on,status,checkout_id,hold_until) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'checkout',$14,now()+make_interval(mins=>$15))")
        .bind(&id).bind(&user.id).bind(&creative.id).bind(&name).bind(&want.placement).bind(&want.genres).bind(&want.languages)
        .bind(&want.audience).bind(want.impressions as i32).bind(cpm as i32).bind(amount as i32).bind(want.starts_on).bind(want.ends_on)
        .bind(session_id).bind(CHECKOUT_MINUTES as i32)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,detail) VALUES($1,$2,'ad',$3,$4)")
        .bind(session_id).bind(&user.id).bind(amount as i32).bind(json!({"campaign": id}))
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"url": url, "campaign": id})))
}

/// A paid ad checkout (from the Stripe webhook, in its transaction). A payment that arrives after
/// its hold lapsed keeps the campaign if the inventory is still there, and is refunded otherwise.
pub(crate) async fn paid(
    tx: &mut PgConnection,
    session: &str,
    detail: &Value,
    intent: Option<&str>,
) -> Res<()> {
    let Some(id) = detail["campaign"].as_str() else {
        return Ok(());
    };
    let campaign = load(tx, id, true).await?;
    if campaign.checkout_id.as_deref() != Some(session) {
        return Ok(());
    }
    let status = match campaign.status.as_str() {
        "checkout" => "paid",
        "expired" | "cancelled" => {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ad_inventory:'||$1,0))")
                .bind(&campaign.placement)
                .execute(&mut *tx)
                .await?;
            let mut want = Want {
                placement: campaign.placement.clone(),
                impressions: campaign.impressions.into(),
                starts_on: campaign.starts_on.max(today()),
                ends_on: campaign.ends_on.max(today()),
                genres: campaign.genres.clone(),
                languages: campaign.languages.clone(),
                audience: campaign.audience.clone(),
            };
            let card = card(tx, &want.placement).await?;
            let daily = forecast(tx, &card, &want).await?;
            let promises = commitments(tx, &want.placement, Some(id)).await?;
            want.ends_on = want.ends_on.max(want.starts_on);
            if campaign.status == "expired"
                && sellable(daily, &promises, want.starts_on, want.ends_on) >= want.impressions
            {
                "paid"
            } else {
                "refund_due"
            }
        }
        _ => return Ok(()),
    };
    sqlx::query("UPDATE ad_campaigns SET status=$2,ending=CASE WHEN $2='refund_due' THEN 'cancelled' END,refund_cents=CASE WHEN $2='refund_due' THEN amount_cents ELSE 0 END,paid_at=now(),payment_intent=$3,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(status)
        .bind(intent)
        .execute(tx)
        .await?;
    Ok(())
}

/// The signed-in advertiser's own campaign, locked.
async fn mine(
    app: &App,
    jar: &CookieJar,
    id: &str,
) -> Res<(
    crate::auth::User,
    sqlx::Transaction<'static, sqlx::Postgres>,
    Campaign,
)> {
    let user = advertiser(app, jar).await?;
    let mut tx = app.db.begin().await?;
    let campaign = load(&mut tx, id, true).await?;
    if campaign.advertiser_id.as_deref() != Some(&user.id) {
        return Err(Fail::missing());
    }
    Ok((user, tx, campaign))
}
/// A full refund or a lost chargeback made outside the campaign's own flow (Stripe's dashboard,
/// `/admin/money`, a dispute): the campaign stops, and a refund it still owed isn't sent again.
/// Called from support.rs.
pub(crate) async fn taken_back(tx: &mut PgConnection, intent: &str) -> Res<()> {
    sqlx::query("UPDATE ad_campaigns SET status=CASE WHEN status='refund_due' THEN coalesce(ending,'cancelled') ELSE 'cancelled' END,ending=coalesce(ending,'cancelled'),refund_cents=amount_cents,updated_at=now() WHERE payment_intent=$1 AND status IN ('paid','running','refund_due')")
        .bind(intent)
        .execute(tx)
        .await?;
    Ok(())
}
async fn owe(db: &mut PgConnection, id: &str, cents: i64, ending: &str) -> Res<()> {
    sqlx::query("UPDATE ad_campaigns SET status='refund_due',refund_cents=$2,ending=$3,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(cents as i32)
        .bind(ending)
        .execute(db)
        .await?;
    Ok(())
}

/// POST /api/advertise/campaigns/{id}/cancel: before anything is delivered, the campaign stops and
/// is refunded in full (an unpaid checkout just ends).
async fn cancel(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let (user, mut tx, campaign) = mine(&app, &jar, &id).await?;
    if !can_cancel(&campaign) {
        return Err(Fail::conflict(
            "This campaign has started. After its end date you can withdraw it and get the undelivered part back.",
        ));
    }
    if campaign.status == "checkout" {
        sqlx::query("UPDATE ad_campaigns SET status='cancelled',updated_at=now() WHERE id=$1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        if let Some(session) = &campaign.checkout_id {
            // Best effort: a payment that still completes is refunded by `paid`.
            let _ = stripe::post(
                &app.http,
                &app.config.stripe,
                &format!("checkout/sessions/{session}/expire"),
                &[],
                None,
            )
            .await;
        }
    } else {
        owe(&mut tx, &id, campaign.amount_cents.into(), "cancelled").await?;
    }
    safety::audit(
        &mut tx,
        Some(&user.id),
        "ad_campaign_cancel",
        "ad_campaign",
        &id,
        &[],
        "Advertiser cancelled",
        json!({"status": campaign.status}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"cancelled": true})))
}

/// POST /api/advertise/campaigns/{id}/withdraw: after the end date, an unfinished campaign stops
/// and the undelivered impressions are refunded.
async fn withdraw(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let (user, mut tx, campaign) = mine(&app, &jar, &id).await?;
    if !can_withdraw(&campaign) {
        return Err(Fail::conflict(
            "A campaign can be withdrawn after its end date while it's unfinished.",
        ));
    }
    let refund = undelivered_refund(
        campaign.amount_cents.into(),
        campaign.impressions.into(),
        campaign.delivered.into(),
    );
    owe(&mut tx, &id, refund, "withdrawn").await?;
    safety::audit(&mut tx, Some(&user.id), "ad_campaign_withdraw", "ad_campaign", &id, &[], "Advertiser withdrew after the end date", json!({"delivered": campaign.delivered, "impressions": campaign.impressions, "refund_cents": refund}), false).await?;
    tx.commit().await?;
    Ok(Json(json!({"withdrawn": true, "refund_cents": refund})))
}

#[derive(Deserialize)]
struct Swap {
    creative_id: String,
}
/// POST /api/advertise/campaigns/{id}/creative: another of the advertiser's ads for a campaign
/// that hasn't started (say, after a rejection).
async fn swap(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Swap>,
) -> Res<Json<Value>> {
    let (user, mut tx, campaign) = mine(&app, &jar, &id).await?;
    if !matches!(campaign.status.as_str(), "checkout" | "paid") {
        return Err(Fail::conflict("This campaign's ad can't change now."));
    }
    let creative = own_creative(&mut tx, &user.id, &input.creative_id, &campaign.placement).await?;
    sqlx::query("UPDATE ad_campaigns SET creative_id=$2,updated_at=now() WHERE id=$1")
        .bind(&id)
        .bind(&creative.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

/// GET /api/advertise: the page's state for the signed-in person.
async fn overview(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let blocked = eligible(&mut db, &user).await?;
    let account: Option<(String, String, String)> =
        sqlx::query_as("SELECT name,website,country FROM advertisers WHERE user_id=$1")
            .bind(&user.id)
            .fetch_optional(&mut *db)
            .await?;
    let cards: Vec<(String, Option<i32>, i32)> = sqlx::query_as(
        "SELECT placement,cpm_cents,min_spend_cents FROM ad_rate_card ORDER BY placement",
    )
    .fetch_all(&mut *db)
    .await?;
    let creatives: Vec<Creative> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{CREATIVE} WHERE advertiser_id=$1 AND status<>'retired' ORDER BY created_at DESC LIMIT 100"
    )))
    .bind(&user.id)
    .fetch_all(&mut *db)
    .await?;
    let creative_values: Vec<Value> = creatives.iter().map(|c| creative_json(&app, c)).collect();
    let campaigns: Vec<Campaign> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{CAMPAIGN} WHERE advertiser_id=$1 AND status NOT IN ('expired') ORDER BY created_at DESC LIMIT 100"
    )))
    .bind(&user.id)
    .fetch_all(&mut *db)
    .await?;
    let campaigns: Vec<Value> = campaigns
        .iter()
        .map(|c| {
            let creative = c.creative_id.as_ref().and_then(|id| {
                creatives
                    .iter()
                    .position(|x| &x.id == id)
                    .map(|i| &creative_values[i])
            });
            campaign_json(c, creative)
        })
        .collect();
    Ok(Json(json!({
        "blocked": blocked,
        "account": account.map(|(name, website, country)| json!({"name": name, "website": website, "country": country})),
        "rate_card": cards.into_iter().map(|(p, cpm, min)| json!({"placement": p, "cpm_cents": cpm, "min_spend_cents": min, "formats": formats(&p)})).collect::<Vec<_>>(),
        "creatives": creative_values,
        "campaigns": campaigns,
        "sales_open": !switches::off(&mut db, "ad_sales").await?,
    })))
}

// ---- Jobs ----

/// Ends lapsed checkouts, refunds paid campaigns whose ad was never approved, and issues the
/// refunds owed. Called by the jobs loop.
pub async fn tick(app: &App) -> Res<()> {
    sqlx::query("UPDATE ad_campaigns SET status='expired',updated_at=now() WHERE status='checkout' AND hold_until<=now()")
        .execute(&app.db)
        .await?;
    let stale: Vec<(String, Option<String>)> = // UNAPPROVED_DAYS is a constant; no input enters this statement.
    sqlx::query_as(sqlx::AssertSqlSafe(format!("UPDATE ad_campaigns c SET status='refund_due',ending='cancelled',refund_cents=amount_cents,updated_at=now() WHERE status='paid' AND delivered=0 AND paid_at<now()-interval '{UNAPPROVED_DAYS} days' AND NOT EXISTS(SELECT 1 FROM ad_creatives a WHERE a.id=c.creative_id AND a.status='approved') RETURNING id,advertiser_id")))
        .fetch_all(&app.db)
        .await?;
    for (id, advertiser) in stale {
        if let Some(user) = advertiser {
            let mut db = app.db.acquire().await?;
            safety::queue_notice(app, &mut db, &user, "Your S.V.E.R ad campaign was refunded", &format!("Your campaign {id} had no approved ad {UNAPPROVED_DAYS} days after payment, so it was cancelled and refunded in full.")).await?;
        }
    }
    let due: Vec<(String, i32, Option<String>, Option<String>)> = sqlx::query_as("SELECT id,refund_cents,payment_intent,ending FROM ad_campaigns WHERE status='refund_due' ORDER BY updated_at LIMIT 20")
        .fetch_all(&app.db)
        .await?;
    // Each refund stands alone: one Stripe refuses (or that is down) never holds up the rest.
    for (id, cents, intent, ending) in due {
        if cents > 0
            && let Some(intent) = &intent
        {
            let refunded = stripe::refund(
                &app.http,
                &app.config.stripe,
                &[
                    ("payment_intent", intent.clone()),
                    ("amount", cents.to_string()),
                ],
                &format!("ad-refund-{id}"),
            )
            .await;
            if !refunded {
                eprintln!("adnet_event=refund outcome=retry");
                continue;
            }
        }
        sqlx::query("UPDATE ad_campaigns SET status=$2,updated_at=now() WHERE id=$1 AND status='refund_due'")
            .bind(&id)
            .bind(ending.as_deref().unwrap_or("cancelled"))
            .execute(&app.db)
            .await?;
    }
    Ok(())
}

// ---- Staff ----

/// GET /api/admin/adnet: the rate card, ads waiting for review, campaigns and the next 30 days of
/// inventory per placement (forecast, promised, sellable).
async fn admin(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let cards: Vec<Card> = sqlx::query_as(
        "SELECT placement,cpm_cents,min_spend_cents,daily_estimate FROM ad_rate_card ORDER BY placement",
    )
    .fetch_all(&mut *db)
    .await?;
    let mut inventory = Vec::new();
    for card in &cards {
        let promises = commitments(&mut db, &card.placement, None).await?;
        let days: Vec<Value> = (0..30)
            .map(|d| {
                let day = today() + Duration::days(d);
                json!({"day": day, "forecast": card.daily_estimate,
                    "promised": committed(&promises, day, day).round() as i64,
                    "sellable": sellable(card.daily_estimate.into(), &promises, day, day)})
            })
            .collect();
        inventory.push(json!({"placement": card.placement, "days": days}));
    }
    let queue: Vec<Creative> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{CREATIVE} WHERE status='review' ORDER BY created_at LIMIT 50"
    )))
    .fetch_all(&mut *db)
    .await?;
    let mut review = Vec::new();
    for c in &queue {
        let mut row = creative_json(&app, c);
        row["advertiser"] = sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('name',a.name,'website',a.website,'username',u.username) FROM advertisers a JOIN users u ON u.id=a.user_id WHERE a.user_id=$1")
            .bind(&c.advertiser_id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or(Value::Null);
        review.push(row);
    }
    let campaigns: Vec<Campaign> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{CAMPAIGN} WHERE status<>'expired' ORDER BY created_at DESC LIMIT 200"
    )))
    .fetch_all(&mut *db)
    .await?;
    let mut rows = Vec::new();
    for c in &campaigns {
        let mut row = campaign_json(c, None);
        row["advertiser"] = json!(match &c.advertiser_id {
            Some(id) =>
                sqlx::query_scalar::<_, String>("SELECT name FROM advertisers WHERE user_id=$1")
                    .bind(id)
                    .fetch_optional(&mut *db)
                    .await?,
            None => None,
        });
        row["payment_intent"] = json!(c.payment_intent);
        rows.push(row);
    }
    Ok(Json(json!({
        "rate_card": cards.iter().map(|c| json!({"placement": c.placement, "cpm_cents": c.cpm_cents, "min_spend_cents": c.min_spend_cents, "daily_estimate": c.daily_estimate})).collect::<Vec<_>>(),
        "inventory": inventory, "review": review, "campaigns": rows,
        "sales_open": !switches::off(&mut db, "ad_sales").await?,
    })))
}

#[derive(Deserialize)]
struct RateInput {
    cpm_cents: Option<i32>,
    min_spend_cents: i32,
    daily_estimate: i32,
}
#[derive(Deserialize)]
struct RateCard {
    placements: std::collections::HashMap<String, RateInput>,
    note: Option<String>,
}
/// PUT /api/admin/adnet/rate-card: CPM (empty takes a placement off sale), minimum spend and the
/// starting daily estimate per placement. Existing campaigns keep the price they paid.
async fn rate_card(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<RateCard>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let before: Value =
        sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(r) ORDER BY placement) FROM ad_rate_card r")
            .fetch_one(&mut *tx)
            .await?;
    for (placement, rate) in &input.placements {
        if !PLACEMENTS.contains(&placement.as_str()) {
            return Err(Fail::field("placements", "Unknown placement."));
        }
        if rate.cpm_cents.is_some_and(|c| !(50..=100_000).contains(&c))
            || !(100..=10_000_000).contains(&rate.min_spend_cents)
            || !(0..=100_000_000).contains(&rate.daily_estimate)
        {
            return Err(Fail::field(
                "placements",
                "CPM is $0.50–$1,000, minimum spend $1–$100,000, and the estimate 0 or more.",
            ));
        }
        sqlx::query("UPDATE ad_rate_card SET cpm_cents=$2,min_spend_cents=$3,daily_estimate=$4,updated_at=now() WHERE placement=$1")
            .bind(placement).bind(rate.cpm_cents).bind(rate.min_spend_cents).bind(rate.daily_estimate)
            .execute(&mut *tx).await?;
    }
    let after: Value =
        sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(r) ORDER BY placement) FROM ad_rate_card r")
            .fetch_one(&mut *tx)
            .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "ad_rate_card",
        "ad_rate_card",
        "rate_card",
        &[],
        &note,
        json!({"before": before, "after": after}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

#[derive(Deserialize)]
struct Review {
    action: String,
    note: Option<String>,
}
/// POST /api/admin/adnet/creatives/{id}/review: approve (18+ only), approve as teen-safe, or
/// reject with a reason the advertiser sees.
async fn review(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Review>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", input.action == "reject")?;
    let (status, teen) = match input.action.as_str() {
        "approve" => ("approved", false),
        "approve_teen" => ("approved", true),
        "reject" => ("rejected", false),
        _ => {
            return Err(Fail::field(
                "action",
                "Choose approve, approve as teen-safe, or reject.",
            ));
        }
    };
    let mut tx = app.db.begin().await?;
    let owner: Option<String> = sqlx::query_scalar("UPDATE ad_creatives SET status=$2,teen_safe=$3,review_note=$4,reviewed_by=$5,reviewed_at=now() WHERE id=$1 AND status='review' RETURNING advertiser_id")
        .bind(&id).bind(status).bind(teen).bind(&note).bind(&staff.id)
        .fetch_optional(&mut *tx).await?;
    let Some(owner) = owner else {
        return Err(Fail::conflict("This ad isn't waiting for review."));
    };
    safety::audit(
        &mut tx,
        Some(&staff.id),
        &format!("ad_{}", input.action),
        "ad_creative",
        &id,
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    if status == "rejected" {
        safety::queue_notice(&app, &mut tx, &owner, "About your S.V.E.R ad", &format!("Your ad wasn't approved: {note}\n\nUpload a fixed version and choose it for your campaign, or cancel the campaign for a full refund, at {}/advertise.", app.config.origin)).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

#[derive(Deserialize)]
struct StaffNote {
    note: Option<String>,
}
/// POST /api/admin/adnet/campaigns/{id}/refund: stops a paid campaign and refunds what hasn't been
/// delivered.
async fn staff_refund(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<StaffNote>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let campaign = load(&mut tx, &id, true).await?;
    if !matches!(campaign.status.as_str(), "paid" | "running") {
        return Err(Fail::conflict("This campaign has no payment to refund."));
    }
    let refund = undelivered_refund(
        campaign.amount_cents.into(),
        campaign.impressions.into(),
        campaign.delivered.into(),
    );
    owe(&mut tx, &id, refund, "cancelled").await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "ad_campaign_refund",
        "ad_campaign",
        &id,
        &[],
        &note,
        json!({"refund_cents": refund, "delivered": campaign.delivered}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "refund_cents": refund})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/advertise", get(overview))
        .route("/api/advertise/account", put(account))
        .route(
            "/api/advertise/creatives",
            post(upload).layer(DefaultBodyLimit::max(UPLOAD_LIMIT + 64 * 1024)),
        )
        .route("/api/advertise/quote", post(quote))
        .route("/api/advertise/campaigns", post(create))
        .route("/api/advertise/campaigns/{id}/cancel", post(cancel))
        .route("/api/advertise/campaigns/{id}/withdraw", post(withdraw))
        .route("/api/advertise/campaigns/{id}/creative", post(swap))
        .route("/api/admin/adnet", get(admin))
        .route("/api/admin/adnet/rate-card", put(rate_card))
        .route("/api/admin/adnet/creatives/{id}/review", post(review))
        .route("/api/admin/adnet/campaigns/{id}/refund", post(staff_refund))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(days: i64) -> NaiveDate {
        today() + Duration::days(days)
    }
    #[test]
    fn prices_round_up_and_refunds_round_down() {
        assert_eq!(price(1000, 500), 500);
        assert_eq!(price(1001, 500), 501);
        assert_eq!(price(1500, 333), 500);
        assert_eq!(undelivered_refund(1000, 1000, 0), 1000);
        assert_eq!(undelivered_refund(1000, 1000, 1000), 0);
        assert_eq!(undelivered_refund(1001, 3000, 1000), 667);
    }
    #[test]
    fn only_the_unpromised_share_of_the_forecast_sells() {
        // 1,000 a day for 10 days at 70% is 7,000.
        assert_eq!(sellable(1000.0, &[], d(0), d(9)), 7000);
        // A campaign owing 2,000 over the same 10 days takes 2,000 of it.
        let one = [(2000, d(0), d(9))];
        assert_eq!(sellable(1000.0, &one, d(0), d(9)), 5000);
        // Only the overlapping days count: half its days overlap, so half its load.
        assert_eq!(sellable(1000.0, &one, d(5), d(14)), 6000);
        // An overdue campaign's remainder falls on today.
        let late = [(3000, d(0), d(0))];
        assert_eq!(sellable(1000.0, &late, d(0), d(0)), 0);
        assert_eq!(sellable(1000.0, &late, d(1), d(1)), 700);
        // Never negative, and nothing sells for past days.
        assert_eq!(sellable(10.0, &one, d(0), d(9)), 0);
        assert_eq!(sellable(1000.0, &[], d(-5), d(-1)), 0);
    }
}
