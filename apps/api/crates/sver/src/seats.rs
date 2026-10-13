//! The Wall (docs/WALL.md): 1,000 cells on `/wall`, sold once as seats, tables and halls. A tile
//! buys its spot on that page and nothing else: nothing here is read by discovery, the war,
//! badges, profiles or chat. (`wall.rs` is the profile wall of posts, a different thing.)
//!
//! A hold reserves every cell of a spot in one transaction (`wall_cells` is keyed by cell), so
//! two buyers never overlap. The buyer then fills the tile in and pays with a one-time Stripe
//! Checkout; support.rs's webhook calls [`paid`]. Staff approve tiles before they show.
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
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

pub const COLUMNS: i32 = 40;
pub const ROWS: i32 = 25;
/// Stored picture sizes; a tile needs one at least as wide as its minimum.
pub const IMAGE_SIZES: [u32; 3] = [100, 200, 400];
const HOLD_MINUTES: i32 = 15;
/// Stripe's shortest Checkout lifetime. A payment after the 15-minute hold is handled in [`paid`].
const CHECKOUT_MINUTES: i64 = 31;

/// (name, side in cells, smallest picture in pixels).
const SIZES: [(&str, i32, u32); 3] = [("seat", 1, 100), ("table", 2, 200), ("hall", 4, 400)];
fn size_named(name: &str) -> Option<(&'static str, i32, u32)> {
    SIZES.iter().copied().find(|s| s.0 == name)
}
fn size_of(side: i32) -> (&'static str, i32, u32) {
    SIZES
        .iter()
        .copied()
        .find(|s| s.1 == side)
        .unwrap_or(SIZES[0])
}
/// Every cell a spot covers, or None when it doesn't fit on the grid.
pub fn cells(x: i32, y: i32, side: i32) -> Option<Vec<(i32, i32)>> {
    if x < 0 || y < 0 || x + side > COLUMNS || y + side > ROWS {
        return None;
    }
    Some(
        (y..y + side)
            .flat_map(|cy| (x..x + side).map(move |cx| (cx, cy)))
            .collect(),
    )
}

#[derive(Clone, sqlx::FromRow)]
struct Settings {
    prices: Value,
    stages: Vec<i32>,
    limits: Value,
    sales_opened_at: Option<chrono::DateTime<chrono::Utc>>,
}
async fn settings(db: &mut PgConnection) -> Res<Settings> {
    Ok(
        sqlx::query_as("SELECT prices,stages,limits,sales_opened_at FROM wall_settings")
            .fetch_one(db)
            .await?,
    )
}
/// Cells sold: paid tiles that weren't refunded (a removed tile's spot stays empty and sold).
async fn sold(db: &mut PgConnection) -> Res<i64> {
    Ok(sqlx::query_scalar("SELECT coalesce(sum(size*size),0)::bigint FROM wall_tiles WHERE paid_at IS NOT NULL AND status NOT IN ('refund_due','refunded')")
        .fetch_one(db)
        .await?)
}
/// The price stage for `sold` cells: 0 below the first threshold, and so on.
pub fn stage(stages: &[i32], sold: i64) -> usize {
    stages.iter().filter(|&&t| sold >= i64::from(t)).count()
}
fn price(settings: &Settings, size: &str, stage: usize) -> Option<i64> {
    settings.prices[size].get(stage)?.as_i64()
}

