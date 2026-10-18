//! Category cover art. A game category gets its art automatically the moment it exists: Steam's
//! library cover for its Steam app id, else the top-rated SteamGridDB cover. Every image is copied
//! into our own media storage, so tiles never depend on a provider or send viewers to one. A
//! category with nothing yet is retried daily (pre-release art often appears late). Staff uploads
//! always win and are never replaced. Categories that aren't games (Art, Coding) use uploads only.
use super::*;
use crate::media::{self, CATEGORY_ART_WIDTHS, Kind};
use axum::extract::Multipart;

/// Where art comes from; tests point these at a local server.
pub(super) struct Sources<'a> {
    /// Steam's CDN (`/steam/apps/{id}/library_600x900.jpg`).
    pub steam: &'a str,
    pub grid: &'a str,
    /// Only images from here are downloaded from SteamGridDB results.
    pub grid_files: &'a str,
    pub grid_key: &'a str,
}

/// `{"300": url, "600": url}` for a stored cover, or null.
pub fn art_json(app: &App, key: Option<&str>) -> Value {
    let Some((prefix, largest)) = key.and_then(|k| k.split_once('@')) else {
        return Value::Null;
    };
    let largest: u32 = largest.parse().unwrap_or(CATEGORY_ART_WIDTHS[0]);
    let urls: serde_json::Map<String, Value> = CATEGORY_ART_WIDTHS
        .iter()
        .filter(|w| **w <= largest)
        .map(|w| {
            (
                w.to_string(),
                json!(crate::profiles::media_url(
                    app,
                    &format!("{prefix}/{w}.webp")
                )),
            )
        })
        .collect();
    Value::Object(urls)
}

/// One category per pass, newest first: those never tried, then those due their daily retry.
pub async fn tick(app: &App) -> Res<()> {
    if !app.config.media.storage.available() {
        return Ok(());
    }
    let key = std::env::var("STEAMGRIDDB_API_KEY").unwrap_or_default();
    tick_at(
        app,
        &Sources {
            steam: "https://cdn.akamai.steamstatic.com",
            grid: "https://www.steamgriddb.com/api/v2",
            grid_files: "https://cdn2.steamgriddb.com/",
            grid_key: &key,
        },
    )
    .await
}
pub(super) async fn tick_at(app: &App, sources: &Sources<'_>) -> Res<()> {
    let claimed: Option<(String, String, Option<i64>)> = sqlx::query_as("UPDATE stream_categories c SET image_checked_at=now() WHERE c.id=(SELECT id FROM stream_categories k WHERE image_key IS NULL AND NOT image_manual AND (image_checked_at IS NULL OR image_checked_at<now()-interval '1 day') AND EXISTS(SELECT 1 FROM game_catalog g WHERE g.category_id=k.id) ORDER BY image_checked_at NULLS FIRST,id LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING c.id,c.name,(SELECT max(steam_app_id) FROM game_catalog g WHERE g.category_id=c.id)")
        .fetch_optional(&app.db).await?;
    let Some((id, name, steam)) = claimed else {
        return Ok(());
    };
    let Some(bytes) = find(app, sources, &name, steam).await else {
        return Ok(());
    };
    let processed = media::process_async(bytes, Kind::CategoryArt, None).await?;
    media::store(app, &processed).await?;
    let mut tx = app.db.begin().await?;
    // A staff upload made meanwhile wins; the stored copy then expires as an unused upload.
    let open: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_categories WHERE id=$1 AND image_key IS NULL AND NOT image_manual FOR UPDATE)")
        .bind(&id).fetch_one(&mut *tx).await?;
    if !open {
        return Ok(());
    }
    media::record_as(&mut tx, None, Kind::CategoryArt, &processed).await?;
    sqlx::query("UPDATE stream_categories SET image_key=$2 WHERE id=$1")
        .bind(&id)
        .bind(&processed.stored)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn find(app: &App, sources: &Sources<'_>, name: &str, steam: Option<i64>) -> Option<Vec<u8>> {
    // Despite its name, Steam's library_600x900 is 300x450; the _2x file is the 600x900 one.
    for file in ["library_600x900_2x.jpg", "library_600x900.jpg"] {
        if let Some(n) = steam
            && let Some(bytes) =
                download(app, &format!("{}/steam/apps/{n}/{file}", sources.steam)).await
        {
            return Some(bytes);
        }
    }
    if sources.grid_key.is_empty() {
        return None;
    }
    let mut games = Vec::new();
    if let Some(n) = steam {
        games.push(format!("steam/{n}"));
    }
    if let Some(found) = grid_game(app, sources, name).await {
        games.push(format!("game/{found}"));
    }
    for game in games {
        let url = format!(
            "{}/grids/{game}?dimensions=600x900&types=static&nsfw=false&humor=false&epilepsy=false",
            sources.grid
        );
        let Some(page) = grid_get(app, sources, &url).await else {
            continue;
        };
        let file = page["data"].as_array().into_iter().flatten().find_map(|g| {
            g["url"]
                .as_str()
                .filter(|u| u.starts_with(sources.grid_files) && !u.contains(".."))
        });
        if let Some(url) = file
            && let Some(bytes) = download(app, url).await
        {
            return Some(bytes);
        }
    }
    None
}

