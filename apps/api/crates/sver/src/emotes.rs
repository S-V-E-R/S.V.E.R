//! Channel-local emotes. Reuses media storage, chat rules and the standing/report system.
use crate::{
    App,
    media::{self, Kind},
    profiles::{self, Fail, Res},
    safety, text,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    routing::{delete, get},
};
use axum_extra::extract::cookie::CookieJar;
use serde_json::{Value, json};
use sqlx::PgConnection;

pub const SIZES: [u32; 3] = [28, 56, 112];
/// Subscriber emote slots per tier, on top of the 10 open emotes (docs/SUPPORT.md).
const TIER_SLOTS: i64 = 5;

fn code(value: &str) -> Res<&str> {
    if !(3..=20).contains(&value.len()) || !value.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Fail::field("code", "Use 3–20 ASCII letters and digits."));
    }
    text::filter(value, "code")?;
    Ok(value)
}
fn urls(app: &App, row: &mut Value) {
    let key = row["image_key"].as_str().unwrap_or("").to_string();
    row.as_object_mut().unwrap().remove("image_key");
    row["image"] = json!({"28": profiles::media_url(app, &format!("{key}/28.webp")), "56": profiles::media_url(app, &format!("{key}/56.webp")), "112": profiles::media_url(app, &format!("{key}/112.webp"))});
}

async fn hidden(app: &App, rows: &[Value]) -> Res<std::collections::HashSet<String>> {
    let roots: Vec<String> = rows
        .iter()
        .filter_map(|row| row["image_key"].as_str().map(str::to_owned))
        .collect();
    media::removal::held_roots(&mut *app.db.acquire().await?, &roots).await
}

/// The current catalog, also used on reconnect and by the HTTP chat fallback.
pub async fn catalog(app: &App, channel: &str) -> Res<Vec<Value>> {
    let mut rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',e.id,'code',e.code,'image_key',e.image_key,'tier',e.tier) FROM channel_emotes e JOIN channel_users c ON c.id=e.channel_id WHERE e.channel_id=$1 AND e.status='VISIBLE' AND c.eligible ORDER BY e.code")
        .bind(channel).fetch_all(&app.db).await?;
    let held = hidden(app, &rows).await?;
    rows.retain(|row| !held.contains(row["image_key"].as_str().unwrap_or("")));
    for row in &mut rows {
        urls(app, row);
    }
    Ok(rows)
}
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',e.id,'code',e.code,'image_key',e.image_key,'status',e.status,'tier',e.tier) FROM channel_emotes e WHERE channel_id=$1 ORDER BY created_at,id")
        .bind(&user.id).fetch_all(&app.db).await?;
    let held = hidden(&app, &rows).await?;
    for row in &mut rows {
        if held.contains(row["image_key"].as_str().unwrap_or("")) {
            row["status"] = json!("UNAVAILABLE");
        }
        if row["status"] == "VISIBLE" {
            urls(&app, row);
        } else {
            row.as_object_mut().unwrap().remove("image_key");
        }
    }
    Ok(Json(
        json!({"items":rows,"max_emotes":10,"max_tier_emotes":TIER_SLOTS}),
    ))
}
async fn upload(State(app): State<App>, jar: CookieJar, multipart: Multipart) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Image uploads aren't available yet."));
    }
    profiles::rate(&app, format!("image-upload:{}", user.id), 20, 3600).await?;
    let (bytes, _, fields) = media::read_upload(multipart, Kind::Emote).await?;
    let code = code(fields.get("code").and_then(Value::as_str).unwrap_or(""))?;
    let tier: Option<i16> = match fields.get("tier").and_then(Value::as_str).unwrap_or("") {
        "" => None,
        "1" => Some(1),
        "2" => Some(2),
        "3" => Some(3),
        _ => return Err(Fail::field("tier", "Choose open, or tier 1, 2 or 3.")),
    };
    crate::moderation::check_emote_code(&mut *app.db.acquire().await?, &user.id, code).await?;
    let processed = media::process_async(bytes, Kind::Emote, None).await?;
    media::store(&app, &processed).await?;
    let mut tx = app.db.begin().await?;
    // The media publication lock also serializes the quota check with concurrent uploads.
    media::record(&mut tx, &user.id, Kind::Emote, &processed).await?;
    profiles::ensure_unrestricted(&mut tx, &user.id).await?;
    crate::moderation::check_emote_code(&mut tx, &user.id, code).await?;
    let (count, duplicate): (i64, bool) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE tier IS NOT DISTINCT FROM $3),coalesce(bool_or(code=$2),false) FROM channel_emotes WHERE channel_id=$1",
    )
    .bind(&user.id)
    .bind(code)
    .bind(tier)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate {
        return Err(Fail::conflict("That code is already in your channel."));
    }
    match tier {
        None if count >= 10 => return Err(Fail::conflict("You can add up to 10 open emotes.")),
        Some(t) if count >= TIER_SLOTS => {
            return Err(Fail::conflict(format!(
                "You can add up to {TIER_SLOTS} Tier {t} emotes."
            )));
        }
        _ => {}
    }
    let id = profiles::new_id();
    sqlx::query(
        "INSERT INTO channel_emotes(id,channel_id,code,image_key,tier) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(code)
    .bind(&processed.stored)
    .bind(tier)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    app.chat
        .publish(&user.id, None, 0, json!({"type":"emotes"}));
    let mut row =
        json!({"id":id,"code":code,"image_key":processed.stored,"status":"VISIBLE","tier":tier});
    urls(&app, &mut row);
    Ok(Json(row))
}
async fn delete_mine(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(1414087745)")
        .execute(&mut *tx)
        .await?;
    let deleted = sqlx::query("DELETE FROM channel_emotes WHERE id=$1 AND channel_id=$2")
        .bind(&id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(Fail::missing());
    }
    tx.commit().await?;
    app.chat
        .publish(&user.id, None, 0, json!({"type":"emotes"}));
    Ok(Json(json!({"saved":true})))
}