/// Who may buy: signed in, verified and 18 or older by date of birth.
async fn adult(db: &mut PgConnection, user: &crate::auth::User) -> Res<bool> {
    Ok(user.email_verified
        && crate::support::age(db, &user.id)
            .await?
            .is_some_and(|a| a >= 18))
}
async fn buyer(app: &App, jar: &CookieJar) -> Res<crate::auth::User> {
    let user = profiles::signed_in(app, jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to buy a seat.")?;
    if !adult(&mut *app.db.acquire().await?, &user).await? {
        return Err(Fail::denied(
            "Seats on the Wall are for adults (18 or older).",
        ));
    }
    Ok(user)
}
/// Holds and checkouts need open sales and the "Wall sales" switch on.
async fn selling(db: &mut PgConnection) -> Res<Settings> {
    switches::guard(db, "wall_sales").await?;
    let settings = settings(db).await?;
    if settings.sales_opened_at.is_none() {
        return Err(Fail::unavailable("Seats on the Wall aren't on sale yet."));
    }
    Ok(settings)
}

fn image_url(app: &App, stored: Option<&str>, size: u32) -> Value {
    let Some(stored) = stored else {
        return Value::Null;
    };
    let largest: u32 = stored
        .split_once('@')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(IMAGE_SIZES[0]);
    let pick = IMAGE_SIZES
        .iter()
        .copied()
        .filter(|s| *s <= largest)
        .find(|s| *s >= size)
        .unwrap_or(largest);
    json!(profiles::media_url(
        app,
        &format!("{}/{pick}.webp", media::stored_prefix(stored))
    ))
}

#[derive(sqlx::FromRow)]
struct Tile {
    n: i64,
    owner_id: Option<String>,
    x: i32,
    y: i32,
    size: i32,
    status: String,
    price_cents: Option<i32>,
    image_key: Option<String>,
    name: Option<String>,
    link: Option<String>,
    message: Option<String>,
    crest: Option<String>,
    pending: Option<Value>,
    review_note: String,
    hold_until: chrono::DateTime<chrono::Utc>,
    checkout_id: Option<String>,
    payment_intent: Option<String>,
    paid_at: Option<chrono::DateTime<chrono::Utc>>,
    rejected_at: Option<chrono::DateTime<chrono::Utc>>,
    approved_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
}
const TILE: &str = "SELECT n,owner_id,x,y,size,status,price_cents,image_key,name,link,message,crest,pending,review_note,hold_until,checkout_id,payment_intent,paid_at,rejected_at,approved_at,created_at FROM wall_tiles";
async fn load(db: &mut PgConnection, n: i64, lock: bool) -> Res<Tile> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{TILE} WHERE n=$1{}",
        if lock { " FOR UPDATE" } else { "" }
    )))
    .bind(n)
    .fetch_optional(db)
    .await?
    .ok_or_else(Fail::missing)
}
/// What everyone sees of a tile: live tiles show their content, others only that the spot is taken.
fn public(app: &App, tile: &Tile, owner: Option<&str>) -> Value {
    let (size, _, _) = size_of(tile.size);
    let mut out = json!({"n":tile.n,"x":tile.x,"y":tile.y,"size":size,"side":tile.size,
    "state": match tile.status.as_str() {
        "live" => "live",
        "held" => "claiming",
        "removed" => "removed",
        _ => "review",
    }});
    if tile.status == "live" {
        out["name"] = json!(tile.name);
        out["link"] = json!(tile.link);
        out["message"] = json!(tile.message);
        out["crest"] = json!(tile.crest);
        out["image"] = image_url(app, tile.image_key.as_deref(), 100 * tile.size as u32);
        out["image_large"] = image_url(app, tile.image_key.as_deref(), 400);
        out["owner"] = json!(owner);
        out["approved_at"] = json!(tile.approved_at);
    }
    out
}
/// The owner's view of their own tile: status, draft content, price and review notes.
fn private(app: &App, tile: &Tile) -> Value {
    let (size, _, min) = size_of(tile.size);
    json!({"n":tile.n,"x":tile.x,"y":tile.y,"size":size,"side":tile.size,"status":tile.status,
    "name":tile.name,"link":tile.link,"message":tile.message,"crest":tile.crest,
    "image":image_url(app,tile.image_key.as_deref(),400),"min_picture":min,
    "pending":tile.pending.as_ref().map(|p| {
        let mut p = p.clone();
        p["image"] = image_url(app, p["image_key"].as_str(), 400);
        p
    }),
    "review_note":tile.review_note,"price_cents":tile.price_cents,"hold_until":tile.hold_until,
    "paid_at":tile.paid_at,"rejected_at":tile.rejected_at,"approved_at":tile.approved_at,
    "resubmit_until":tile.rejected_at.map(|at| at + chrono::Duration::days(30)),
    "refundable": matches!(tile.status.as_str(), "in_review" | "rejected"),
    "hold_active": tile.status == "held" && tile.hold_until > chrono::Utc::now(),
    "can_edit": match tile.status.as_str() {
        "live" | "in_review" => true,
        "rejected" => tile.rejected_at.is_some_and(|at| at + chrono::Duration::days(30) > chrono::Utc::now()),
        _ => false,
    }})
}

/// GET /api/wall: the grid, the count, prices and whether the viewer can buy.
async fn wall(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    let settings = settings(&mut db).await?;
    let sold = sold(&mut db).await?;
    let stage = stage(&settings.stages, sold);
    let rows: Vec<(Tile, Option<String>)> = {
        let tiles: Vec<Tile> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{TILE} WHERE status IN ('held','in_review','live','rejected','removed') ORDER BY y,x"
        )))
        .fetch_all(&mut *db)
        .await?;
        let mut out = Vec::new();
        for tile in tiles {
            let owner: Option<String> = match &tile.owner_id {
                Some(id) if tile.status == "live" => {
                    sqlx::query_scalar(
                        "SELECT username FROM users WHERE id=$1 AND deleted_at IS NULL",
                    )
                    .bind(id)
                    .fetch_optional(&mut *db)
                    .await?
                }
                _ => None,
            };
            out.push((tile, owner));
        }
        out
    };
    let taken: i64 = rows.iter().map(|(t, _)| i64::from(t.size * t.size)).sum();
    let viewer = profiles::viewer(&app, &jar).await?;
    let (can_buy, mine) = match &viewer {
        Some(user) => {
            let mine: Vec<Value> = rows
                .iter()
                .filter(|(t, _)| t.owner_id.as_deref() == Some(&user.id))
                .map(|(t, _)| private(&app, t))
                .collect();
            (adult(&mut db, user).await?, mine)
        }
        None => (false, vec![]),
    };
    let next = settings.stages.get(stage).copied();
    let counts: Value = sqlx::query_scalar("SELECT jsonb_build_object('table',count(*) FILTER (WHERE size=2),'hall',count(*) FILTER (WHERE size=4)) FROM wall_tiles WHERE status NOT IN ('expired','refund_due','refunded')")
        .fetch_one(&mut *db).await?;
    let open = settings.sales_opened_at.is_some() && !switches::off(&mut db, "wall_sales").await?;
    Ok(Json(json!({
        "columns": COLUMNS, "rows": ROWS,
        "tiles": rows.iter().map(|(t, o)| public(&app, t, o.as_deref())).collect::<Vec<_>>(),
        "taken": taken, "sold": sold, "full": taken >= i64::from(COLUMNS * ROWS),
        "stage": stage, "next_stage_at": next,
        "prices": SIZES.iter().map(|(name, side, min)| json!({"size":name,"side":side,"min_picture":min,"cents":price(&settings,name,stage),"limit":settings.limits[*name],"taken":counts[*name]})).collect::<Vec<_>>(),
        "sales_open": open, "can_buy": can_buy, "signed_in": viewer.is_some(), "mine": mine,
        "hold_minutes": HOLD_MINUTES,
    })))
}

