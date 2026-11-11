//! The CrowdSync catalog (docs/CROWDSYNC.md "Catalog"): the owner of a developer app publishes one
//! listing for it, a game that sets its own board or a board streamers can add to their library.
//! Staff review every new listing and every change before the public sees it; the last approved
//! copy stays up while a change waits. Listed newest first: nothing here is ranked or paid for.
use crate::{
    App, auth, boards,
    media::{self, Kind},
    profiles::{self, Fail, Res},
    safety, text,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

const MAX_SCREENSHOTS: usize = 4;
/// Who may see a listing: approved, its app not suspended, its developer's channel in good standing.
const PUBLIC: &str = "FROM catalog_listings l JOIN dev_apps a ON a.id=l.app_id AND a.suspended_at IS NULL JOIN channel_users u ON u.id=a.owner_id AND u.eligible JOIN users o ON o.id=u.id WHERE l.approved IS NOT NULL";
/// The developer's public name. Under-18 developers (or no birth date) aren't named.
const DEVELOPER: &str = "CASE WHEN coalesce(o.date_of_birth<=current_date-interval '18 years',false) THEN jsonb_build_object('username',u.username,'display_name',u.display_name) END";
/// Channels that added the board; for a game that sets its own board, channels that connected it.
const INSTALLS: &str = "(SELECT count(*) FROM (SELECT channel_id FROM catalog_installs WHERE app_id=l.app_id UNION SELECT user_id FROM oauth_grants WHERE app_id=l.app_id AND revoked_at IS NULL AND 'board:control'=ANY(scopes) AND l.approved->'board'='null'::jsonb) c)";

/// The stored image roots a listing's content names (its screenshots and icon).
fn keys(content: &Value) -> Vec<String> {
    let shots = content["screenshots"].as_array().into_iter().flatten();
    shots
        .chain(content.get("icon"))
        .filter_map(|k| k.as_str().map(str::to_owned))
        .collect()
}
/// A listing's content as pages show it: image URLs instead of stored keys, without any image
/// held for a Take It Down request. `own` keeps each screenshot's key, to send back in a change.
async fn present(app: &App, content: &Value, own: bool) -> Res<Value> {
    let held = media::removal::held_roots(&mut *app.db.acquire().await?, &keys(content)).await?;
    let screenshots: Vec<Value> = content["screenshots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|k| !held.contains(*k))
        .map(|k| {
            let mut shot = json!({"small": profiles::media_url(app, &format!("{k}/400.webp")), "large": profiles::media_url(app, &format!("{k}/1600.webp"))});
            if own {
                shot["key"] = json!(k);
            }
            shot
        })
        .collect();
    let icon = content["icon"].as_str().filter(|k| !held.contains(*k));
    Ok(json!({
        "name": content["name"], "summary": content["summary"], "description": content["description"], "link": content["link"],
        "icon": icon.map_or(Value::Null, |k| profiles::avatar_json(app, Some(k))),
        "screenshots": screenshots, "board": content["board"],
    }))
}
/// Whether a listing still uses a stored object, so media cleanup leaves it alone.
pub async fn referenced(db: &mut PgConnection, key: &str) -> Res<bool> {
    let root = format!("\"{}\"", media::removal::root_of_key(key));
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog_listings WHERE position($1 IN content::text)>0 OR position($1 IN approved::text)>0)")
        .bind(root)
        .fetch_one(db)
        .await?)
}
/// Queues images a listing stopped using for deletion (cleanup keeps any still used elsewhere).
async fn release(db: &mut PgConnection, old: &[&Value], kept: &[&Value]) -> Res<()> {
    let kept: Vec<String> = kept.iter().flat_map(|c| keys(c)).collect();
    for key in old.iter().flat_map(|c| keys(c)) {
        if !kept.contains(&key) {
            media::queue_delete(db, Some(&key), None).await?;
        }
    }
    Ok(())
}
/// Take It Down: every listing drops the removed image. Returns the developers it was taken from.
pub async fn remove_media(db: &mut PgConnection, root: &str) -> Res<Vec<String>> {
    let rows: Vec<(String, String, Value, Option<Value>)> = sqlx::query_as("SELECT l.app_id,a.owner_id,l.content,l.approved FROM catalog_listings l JOIN dev_apps a ON a.id=l.app_id WHERE position($1 IN l.content::text)>0 OR position($1 IN l.approved::text)>0 FOR UPDATE OF l")
        .bind(format!("\"{root}\""))
        .fetch_all(&mut *db)
        .await?;
    let strip = |mut content: Value| {
        if content["icon"] == root {
            content["icon"] = Value::Null;
        }
        if let Some(shots) = content["screenshots"].as_array_mut() {
            shots.retain(|k| k != root);
        }
        content
    };
    let mut owners = Vec::new();
    for (id, owner, content, approved) in rows {
        sqlx::query("UPDATE catalog_listings SET content=$2,approved=$3 WHERE app_id=$1")
            .bind(&id)
            .bind(strip(content))
            .bind(approved.map(strip))
            .execute(&mut *db)
            .await?;
        owners.push(owner);
    }
    Ok(owners)
}
/// Deletes an app's listing (the owner withdrew it, or is deleting the app) and queues its images
/// for deletion.
pub(crate) async fn forget(db: &mut PgConnection, app_id: &str) -> Res<()> {
    let row: Option<(Value, Option<Value>)> =
        sqlx::query_as("DELETE FROM catalog_listings WHERE app_id=$1 RETURNING content,approved")
            .bind(app_id)
            .fetch_optional(&mut *db)
            .await?;
    if let Some((content, approved)) = row {
        let approved = approved.unwrap_or(Value::Null);
        release(db, &[&content, &approved], &[]).await?;
    }
    Ok(())
}

