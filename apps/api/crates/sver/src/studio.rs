//! Creator Studio sections: profile song, schedule, sponsors, streaming setup, custom blocks and
//! fan art, plus their public reads (docs/PROFILES.md).
// Row tuples from runtime sqlx queries read more clearly inline than as aliases.
#![allow(clippy::type_complexity)]
use crate::{
    App, media,
    media::Kind,
    profiles::{
        CursorQuery, Fail, Res, blocked_between, bump_section, chip_sql, eligible_by_name,
        ensure_profile, ensure_unrestricted, ensure_verified, hydrate, make_cursor, media_url,
        new_id, parse_cursor, rate, section_revision, signed_in, viewer,
    },
    text,
};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::str::FromStr;

// ---------------------------------------------------------------- Profile song

#[derive(Debug, PartialEq)]
pub struct SongRef {
    pub provider: &'static str,
    /// YouTube video ID; SoundCloud track IDs come from oEmbed.
    pub media_id: Option<String>,
    pub url: String,
}
const NOT_SUPPORTED: &str = "Use a YouTube or SoundCloud link.";
/// Accepts only official YouTube video and SoundCloud track URL forms.
pub fn parse_song_url(value: &str) -> Res<SongRef> {
    let bad = || Fail::field("url", NOT_SUPPORTED);
    let value = value.trim();
    if value.len() > 2048 {
        return Err(bad());
    }
    let url = url::Url::parse(value).map_err(|_| bad())?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(bad());
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let valid_id = |id: &str| {
        id.len() == 11
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    let youtube = |id: &str| -> Res<SongRef> {
        if !valid_id(id) {
            return Err(bad());
        }
        Ok(SongRef {
            provider: "youtube",
            media_id: Some(id.to_string()),
            url: format!("https://www.youtube.com/watch?v={id}"),
        })
    };
    match host.as_str() {
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            match segments.as_slice() {
                ["watch"] => {
                    let id = url
                        .query_pairs()
                        .find(|(k, _)| k == "v")
                        .map(|(_, v)| v.to_string())
                        .ok_or_else(bad)?;
                    youtube(&id)
                }
                ["shorts", id] if host != "music.youtube.com" => youtube(id),
                _ => Err(bad()),
            }
        }
        "youtu.be" => match segments.as_slice() {
            [id] => youtube(id),
            _ => Err(bad()),
        },
        "soundcloud.com" | "www.soundcloud.com" | "m.soundcloud.com" => match segments.as_slice() {
            [user, track]
                if ![
                    "discover",
                    "stream",
                    "you",
                    "search",
                    "upload",
                    "pages",
                    "charts",
                    "settings",
                    "messages",
                    "notifications",
                    "people",
                    "tags",
                    "jobs",
                    "imprint",
                    "terms-of-use",
                ]
                .contains(user)
                    && ![
                        "sets",
                        "likes",
                        "tracks",
                        "albums",
                        "reposts",
                        "followers",
                        "following",
                        "popular-tracks",
                        "comments",
                        "spotlight",
                    ]
                    .contains(track) =>
            {
                Ok(SongRef {
                    provider: "soundcloud",
                    media_id: None,
                    url: format!("https://soundcloud.com/{user}/{track}"),
                })
            }
            _ => Err(bad()),
        },
        "on.soundcloud.com" => match segments.as_slice() {
            [code] if code.len() <= 32 && code.bytes().all(|b| b.is_ascii_alphanumeric()) => {
                Ok(SongRef {
                    provider: "soundcloud",
                    media_id: None,
                    url: format!("https://on.soundcloud.com/{code}"),
                })
            }
            _ => Err(bad()),
        },
        _ => Err(bad()),
    }
}
pub struct OEmbed {
    pub title: String,
    pub author: String,
    pub thumbnail_url: Option<String>,
    pub media_id: String,
}
async fn capped_body(mut response: reqwest::Response, cap: usize) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() > cap {
            return None;
        }
    }
    Some(body)
}
/// Official oEmbed lookup: fixed endpoint, no redirects, 5-second timeout and a 64 KB cap.
pub async fn oembed(app: &App, song: &SongRef) -> Res<OEmbed> {
    let (endpoint, name) = if song.provider == "youtube" {
        (&app.config.youtube_oembed_url, "YouTube")
    } else {
        (&app.config.soundcloud_oembed_url, "SoundCloud")
    };
    let unreachable = || Fail::unavailable(format!("Couldn't reach {name}. Try again."));
    let mut url = url::Url::parse(endpoint).map_err(|_| Fail::internal())?;
    url.query_pairs_mut()
        .append_pair("url", &song.url)
        .append_pair("format", "json");
    let response = app
        .http
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| unreachable())?;
    let status = response.status();
    if matches!(status.as_u16(), 400 | 401 | 403 | 404) {
        return Err(Fail::field("url", "That track can't be embedded."));
    }
    if !status.is_success() {
        // Server errors and redirects (never followed) are both treated as unavailable.
        return Err(unreachable());
    }
    let body = capped_body(response, 64 * 1024)
        .await
        .ok_or_else(unreachable)?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| unreachable())?;
    let field = |key: &str| {
        value[key]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(100)
            .collect::<String>()
    };
    let media_id = match song.provider {
        "youtube" => song.media_id.clone().unwrap_or_default(),
        _ => {
            let html = value["html"].as_str().unwrap_or("");
            let decoded = html.replace("%2F", "/");
            decoded
                .split("tracks/")
                .nth(1)
                .map(|rest| {
                    rest.chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                })
                .filter(|id| !id.is_empty() && id.len() <= 20)
                .ok_or_else(|| Fail::field("url", "That track can't be embedded."))?
        }
    };
    Ok(OEmbed {
        title: field("title"),
        author: field("author_name"),
        thumbnail_url: value["thumbnail_url"].as_str().map(str::to_string),
        media_id,
    })
}
/// Copies the oEmbed thumbnail into the media store as a 400 px WebP. Failures leave no art.
pub async fn copy_thumbnail(app: &App, owner: &str, thumbnail: Option<&str>) -> Option<String> {
    let url = url::Url::parse(thumbnail?).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let allowed = app
        .config
        .thumbnail_hosts
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")));
    let loopback = matches!(host.as_str(), "127.0.0.1" | "localhost");
    if !allowed
        || (url.scheme() != "https" && !(loopback && !app.config.production))
        || !app.config.media.storage.available()
    {
        return None;
    }
    let response = app
        .http
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = capped_body(response, Kind::SongThumb.max_bytes()).await?;
    let processed = media::process_async(bytes, Kind::SongThumb, None)
        .await
        .ok()?;
    media::store(app, &processed).await.ok()?;
    let mut db = app.db.acquire().await.ok()?;
    media::record(&mut db, owner, Kind::SongThumb, &processed)
        .await
        .ok()?;
    Some(processed.stored)
}
#[derive(Deserialize)]
pub struct SongPreview {
    url: String,
}
/// POST /api/me/song/preview: oEmbed lookup (10 per user per minute).
pub async fn song_preview(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<SongPreview>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let song = parse_song_url(&input.url)?;
    rate(&app, format!("oembed:{}", user.id), 10, 60).await?;
    let found = oembed(&app, &song).await?;
    Ok(Json(
        json!({"provider": song.provider, "media_id": found.media_id, "url": song.url, "title": found.title, "artist": found.author}),
    ))
}
#[derive(Deserialize)]
pub struct SongInput {
    url: String,
    title: Option<String>,
    artist: Option<String>,
    volume: Option<i32>,
    revision: Option<i64>,
}
/// PUT /api/me/song
pub async fn save_song(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<SongInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    {
        let mut db = app.db.acquire().await?;
        ensure_unrestricted(&mut db, &user.id).await?;
    }
    let song = parse_song_url(&input.url)?;
    let volume = input.volume.unwrap_or(70);
    if !(0..=100).contains(&volume) {
        return Err(Fail::field("volume", "Volume must be 0-100."));
    }
    let title = input
        .title
        .as_deref()
        .map(|t| text::plain(t, "title", 0, 100, 0, true))
        .transpose()?;
    let artist = input
        .artist
        .as_deref()
        .map(|t| text::plain(t, "artist", 0, 100, 0, true))
        .transpose()?;
    rate(&app, format!("oembed:{}", user.id), 10, 60).await?;
    let found = oembed(&app, &song).await?;
    let thumb = copy_thumbnail(&app, &user.id, found.thumbnail_url.as_deref()).await;
    let title = title.filter(|t| !t.is_empty()).unwrap_or(found.title);
    let artist = artist.filter(|t| !t.is_empty()).unwrap_or(found.author);
    let mut tx = app.db.begin().await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "song", input.revision).await?;
    let old: Option<String> =
        sqlx::query_scalar("SELECT song_thumb_key FROM profiles WHERE user_id=$1 FOR UPDATE")
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("UPDATE profiles SET song_provider=$2,song_media_id=$3,song_url=$4,song_title=$5,song_artist=$6,song_thumb_key=$7,song_volume=$8,song_notice=NULL,song_updated_at=now(),updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(song.provider)
        .bind(&found.media_id)
        .bind(&song.url)
        .bind(&title)
        .bind(&artist)
        .bind(&thumb)
        .bind(volume)
        .execute(&mut *tx)
        .await?;
    media::queue_delete(&mut tx, old.as_deref(), thumb.as_deref()).await?;
    crate::activity::record(
        &mut tx,
        &user.id,
        "song",
        None,
        None,
        json!({"title": title, "artist": artist}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"saved": true, "revision": revision, "song": {"provider": song.provider, "media_id": found.media_id, "url": song.url, "title": title, "artist": artist, "volume": volume, "thumbnail": thumb.map(|k| media_url(&app, &k))}}),
    ))
}
/// DELETE /api/me/song
pub async fn delete_song(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    ensure_profile(&mut tx, &user.id).await?;
    bump_section(&mut tx, &user.id, "song", None).await?;
    let old: Option<String> =
        sqlx::query_scalar("SELECT song_thumb_key FROM profiles WHERE user_id=$1 FOR UPDATE")
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("UPDATE profiles SET song_provider=NULL,song_media_id=NULL,song_url=NULL,song_title=NULL,song_artist=NULL,song_thumb_key=NULL,song_notice=NULL,song_updated_at=now() WHERE user_id=$1").bind(&user.id).execute(&mut *tx).await?;
    media::queue_delete(&mut tx, old.as_deref(), None).await?;
    crate::activity::forget(&mut tx, &user.id, "song").await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
/// GET /api/me/song
pub async fn my_song(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let row: Option<(Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i32, Option<String>)> = sqlx::query_as("SELECT song_provider,song_media_id,song_url,song_title,song_artist,song_thumb_key,song_volume,song_notice FROM profiles WHERE user_id=$1")
        .bind(&user.id)
        .fetch_optional(&mut *db)
        .await?;
    let song = match &row {
        Some((Some(provider), media, url, title, artist, thumb, volume, _)) => {
            json!({"provider": provider, "media_id": media, "url": url, "title": title, "artist": artist, "volume": volume, "thumbnail": thumb.as_ref().map(|k| media_url(&app, k))})
        }
        _ => Value::Null,
    };
    Ok(Json(
        json!({"song": song, "notice": row.and_then(|r| r.7), "revision": section_revision(&mut db, &user.id, "song").await?}),
    ))
}

// ---------------------------------------------------------------- Schedule

fn parse_hhmm(value: &str, field: &'static str) -> Res<i32> {
    let time = NaiveTime::parse_from_str(value.trim(), "%H:%M")
        .map_err(|_| Fail::field(field, "Use 24-hour HH:MM times."))?;
    Ok((time.hour_minute().0 * 60 + time.hour_minute().1) as i32)
}
trait HourMinute {
    fn hour_minute(&self) -> (u32, u32);
}
impl HourMinute for NaiveTime {
    fn hour_minute(&self) -> (u32, u32) {
        use chrono::Timelike;
        (self.hour(), self.minute())
    }
}
fn hhmm(minutes: i32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}
#[derive(Deserialize)]
pub struct BlockInput {
    weekday: i32,
    start: String,
    end: String,
    #[serde(default)]
    label: String,
}
#[derive(Deserialize)]
pub struct EventInput {
    title: String,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
}
#[derive(Deserialize)]
pub struct ScheduleInput {
    timezone: String,
    #[serde(default)]
    blocks: Vec<BlockInput>,
    #[serde(default)]
    events: Vec<EventInput>,
    revision: Option<i64>,
}
pub struct Block {
    pub weekday: i32,
    pub start: i32,
    pub end: i32,
    pub label: String,
}
pub fn validate_blocks(input: &[BlockInput]) -> Res<Vec<Block>> {
    if input.len() > 21 {
        return Err(Fail::field("blocks", "Add up to 21 weekly blocks."));
    }
    let mut blocks: Vec<Block> = Vec::new();
    for b in input {
        if !(1..=7).contains(&b.weekday) {
            return Err(Fail::field("blocks", "Choose a weekday."));
        }
        let start = parse_hhmm(&b.start, "blocks")?;
        let end = parse_hhmm(&b.end, "blocks")?;
        if start == end {
            return Err(Fail::field(
                "blocks",
                "A block's start and end can't be the same.",
            ));
        }
        let label = text::plain(&b.label, "blocks", 0, 60, 0, true)?;
        // Blocks that end before they start cross midnight; same-day overlap is compared within the day.
        let span = |s: i32, e: i32| if e > s { (s, e) } else { (s, 1440) };
        let (s1, e1) = span(start, end);
        if blocks.iter().filter(|o| o.weekday == b.weekday).any(|o| {
            let (s2, e2) = span(o.start, o.end);
            s1 < e2 && s2 < e1
        }) {
            return Err(Fail::field(
                "blocks",
                "Blocks on the same day can't overlap.",
            ));
        }
        blocks.push(Block {
            weekday: b.weekday,
            start,
            end,
            label,
        });
    }
    Ok(blocks)
}
/// Local wall-clock time to UTC with DST-correct rules: gaps move forward, overlaps take the earlier.
fn local_to_utc(tz: Tz, naive: chrono::NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(a, _) => a.with_timezone(&Utc),
        chrono::LocalResult::None => {
            let mut probe = naive;
            for _ in 0..180 {
                probe += Duration::minutes(1);
                if let Some(t) = tz.from_local_datetime(&probe).earliest() {
                    return t.with_timezone(&Utc);
                }
            }
            Utc.from_utc_datetime(&naive)
        }
    }
}
/// Expands weekly blocks into concrete occurrences overlapping [from, until).
pub fn expand(
    tz: Tz,
    blocks: &[Block],
    from: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, DateTime<Utc>, String)> {
    let mut out = Vec::new();
    let first = from.with_timezone(&tz).date_naive() - Duration::days(1);
    let days = (until - from).num_days() + 2;
    for offset in 0..=days {
        let day = first + Duration::days(offset);
        let weekday = day.weekday().number_from_monday() as i32;
        for b in blocks.iter().filter(|b| b.weekday == weekday) {
            let start = local_to_utc(
                tz,
                day.and_hms_opt(0, 0, 0).unwrap() + Duration::minutes(b.start as i64),
            );
            let end_day = if b.end > b.start {
                day
            } else {
                day + Duration::days(1)
            };
            let end = local_to_utc(
                tz,
                end_day.and_hms_opt(0, 0, 0).unwrap() + Duration::minutes(b.end as i64),
            );
            if end > from && start < until {
                out.push((start, end, b.label.clone()));
            }
        }
    }
    out.sort_by_key(|o| o.0);
    out
}
async fn load_schedule(
    db: &mut PgConnection,
    user_id: &str,
) -> Res<(
    Option<String>,
    Vec<Block>,
    Vec<(String, DateTime<Utc>, DateTime<Utc>)>,
)> {
    let tz: Option<String> = sqlx::query_scalar("SELECT timezone FROM schedules WHERE user_id=$1")
        .bind(user_id)
        .fetch_optional(&mut *db)
        .await?;
    let blocks: Vec<(i32, i32, i32, String)> = sqlx::query_as("SELECT weekday,start_minute,end_minute,label FROM schedule_blocks WHERE user_id=$1 ORDER BY position").bind(user_id).fetch_all(&mut *db).await?;
    let events: Vec<(String, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as("SELECT title,start_at,end_at FROM schedule_events WHERE user_id=$1 AND end_at>now() ORDER BY start_at").bind(user_id).fetch_all(&mut *db).await?;
    Ok((
        tz,
        blocks
            .into_iter()
            .map(|b| Block {
                weekday: b.0,
                start: b.1,
                end: b.2,
                label: b.3,
            })
            .collect(),
        events,
    ))
}
async fn occurrences(
    db: &mut PgConnection,
    user_id: &str,
    days: i64,
) -> Res<(Option<String>, Vec<Value>)> {
    let (tz, blocks, events) = load_schedule(db, user_id).await?;
    let now = Utc::now();
    let until = now + Duration::days(days);
    let mut all: Vec<(DateTime<Utc>, DateTime<Utc>, String, &str)> = Vec::new();
    if let Some(zone) = tz.as_deref().and_then(|z| Tz::from_str(z).ok()) {
        all.extend(
            expand(zone, &blocks, now, until)
                .into_iter()
                .map(|(s, e, l)| (s, e, l, "weekly")),
        );
    }
    all.extend(
        events
            .into_iter()
            .filter(|e| e.1 < until)
            .map(|(t, s, e)| (s, e, t, "event")),
    );
    all.sort_by_key(|o| o.0);
    Ok((tz, all.into_iter().map(|(s, e, l, kind)| json!({"start_at": s, "end_at": e, "label": l, "kind": kind, "live": false})).collect()))
}
/// The next `n` occurrences for the Home tab.
pub async fn next_occurrences(db: &mut PgConnection, user_id: &str, n: usize) -> Res<Value> {
    let (tz, items) = occurrences(db, user_id, 14).await?;
    Ok(json!({"timezone": tz, "items": items.into_iter().take(n).collect::<Vec<_>>()}))
}
/// GET /api/channels/{username}/schedule: the next 7 days of occurrences.
pub async fn channel_schedule(
    State(app): State<App>,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    let owner = eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let (tz, items) = occurrences(&mut db, &owner.id, 7).await?;
    Ok(Json(
        json!({"owner": {"username": owner.username, "display_name": owner.display_name}, "timezone": tz, "items": items}),
    ))
}
/// GET /api/me/schedule
pub async fn my_schedule(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let (tz, blocks, events) = load_schedule(&mut db, &user.id).await?;
    Ok(Json(json!({
        "timezone": tz,
        "blocks": blocks.iter().map(|b| json!({"weekday": b.weekday, "start": hhmm(b.start), "end": hhmm(b.end), "label": b.label})).collect::<Vec<_>>(),
        "events": events.iter().map(|(t, s, e)| json!({"title": t, "start_at": s, "end_at": e})).collect::<Vec<_>>(),
        "revision": section_revision(&mut db, &user.id, "schedule").await?,
    })))
}
/// PUT /api/me/schedule: replaces the weekly blocks and upcoming events atomically.
pub async fn save_schedule(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<ScheduleInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let tz = input.timezone.trim();
    // A named IANA zone only; fixed offsets such as "UTC+2" or "+02:00" are rejected.
    if Tz::from_str(tz).is_err()
        || tz.starts_with(['+', '-'])
        || tz.contains("GMT+")
        || tz.contains("GMT-")
        || tz.starts_with("Etc/")
    {
        return Err(Fail::field("timezone", "Choose a named time zone."));
    }
    let blocks = validate_blocks(&input.blocks)?;
    if input.events.len() > 20 {
        return Err(Fail::field("events", "Add up to 20 events."));
    }
    let now = Utc::now();
    let mut events = Vec::new();
    for e in &input.events {
        let title = text::plain(&e.title, "events", 1, 100, 0, true)?;
        if e.end_at <= e.start_at || e.end_at - e.start_at > Duration::hours(24) {
            return Err(Fail::field(
                "events",
                "An event must end after it starts and last up to 24 hours.",
            ));
        }
        if e.end_at <= now || e.start_at > now + Duration::days(365) {
            return Err(Fail::field(
                "events",
                "Events must be upcoming and start within the next 365 days.",
            ));
        }
        events.push((title, e.start_at, e.end_at));
    }
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "schedule", input.revision).await?;
    sqlx::query("INSERT INTO schedules(user_id,timezone) VALUES($1,$2) ON CONFLICT (user_id) DO UPDATE SET timezone=EXCLUDED.timezone").bind(&user.id).bind(tz).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM schedule_blocks WHERE user_id=$1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM schedule_events WHERE user_id=$1 AND end_at>now()")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    for (i, b) in blocks.iter().enumerate() {
        sqlx::query("INSERT INTO schedule_blocks(id,user_id,position,weekday,start_minute,end_minute,label) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(new_id())
            .bind(&user.id)
            .bind(i as i32)
            .bind(b.weekday)
            .bind(b.start)
            .bind(b.end)
            .bind(&b.label)
            .execute(&mut *tx)
            .await?;
    }
    for (title, start, end) in &events {
        sqlx::query(
            "INSERT INTO schedule_events(id,user_id,title,start_at,end_at) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(new_id())
        .bind(&user.id)
        .bind(title)
        .bind(start)
        .bind(end)
        .execute(&mut *tx)
        .await?;
    }
    if !blocks.is_empty() || !events.is_empty() {
        crate::activity::record(
            &mut tx,
            &user.id,
            "schedule",
            None,
            None,
            json!({"blocks": blocks.len(), "events": events.len()}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "revision": revision})))
}