/// GET /api/wall/{n}: one tile's page. Only live tiles are public; the owner sees their own.
async fn tile(State(app): State<App>, jar: CookieJar, Path(n): Path<i64>) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    let tile = load(&mut db, n, false).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    let owns = viewer
        .as_ref()
        .is_some_and(|v| tile.owner_id.as_deref() == Some(&v.id));
    if tile.status != "live" && !owns {
        return Err(Fail::missing());
    }
    let owner: Option<String> = match &tile.owner_id {
        Some(id) => {
            sqlx::query_scalar("SELECT username FROM users WHERE id=$1 AND deleted_at IS NULL")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        None => None,
    };
    let mut out = json!({"tile": public(&app, &tile, owner.as_deref())});
    if owns {
        out["mine"] = private(&app, &tile);
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
struct HoldInput {
    x: i32,
    y: i32,
    size: String,
}
/// POST /api/wall/holds: reserves a free spot for 15 minutes.
async fn hold(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<HoldInput>,
) -> Res<Json<Value>> {
    let user = buyer(&app, &jar).await?;
    let (size, side, _) = size_named(&input.size)
        .ok_or_else(|| Fail::field("size", "Choose a seat, table or hall."))?;
    let spot = cells(input.x, input.y, side)
        .ok_or_else(|| Fail::field("x", "That spot doesn't fit on the Wall."))?;
    profiles::rate(&app, format!("wall-hold:{}", user.id), 20, 3600).await?;
    let mut tx = app.db.begin().await?;
    let settings = selling(&mut tx).await?;
    // Size limits and one open hold per buyer serialize here; cells are guarded by their keys.
    sqlx::query("SELECT pg_advisory_xact_lock(1464421452)")
        .execute(&mut *tx)
        .await?;
    let open_holds: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wall_tiles WHERE owner_id=$1 AND status='held' AND hold_until>now()",
    )
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    if open_holds > 0 {
        return Err(Fail::conflict(
            "Finish or release your current hold before choosing another spot.",
        ));
    }
    if let Some(limit) = settings.limits[size].as_i64() {
        let used: i64 = sqlx::query_scalar("SELECT count(*) FROM wall_tiles WHERE size=$1 AND status NOT IN ('expired','refund_due','refunded')")
            .bind(side)
            .fetch_one(&mut *tx)
            .await?;
        if used >= limit {
            return Err(Fail::field_owned(
                "size",
                format!("All {limit} {size}s are taken. Choose a smaller size."),
            ));
        }
    }
    let n: i64 = sqlx::query_scalar("INSERT INTO wall_tiles(owner_id,x,y,size,hold_until) VALUES($1,$2,$3,$4,now()+make_interval(mins=>$5)) RETURNING n")
        .bind(&user.id).bind(input.x).bind(input.y).bind(side).bind(HOLD_MINUTES)
        .fetch_one(&mut *tx)
        .await?;
    let (xs, ys): (Vec<i32>, Vec<i32>) = spot.into_iter().unzip();
    let claimed = sqlx::query("INSERT INTO wall_cells(x,y,tile_n) SELECT x,y,$3 FROM unnest($1::int[],$2::int[]) c(x,y) ON CONFLICT DO NOTHING")
        .bind(&xs).bind(&ys).bind(n)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if claimed != xs.len() as u64 {
        return Err(Fail::conflict(
            "Part of that spot is already taken. Choose another.",
        ));
    }
    tx.commit().await?;
    let tile = load(&mut *app.db.acquire().await?, n, false).await?;
    Ok(Json(json!({"tile": private(&app, &tile)})))
}

/// DELETE /api/wall/{n}/hold: gives an unpaid hold back.
async fn release(State(app): State<App>, jar: CookieJar, Path(n): Path<i64>) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let tile = load(&mut tx, n, true).await?;
    if tile.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::missing());
    }
    if tile.status != "held" {
        return Err(Fail::conflict("Only an unpaid hold can be released."));
    }
    expire(&mut tx, n).await?;
    tx.commit().await?;
    Ok(Json(json!({"released": true})))
}
async fn expire(db: &mut PgConnection, n: i64) -> Res<()> {
    sqlx::query("DELETE FROM wall_cells WHERE tile_n=$1")
        .bind(n)
        .execute(&mut *db)
        .await?;
    sqlx::query("UPDATE wall_tiles SET status='expired',updated_at=now() WHERE n=$1")
        .bind(n)
        .execute(db)
        .await?;
    Ok(())
}

/// The tile's content from a multipart form: picture (optional when one is already set), name,
/// link, message and crest.
struct Content {
    image: Option<media::Processed>,
    name: String,
    link: Option<String>,
    message: Option<String>,
    crest: bool,
}
async fn content(multipart: Multipart, min: u32) -> Res<Content> {
    let mut multipart = multipart;
    let mut file = None;
    let mut fields = serde_json::Map::new();
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
            if bytes.len() > media::Kind::WallTile.max_bytes() {
                return Err(Fail::field("file", "Wall pictures can be up to 5 MB."));
            }
            if !bytes.is_empty() {
                file = Some(bytes.to_vec());
            }
        } else if bytes.len() <= 4096 {
            fields.insert(name, Value::String(String::from_utf8_lossy(&bytes).into()));
        }
    }
    let field = |name: &str| {
        fields
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let name = text::plain(&field("name"), "name", 1, 40, 0, true)?;
    let link = match field("link") {
        l if l.is_empty() => None,
        l => Some(text::website_url(&l, "link")?),
    };
    let message = match text::plain(&field("message"), "message", 0, 140, 0, true)? {
        m if m.is_empty() => None,
        m => Some(m),
    };
    let image = match file {
        Some(bytes) => {
            let processed = media::process_async(bytes, media::Kind::WallTile, None).await?;
            let largest = processed.variants.last().map_or(0, |v| v.width);
            if largest < min {
                return Err(Fail::field_owned(
                    "file",
                    format!("This size needs a picture at least {min} pixels wide."),
                ));
            }
            Some(processed)
        }
        None => None,
    };
    Ok(Content {
        image,
        name,
        link,
        message,
        crest: matches!(field("crest").as_str(), "on" | "true" | "1"),
    })
}
/// The buyer's faction for the crest, only when they asked for it and are enlisted.
async fn crest(db: &mut PgConnection, user: &str, wanted: bool) -> Res<Option<String>> {
    if !wanted {
        return Ok(None);
    }
    crate::factions::membership(db, user).await
}

