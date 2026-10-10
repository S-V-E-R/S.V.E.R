//! Third-party emotes (docs/DEVELOPER_PLATFORM.md §7): a streamer who linked Twitch can show their
//! 7TV, BTTV and FrankerFaceZ channel emotes, and optionally the global sets, in S.V.E.R chat. Lists
//! are read every 10 minutes and images are copied to S.V.E.R's media storage, so viewers' IP
//! addresses never reach those services. The channel's own emotes win name clashes, names pass the
//! channel's banned-word list, the streamer can hide any emote, and a viewer report hides one until
//! the streamer reviews it. `OUTSIDE_EMOTES` (e.g. `7tv,bttv,ffz`) switches services on; unset, the
//! feature is off, which is how a service that objects is switched off.
use crate::{
    App, auth, moderation,
    profiles::{self, Fail, Res},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicI64, Ordering},
};

const PROVIDERS: [&str; 3] = ["7tv", "bttv", "ffz"];
/// Each image copied is at most this size.
const MAX_FILE: usize = 1024 * 1024;
/// At most this many emotes per channel and service.
const MAX_PER_SET: usize = 1000;
/// Display sizes and the service scale each one is copied from.
const SCALES: [(u32, u8); 3] = [(28, 1), (56, 2), (112, 4)];

/// Services switched on for this server.
fn enabled() -> Vec<String> {
    std::env::var("OUTSIDE_EMOTES")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| PROVIDERS.contains(&s.as_str()))
        .collect()
}
fn base(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.into())
}

#[derive(Debug, PartialEq)]
struct Found {
    id: String,
    code: String,
    animated: bool,
}
fn push(out: &mut Vec<Found>, id: Option<&str>, code: Option<&str>, animated: bool) {
    let (Some(id), Some(code)) = (id, code) else {
        return;
    };
    let id_ok = (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric());
    let code_ok = (1..=40).contains(&code.chars().count())
        && !code.chars().any(|c| c.is_whitespace() || c.is_control());
    if id_ok && code_ok {
        out.push(Found {
            id: id.into(),
            code: code.into(),
            animated,
        });
    }
}
/// Reads one service's list: a channel's emotes by its Twitch user ID, or the global set.
fn parse(provider: &str, body: &Value, channel: bool) -> Vec<Found> {
    let mut out = Vec::new();
    match provider {
        "7tv" => {
            let set = if channel { &body["emote_set"] } else { body };
            for e in set["emotes"].as_array().into_iter().flatten() {
                push(
                    &mut out,
                    e["id"].as_str(),
                    e["name"].as_str(),
                    e["data"]["animated"].as_bool().unwrap_or(false),
                );
            }
        }
        "bttv" => {
            let list: Vec<&Value> = if channel {
                ["channelEmotes", "sharedEmotes"]
                    .iter()
                    .flat_map(|k| body[*k].as_array().into_iter().flatten())
                    .collect()
            } else {
                body.as_array().into_iter().flatten().collect()
            };
            for e in list {
                push(
                    &mut out,
                    e["id"].as_str(),
                    e["code"].as_str(),
                    e["animated"].as_bool().unwrap_or(false),
                );
            }
        }
        _ => {
            // The global endpoint lists every set; only its default sets are shown to everyone.
            let defaults: Option<Vec<String>> = (!channel).then(|| {
                body["default_sets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(Value::to_string)
                    .collect()
            });
            for (set, value) in body["sets"].as_object().into_iter().flatten() {
                if defaults.as_ref().is_some_and(|d| !d.contains(set)) {
                    continue;
                }
                for e in value["emoticons"].as_array().into_iter().flatten() {
                    let id = e["id"].as_i64().map(|n| n.to_string());
                    push(
                        &mut out,
                        id.as_deref(),
                        e["name"].as_str(),
                        e["animated"].is_object(),
                    );
                }
            }
        }
    }
    out.truncate(MAX_PER_SET);
    out
}
/// A channel with no account on that service has no emotes there (404); other failures keep the
/// last list.
async fn fetch(app: &App, provider: &str, twitch: Option<&str>) -> Option<Vec<Found>> {
    let seventv = base("SEVENTV_API", "https://7tv.io");
    let bttv = base("BTTV_API", "https://api.betterttv.net");
    let ffz = base("FFZ_API", "https://api.frankerfacez.com");
    let url = match (provider, twitch) {
        ("7tv", Some(id)) => format!("{seventv}/v3/users/twitch/{id}"),
        ("7tv", None) => format!("{seventv}/v3/emote-sets/global"),
        ("bttv", Some(id)) => format!("{bttv}/3/cached/users/twitch/{id}"),
        ("bttv", None) => format!("{bttv}/3/cached/emotes/global"),
        (_, Some(id)) => format!("{ffz}/v1/room/id/{id}"),
        (_, None) => format!("{ffz}/v1/set/global"),
    };
    let response = app.http.get(url).send().await.ok()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Some(Vec::new());
    }
    if !response.status().is_success() {
        return None;
    }
    let body: Value = response.json().await.ok()?;
    Some(parse(provider, &body, twitch.is_some()))
}