// ---- Publishing (Settings → Developer) ----

/// The caller's own app: its owner, its icon and whether staff suspended it.
async fn own_app(app: &App, jar: &CookieJar, id: &str) -> Res<(auth::User, Option<String>, bool)> {
    let user = crate::devapps::developer(app, jar).await?;
    let row: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT icon_key,suspended_at IS NOT NULL FROM dev_apps WHERE id=$1 AND owner_id=$2",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&app.db)
    .await?;
    let (icon, suspended) = row.ok_or_else(Fail::missing)?;
    Ok((user, icon, suspended))
}
async fn listing(app: &App, id: &str) -> Res<Json<Value>> {
    let row: Option<(String, Value, bool, String, Option<String>)> = sqlx::query_as("SELECT slug,content,approved IS NOT NULL,status,reason FROM catalog_listings WHERE app_id=$1")
        .bind(id)
        .fetch_optional(&app.db)
        .await?;
    let listing = match row {
        Some((slug, content, live, status, reason)) => {
            json!({"slug": slug, "status": status, "reason": reason, "live": live, "content": present(app, &content, true).await?})
        }
        None => Value::Null,
    };
    Ok(Json(
        json!({"listing": listing, "max_screenshots": MAX_SCREENSHOTS}),
    ))
}
/// GET /api/me/apps/{id}/listing: the app's listing as last sent in, and where its review stands.
async fn mine(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    own_app(&app, &jar, &id).await?;
    listing(&app, &id).await
}
/// POST /api/me/apps/{id}/listing/screenshot: one screenshot through the image pipeline
/// (fingerprinted, covered by Take It Down). It joins the listing when its key is sent in.
async fn screenshot(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let (user, _, _) = own_app(&app, &jar, &id).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Uploads aren't available yet."));
    }
    // Screenshots are kept like setup photos: any shape, at 400 and 1600 pixels.
    let (bytes, _, _) = media::read_upload(multipart, Kind::SetupPhoto).await?;
    profiles::rate(&app, format!("image-upload:{}", user.id), 20, 3600).await?;
    let processed = media::process_async(bytes, Kind::SetupPhoto, None).await?;
    media::store(&app, &processed).await?;
    let mut tx = app.db.begin().await?;
    media::record(&mut tx, &user.id, Kind::SetupPhoto, &processed).await?;
    let key = &processed.stored;
    // Not on a listing yet: it is deleted after a day unless a listing (or anything else) uses it.
    if !referenced(&mut tx, key).await? && !media::in_use(&mut tx, key).await? {
        sqlx::query(
            "UPDATE media_objects SET delete_after=now()+interval '1 day' WHERE key LIKE $1||'/%'",
        )
        .bind(key)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"key": key, "small": profiles::media_url(&app, &format!("{key}/400.webp")), "large": profiles::media_url(&app, &format!("{key}/1600.webp"))}),
    ))
}
/// Where a listing's board comes from.
#[derive(Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum BoardChoice {
    /// "This game sets its own board": nothing to add, the game defines it when connected.
    Game,
    /// The board the listing already has.
    Keep,
    /// The owner's draft in Studio → Board.
    Draft,
    /// One of the owner's library boards.
    Library { id: String },
    /// An uploaded `.sverboard.json` file.
    File { file: Value },
}
#[derive(Deserialize)]
pub struct Submit {
    name: String,
    summary: String,
    description: String,
    link: String,
    #[serde(default)]
    screenshots: Vec<String>,
    board: BoardChoice,
}
/// A URL-safe name for the listing's page, from its name.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}
/// PUT /api/me/apps/{id}/listing: sends in a new listing or a change. Either waits for staff; an
/// approved listing stays up as it was until the change is approved.
async fn submit(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Submit>,
) -> Res<Json<Value>> {
    let (user, icon, suspended) = own_app(&app, &jar, &id).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    if suspended {
        return Err(Fail::conflict(
            "This app is suspended, so its listing can't change.",
        ));
    }
    let name = text::plain(&input.name, "name", 3, 40, 0, true)?;
    let summary = text::plain(&input.summary, "summary", 1, 120, 0, true)?;
    let description = text::plain(&input.description, "description", 1, 2000, 40, true)?;
    let link = text::website_url(&input.link, "link")?;
    let shots = &input.screenshots;
    let well_formed = |k: &String| {
        k.strip_prefix("setup/")
            .is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    let distinct: std::collections::HashSet<&String> = shots.iter().collect();
    if shots.len() > MAX_SCREENSHOTS
        || distinct.len() != shots.len()
        || !shots.iter().all(well_formed)
    {
        return Err(Fail::field("screenshots", "Add up to 4 screenshots."));
    }
    let existing: Option<(Value, Option<Value>)> =
        sqlx::query_as("SELECT content,approved FROM catalog_listings WHERE app_id=$1")
            .bind(&id)
            .fetch_optional(&app.db)
            .await?;
    let board = match input.board {
        BoardChoice::Game => Value::Null,
        BoardChoice::Keep => match &existing {
            Some((content, _)) => content["board"].clone(),
            None => return Err(Fail::field("board", "Choose a board.")),
        },
        BoardChoice::Draft => boards::shareable(boards::saved(&app, &user.id, None).await?)?,
        BoardChoice::Library { id } => {
            boards::shareable(boards::saved(&app, &user.id, Some(&id)).await?)?
        }
        BoardChoice::File { file } => {
            if file["sver_board"] != 1 {
                return Err(Fail::bad(
                    "This board file is in a format S.V.E.R doesn't read yet.",
                ));
            }
            boards::shareable(file["board"].clone())?
        }
    };
    profiles::rate(&app, format!("catalog-submit:{}", user.id), 20, 3600).await?;
    let content = json!({"name": name, "summary": summary, "description": description, "link": link, "icon": icon, "screenshots": shots, "board": board});
    let mut tx = app.db.begin().await?;
    // Each screenshot is the owner's own upload (or already on this listing) and not held.
    let own: i64 = sqlx::query_scalar("SELECT count(*) FROM unnest($1::text[]) k WHERE EXISTS(SELECT 1 FROM media_objects m WHERE m.key=k||'/400.webp' AND m.owner_id=$2) OR EXISTS(SELECT 1 FROM catalog_listings l WHERE l.app_id=$3 AND (jsonb_exists(l.content->'screenshots',k) OR jsonb_exists(l.approved->'screenshots',k)))")
        .bind(shots)
        .bind(&user.id)
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    let held = media::removal::held_roots(&mut tx, &keys(&content)).await?;
    if own != shots.len() as i64 || shots.iter().any(|k| held.contains(k)) {
        return Err(Fail::field(
            "screenshots",
            "One of these screenshots can't be used. Upload it again.",
        ));
    }
    // The page's address comes from the name; a second listing with that name (or a name with no
    // Latin letters or digits) gets the app's ID added.
    let mut slug = slug(&name);
    let taken: bool = sqlx::query_scalar(
        "SELECT $1='' OR EXISTS(SELECT 1 FROM catalog_listings WHERE slug=$1 AND app_id<>$2)",
    )
    .bind(&slug)
    .bind(&id)
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        let tail: String = id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(11)
            .collect();
        slug = format!("{slug}-{}", tail.to_lowercase())
            .trim_start_matches('-')
            .to_string();
    }
    // Going back to exactly what's approved needs no second review. The address is fixed once the
    // listing has been public.
    sqlx::query("INSERT INTO catalog_listings(app_id,slug,content,status) VALUES($1,$2,$3,'PENDING')
        ON CONFLICT (app_id) DO UPDATE SET content=EXCLUDED.content,reason=NULL,submitted_at=now(),
            status=CASE WHEN catalog_listings.approved=EXCLUDED.content THEN 'APPROVED' ELSE 'PENDING' END,
            slug=CASE WHEN catalog_listings.approved_at IS NULL THEN EXCLUDED.slug ELSE catalog_listings.slug END")
        .bind(&id)
        .bind(&slug)
        .bind(&content)
        .execute(&mut *tx)
        .await?;
    for key in keys(&content) {
        sqlx::query("UPDATE media_objects SET delete_after=NULL WHERE key LIKE $1||'/%'")
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    if let Some((old, approved)) = &existing {
        let approved = approved.as_ref().unwrap_or(&Value::Null);
        release(&mut tx, &[old], &[&content, approved]).await?;
    }
    tx.commit().await?;
    listing(&app, &id).await
}
/// DELETE /api/me/apps/{id}/listing: takes the listing out of the catalog.
async fn withdraw(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    own_app(&app, &jar, &id).await?;
    let mut tx = app.db.begin().await?;
    forget(&mut tx, &id).await?;
    tx.commit().await?;
    listing(&app, &id).await
}

// ---- Browsing (/crowdsync) ----

#[derive(Deserialize)]
pub struct Browse {
    #[serde(default)]
    q: String,
    cursor: Option<String>,
}
/// GET /api/crowdsync?q=&cursor=: approved listings, newest first, searched by name.
async fn browse(State(app): State<App>, Query(input): Query<Browse>) -> Res<Json<Value>> {
    let cursor = profiles::parse_cursor(&input.cursor)?;
    let q: String = input.q.trim().chars().take(40).collect();
    let pattern = format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows: Vec<(DateTime<Utc>, String, Value, Value, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT l.approved_at,l.slug,l.approved,{DEVELOPER},{INSTALLS} {PUBLIC} AND l.approved->>'name' ILIKE $1 AND ($2::timestamptz IS NULL OR (l.approved_at,l.slug)<($2,$3)) ORDER BY l.approved_at DESC,l.slug DESC LIMIT 31"
    )))
    .bind(pattern)
    .bind(cursor.as_ref().map(|c| c.0))
    .bind(cursor.as_ref().map_or("", |c| c.1.as_str()))
    .fetch_all(&app.db)
    .await?;
    let next = (rows.len() > 30).then(|| profiles::make_cursor(rows[29].0, &rows[29].1));
    let mut items = Vec::new();
    for (_, slug, approved, developer, installs) in rows.into_iter().take(30) {
        let shown = present(&app, &approved, false).await?;
        items.push(json!({"slug": slug, "name": shown["name"], "summary": shown["summary"], "icon": shown["icon"], "sets_own_board": shown["board"].is_null(), "developer": developer, "installs": installs}));
    }
    Ok(Json(json!({"items": items, "next_cursor": next})))
}
/// A public listing by its page address: (app ID, approved content, developer, installs, first
/// approved).
async fn public(app: &App, slug: &str) -> Res<(String, Value, Value, i64, DateTime<Utc>)> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT l.app_id,l.approved,{DEVELOPER},{INSTALLS},l.approved_at {PUBLIC} AND l.slug=$1"
    )))
    .bind(slug)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Fail::missing)
}
/// GET /api/crowdsync/{slug}: one listing's page.
async fn detail(State(app): State<App>, Path(slug): Path<String>) -> Res<Json<Value>> {
    let (_, approved, developer, installs, since) = public(&app, &slug).await?;
    let mut body = present(&app, &approved, false).await?;
    body["slug"] = json!(slug);
    body["developer"] = developer;
    body["installs"] = json!(installs);
    body["listed_at"] = json!(since);
    Ok(Json(body))
}
/// POST /api/crowdsync/{slug}/install: "Add to my boards". The listing's board is copied into the
/// channel's library, checked like an imported file, and the channel is counted once.
async fn install(
    State(app): State<App>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> Res<Json<Value>> {
    let (tx, user, session) = auth::session(&app, &jar, false).await?;
    tx.commit().await?;
    auth::authorize_streaming(&user, &session).map_err(|_| {
        Fail::denied("Adding a board needs a verified email and two-factor sign-in.")
    })?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let (id, approved, ..) = public(&app, &slug).await?;
    if approved["board"].is_null() {
        return Err(Fail::bad(
            "This game sets its own board. Connect the game to your channel instead.",
        ));
    }
    let name = approved["name"].as_str().unwrap_or_default();
    let source = format!("catalog:{id}");
    let changed = boards::add(&app, &user.id, name, &source, approved["board"].clone()).await?;
    sqlx::query(
        "INSERT INTO catalog_installs(app_id,channel_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(&id)
    .bind(&user.id)
    .execute(&app.db)
    .await?;
    Ok(Json(
        json!({"added": true, "name": name, "changed": changed}),
    ))
}

// ---- Review (/admin/catalog) ----

/// GET /api/admin/catalog: listings waiting for review (oldest first), then everything public.
async fn review_queue(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let rows: Vec<(Value, Value, Option<Value>)> = sqlx::query_as("SELECT jsonb_build_object('client_id',l.app_id,'slug',l.slug,'status',l.status,'app',a.name,'owner',u.username,'suspended',a.suspended_at IS NOT NULL,'submitted_at',l.submitted_at),l.content,l.approved
        FROM catalog_listings l JOIN dev_apps a ON a.id=l.app_id LEFT JOIN users u ON u.id=a.owner_id
        WHERE l.status='PENDING' OR l.approved IS NOT NULL ORDER BY l.status<>'PENDING',l.submitted_at LIMIT 200")
        .fetch_all(&app.db)
        .await?;
    let mut items = Vec::new();
    for (mut item, content, approved) in rows {
        if item["status"] == "PENDING" {
            item["pending"] = present(&app, &content, false).await?;
        }
        if let Some(approved) = approved {
            item["live"] = present(&app, &approved, false).await?;
        }
        items.push(item);
    }
    Ok(Json(json!({"items": items})))
}
#[derive(Deserialize)]
pub struct Review {
    action: String,
    note: Option<String>,
}
/// POST /api/admin/catalog/{id}/review: approve or reject what's waiting, or remove a listing
/// from the catalog. The owner sees the reason for a rejection or removal.
async fn review(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Review>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", input.action != "approve")?;
    let mut tx = app.db.begin().await?;
    let row: Option<(String, Value, Option<Value>, String)> = sqlx::query_as(
        "SELECT slug,content,approved,status FROM catalog_listings WHERE app_id=$1 FOR UPDATE",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;
    let (slug, content, approved, status) = row.ok_or_else(Fail::missing)?;
    let live = approved.as_ref().unwrap_or(&Value::Null);
    match (input.action.as_str(), status == "PENDING") {
        ("approve", true) => {
            sqlx::query("UPDATE catalog_listings SET approved=content,status='APPROVED',reason=NULL,approved_at=coalesce(approved_at,now()) WHERE app_id=$1")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            release(&mut tx, &[live], &[&content]).await?;
        }
        ("reject", true) => {
            sqlx::query("UPDATE catalog_listings SET status='REJECTED',reason=$2 WHERE app_id=$1")
                .bind(&id)
                .bind(&note)
                .execute(&mut *tx)
                .await?;
        }
        ("remove", pending) if pending || approved.is_some() => {
            sqlx::query("UPDATE catalog_listings SET approved=NULL,status='REMOVED',reason=$2 WHERE app_id=$1")
                .bind(&id)
                .bind(&note)
                .execute(&mut *tx)
                .await?;
            release(&mut tx, &[live], &[&content]).await?;
        }
        ("approve" | "reject" | "remove", _) => {
            return Err(Fail::conflict(
                "Nothing on this listing is waiting for review.",
            ));
        }
        _ => {
            return Err(Fail::field("action", "Choose approve, reject or remove."));
        }
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        &format!("catalog_{}", input.action),
        "catalog_listing",
        &id,
        &[],
        &note,
        json!({"slug": slug, "name": content["name"]}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/me/apps/{id}/listing",
            get(mine)
                .put(submit)
                .delete(withdraw)
                .layer(DefaultBodyLimit::max(96 * 1024)),
        )
        .route(
            "/api/me/apps/{id}/listing/screenshot",
            post(screenshot).layer(DefaultBodyLimit::max(media::UPLOAD_LIMIT)),
        )
        .route("/api/crowdsync", get(browse))
        .route("/api/crowdsync/{slug}", get(detail))
        .route("/api/crowdsync/{slug}/install", post(install))
        .route("/api/admin/catalog", get(review_queue))
        .route("/api/admin/catalog/{id}/review", post(review))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_addresses_and_image_keys() {
        assert_eq!(
            slug("  Crowd Runner 2: The Return! "),
            "crowd-runner-2-the-return"
        );
        assert_eq!(slug("日本"), "");
        let content = json!({"icon": "avatars/a", "screenshots": ["setup/b", "setup/c"]});
        assert_eq!(keys(&content), ["setup/b", "setup/c", "avatars/a"]);
        assert!(keys(&json!({"icon": null, "screenshots": []})).is_empty());
    }
}