/// POST /api/wall/{n}/checkout (multipart): fills the held tile in and starts Stripe Checkout at
/// the price stage of this moment.
async fn checkout(
    State(app): State<App>,
    jar: CookieJar,
    Path(n): Path<i64>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let user = buyer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let tile = load(&mut db, n, false).await?;
    if tile.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::missing());
    }
    if tile.status != "held" || tile.hold_until <= chrono::Utc::now() {
        return Err(Fail::conflict(
            "This hold has ended. Choose the spot again.",
        ));
    }
    drop(db);
    let (size, _, min) = size_of(tile.size);
    let content = content(multipart, min).await?;
    let image = content
        .image
        .as_ref()
        .ok_or_else(|| Fail::field("file", "Choose a picture for your tile."))?;
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Uploads aren't available yet."));
    }
    sec::reserve(&app, vec![format!("wall-checkout:{}", user.id)], 10, 3600).await?;
    media::store(&app, image).await?;
    let mut tx = app.db.begin().await?;
    let settings = selling(&mut tx).await?;
    let tile = load(&mut tx, n, true).await?;
    if tile.status != "held" || tile.hold_until <= chrono::Utc::now() {
        return Err(Fail::conflict(
            "This hold has ended. Choose the spot again.",
        ));
    }
    let stage = stage(&settings.stages, sold(&mut tx).await?);
    let cents = price(&settings, size, stage).ok_or_else(Fail::internal)?;
    media::record(&mut tx, &user.id, media::Kind::WallTile, image).await?;
    let crest = crest(&mut tx, &user.id, content.crest).await?;
    // One open checkout per hold: a new one first closes the last, so a second tab can't pay
    // twice. If Stripe can't close it, it was already paid or is finishing.
    if let Some(previous) = &tile.checkout_id {
        stripe::post(
            &app.http,
            &app.config.stripe,
            &format!("checkout/sessions/{previous}/expire"),
            &[],
            None,
        )
        .await
        .map_err(|_| Fail::conflict("A payment for this tile is already being processed."))?;
    }
    let origin = &app.config.origin;
    let expires = chrono::Utc::now().timestamp() + CHECKOUT_MINUTES * 60;
    let session = stripe::post(
        &app.http,
        &app.config.stripe,
        "checkout/sessions",
        &[
            ("mode", "payment".into()),
            ("line_items[0][quantity]", "1".into()),
            ("line_items[0][price_data][currency]", "usd".into()),
            ("line_items[0][price_data][unit_amount]", cents.to_string()),
            (
                "line_items[0][price_data][product_data][name]",
                format!("A {size} on the S.V.E.R Wall (tile {n})"),
            ),
            ("client_reference_id", user.id.clone()),
            ("metadata[sver_kind]", "wall".into()),
            ("payment_intent_data[metadata][sver_user]", user.id.clone()),
            ("payment_intent_data[receipt_email]", user.email.clone()),
            ("expires_at", expires.to_string()),
            ("success_url", format!("{origin}/wall/{n}?checkout=done")),
            ("cancel_url", format!("{origin}/wall/{n}")),
        ],
        Some(&format!("wall-checkout-{n}-{}", profiles::new_id())),
    )
    .await?;
    let (Some(id), Some(url)) = (session["id"].as_str(), session["url"].as_str()) else {
        return Err(Fail::unavailable("Stripe sent an unexpected response."));
    };
    sqlx::query("INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,detail) VALUES($1,$2,'wall',$3,$4)")
        .bind(id).bind(&user.id).bind(cents as i32).bind(json!({"tile": n}))
        .execute(&mut *tx).await?;
    let old = tile.image_key.clone();
    sqlx::query("UPDATE wall_tiles SET image_key=$2,name=$3,link=$4,message=$5,crest=$6,price_cents=$7,stage=$8,checkout_id=$9,updated_at=now() WHERE n=$1")
        .bind(n).bind(&image.stored).bind(&content.name).bind(&content.link).bind(&content.message).bind(&crest)
        .bind(cents as i32).bind(stage as i32).bind(id)
        .execute(&mut *tx).await?;
    media::queue_delete(&mut tx, old.as_deref(), Some(&image.stored)).await?;
    tx.commit().await?;
    Ok(Json(json!({"url": url})))
}