// ---------------------------------------------------------------- Sponsors and setup

const SPONSOR_CATEGORIES: &[&str] = &[
    "HARDWARE",
    "PERIPHERALS",
    "SOFTWARE",
    "APPAREL",
    "FOOD_DRINK",
    "SERVICES",
    "OTHER",
];
const SETUP_CATEGORIES: &[&str] = &[
    "CAMERA",
    "MICROPHONE",
    "AUDIO_INTERFACE",
    "HEADPHONES",
    "PC",
    "CPU",
    "GPU",
    "CAPTURE",
    "LIGHTING",
    "MONITOR",
    "KEYBOARD",
    "MOUSE",
    "CONTROLLER",
    "OTHER",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SponsorInput {
    id: Option<String>,
    name: String,
    #[serde(default)]
    description: String,
    link: String,
    #[serde(default)]
    discount_code: String,
    category: String,
    #[serde(default = "yes")]
    active: bool,
}
fn yes() -> bool {
    true
}
#[derive(Deserialize)]
pub struct SponsorsInput {
    items: Vec<SponsorInput>,
    revision: Option<i64>,
}
async fn sponsors_json(
    app: &App,
    db: &mut PgConnection,
    user_id: &str,
    active_only: bool,
) -> Res<Vec<Value>> {
    let rows: Vec<(String, bool, String, String, String, String, String, Option<String>)> = sqlx::query_as("SELECT id,active,name,description,link,discount_code,category,logo_key FROM sponsors WHERE user_id=$1 AND (active OR NOT $2) ORDER BY position")
        .bind(user_id)
        .bind(active_only)
        .fetch_all(&mut *db)
        .await?;
    Ok(rows.into_iter().map(|r| json!({"id": r.0, "active": r.1, "name": r.2, "description": r.3, "link": r.4, "discount_code": r.5, "category": r.6, "logo": r.7.map(|k| media_url(app, &k))})).collect())
}
/// GET /api/me/sponsors
pub async fn my_sponsors(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    Ok(Json(
        json!({"items": sponsors_json(&app, &mut db, &user.id, false).await?, "categories": SPONSOR_CATEGORIES, "revision": section_revision(&mut db, &user.id, "sponsors").await?}),
    ))
}
/// PUT /api/me/sponsors: up to 10 entries in owner order; logos are kept for existing entries.
pub async fn save_sponsors(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<SponsorsInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if input.items.len() > 10 {
        return Err(Fail::field("items", "Add up to 10 sponsors."));
    }
    let mut rows = Vec::new();
    for s in &input.items {
        let name = text::plain(&s.name, "name", 1, 80, 0, true)?;
        let description = text::plain(&s.description, "description", 0, 300, 4, true)?;
        let link = text::website_url(&s.link, "link")?;
        let code = text::plain(&s.discount_code, "discount_code", 0, 50, 0, true)?;
        if !SPONSOR_CATEGORIES.contains(&s.category.as_str()) {
            return Err(Fail::field("category", "Choose a category."));
        }
        rows.push((
            s.id.clone(),
            name,
            description,
            link,
            code,
            s.category.clone(),
            s.active,
        ));
    }
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "sponsors", input.revision).await?;
    let existing: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id,logo_key FROM sponsors WHERE user_id=$1")
            .bind(&user.id)
            .fetch_all(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM sponsors WHERE user_id=$1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let mut kept = Vec::new();
    for (i, (id, name, description, link, code, category, active)) in rows.iter().enumerate() {
        let previous = id
            .as_ref()
            .and_then(|id| existing.iter().find(|(e, _)| e == id));
        let row_id = previous.map(|p| p.0.clone()).unwrap_or_else(new_id);
        let logo = previous.and_then(|p| p.1.clone());
        kept.push(row_id.clone());
        sqlx::query("INSERT INTO sponsors(id,user_id,position,active,name,description,link,discount_code,category,logo_key) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(&row_id)
            .bind(&user.id)
            .bind(i as i32)
            .bind(active)
            .bind(name)
            .bind(description)
            .bind(link)
            .bind(code)
            .bind(category)
            .bind(logo)
            .execute(&mut *tx)
            .await?;
    }
    for (id, logo) in &existing {
        if !kept.contains(id) {
            media::queue_delete(&mut tx, logo.as_deref(), None).await?;
        }
    }
    tx.commit().await?;
    Ok(Json(
        json!({"saved": true, "revision": revision, "ids": kept}),
    ))
}
/// POST /api/me/sponsors/{id}/logo
pub async fn sponsor_logo(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    {
        let mut db = app.db.acquire().await?;
        ensure_unrestricted(&mut db, &user.id).await?;
        let owns: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sponsors WHERE id=$1 AND user_id=$2)")
                .bind(&id)
                .bind(&user.id)
                .fetch_one(&mut *db)
                .await?;
        if !owns {
            return Err(Fail::missing());
        }
    }
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Image uploads aren't available yet."));
    }
    let (bytes, _, _) = media::read_upload(multipart, Kind::SponsorLogo).await?;
    rate(&app, format!("image-upload:{}", user.id), 20, 3600).await?;
    let processed = media::process_async(bytes, Kind::SponsorLogo, None).await?;
    media::store(&app, &processed).await?;
    let mut tx = app.db.begin().await?;
    let old: Option<Option<String>> =
        sqlx::query_scalar("SELECT logo_key FROM sponsors WHERE id=$1 AND user_id=$2 FOR UPDATE")
            .bind(&id)
            .bind(&user.id)
            .fetch_optional(&mut *tx)
            .await?;
    let old = old.ok_or_else(Fail::missing)?;
    media::record(&mut tx, &user.id, Kind::SponsorLogo, &processed).await?;
    sqlx::query("UPDATE sponsors SET logo_key=$2 WHERE id=$1")
        .bind(&id)
        .bind(&processed.stored)
        .execute(&mut *tx)
        .await?;
    media::queue_delete(&mut tx, old.as_deref(), Some(&processed.stored)).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"saved": true, "logo": media_url(&app, &processed.stored)}),
    ))
}
/// DELETE /api/me/sponsors/{id}/logo
pub async fn delete_sponsor_logo(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let old: Option<Option<String>> =
        sqlx::query_scalar("SELECT logo_key FROM sponsors WHERE id=$1 AND user_id=$2 FOR UPDATE")
            .bind(&id)
            .bind(&user.id)
            .fetch_optional(&mut *tx)
            .await?;
    let old = old.ok_or_else(Fail::missing)?;
    sqlx::query("UPDATE sponsors SET logo_key=NULL WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    media::queue_delete(&mut tx, old.as_deref(), None).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupInput {
    category: String,
    name: String,
    #[serde(default)]
    note: String,
    link: Option<String>,
}
#[derive(Deserialize)]
pub struct SetupList {
    items: Vec<SetupInput>,
    /// Omitted means unchanged (decision P4).
    title: Option<String>,
    description: Option<String>,
    revision: Option<i64>,
}
pub const MAX_SETUP_PHOTOS: i64 = 3;
fn setup_photo_urls(app: &App, prefix: &str) -> Value {
    json!({"400": media_url(app, &format!("{prefix}/400.webp")), "1600": media_url(app, &format!("{prefix}/1600.webp"))})
}
/// Setup photos in owner order. `all` includes REMOVED photos (the owner's Studio view).
async fn setup_photos_json(
    app: &App,
    db: &mut PgConnection,
    user_id: &str,
    all: bool,
) -> Res<Vec<Value>> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as("SELECT id,image_key,alt,status FROM setup_photos WHERE user_id=$1 AND ($2 OR status='VISIBLE') ORDER BY position,created_at")
        .bind(user_id)
        .bind(all)
        .fetch_all(&mut *db)
        .await?;
    Ok(rows
        .iter()
        .map(
            |r| json!({"id": r.0, "image": setup_photo_urls(app, &r.1), "alt": r.2, "status": r.3}),
        )
        .collect())
}
async fn setup_text(db: &mut PgConnection, user_id: &str) -> Res<(String, String)> {
    Ok(sqlx::query_as(
        "SELECT coalesce((SELECT setup_title FROM profiles WHERE user_id=$1),''),coalesce((SELECT setup_description FROM profiles WHERE user_id=$1),'')",
    )
    .bind(user_id)
    .fetch_one(&mut *db)
    .await?)
}
async fn setup_json(db: &mut PgConnection, user_id: &str) -> Res<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('category',category,'name',name,'note',note,'link',link) FROM setup_items WHERE user_id=$1 ORDER BY position").bind(user_id).fetch_all(&mut *db).await?)
}
/// GET /api/me/setup
pub async fn my_setup(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let (title, description) = setup_text(&mut db, &user.id).await?;
    Ok(Json(json!({
        "items": setup_json(&mut db, &user.id).await?,
        "title": title,
        "description": description,
        "photos": setup_photos_json(&app, &mut db, &user.id, true).await?,
        "max_photos": MAX_SETUP_PHOTOS,
        "categories": SETUP_CATEGORIES,
        "revision": section_revision(&mut db, &user.id, "setup").await?,
    })))
}
/// PUT /api/me/setup: up to 20 items in owner order.
pub async fn save_setup(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<SetupList>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if input.items.len() > 20 {
        return Err(Fail::field("items", "Add up to 20 items."));
    }
    let mut rows = Vec::new();
    for item in &input.items {
        if !SETUP_CATEGORIES.contains(&item.category.as_str()) {
            return Err(Fail::field("category", "Choose a category."));
        }
        let name = text::plain(&item.name, "name", 1, 80, 0, true)?;
        let note = text::plain(&item.note, "note", 0, 120, 0, true)?;
        let link = item
            .link
            .as_deref()
            .filter(|l| !l.trim().is_empty())
            .map(|l| text::website_url(l, "link"))
            .transpose()?;
        rows.push((item.category.clone(), name, note, link));
    }
    let title = input
        .title
        .as_deref()
        .map(|t| text::plain(t, "title", 0, 80, 0, true))
        .transpose()?;
    let description = input
        .description
        .as_deref()
        .map(|t| text::plain(t, "description", 0, 500, 6, true))
        .transpose()?;
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "setup", input.revision).await?;
    sqlx::query("UPDATE profiles SET setup_title=coalesce($2,setup_title),setup_description=coalesce($3,setup_description),updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(&title)
        .bind(&description)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM setup_items WHERE user_id=$1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    for (i, (category, name, note, link)) in rows.iter().enumerate() {
        sqlx::query("INSERT INTO setup_items(id,user_id,position,category,name,note,link) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(new_id()).bind(&user.id).bind(i as i32).bind(category).bind(name).bind(note).bind(link).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "revision": revision})))
}

/// Queues a setup photo's media for deletion unless another setup photo still uses the same
/// content-addressed key.
pub async fn release_setup_photo(db: &mut PgConnection, key: &str) -> Res<()> {
    let shared: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM setup_photos WHERE image_key=$1)")
            .bind(key)
            .fetch_one(&mut *db)
            .await?;
    if !shared {
        media::queue_delete(db, Some(key), None).await?;
    }
    Ok(())
}
/// POST /api/me/setup/photos (multipart `file`, optional `alt`).
pub async fn upload_setup_photo(
    State(app): State<App>,
    jar: CookieJar,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    {
        let mut db = app.db.acquire().await?;
        ensure_unrestricted(&mut db, &user.id).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM setup_photos WHERE user_id=$1")
            .bind(&user.id)
            .fetch_one(&mut *db)
            .await?;
        if count >= MAX_SETUP_PHOTOS {
            return Err(Fail::conflict("You can add up to 3 setup photos."));
        }
    }
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Image uploads aren't available yet."));
    }
    let (bytes, _, fields) = media::read_upload(multipart, Kind::SetupPhoto).await?;
    let alt = text::plain(
        fields.get("alt").and_then(Value::as_str).unwrap_or(""),
        "alt",
        0,
        120,
        0,
        true,
    )?;
    rate(&app, format!("image-upload:{}", user.id), 20, 3600).await?;
    let processed = media::process_async(bytes, Kind::SetupPhoto, None).await?;
    media::store(&app, &processed).await?;
    let mut tx = app.db.begin().await?;
    ensure_profile(&mut tx, &user.id).await?;
    // Lock the profile so concurrent uploads can't pass the limit together.
    sqlx::query("SELECT 1 FROM profiles WHERE user_id=$1 FOR UPDATE")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let (count, duplicate): (i64, bool) = sqlx::query_as(
        "SELECT count(*),coalesce(bool_or(image_key=$2),false) FROM setup_photos WHERE user_id=$1",
    )
    .bind(&user.id)
    .bind(&processed.stored)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate {
        return Err(Fail::conflict("You already added that photo."));
    }
    if count >= MAX_SETUP_PHOTOS {
        return Err(Fail::conflict("You can add up to 3 setup photos."));
    }
    media::record(&mut tx, &user.id, Kind::SetupPhoto, &processed).await?;
    let id = new_id();
    sqlx::query(
        "INSERT INTO setup_photos(id,user_id,position,image_key,alt) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(count as i32)
    .bind(&processed.stored)
    .bind(&alt)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id": id, "image": setup_photo_urls(&app, &processed.stored), "alt": alt, "status": "VISIBLE"}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhotoOrderItem {
    id: String,
    #[serde(default)]
    alt: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhotoOrder {
    photos: Vec<PhotoOrderItem>,
}
/// PUT /api/me/setup/photos: order and alt text; must list exactly the owner's photos.
pub async fn save_setup_photos(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<PhotoOrder>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut alts = Vec::new();
    for p in &input.photos {
        alts.push(text::plain(&p.alt, "alt", 0, 120, 0, true)?);
    }
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let mut current: Vec<String> =
        sqlx::query_scalar("SELECT id FROM setup_photos WHERE user_id=$1 FOR UPDATE")
            .bind(&user.id)
            .fetch_all(&mut *tx)
            .await?;
    let mut given: Vec<String> = input.photos.iter().map(|p| p.id.clone()).collect();
    current.sort();
    given.sort();
    if current != given {
        return Err(Fail::field(
            "photos",
            "The photo list changed. Reload and try again.",
        ));
    }
    for (i, (p, alt)) in input.photos.iter().zip(&alts).enumerate() {
        sqlx::query("UPDATE setup_photos SET position=$3,alt=$4 WHERE id=$1 AND user_id=$2")
            .bind(&p.id)
            .bind(&user.id)
            .bind(i as i32)
            .bind(alt)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
/// DELETE /api/me/setup/photos/{id}
pub async fn delete_setup_photo(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let key: Option<String> = sqlx::query_scalar(
        "DELETE FROM setup_photos WHERE id=$1 AND user_id=$2 RETURNING image_key",
    )
    .bind(&id)
    .bind(&user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let key = key.ok_or_else(Fail::missing)?;
    release_setup_photo(&mut tx, &key).await?;
    // Keep positions dense (0..n) in the current order.
    sqlx::query("UPDATE setup_photos p SET position=o.n FROM (SELECT id,(row_number() OVER (ORDER BY position,created_at))-1 AS n FROM setup_photos WHERE user_id=$1) o WHERE p.id=o.id")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

// ---------------------------------------------------------------- Custom page blocks

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AboutConfig {
    body: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelConfig {
    title: String,
    body: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Quote {
    text: String,
    #[serde(default)]
    attribution: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuotesConfig {
    quotes: Vec<Quote>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShelfConfig {
    games: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockItem {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default = "yes")]
    enabled: bool,
    config: Value,
}
#[derive(Deserialize)]
pub struct BlocksInput {
    items: Vec<BlockItem>,
    revision: Option<i64>,
}
/// Strict per-type validation; unknown fields are rejected. Returns the normalized config.
pub fn validate_block(kind: &str, config: &Value) -> Res<Value> {
    let invalid = || Fail::field("config", "That block's settings aren't valid.");
    Ok(match kind {
        "ABOUT" => {
            let c: AboutConfig = serde_json::from_value(config.clone()).map_err(|_| invalid())?;
            json!({"body": text::markdown(&c.body, "body", 2000)?})
        }
        "PANEL" => {
            let c: PanelConfig = serde_json::from_value(config.clone()).map_err(|_| invalid())?;
            json!({"title": text::plain(&c.title, "title", 1, 80, 0, true)?, "body": text::markdown(&c.body, "body", 2000)?})
        }
        "QUOTES" => {
            let c: QuotesConfig = serde_json::from_value(config.clone()).map_err(|_| invalid())?;
            if c.quotes.is_empty() || c.quotes.len() > 5 {
                return Err(Fail::field("quotes", "Add 1-5 quotes."));
            }
            let quotes = c.quotes.iter().map(|q| Ok(json!({"text": text::plain(&q.text, "quotes", 1, 280, 4, true)?, "attribution": text::plain(&q.attribution, "quotes", 0, 80, 0, true)?}))).collect::<Res<Vec<_>>>()?;
            json!({"quotes": quotes})
        }
        "GAME_SHELF" => {
            let c: ShelfConfig = serde_json::from_value(config.clone()).map_err(|_| invalid())?;
            if c.games.is_empty() || c.games.len() > 12 {
                return Err(Fail::field("games", "Add 1-12 games."));
            }
            json!({"games": c.games.iter().map(|g| text::plain(g, "games", 1, 80, 0, true)).collect::<Res<Vec<_>>>()?})
        }
        _ => return Err(Fail::field("type", "Choose a block type.")),
    })
}
async fn blocks_json(db: &mut PgConnection, user_id: &str, enabled_only: bool) -> Res<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('type',type,'enabled',enabled,'config',config) FROM profile_blocks WHERE user_id=$1 AND (enabled OR NOT $2) ORDER BY position").bind(user_id).bind(enabled_only).fetch_all(&mut *db).await?)
}
/// GET /api/me/blocks
pub async fn my_blocks(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    Ok(Json(
        json!({"items": blocks_json(&mut db, &user.id, false).await?, "revision": section_revision(&mut db, &user.id, "blocks").await?}),
    ))
}
/// PUT /api/me/blocks: up to 10 typed blocks in owner order.
pub async fn save_blocks(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<BlocksInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if input.items.len() > 10 {
        return Err(Fail::field("items", "Add up to 10 blocks."));
    }
    let rows = input
        .items
        .iter()
        .map(|b| {
            Ok((
                b.kind.clone(),
                b.enabled,
                validate_block(&b.kind, &b.config)?,
            ))
        })
        .collect::<Res<Vec<_>>>()?;
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "blocks", input.revision).await?;
    sqlx::query("DELETE FROM profile_blocks WHERE user_id=$1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    for (i, (kind, enabled, config)) in rows.iter().enumerate() {
        sqlx::query("INSERT INTO profile_blocks(id,user_id,position,type,enabled,config) VALUES($1,$2,$3,$4,$5,$6)").bind(new_id()).bind(&user.id).bind(i as i32).bind(kind).bind(enabled).bind(config).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "revision": revision})))
}

/// GET /api/channels/{username}/about: bio, enabled blocks, active sponsors and setup.
pub async fn channel_about(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let owner = eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let (title, description) = setup_text(&mut db, &owner.id).await?;
    let is_owner = viewer.as_ref().is_some_and(|v| v.id == owner.id);
    Ok(Json(json!({
        "owner": {"username": owner.username, "display_name": owner.display_name},
        "bio": owner.bio,
        "blocks": blocks_json(&mut db, &owner.id, true).await?,
        "sponsors": sponsors_json(&app, &mut db, &owner.id, true).await?,
        "setup": setup_json(&mut db, &owner.id).await?,
        "setup_title": title,
        "setup_description": description,
        "setup_photos": setup_photos_json(&app, &mut db, &owner.id, false).await?,
        "viewer": {"signed_in": viewer.is_some(), "is_owner": is_owner, "can_report": viewer.is_some() && !is_owner},
    })))
}

// ---------------------------------------------------------------- Fan art

fn fan_art_urls(app: &App, prefix: &str) -> Value {
    json!({"400": media_url(app, &format!("{prefix}/400.webp")), "1600": media_url(app, &format!("{prefix}/1600.webp"))})
}
#[derive(Deserialize)]
pub struct FanArtSettings {
    enabled: bool,
}
/// PUT /api/me/fan-art/settings
pub async fn fan_art_settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<FanArtSettings>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    ensure_profile(&mut tx, &user.id).await?;
    sqlx::query("UPDATE profiles SET fan_art_enabled=$2,updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "enabled": input.enabled})))
}
/// POST /api/channels/{username}/fan-art: multipart submission (PENDING until the owner approves).
pub async fn submit_fan_art(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    ensure_verified(&user, "Verify your email to submit fan art.")?;
    let channel = {
        let mut db = app.db.acquire().await?;
        let channel = eligible_by_name(&mut db, &name)
            .await?
            .ok_or_else(Fail::channel_missing)?;
        let enabled: bool = sqlx::query_scalar(
            "SELECT coalesce((SELECT fan_art_enabled FROM profiles WHERE user_id=$1),false)",
        )
        .bind(&channel.id)
        .fetch_one(&mut *db)
        .await?;
        let me = crate::profiles::channel_user_by_id(&mut db, &user.id)
            .await?
            .ok_or_else(Fail::missing)?;
        if !enabled
            || me.internal
            || me.restricted
            || blocked_between(&mut db, &channel.id, &user.id).await?
        {
            return Err(Fail::denied("You can't submit fan art to this channel."));
        }
        channel
    };
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Image uploads aren't available yet."));
    }
    let (bytes, _, fields) = media::read_upload(multipart, Kind::FanArt).await?;
    let field = |k: &str| {
        fields
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    if !matches!(field("attest").as_str(), "true" | "on" | "1") {
        return Err(Fail::field(
            "attest",
            "Confirm that you made this or have permission to share it.",
        ));
    }
    let display: String = {
        let mut db = app.db.acquire().await?;
        sqlx::query_scalar("SELECT display_name FROM channel_users WHERE id=$1")
            .bind(&user.id)
            .fetch_one(&mut *db)
            .await?
    };
    let artist = if field("artist_name").trim().is_empty() {
        display
    } else {
        text::plain(&field("artist_name"), "artist_name", 1, 80, 0, true)?
    };
    let link = Some(field("artist_link"))
        .filter(|l| !l.trim().is_empty())
        .map(|l| text::website_url(&l, "artist_link"))
        .transpose()?;
    let caption = text::plain(&field("caption"), "caption", 0, 200, 2, true)?;
    {
        let mut db = app.db.acquire().await?;
        let pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM fan_art WHERE channel_id=$1 AND status='PENDING'",
        )
        .bind(&channel.id)
        .fetch_one(&mut *db)
        .await?;
        if pending >= 20 {
            return Err(Fail::conflict("This channel's fan art queue is full."));
        }
    }
    rate(&app, format!("fan-art:{}", user.id), 5, 86400).await?;
    let processed = media::process_async(bytes, Kind::FanArt, None).await?;
    media::store(&app, &processed).await?;
    let id = new_id();
    let mut tx = app.db.begin().await?;
    // Re-check the queue cap under a lock on the channel's profile row.
    ensure_profile(&mut tx, &channel.id).await?;
    sqlx::query("SELECT 1 FROM profiles WHERE user_id=$1 FOR UPDATE")
        .bind(&channel.id)
        .execute(&mut *tx)
        .await?;
    let pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM fan_art WHERE channel_id=$1 AND status='PENDING'")
            .bind(&channel.id)
            .fetch_one(&mut *tx)
            .await?;
    if pending >= 20 {
        return Err(Fail::conflict("This channel's fan art queue is full."));
    }
    media::record(&mut tx, &user.id, Kind::FanArt, &processed).await?;
    sqlx::query("INSERT INTO fan_art(id,channel_id,submitter_id,image_key,artist_name,artist_link,caption,status) VALUES($1,$2,$3,$4,$5,$6,$7,'PENDING')")
        .bind(&id)
        .bind(&channel.id)
        .bind(&user.id)
        .bind(&processed.stored)
        .bind(&artist)
        .bind(&link)
        .bind(&caption)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"id": id, "status": "PENDING"})))
}
/// GET /api/channels/{username}/fan-art?cursor=: approved items plus the viewer's own pending ones.
pub async fn channel_fan_art(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let viewer_id = viewer.as_ref().map(|v| v.id.clone());
    let mut db = app.db.acquire().await?;
    let channel = eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let enabled: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT fan_art_enabled FROM profiles WHERE user_id=$1),false)",
    )
    .bind(&channel.id)
    .fetch_one(&mut *db)
    .await?;
    let is_owner = viewer_id.as_deref() == Some(channel.id.as_str());
    if !enabled && !is_owner {
        return Err(Fail::missing());
    }
    let after = parse_cursor(&q.cursor)?;
    let rows: Vec<(String, String, String, Option<String>, String, String, DateTime<Utc>, Value, String)> = sqlx::query_as(&format!(
        "SELECT f.id,f.image_key,f.artist_name,f.artist_link,f.caption,f.status,f.submitted_at,{chip},f.submitter_id FROM fan_art f JOIN channel_users c ON c.id=f.submitter_id \
         WHERE f.channel_id=$1 AND c.deleted_at IS NULL AND (f.status='APPROVED' OR (f.status IN ('PENDING','REJECTED') AND f.submitter_id=$2)) \
         AND ($3::timestamptz IS NULL OR (f.submitted_at,f.id)<($3,$4)) ORDER BY f.submitted_at DESC,f.id DESC LIMIT 21",
        chip = chip_sql("c")
    ))
    .bind(&channel.id)
    .bind(&viewer_id)
    .bind(after.as_ref().map(|a| a.0))
    .bind(after.as_ref().map(|a| a.1.clone()).unwrap_or_default())
    .fetch_all(&mut *db)
    .await?;
    let more = rows.len() > 20;
    let rows = &rows[..rows.len().min(20)];
    let mut items: Vec<Value> = rows
        .iter()
        .map(|r| json!({"id": r.0, "image": fan_art_urls(&app, &r.1), "artist_name": r.2, "artist_link": r.3, "caption": r.4, "status": r.5, "submitted_at": r.6, "submitter": r.7, "can_delete": viewer_id.as_deref().is_some_and(|v| v == r.8 || is_owner), "can_report": viewer_id.as_deref().is_some_and(|v| v != r.8) && r.5 == "APPROVED"}))
        .collect();
    items.iter_mut().for_each(|v| hydrate(&app, v));
    Ok(Json(json!({
        "owner": {"username": channel.username, "display_name": channel.display_name},
        "enabled": enabled,
        "items": items,
        "next_cursor": if more { rows.last().map(|r| make_cursor(r.6, &r.0)) } else { None },
        "viewer": {"signed_in": viewer.is_some(), "is_owner": is_owner, "verified": viewer.as_ref().is_some_and(|v| v.email_verified)},
    })))
}
/// DELETE /api/fan-art/{id}: the submitter withdraws or the owner removes.
pub async fn delete_fan_art(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT channel_id,submitter_id,image_key,status FROM fan_art WHERE id=$1 FOR UPDATE",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;
    let (channel, submitter, key, status) = row.ok_or_else(Fail::missing)?;
    if (user.id != channel && user.id != submitter) || status == "REMOVED" {
        return Err(Fail::missing());
    }
    sqlx::query("DELETE FROM fan_art WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    media::queue_delete(&mut tx, Some(&key), None).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted": true})))
}
/// GET /api/me/fan-art: settings, the pending queue and counts for Studio.
pub async fn my_fan_art(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let enabled: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT fan_art_enabled FROM profiles WHERE user_id=$1),false)",
    )
    .bind(&user.id)
    .fetch_one(&mut *db)
    .await?;
    let rows: Vec<(String, String, String, Option<String>, String, DateTime<Utc>, Value)> = sqlx::query_as(&format!(
        "SELECT f.id,f.image_key,f.artist_name,f.artist_link,f.caption,f.submitted_at,{chip} FROM fan_art f JOIN channel_users c ON c.id=f.submitter_id WHERE f.channel_id=$1 AND f.status='PENDING' ORDER BY f.submitted_at,f.id LIMIT 20",
        chip = chip_sql("c")
    ))
    .bind(&user.id)
    .fetch_all(&mut *db)
    .await?;
    let approved: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM fan_art WHERE channel_id=$1 AND status='APPROVED'",
    )
    .bind(&user.id)
    .fetch_one(&mut *db)
    .await?;
    let mut pending: Vec<Value> = rows.iter().map(|r| json!({"id": r.0, "image": fan_art_urls(&app, &r.1), "artist_name": r.2, "artist_link": r.3, "caption": r.4, "submitted_at": r.5, "submitter": r.6})).collect();
    pending.iter_mut().for_each(|v| hydrate(&app, v));
    Ok(Json(
        json!({"enabled": enabled, "pending": pending, "approved_count": approved}),
    ))
}
/// GET /api/me/fan-art/pending
pub async fn pending_fan_art(state: State<App>, jar: CookieJar) -> Res<Json<Value>> {
    my_fan_art(state, jar).await
}
/// POST /api/fan-art/{id}/{approve|reject}
pub async fn review_fan_art(
    State(app): State<App>,
    jar: CookieJar,
    Path((id, action)): Path<(String, String)>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT channel_id,status FROM fan_art WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?;
    let (channel, status) = row.ok_or_else(Fail::missing)?;
    if channel != user.id {
        return Err(Fail::missing());
    }
    if status != "PENDING" {
        return Err(Fail::conflict("This was already reviewed."));
    }
    let next = match action.as_str() {
        "approve" => {
            let approved: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM fan_art WHERE channel_id=$1 AND status='APPROVED'",
            )
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
            if approved >= 100 {
                return Err(Fail::conflict(
                    "Your channel can show up to 100 fan art items. Remove one first.",
                ));
            }
            "APPROVED"
        }
        "reject" => "REJECTED",
        _ => return Err(Fail::new(StatusCode::NOT_FOUND, "Not found.")),
    };
    sqlx::query("UPDATE fan_art SET status=$2,reviewed_at=now() WHERE id=$1")
        .bind(&id)
        .bind(next)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"status": next})))
}