// Public module interfaces for moderation, removals and media collection.
pub async fn target(db: &mut PgConnection, id: &str) -> Res<Option<(String, String, Value)>> {
    let row: Option<(String,String,Value)> = sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('code',e.code,'image',e.image_key||'/112.webp') FROM channel_emotes e JOIN channel_users c ON c.id=e.channel_id WHERE e.id=$1 AND e.status='VISIBLE' AND c.eligible").bind(id).fetch_optional(&mut *db).await?;
    if let Some((_, _, value)) = &row
        && media::removal::held(db, value["image"].as_str().unwrap_or("")).await?
    {
        return Ok(None);
    }
    Ok(row)
}
pub async fn owner(db: &mut PgConnection, id: &str) -> Res<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT channel_id FROM channel_emotes WHERE id=$1")
            .bind(id)
            .fetch_optional(db)
            .await?,
    )
}
pub async fn current(db: &mut PgConnection, id: &str) -> Res<Value> {
    let mut row: Value = sqlx::query_scalar("SELECT jsonb_build_object('code',code,'image',image_key||'/112.webp','status',status) FROM channel_emotes WHERE id=$1").bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null);
    if let Some(key) = row["image"].as_str()
        && media::removal::held(db, key).await?
    {
        row["image"] = Value::Null;
    }
    Ok(row)
}
pub async fn remove(db: &mut PgConnection, id: &str) -> Res<Value> {
    let previous: String =
        sqlx::query_scalar("SELECT status FROM channel_emotes WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(Fail::missing)?;
    sqlx::query("UPDATE channel_emotes SET status='REMOVED' WHERE id=$1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(json!({"type":"emote","id":id,"previous":previous}))
}
pub async fn restore(db: &mut PgConnection, id: &str, previous: &str) -> Res<()> {
    sqlx::query("UPDATE channel_emotes SET status=$2 WHERE id=$1 AND status='REMOVED'")
        .bind(id)
        .bind(previous)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn reviewed(db: &mut PgConnection, id: &str) -> Res<()> {
    sqlx::query("UPDATE channel_emotes SET reviewed_at=now() WHERE id=$1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn remove_media(db: &mut PgConnection, root: &str) -> Res<Vec<String>> {
    Ok(
        sqlx::query_scalar("DELETE FROM channel_emotes WHERE image_key=$1 RETURNING channel_id")
            .bind(root)
            .fetch_all(db)
            .await?,
    )
}
pub async fn referenced(db: &mut PgConnection, key: &str) -> Res<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_emotes WHERE image_key=$1)")
            .bind(media::removal::root_of_key(key))
            .fetch_one(db)
            .await?,
    )
}
async fn review_queue(
    State(app): State<App>,
    jar: CookieJar,
    Query(query): profiles::CursorQuery,
) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let cursor = profiles::parse_cursor(&query.cursor)?;
    let rows: Vec<(chrono::DateTime<chrono::Utc>,String,Value)> = sqlx::query_as("SELECT e.created_at,e.id,jsonb_build_object('id',e.id,'code',e.code,'image_key',e.image_key,'username',c.username,'created_at',e.created_at) FROM channel_emotes e JOIN channel_users c ON c.id=e.channel_id WHERE e.reviewed_at IS NULL AND e.status='VISIBLE' AND c.eligible AND ($1::timestamptz IS NULL OR (e.created_at,e.id)>($1,$2)) ORDER BY e.created_at,e.id LIMIT 51")
        .bind(cursor.as_ref().map(|c|c.0)).bind(cursor.as_ref().map(|c|c.1.as_str()).unwrap_or("")).fetch_all(&app.db).await?;
    let next = (rows.len() > 50).then(|| profiles::make_cursor(rows[49].0, &rows[49].1));
    let mut items: Vec<Value> = rows.into_iter().take(50).map(|r| r.2).collect();
    let held = hidden(&app, &items).await?;
    for row in &mut items {
        if held.contains(row["image_key"].as_str().unwrap_or("")) {
            row.as_object_mut().unwrap().remove("image_key");
            row["unavailable"] = json!(true);
        } else {
            urls(&app, row);
        }
    }
    Ok(Json(json!({"items":items,"next_cursor":next})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/me/emotes",
            get(mine)
                .post(upload)
                .layer(DefaultBodyLimit::max(1024 * 1024 + 16 * 1024)),
        )
        .route("/api/me/emotes/{id}", delete(delete_mine))
        .route("/api/admin/media", get(review_queue))
}