/// A paid Wall checkout (from the Stripe webhook, in its transaction). A payment that arrives
/// after its hold lapsed gets the spot back if it's still free, and is refunded otherwise.
pub(crate) async fn paid(
    tx: &mut PgConnection,
    session: &str,
    detail: &Value,
    intent: Option<&str>,
) -> Res<()> {
    let Some(n) = detail["tile"].as_i64() else {
        return Ok(());
    };
    let tile = load(tx, n, true).await?;
    // A payment for a superseded checkout, or for a tile already paid, is refunded in full.
    if tile.checkout_id.as_deref() != Some(session)
        || !matches!(tile.status.as_str(), "held" | "expired")
    {
        if let Some(intent) = intent {
            sqlx::query("INSERT INTO wall_extra_payments(payment_intent,tile_n) VALUES($1,$2) ON CONFLICT DO NOTHING")
                .bind(intent)
                .bind(n)
                .execute(&mut *tx)
                .await?;
        }
        return Ok(());
    }
    let status = match tile.status.as_str() {
        "held" => "in_review",
        "expired" => {
            let (xs, ys): (Vec<i32>, Vec<i32>) = cells(tile.x, tile.y, tile.size)
                .unwrap_or_default()
                .into_iter()
                .unzip();
            let claimed = sqlx::query("INSERT INTO wall_cells(x,y,tile_n) SELECT x,y,$3 FROM unnest($1::int[],$2::int[]) c(x,y) ON CONFLICT DO NOTHING")
                .bind(&xs).bind(&ys).bind(n)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if claimed == xs.len() as u64 {
                "in_review"
            } else {
                sqlx::query("DELETE FROM wall_cells WHERE tile_n=$1")
                    .bind(n)
                    .execute(&mut *tx)
                    .await?;
                "refund_due"
            }
        }
        _ => return Ok(()),
    };
    sqlx::query("UPDATE wall_tiles SET status=$2,paid_at=now(),payment_intent=$3,updated_at=now() WHERE n=$1")
        .bind(n)
        .bind(status)
        .bind(intent)
        .execute(tx)
        .await?;
    Ok(())
}

/// PUT /api/wall/{n} (multipart): the owner changes a paid tile. A live tile's change waits for
/// review while the approved version stays up; a tile in review or rejected is replaced and
/// (re)enters review. The spot and size never change.
async fn edit(
    State(app): State<App>,
    jar: CookieJar,
    Path(n): Path<i64>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let tile = load(&mut *app.db.acquire().await?, n, false).await?;
    if tile.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::missing());
    }
    let (_, _, min) = size_of(tile.size);
    let content = content(multipart, min).await?;
    profiles::rate(&app, format!("wall-edit:{}", user.id), 20, 3600).await?;
    if let Some(image) = &content.image {
        media::store(&app, image).await?;
    }
    let mut tx = app.db.begin().await?;
    let tile = load(&mut tx, n, true).await?;
    if let Some(image) = &content.image {
        media::record(&mut tx, &user.id, media::Kind::WallTile, image).await?;
    }
    let crest = crest(&mut tx, &user.id, content.crest).await?;
    let image = content
        .image
        .as_ref()
        .map(|i| i.stored.clone())
        .or_else(|| match tile.status.as_str() {
            "live" => tile
                .pending
                .as_ref()
                .and_then(|p| p["image_key"].as_str().map(String::from))
                .or(tile.image_key.clone()),
            _ => tile.image_key.clone(),
        });
    match tile.status.as_str() {
        "live" => {
            let previous = tile
                .pending
                .as_ref()
                .and_then(|p| p["image_key"].as_str().map(String::from));
            sqlx::query("UPDATE wall_tiles SET pending=$2,updated_at=now() WHERE n=$1")
                .bind(n)
                .bind(json!({"image_key":image,"name":content.name,"link":content.link,"message":content.message,"crest":crest,"submitted_at":chrono::Utc::now()}))
                .execute(&mut *tx)
                .await?;
            // A replaced, never-approved picture is no longer needed.
            if previous != tile.image_key {
                media::queue_delete(&mut tx, previous.as_deref(), image.as_deref()).await?;
            }
        }
        "in_review" | "rejected" => {
            if tile.status == "rejected"
                && tile
                    .rejected_at
                    .is_some_and(|at| at + chrono::Duration::days(30) < chrono::Utc::now())
            {
                return Err(Fail::conflict("The 30 days to resubmit have passed."));
            }
            sqlx::query("UPDATE wall_tiles SET status='in_review',image_key=$2,name=$3,link=$4,message=$5,crest=$6,updated_at=now() WHERE n=$1")
                .bind(n).bind(&image).bind(&content.name).bind(&content.link).bind(&content.message).bind(&crest)
                .execute(&mut *tx).await?;
            media::queue_delete(&mut tx, tile.image_key.as_deref(), image.as_deref()).await?;
        }
        _ => return Err(Fail::conflict("This tile can't be changed.")),
    }
    tx.commit().await?;
    let tile = load(&mut *app.db.acquire().await?, n, false).await?;
    Ok(Json(json!({"tile": private(&app, &tile)})))
}

/// POST /api/wall/{n}/refund: the owner asks for a full refund before approval. The spot is free
/// at once; the jobs loop issues the Stripe refund.
async fn refund(State(app): State<App>, jar: CookieJar, Path(n): Path<i64>) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let tile = load(&mut tx, n, true).await?;
    if tile.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::missing());
    }
    if !matches!(tile.status.as_str(), "in_review" | "rejected") {
        return Err(Fail::conflict(
            "Refunds are available until the tile is approved.",
        ));
    }
    owe_refund(&mut tx, n).await?;
    safety::audit(
        &mut tx,
        Some(&user.id),
        "wall_refund_requested",
        "wall_tile",
        &n.to_string(),
        &[],
        "Owner asked for a refund before approval",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"refunding": true})))
}
async fn owe_refund(db: &mut PgConnection, n: i64) -> Res<()> {
    sqlx::query("DELETE FROM wall_cells WHERE tile_n=$1")
        .bind(n)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "UPDATE wall_tiles SET status='refund_due',pending=NULL,updated_at=now() WHERE n=$1",
    )
    .bind(n)
    .execute(db)
    .await?;
    Ok(())
}