// ---------------------------------------------------------------- Page header copy (P9)

pub const DEFAULT_PAGE_LABEL: &str = "Creator Page";
pub const DEFAULT_WELCOME: &str = "Welcome to my page";
type HeaderRow = (String, String, String, String, String, bool);
async fn header_row(db: &mut PgConnection, user_id: &str) -> Res<HeaderRow> {
    Ok(sqlx::query_as("SELECT page_label,welcome_line,intro_title,intro_body,page_vibe,header_copy_enabled FROM profiles WHERE user_id=$1")
        .bind(user_id)
        .fetch_optional(&mut *db)
        .await?
        .unwrap_or_else(|| (String::new(), String::new(), String::new(), String::new(), String::new(), true)))
}
/// The header copy as rendered on the channel: defaults resolved, label and welcome null when
/// the owner turned them off, intro fields empty when there is no intro body.
pub async fn channel_header(db: &mut PgConnection, user_id: &str) -> Res<Value> {
    let (label, welcome, intro_title, intro_body, vibe, enabled) = header_row(db, user_id).await?;
    let or = |v: String, d: &str| if v.is_empty() { d.to_string() } else { v };
    Ok(json!({
        "label": enabled.then(|| or(label, DEFAULT_PAGE_LABEL)),
        "welcome": enabled.then(|| or(welcome, DEFAULT_WELCOME)),
        "intro_title": if intro_body.is_empty() { String::new() } else { intro_title },
        "intro_body": intro_body,
        "vibe": vibe,
    }))
}
/// GET /api/me/header
pub async fn my_header(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let (label, welcome, intro_title, intro_body, vibe, enabled) =
        header_row(&mut db, &user.id).await?;
    Ok(Json(json!({
        "page_label": label,
        "welcome_line": welcome,
        "intro_title": intro_title,
        "intro_body": intro_body,
        "page_vibe": vibe,
        "enabled": enabled,
        "defaults": {"page_label": DEFAULT_PAGE_LABEL, "welcome_line": DEFAULT_WELCOME},
        "revision": section_revision(&mut db, &user.id, "header").await?,
    })))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderInput {
    #[serde(default)]
    page_label: String,
    #[serde(default)]
    welcome_line: String,
    #[serde(default)]
    intro_title: String,
    #[serde(default)]
    intro_body: String,
    #[serde(default)]
    page_vibe: String,
    #[serde(default = "yes")]
    enabled: bool,
    revision: Option<i64>,
}
/// PUT /api/me/header
pub async fn save_header(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<HeaderInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let label = text::plain(&input.page_label, "page_label", 0, 24, 0, true)?;
    let welcome = text::plain(&input.welcome_line, "welcome_line", 0, 80, 0, true)?;
    let intro_title = text::plain(&input.intro_title, "intro_title", 0, 60, 0, true)?;
    let intro_body = text::plain(&input.intro_body, "intro_body", 0, 500, 6, true)?;
    let vibe = text::plain(&input.page_vibe, "page_vibe", 0, 24, 0, true)?;
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "header", input.revision).await?;
    sqlx::query("UPDATE profiles SET page_label=$2,welcome_line=$3,intro_title=$4,intro_body=$5,page_vibe=$6,header_copy_enabled=$7,updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(&label)
        .bind(&welcome)
        .bind(&intro_title)
        .bind(&intro_body)
        .bind(&vibe)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "revision": revision})))
}