fn image_url(provider: &str, id: &str, animated: bool, scale: u8) -> String {
    let ffz = base("FFZ_CDN", "https://cdn.frankerfacez.com");
    match provider {
        "7tv" => format!(
            "{}/emote/{id}/{scale}x.webp",
            base("SEVENTV_CDN", "https://cdn.7tv.app")
        ),
        "bttv" => format!(
            "{}/emote/{id}/{}x.webp",
            base("BTTV_CDN", "https://cdn.betterttv.net"),
            scale.min(3)
        ),
        _ if animated => format!("{ffz}/emote/{id}/animated/{scale}.webp"),
        _ => format!("{ffz}/emote/{id}/{scale}"),
    }
}
/// The image type from its first bytes; anything else is refused.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else {
        None
    }
}
async fn download(app: &App, url: &str) -> Option<(Vec<u8>, &'static str)> {
    let response = app.http.get(url).send().await.ok()?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|n| n > MAX_FILE as u64)
    {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    if bytes.len() > MAX_FILE {
        return None;
    }
    let ext = sniff(&bytes)?;
    Some((bytes.to_vec(), ext))
}
fn key(provider: &str, id: &str, size: u32, ext: &str) -> String {
    format!("outside/{provider}/{}/{size}.{ext}", id.to_lowercase())
}
/// Copies one emote's three sizes to S.V.E.R's storage. A size the service doesn't have reuses the
/// next smaller one.
async fn mirror(app: &App, provider: &str, id: &str, animated: bool) -> Res<bool> {
    let mut files: Vec<(u32, Vec<u8>, &'static str)> = Vec::new();
    for (size, scale) in SCALES {
        let (bytes, ext) = match download(app, &image_url(provider, id, animated, scale)).await {
            Some(got) => got,
            None => match files.last() {
                Some((_, bytes, ext)) => (bytes.clone(), *ext),
                None => return Ok(false),
            },
        };
        files.push((size, bytes, ext));
    }
    let ext = files[0].2;
    if files.iter().any(|f| f.2 != ext) {
        return Ok(false);
    }
    for (size, bytes, _) in files {
        app.config
            .media
            .storage
            .put_typed(
                &app.http,
                &key(provider, id, size, ext),
                bytes,
                &format!("image/{ext}"),
                "public, max-age=31536000, immutable",
            )
            .await?;
    }
    sqlx::query("INSERT INTO outside_emote_files(provider,emote_id,ext) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(provider)
        .bind(id)
        .bind(ext)
        .execute(&app.db)
        .await?;
    Ok(true)
}
/// Replaces a channel's (or the global) list for one service.
async fn replace(app: &App, channel: Option<&str>, provider: &str, found: &[Found]) -> Res<()> {
    let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
    let codes: Vec<&str> = found.iter().map(|f| f.code.as_str()).collect();
    let animated: Vec<bool> = found.iter().map(|f| f.animated).collect();
    let mut tx = app.db.begin().await?;
    sqlx::query("DELETE FROM outside_emotes WHERE channel_id IS NOT DISTINCT FROM $1 AND provider=$2 AND NOT (emote_id=ANY($3))")
        .bind(channel).bind(provider).bind(&ids).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO outside_emotes(channel_id,provider,emote_id,code,animated) SELECT $1,$2,i,c,a FROM unnest($3::text[],$4::text[],$5::bool[]) AS t(i,c,a)
        ON CONFLICT ((coalesce(channel_id,'')),provider,emote_id) DO UPDATE SET code=EXCLUDED.code,animated=EXCLUDED.animated,seen_at=now()")
        .bind(channel).bind(provider).bind(&ids).bind(&codes).bind(&animated).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

static GLOBAL_SYNCED: AtomicI64 = AtomicI64::new(0);
/// Every minute (its own loop): refreshes channels last read over 10 minutes ago, the global sets
/// every 10 minutes while any channel shows them, and copies images not yet stored.
pub async fn sync(app: &App) -> Res<()> {
    let on = enabled();
    if on.is_empty() || !app.config.media.storage.available() {
        return Ok(());
    }
    let due: Vec<(String, Vec<String>, Option<String>)> = sqlx::query_as("UPDATE chat_settings s SET outside_synced_at=now() WHERE channel_id IN (
            SELECT channel_id FROM chat_settings WHERE cardinality(outside_emotes)>0 AND (outside_synced_at IS NULL OR outside_synced_at<now()-interval '10 minutes')
            ORDER BY outside_synced_at NULLS FIRST LIMIT 20)
        RETURNING s.channel_id,s.outside_emotes,coalesce((SELECT subject FROM identities WHERE user_id=s.channel_id AND provider='twitch'),
            (SELECT subject FROM linked_chat_accounts WHERE owner_id=s.channel_id AND platform='twitch' LIMIT 1))")
        .fetch_all(&app.db).await?;
    for (channel, providers, twitch) in due {
        let Some(twitch) = twitch else { continue };
        for provider in providers.iter().filter(|p| on.contains(p)) {
            if let Some(found) = fetch(app, provider, Some(&twitch)).await {
                replace(app, Some(&channel), provider, &found).await?;
            }
        }
        app.chat
            .publish(&channel, None, 0, json!({"type": "emotes"}));
    }
    let now = chrono::Utc::now().timestamp();
    let wanted: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_settings WHERE outside_global AND cardinality(outside_emotes)>0)")
        .fetch_one(&app.db).await?;
    if wanted && now - GLOBAL_SYNCED.load(Ordering::Relaxed) >= 600 {
        GLOBAL_SYNCED.store(now, Ordering::Relaxed);
        for provider in &on {
            if let Some(found) = fetch(app, provider, None).await {
                replace(app, None, provider, &found).await?;
            }
        }
    }
    // ponytail: 30 images per pass, one at a time; copy concurrently if new channels wait too long.
    let missing: Vec<(String, String, bool)> = sqlx::query_as("SELECT DISTINCT ON (e.provider,e.emote_id) e.provider,e.emote_id,e.animated FROM outside_emotes e
        WHERE NOT EXISTS(SELECT 1 FROM outside_emote_files f WHERE f.provider=e.provider AND f.emote_id=e.emote_id) AND e.provider=ANY($1) LIMIT 30")
        .bind(&on).fetch_all(&app.db).await?;
    for (provider, id, animated) in missing {
        mirror(app, &provider, &id, animated).await?;
    }
    Ok(())
}

fn image(app: &App, provider: &str, id: &str, ext: &str) -> Value {
    json!({
        "28": profiles::media_url(app, &key(provider, id, 28, ext)),
        "56": profiles::media_url(app, &key(provider, id, 56, ext)),
        "112": profiles::media_url(app, &key(provider, id, 112, ext)),
    })
}
type Row = (String, String, String, bool, bool, String, Option<String>);
/// Every stored emote a channel's settings show, with any hide reason; channel sets first.
async fn rows(app: &App, channel: &str) -> Res<Vec<Row>> {
    Ok(sqlx::query_as("SELECT e.provider,e.emote_id,e.code,e.animated,e.channel_id IS NULL,f.ext,h.reason
        FROM chat_settings s JOIN outside_emotes e ON e.provider=ANY(s.outside_emotes) AND e.provider=ANY($2) AND (e.channel_id=s.channel_id OR (e.channel_id IS NULL AND s.outside_global))
        JOIN outside_emote_files f ON f.provider=e.provider AND f.emote_id=e.emote_id
        LEFT JOIN outside_emote_hides h ON h.channel_id=s.channel_id AND h.provider=e.provider AND h.emote_id=e.emote_id
        WHERE s.channel_id=$1 ORDER BY e.channel_id NULLS LAST,e.code,e.provider")
        .bind(channel).bind(enabled()).fetch_all(&app.db).await?)
}
/// What chat shows: not hidden, allowed by the banned-word list, not held for removal, and not
/// clashing with the channel's own emotes or an earlier outside one.
async fn shown(app: &App, channel: &str) -> Res<Vec<Value>> {
    let mut db = app.db.acquire().await?;
    let (links, words): (bool, Vec<String>) =
        sqlx::query_as("SELECT block_links,banned_words FROM chat_settings WHERE channel_id=$1")
            .bind(channel)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or_default();
    let mut taken: HashSet<String> = sqlx::query_scalar(
        "SELECT code FROM channel_emotes WHERE channel_id=$1 AND status='VISIBLE'",
    )
    .bind(channel)
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .collect();
    let all = rows(app, channel).await?;
    let root = |provider: &str, id: &str| format!("outside/{provider}/{}", id.to_lowercase());
    let roots: Vec<String> = all.iter().map(|r| root(&r.0, &r.1)).collect();
    let held = crate::media::removal::held_roots(&mut db, &roots).await?;
    let mut out = Vec::new();
    for (provider, id, code, animated, _, ext, hidden) in all {
        if hidden.is_some()
            || held.contains(&root(&provider, &id))
            || !moderation::code_allowed(&words, links, &code)
            || crate::text::filter(&code, "code").is_err()
            || !taken.insert(code.clone())
        {
            continue;
        }
        out.push(json!({"code": code, "provider": provider, "id": id, "animated": animated, "image": image(app, &provider, &id, &ext)}));
    }
    Ok(out)
}
async fn channel_id(app: &App, name: &str) -> Res<String> {
    Ok(
        profiles::eligible_by_name(&mut *app.db.acquire().await?, name)
            .await?
            .ok_or_else(Fail::channel_missing)?
            .id,
    )
}
/// GET /api/channels/{username}/outside-emotes: the 7TV, BTTV and FFZ emotes chat shows. Chat
/// reloads it when told the emotes changed.
async fn list(State(app): State<App>, Path(name): Path<String>) -> Res<impl IntoResponse> {
    let channel = channel_id(&app, &name).await?;
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=30")],
        Json(json!({"emotes": shown(&app, &channel).await?})),
    ))
}
/// POST /api/channels/{username}/outside-emotes/{provider}/{id}/report: hides the emote in this
/// chat until the streamer reviews it.
async fn report(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, provider, id)): Path<(String, String, String)>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::rate(&app, format!("outside-report:{}", user.id), 10, 3600).await?;
    let channel = channel_id(&app, &name).await?;
    if !rows(&app, &channel)
        .await?
        .iter()
        .any(|r| r.0 == provider && r.1 == id)
    {
        return Err(Fail::missing());
    }
    sqlx::query("INSERT INTO outside_emote_hides(channel_id,provider,emote_id,reason,reported_by) VALUES($1,$2,$3,'report',$4) ON CONFLICT DO NOTHING")
        .bind(&channel).bind(&provider).bind(&id).bind(&user.id).execute(&app.db).await?;
    app.chat
        .publish(&channel, None, 0, json!({"type": "emotes"}));
    Ok(Json(json!({"hidden": true})))
}