/// Expires unpaid holds, refunds what's owed (a late payment for a taken spot, a refund request,
/// 30 days rejected) and sends rejection notices. Called by the jobs loop.
pub async fn tick(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let lapsed: Vec<i64> = sqlx::query_scalar(
        "SELECT n FROM wall_tiles WHERE status='held' AND hold_until<=now() FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(&mut *tx)
    .await?;
    for n in lapsed {
        expire(&mut tx, n).await?;
    }
    let stale: Vec<i64> = sqlx::query_scalar("SELECT n FROM wall_tiles WHERE status='rejected' AND rejected_at<now()-interval '30 days' FOR UPDATE SKIP LOCKED")
        .fetch_all(&mut *tx)
        .await?;
    for n in stale {
        owe_refund(&mut tx, n).await?;
    }
    tx.commit().await?;
    let due: Vec<(i64, Option<String>)> = sqlx::query_as("SELECT n,payment_intent FROM wall_tiles WHERE status='refund_due' ORDER BY updated_at LIMIT 20")
        .fetch_all(&app.db)
        .await?;
    let extra: Vec<String> = sqlx::query_scalar("SELECT payment_intent FROM wall_extra_payments WHERE refunded_at IS NULL ORDER BY created_at LIMIT 20")
        .fetch_all(&app.db)
        .await?;
    for intent in extra {
        stripe::post(
            &app.http,
            &app.config.stripe,
            "refunds",
            &[("payment_intent", intent.clone())],
            Some(&format!("wall-extra-refund-{intent}")),
        )
        .await?;
        sqlx::query("UPDATE wall_extra_payments SET refunded_at=now() WHERE payment_intent=$1")
            .bind(&intent)
            .execute(&app.db)
            .await?;
    }
    for (n, intent) in due {
        if let Some(intent) = &intent {
            // Idempotent by tile: a retry after a lost response never refunds twice.
            stripe::post(
                &app.http,
                &app.config.stripe,
                "refunds",
                &[("payment_intent", intent.clone())],
                Some(&format!("wall-refund-{n}")),
            )
            .await?;
        }
        sqlx::query("UPDATE wall_tiles SET status='refunded',updated_at=now() WHERE n=$1 AND status='refund_due'")
            .bind(n)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}

// ---- Staff ----

/// GET /api/admin/wall: every paid or pending tile, the sales total, prices and limits.
async fn admin(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let settings = settings(&mut db).await?;
    let sold = sold(&mut db).await?;
    let tiles: Vec<Tile> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{TILE} WHERE status NOT IN ('expired') ORDER BY (status='in_review' OR pending IS NOT NULL) DESC,updated_at DESC LIMIT 500"
    )))
    .fetch_all(&mut *db)
    .await?;
    let mut out = Vec::new();
    for tile in &tiles {
        let mut row = private(&app, tile);
        row["owner"] = json!(match &tile.owner_id {
            Some(id) =>
                sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id=$1")
                    .bind(id)
                    .fetch_optional(&mut *db)
                    .await?,
            None => None,
        });
        row["payment_intent"] = json!(tile.payment_intent);
        row["created_at"] = json!(tile.created_at);
        out.push(row);
    }
    let total: i64 = sqlx::query_scalar("SELECT coalesce(sum(price_cents),0)::bigint FROM wall_tiles WHERE paid_at IS NOT NULL AND status NOT IN ('refund_due','refunded')")
        .fetch_one(&mut *db)
        .await?;
    Ok(Json(json!({
        "tiles": out, "sold_cells": sold, "sales_cents": total,
        "stage": stage(&settings.stages, sold), "prices": settings.prices, "stages": settings.stages,
        "limits": settings.limits, "sales_opened_at": settings.sales_opened_at,
    })))
}