/// The SteamGridDB game whose title matches the category exactly, never a near miss.
async fn grid_game(app: &App, sources: &Sources<'_>, name: &str) -> Option<i64> {
    let mut url = url::Url::parse(&format!("{}/search/autocomplete/", sources.grid)).ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(name);
    let page = grid_get(app, sources, url.as_str()).await?;
    let wanted = text::part_norm(name);
    page["data"]
        .as_array()?
        .iter()
        .find(|g| g["name"].as_str().map(text::part_norm).as_deref() == Some(wanted.as_str()))?["id"]
        .as_i64()
}

async fn grid_get(app: &App, sources: &Sources<'_>, url: &str) -> Option<Value> {
    let response = app
        .http
        .get(url)
        .bearer_auth(sources.grid_key)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    serde_json::from_slice(&read(response, 1_000_000).await?).ok()
}

async fn download(app: &App, url: &str) -> Option<Vec<u8>> {
    let response = app
        .http
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    read(response, Kind::CategoryArt.max_bytes()).await
}

/// POST /api/admin/categories/{id}/art: staff art (any category, games included), centered 3:4.
pub async fn upload(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Uploads aren't available yet."));
    }
    let (bytes, _, _) = media::read_upload(multipart, Kind::CategoryArt).await?;
    let processed = media::process_async(bytes, Kind::CategoryArt, None).await?;
    media::store(&app, &processed).await?;
    let mut tx = app.db.begin().await?;
    let old: Option<String> =
        sqlx::query_scalar("SELECT image_key FROM stream_categories WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Fail::missing)?;
    media::record_as(&mut tx, None, Kind::CategoryArt, &processed).await?;
    media::queue_delete(&mut tx, old.as_deref(), Some(&processed.stored)).await?;
    sqlx::query("UPDATE stream_categories SET image_key=$2,image_manual=true WHERE id=$1")
        .bind(&id)
        .bind(&processed.stored)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "category_art",
        "category",
        &id,
        &[],
        "Uploaded category art.",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"saved":true,"image":art_json(&app, Some(&processed.stored))}),
    ))
}

/// DELETE /api/admin/categories/{id}/art: removes the art; games then fetch theirs again.
pub async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let old: Option<String> =
        sqlx::query_scalar("SELECT image_key FROM stream_categories WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Fail::missing)?;
    media::queue_delete(&mut tx, old.as_deref(), None).await?;
    sqlx::query("UPDATE stream_categories SET image_key=NULL,image_manual=false,image_checked_at=NULL WHERE id=$1")
        .bind(&id).execute(&mut *tx).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "category_art_removed",
        "category",
        &id,
        &[],
        "Removed category art.",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