type Saved = (Vec<String>, bool, Option<chrono::DateTime<chrono::Utc>>);
async fn studio(app: &App, user: &auth::User) -> Res<Json<Value>> {
    let (providers, global, synced): Saved = sqlx::query_as(
        "SELECT outside_emotes,outside_global,outside_synced_at FROM chat_settings WHERE channel_id=$1",
    )
    .bind(&user.id)
    .fetch_optional(&app.db)
    .await?
    .unwrap_or_default();
    let twitch: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identities WHERE user_id=$1 AND provider='twitch') OR EXISTS(SELECT 1 FROM linked_chat_accounts WHERE owner_id=$1 AND platform='twitch')")
        .bind(&user.id).fetch_one(&app.db).await?;
    let emotes: Vec<Value> = rows(app, &user.id)
        .await?
        .into_iter()
        .map(|(provider, id, code, animated, global, ext, hidden)| {
            json!({"provider": provider, "id": id, "code": code, "animated": animated, "global": global,
                "hidden": hidden, "image": image(app, &provider, &id, &ext)})
        })
        .collect();
    Ok(Json(
        json!({"available": enabled(), "providers": providers, "global": global,
        "twitch_linked": twitch, "synced_at": synced, "emotes": emotes}),
    ))
}
/// GET /api/me/outside-emotes (Studio → Chat)
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    studio(&app, &user).await
}
#[derive(Deserialize)]
pub struct Settings {
    providers: Vec<String>,
    global: bool,
}
/// PUT /api/me/outside-emotes: which services to show, and whether to include their global sets.
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut providers = input.providers;
    providers.sort();
    providers.dedup();
    if providers.iter().any(|p| !PROVIDERS.contains(&p.as_str())) {
        return Err(Fail::field("providers", "Choose 7tv, bttv or ffz."));
    }
    sqlx::query("INSERT INTO chat_settings(channel_id,outside_emotes,outside_global) VALUES($1,$2,$3)
        ON CONFLICT(channel_id) DO UPDATE SET outside_emotes=EXCLUDED.outside_emotes,outside_global=EXCLUDED.outside_global,outside_synced_at=NULL")
        .bind(&user.id).bind(&providers).bind(input.global).execute(&app.db).await?;
    app.chat
        .publish(&user.id, None, 0, json!({"type": "emotes"}));
    studio(&app, &user).await
}
#[derive(Deserialize)]
pub struct Hide {
    hidden: bool,
}
/// PUT /api/me/outside-emotes/{provider}/{id}: hide an emote, or show one (also clears a report).
async fn hide(
    State(app): State<App>,
    jar: CookieJar,
    Path((provider, id)): Path<(String, String)>,
    Json(input): Json<Hide>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if input.hidden {
        sqlx::query("INSERT INTO outside_emote_hides(channel_id,provider,emote_id,reason) VALUES($1,$2,$3,'streamer') ON CONFLICT(channel_id,provider,emote_id) DO UPDATE SET reason='streamer'")
            .bind(&user.id).bind(&provider).bind(&id).execute(&app.db).await?;
    } else {
        sqlx::query(
            "DELETE FROM outside_emote_hides WHERE channel_id=$1 AND provider=$2 AND emote_id=$3",
        )
        .bind(&user.id)
        .bind(&provider)
        .bind(&id)
        .execute(&app.db)
        .await?;
    }
    app.chat
        .publish(&user.id, None, 0, json!({"type": "emotes"}));
    studio(&app, &user).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/outside-emotes", get(list))
        .route(
            "/api/channels/{username}/outside-emotes/{provider}/{id}/report",
            post(report),
        )
        .route("/api/me/outside-emotes", get(mine).put(save))
        .route("/api/me/outside-emotes/{provider}/{id}", put(hide))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_service() {
        let seventv = json!({"emote_set": {"emotes": [{"id": "01ABC", "name": "catJAM", "data": {"animated": true}}, {"id": "bad id", "name": "x"}]}});
        assert_eq!(
            parse("7tv", &seventv, true),
            [Found {
                id: "01ABC".into(),
                code: "catJAM".into(),
                animated: true
            }]
        );
        let bttv = json!({"channelEmotes": [{"id": "5f1b", "code": "D:", "animated": false}], "sharedEmotes": [{"id": "5f1c", "code": "has space"}]});
        assert_eq!(parse("bttv", &bttv, true).len(), 1);
        let ffz = json!({"default_sets": [3], "sets": {"3": {"emoticons": [{"id": 25927, "name": "CatBag", "animated": {"1": "x"}}]}, "4": {"emoticons": [{"id": 1, "name": "Hidden"}]}}});
        assert_eq!(
            parse("ffz", &ffz, false),
            [Found {
                id: "25927".into(),
                code: "CatBag".into(),
                animated: true
            }]
        );
        assert_eq!(
            parse("ffz", &ffz, true).len(),
            2,
            "a room shows all its sets"
        );
    }

    #[test]
    fn only_images_are_kept() {
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff(b"GIF89a..."), Some("gif"));
        assert_eq!(sniff(b"<svg onload=alert(1)>"), None);
    }
}