#[derive(Deserialize)]
struct Review {
    action: String,
    note: Option<String>,
}
/// POST /api/admin/wall/{n}/review: approve or reject a tile in review, or a change to a live
/// tile. Rejecting tells the buyer why; they can resubmit for 30 days or ask for a refund.
async fn review(
    State(app): State<App>,
    jar: CookieJar,
    Path(n): Path<i64>,
    Json(input): Json<Review>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", input.action == "reject")?;
    let mut tx = app.db.begin().await?;
    let tile = load(&mut tx, n, true).await?;
    let change = tile.status == "live" && tile.pending.is_some();
    if tile.status != "in_review" && !change {
        return Err(Fail::conflict(
            "Nothing on this tile is waiting for review.",
        ));
    }
    match (input.action.as_str(), change) {
        ("approve", false) => {
            sqlx::query("UPDATE wall_tiles SET status='live',approved_at=now(),review_note='',updated_at=now() WHERE n=$1")
                .bind(n).execute(&mut *tx).await?;
        }
        ("approve", true) => {
            let p = tile.pending.clone().unwrap_or_default();
            let image = p["image_key"].as_str();
            sqlx::query("UPDATE wall_tiles SET image_key=$2,name=$3,link=$4,message=$5,crest=$6,pending=NULL,review_note='',updated_at=now() WHERE n=$1")
                .bind(n).bind(image).bind(p["name"].as_str()).bind(p["link"].as_str()).bind(p["message"].as_str()).bind(p["crest"].as_str())
                .execute(&mut *tx).await?;
            media::queue_delete(&mut tx, tile.image_key.as_deref(), image).await?;
        }
        ("reject", false) => {
            sqlx::query("UPDATE wall_tiles SET status='rejected',rejected_at=now(),review_note=$2,updated_at=now() WHERE n=$1")
                .bind(n).bind(&note).execute(&mut *tx).await?;
        }
        ("reject", true) => {
            let image = tile
                .pending
                .as_ref()
                .and_then(|p| p["image_key"].as_str().map(String::from));
            sqlx::query(
                "UPDATE wall_tiles SET pending=NULL,review_note=$2,updated_at=now() WHERE n=$1",
            )
            .bind(n)
            .bind(&note)
            .execute(&mut *tx)
            .await?;
            media::queue_delete(&mut tx, image.as_deref(), tile.image_key.as_deref()).await?;
        }
        _ => return Err(Fail::field("action", "Choose approve or reject.")),
    }
    let action = format!(
        "wall_{}{}",
        input.action,
        if change { "_change" } else { "" }
    );
    safety::audit(
        &mut tx,
        Some(&staff.id),
        &action,
        "wall_tile",
        &n.to_string(),
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    if input.action == "reject"
        && let Some(owner) = &tile.owner_id
    {
        let body = if change {
            format!(
                "Your change to Wall tile {n} wasn't approved: {note}\n\nYour tile stays as it was. You can send another change at {}/wall/{n}.",
                app.config.origin
            )
        } else {
            format!(
                "Your Wall tile {n} wasn't approved: {note}\n\nYou can change it and resubmit within 30 days, or ask for a full refund, at {}/wall/{n}.",
                app.config.origin
            )
        };
        safety::queue_notice(&app, &mut tx, owner, "About your S.V.E.R Wall tile", &body).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

#[derive(Deserialize)]
struct StaffAction {
    note: Option<String>,
}
/// POST /api/admin/wall/{n}/refund: a full refund of any paid tile; its spot becomes free.
async fn staff_refund(
    State(app): State<App>,
    jar: CookieJar,
    Path(n): Path<i64>,
    Json(input): Json<StaffAction>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let tile = load(&mut tx, n, true).await?;
    if tile.paid_at.is_none() || matches!(tile.status.as_str(), "refund_due" | "refunded") {
        return Err(Fail::conflict("This tile has no payment to refund."));
    }
    owe_refund(&mut tx, n).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "wall_refund",
        "wall_tile",
        &n.to_string(),
        &[],
        &note,
        json!({"status": tile.status}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
/// POST /api/admin/wall/{n}/remove: a live tile that broke the rules comes down; its spot stays
/// empty (docs/WALL.md "Removing a live tile").
async fn staff_remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(n): Path<i64>,
    Json(input): Json<StaffAction>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let previous = remove(&mut tx, &n.to_string()).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "wall_remove",
        "wall_tile",
        &n.to_string(),
        &[],
        &note,
        previous,
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
/// POST /api/admin/wall/{n}/unlink: the link now leads somewhere banned; the tile stays.
async fn staff_unlink(
    State(app): State<App>,
    jar: CookieJar,
    Path(n): Path<i64>,
    Json(input): Json<StaffAction>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let tile = load(&mut tx, n, true).await?;
    sqlx::query("UPDATE wall_tiles SET link=NULL,pending=CASE WHEN pending IS NULL THEN NULL ELSE jsonb_set(pending,'{link}','null') END,updated_at=now() WHERE n=$1")
        .bind(n)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "wall_unlink",
        "wall_tile",
        &n.to_string(),
        &[],
        &note,
        json!({"link": tile.link}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

#[derive(Deserialize)]
struct SettingsInput {
    prices: Value,
    limits: Value,
    note: Option<String>,
}
/// PUT /api/admin/wall/settings: prices and limits. Before sales open anything can change; after,
/// only the prices of stages not reached yet.
async fn update_settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<SettingsInput>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let current = sqlx::query_as::<_, Settings>(
        "SELECT prices,stages,limits,sales_opened_at FROM wall_settings FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await?;
    let stages = current.stages.len() + 1;
    for (size, _, _) in SIZES {
        let list = input.prices[size]
            .as_array()
            .filter(|l| l.len() == stages)
            .ok_or_else(|| {
                Fail::field_owned("prices", format!("Give {stages} prices for each size."))
            })?;
        if list
            .iter()
            .any(|p| !p.as_i64().is_some_and(|c| (100..=1_000_000).contains(&c)))
        {
            return Err(Fail::field(
                "prices",
                "Prices are whole cents from $1 to $10,000.",
            ));
        }
    }
    for size in ["table", "hall"] {
        if !input.limits[size]
            .as_i64()
            .is_some_and(|l| (0..=250).contains(&l))
        {
            return Err(Fail::field("limits", "Limits are 0 to 250."));
        }
    }
    if current.sales_opened_at.is_some() {
        let reached = stage(&current.stages, sold(&mut tx).await?);
        for (size, _, _) in SIZES {
            for i in 0..=reached {
                if input.prices[size][i] != current.prices[size][i] {
                    return Err(Fail::field(
                        "prices",
                        "Sales are open: only prices of stages not reached yet can change.",
                    ));
                }
            }
        }
        if input.limits != current.limits {
            return Err(Fail::field(
                "limits",
                "Sales are open: the size limits are fixed.",
            ));
        }
    }
    sqlx::query("UPDATE wall_settings SET prices=$1,limits=$2")
        .bind(&input.prices)
        .bind(&input.limits)
        .execute(&mut *tx)
        .await?;
    safety::audit(&mut tx, Some(&staff.id), "wall_settings", "wall", "settings", &[], &note, json!({"before":{"prices":current.prices,"limits":current.limits},"after":{"prices":input.prices,"limits":input.limits}}), false).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
/// POST /api/admin/wall/open: opens sales, once the Wall terms, tax and prices are settled.
async fn open_sales(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<StaffAction>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let opened =
        sqlx::query("UPDATE wall_settings SET sales_opened_at=now() WHERE sales_opened_at IS NULL")
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if opened == 0 {
        return Err(Fail::conflict("Sales are already open."));
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "wall_open_sales",
        "wall",
        "settings",
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

/// GET /api/admin/wall/review: tiles and changes waiting for review, for /admin/media.
async fn review_queue(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let tiles: Vec<Tile> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{TILE} WHERE status='in_review' OR (status='live' AND pending IS NOT NULL) ORDER BY updated_at LIMIT 50"
    )))
    .fetch_all(&app.db)
    .await?;
    let mut db = app.db.acquire().await?;
    let mut items = Vec::new();
    for tile in &tiles {
        let mut row = private(&app, tile);
        row["owner"] = json!(match &tile.owner_id {
            Some(id) =>
                sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id=$1")
                    .bind(id)
                    .fetch_optional(&mut *db)
                    .await?,
            None => None,
        });
        items.push(row);
    }
    Ok(Json(json!({"items": items})))
}

// ---- Reports (safety.rs) ----

pub async fn report_target(
    db: &mut PgConnection,
    id: &str,
) -> Res<Option<(String, String, Value)>> {
    let Ok(n) = id.parse::<i64>() else {
        return Ok(None);
    };
    Ok(sqlx::query_as("SELECT u.id,u.username,jsonb_build_object('n',t.n,'name',t.name,'link',t.link,'message',t.message,'image_key',t.image_key) FROM wall_tiles t JOIN users u ON u.id=t.owner_id WHERE t.n=$1 AND t.status='live' AND u.deleted_at IS NULL")
        .bind(n).fetch_optional(db).await?)
}
pub async fn owner(db: &mut PgConnection, id: &str) -> Res<Option<String>> {
    let Ok(n) = id.parse::<i64>() else {
        return Ok(None);
    };
    Ok(
        sqlx::query_scalar::<_, Option<String>>("SELECT owner_id FROM wall_tiles WHERE n=$1")
            .bind(n)
            .fetch_optional(db)
            .await?
            .flatten(),
    )
}
pub async fn current(db: &mut PgConnection, id: &str) -> Res<Value> {
    let Ok(n) = id.parse::<i64>() else {
        return Ok(Value::Null);
    };
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('n',n,'name',name,'link',link,'message',message,'status',status) FROM wall_tiles WHERE n=$1")
        .bind(n).fetch_optional(db).await?.unwrap_or(Value::Null))
}
/// Takes a live tile down under the Wall terms; its cells stay taken so the spot stays empty.
pub async fn remove(db: &mut PgConnection, id: &str) -> Res<Value> {
    let n: i64 = id.parse().map_err(|_| Fail::missing())?;
    let tile = load(db, n, true).await?;
    if tile.status != "live" {
        return Err(Fail::conflict("Only a live tile can be removed."));
    }
    sqlx::query("UPDATE wall_tiles SET status='removed',pending=NULL,updated_at=now() WHERE n=$1")
        .bind(n)
        .execute(db)
        .await?;
    Ok(json!({"type":"wall_tile","id":id,"previous":"live"}))
}

/// An overturned removal puts the tile back up.
pub async fn restore(db: &mut PgConnection, id: &str, previous: &str) -> Res<()> {
    if previous == "live"
        && let Ok(n) = id.parse::<i64>()
    {
        sqlx::query(
            "UPDATE wall_tiles SET status='live',updated_at=now() WHERE n=$1 AND status='removed'",
        )
        .bind(n)
        .execute(db)
        .await?;
    }
    Ok(())
}

pub fn routes() -> Router<App> {
    // A picture of up to 5 MB plus the form fields.
    let upload = DefaultBodyLimit::max(media::Kind::WallTile.max_bytes() + 64 * 1024);
    Router::new()
        .route("/api/wall", get(wall))
        .route("/api/wall/holds", post(hold))
        .route("/api/wall/{n}", get(tile).put(edit).layer(upload))
        .route("/api/wall/{n}/hold", axum::routing::delete(release))
        .route("/api/wall/{n}/checkout", post(checkout).layer(upload))
        .route("/api/wall/{n}/refund", post(refund))
        .route("/api/admin/wall", get(admin))
        .route("/api/admin/wall/review", get(review_queue))
        .route("/api/admin/wall/settings", put(update_settings))
        .route("/api/admin/wall/open", post(open_sales))
        .route("/api/admin/wall/{n}/review", post(review))
        .route("/api/admin/wall/{n}/refund", post(staff_refund))
        .route("/api/admin/wall/{n}/remove", post(staff_remove))
        .route("/api/admin/wall/{n}/unlink", post(staff_unlink))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spots_fit_the_grid() {
        assert_eq!(cells(0, 0, 1), Some(vec![(0, 0)]));
        assert_eq!(cells(38, 23, 2).unwrap().len(), 4);
        assert_eq!(cells(39, 0, 2), None);
        assert_eq!(cells(36, 21, 4).unwrap().len(), 16);
        assert_eq!(cells(0, 22, 4), None);
        assert_eq!(cells(-1, 0, 1), None);
    }
    #[test]
    fn prices_follow_cells_sold() {
        let stages = [250, 600];
        assert_eq!(stage(&stages, 0), 0);
        assert_eq!(stage(&stages, 249), 0);
        assert_eq!(stage(&stages, 250), 1);
        assert_eq!(stage(&stages, 599), 1);
        assert_eq!(stage(&stages, 600), 2);
        assert_eq!(stage(&stages, 1000), 2);
    }
}