// ---------------------------------------------------------------- Page readiness (P3)

/// GET /api/me/readiness: the owner's 7 page steps, computed on the server.
pub async fn my_readiness(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let (avatar, banner, bio, links, song, council, schedule, dismissed): (bool, bool, bool, bool, bool, bool, bool, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT coalesce(p.avatar_key IS NOT NULL,false),coalesce(p.banner_key IS NOT NULL,false),coalesce(p.bio<>'',false),\
         EXISTS(SELECT 1 FROM social_links WHERE user_id=$1),coalesce(p.song_provider IS NOT NULL,false),\
         EXISTS(SELECT 1 FROM war_council WHERE user_id=$1),\
         EXISTS(SELECT 1 FROM schedule_blocks WHERE user_id=$1) OR EXISTS(SELECT 1 FROM schedule_events WHERE user_id=$1 AND end_at>now()),\
         p.readiness_dismissed_at FROM (SELECT $1::text AS id) u LEFT JOIN profiles p ON p.user_id=u.id",
    )
    .bind(&user.id)
    .fetch_one(&mut *db)
    .await?;
    let steps = [
        ("avatar", "Upload an avatar", avatar, "/settings/profile"),
        ("banner", "Upload a banner", banner, "/settings/profile"),
        ("bio", "Write a bio", bio, "/settings/profile"),
        ("links", "Add a social link", links, "/settings/profile"),
        ("song", "Set a profile song", song, "/studio/channel/song"),
        (
            "council",
            "Pick your War Council",
            council,
            "/studio/channel/war-council",
        ),
        (
            "schedule",
            "Add your schedule",
            schedule,
            "/studio/channel/schedule",
        ),
    ];
    let done = steps.iter().filter(|s| s.2).count();
    Ok(Json(json!({
        "steps": steps.iter().map(|(key, label, done, href)| json!({"key": key, "label": label, "done": done, "href": href})).collect::<Vec<_>>(),
        "done": done,
        "total": steps.len(),
        "complete": done == steps.len(),
        "dismissed": dismissed.is_some(),
    })))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessInput {
    dismissed: bool,
}
/// PUT /api/me/readiness: dismiss or restore the Studio reminder.
pub async fn save_readiness(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<ReadinessInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    ensure_profile(&mut tx, &user.id).await?;
    sqlx::query(
        "UPDATE profiles SET readiness_dismissed_at=CASE WHEN $2 THEN now() END WHERE user_id=$1",
    )
    .bind(&user.id)
    .bind(input.dismissed)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "dismissed": input.dismissed})))
}
