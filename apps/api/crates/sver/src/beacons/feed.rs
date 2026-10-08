//! Where Beacons appear: the feed, each Beacon's page, the home shelf, the channel's Beacons tab,
//! link previews and search. Ordering never reads money, viewer counts, views or likes: sources
//! are newest first, and the rotation hands every creator a turn before anyone gets a second.
use super::*;
use axum::{
    Json,
    extract::{Path, Query, State},
    routing::get,
};

const PAGE: usize = 10;

/// Filters every public list shares. $1 is the viewer id ('' signed out), $2 whether the viewer
/// has passed the 18+ gate.
fn visible(alias: &str) -> String {
    format!(
        "{a}.status='PUBLISHED' AND NOT {a}.hidden AND EXISTS(SELECT 1 FROM channel_users cu WHERE cu.id={a}.owner_id AND cu.eligible) \
         AND ($2 OR NOT {a}.mature) AND NOT coalesce({held},false) \
         AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE (k.blocker_id=$1 AND k.blocked_id={a}.owner_id) OR (k.blocker_id={a}.owner_id AND k.blocked_id=$1)) \
         AND NOT EXISTS(SELECT 1 FROM beacon_mutes m WHERE m.user_id=$1 AND m.muted_id={a}.owner_id) \
         AND NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id={a}.owner_id AND r.user_id=$1 AND r.kind='ban')",
        a = alias,
        held = crate::videos::review::source_held_sql(&format!("{alias}.clip_id"))
    )
}
/// Only a signed-in adult, or a guest, who confirmed the 18+ gate sees 18+ Beacons.
async fn passed_gate(app: &App, user: Option<&auth::User>, age_ack: bool) -> Res<bool> {
    Ok(match user {
        Some(user) => age_ack && auth::is_adult(&mut *app.db.acquire().await?, &user.id).await?,
        None => age_ack,
    })
}
fn seed(value: &str) -> Res<String> {
    if value.is_empty() {
        return Ok("0".into());
    }
    if value.len() > 32 || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(Fail::bad("Invalid feed position."));
    }
    Ok(value.into())
}
/// The fair rotation: each creator's Beacons newest first, every creator's first turn before any
/// second, with creators shuffled by the viewer's seed so newcomers aren't buried.
async fn rotation(
    db: &mut PgConnection,
    viewer: &str,
    gate: bool,
    seed: &str,
    exclude: &str,
    limit: i64,
) -> Res<Vec<Beacon>> {
    let sql = format!(
        "WITH turns AS (SELECT b.*,row_number() OVER (PARTITION BY b.owner_id ORDER BY b.published_at DESC,b.id DESC) AS turn FROM beacons b WHERE {} {exclude}) SELECT * FROM turns WHERE turn<=10 ORDER BY turn,md5(owner_id||$3),published_at DESC,id LIMIT $4",
        visible("b")
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(viewer)
        .bind(gate)
        .bind(seed)
        .bind(limit)
        .fetch_all(db)
        .await?)
}
#[derive(Deserialize)]
struct FeedQuery {
    #[serde(default)]
    seed: String,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    age_ack: bool,
}
/// GET /api/beacons/feed
async fn feed(
    State(app): State<App>,
    jar: CookieJar,
    Query(query): Query<FeedQuery>,
) -> Res<Json<Value>> {
    let seed = seed(&query.seed)?;
    if query.offset > 500 {
        return Ok(Json(json!({"items":[],"has_more":false,"live":[]})));
    }
    let user = profiles::viewer(&app, &jar).await?;
    let gate = passed_gate(&app, user.as_ref(), query.age_ack).await?;
    let viewer = user.as_ref().map(|u| u.id.clone()).unwrap_or_default();
    let mut db = app.db.acquire().await?;
    let mut sources: Vec<Vec<Beacon>> = Vec::new();
    // Signed-out visitors see the fair rotation only.
    if user.is_some() {
        let followed = format!(
            "SELECT b.* FROM beacons b WHERE {} AND EXISTS(SELECT 1 FROM follows f WHERE f.follower_id=$1 AND f.following_id=b.owner_id) ORDER BY b.published_at DESC,b.id DESC LIMIT 100",
            visible("b")
        );
        sources.push(
            sqlx::query_as(sqlx::AssertSqlSafe(followed))
                .bind(&viewer)
                .bind(gate)
                .fetch_all(&mut *db)
                .await?,
        );
        let faction = format!(
            "SELECT b.* FROM beacons b WHERE {} AND {} IS NOT NULL AND {}={} AND NOT EXISTS(SELECT 1 FROM follows f WHERE f.follower_id=$1 AND f.following_id=b.owner_id) AND b.owner_id<>$1 ORDER BY b.published_at DESC,b.id DESC LIMIT 100",
            visible("b"),
            crate::factions::membership_sql("$1"),
            crate::factions::membership_sql("b.owner_id"),
            crate::factions::membership_sql("$1"),
        );
        sources.push(
            sqlx::query_as(sqlx::AssertSqlSafe(faction))
                .bind(&viewer)
                .bind(gate)
                .fetch_all(&mut *db)
                .await?,
        );
    }
    sources.push(rotation(&mut db, &viewer, gate, &seed, "", 300).await?);
    let mut order = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cursors = vec![0; sources.len()];
    while order.len() <= query.offset + PAGE {
        let mut moved = false;
        for (source, cursor) in sources.iter().zip(cursors.iter_mut()) {
            while let Some(beacon) = source.get(*cursor) {
                *cursor += 1;
                if seen.insert(beacon.id.clone()) {
                    order.push(beacon.clone());
                    moved = true;
                    break;
                }
            }
        }
        if !moved {
            break;
        }
    }
    let has_more = order.len() > query.offset + PAGE;
    let page: Vec<Beacon> = order.into_iter().skip(query.offset).take(PAGE).collect();
    let items = present(&app, &mut db, page, user.as_ref(), gate).await?;
    let live = if query.offset == 0 {
        live_now(&app, &mut db, &viewer, gate, &seed).await?
    } else {
        vec![]
    };
    Ok(Json(
        json!({"items":items,"has_more":has_more,"next":query.offset+PAGE,"live":live,"signed_in":user.is_some()}),
    ))
}
/// Live creators who have recent Beacons, in the viewer's rotation order (never viewer count).
async fn live_now(
    app: &App,
    db: &mut PgConnection,
    viewer: &str,
    gate: bool,
    seed: &str,
) -> Res<Vec<Value>> {
    let sql = format!(
        "SELECT {} FROM channel_users c WHERE c.eligible AND {} AND EXISTS(SELECT 1 FROM beacons b WHERE b.owner_id=c.id AND b.published_at>now()-interval '14 days' AND {}) ORDER BY md5(c.id||$3) LIMIT 12",
        profiles::chip_sql("c"),
        crate::playback::live_sql("c.id"),
        visible("b")
    );
    let mut chips: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(viewer)
        .bind(gate)
        .bind(seed)
        .fetch_all(db)
        .await?;
    for chip in &mut chips {
        profiles::hydrate(app, chip);
    }
    Ok(chips)
}
#[derive(Deserialize, Default)]
pub(super) struct PageQuery {
    #[serde(default)]
    age_ack: bool,
}
/// GET /api/beacons/{id}: one Beacon's page.
pub(super) async fn page(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Res<Json<Value>> {
    let (beacon, user) = watched(&app, &jar, &id, query.age_ack).await?;
    let is_owner = user
        .as_ref()
        .is_some_and(|u| beacon.owner_id.as_deref() == Some(&u.id));
    let mut db = app.db.acquire().await?;
    let mut item = present(&app, &mut db, vec![beacon], user.as_ref(), query.age_ack)
        .await?
        .pop()
        .ok_or_else(Fail::missing)?;
    item["signed_in"] = json!(user.is_some());
    item["is_owner"] = json!(is_owner);
    Ok(Json(item))
}
/// GET /api/beacons/shelf: the home page's 9:16 shelf, from the fair rotation.
async fn shelf(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::viewer(&app, &jar).await?;
    let viewer = user.as_ref().map(|u| u.id.clone()).unwrap_or_default();
    // The shelf turns over hourly for everyone, in the same fair order.
    let seed = (Utc::now().timestamp() / 3600).to_string();
    let mut db = app.db.acquire().await?;
    let rows = rotation(&mut db, &viewer, false, &seed, "", 12).await?;
    let items = present(&app, &mut db, rows, user.as_ref(), false).await?;
    Ok(Json(json!({"items":items})))
}
#[derive(Deserialize, Default)]
struct Listing {
    #[serde(default)]
    offset: i64,
}
/// GET /api/channels/{name}/beacons: the channel's Beacons tab, newest first.
async fn channel(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(query): Query<Listing>,
) -> Res<Json<Value>> {
    if !(0..=100_000).contains(&query.offset) {
        return Err(Fail::bad("Choose a valid page."));
    }
    let user = profiles::viewer(&app, &jar).await?;
    let viewer = user.as_ref().map(|u| u.id.clone()).unwrap_or_default();
    let mut db = app.db.acquire().await?;
    let channel = profiles::eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let gate = passed_gate(&app, user.as_ref(), false).await?;
    let sql = format!(
        "SELECT b.* FROM beacons b WHERE b.owner_id=$3 AND {} ORDER BY b.published_at DESC,b.id DESC LIMIT 25 OFFSET $4",
        visible("b")
    );
    let mut rows: Vec<Beacon> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(&viewer)
        .bind(gate)
        .bind(&channel.id)
        .bind(query.offset)
        .fetch_all(&mut *db)
        .await?;
    let has_more = rows.len() > 24;
    rows.truncate(24);
    let items = present(&app, &mut db, rows, user.as_ref(), false).await?;
    Ok(Json(json!({"items":items,"has_more":has_more})))
}
/// GET /api/beacons/{id}/share: preview tags for Discord, X and Reddit (public, non-18+ only).
async fn share(State(app): State<App>, Path(id): Path<String>) -> Res<Json<Value>> {
    let beacon = load(&mut *app.db.acquire().await?, &id).await?;
    if beacon.mature {
        return Err(Fail::missing());
    }
    accessible(&app, &beacon, None, false).await?;
    let owner = profiles::channel_user_by_id(
        &mut *app.db.acquire().await?,
        beacon.owner_id.as_deref().ok_or_else(Fail::missing)?,
    )
    .await?
    .ok_or_else(Fail::missing)?;
    let token = super::share_ticket(&app, &beacon)?;
    let origin = &app.config.origin;
    Ok(Json(json!({
        "id": id,
        "title": beacon.title,
        "author_name": owner.display_name,
        "author_url": format!("{origin}/{}", owner.username),
        "url": format!("{origin}/beacons/{id}"),
        "mp4": format!("{origin}/api/beacons/{id}/file?q=sd&ticket={token}"),
        "thumbnail": beacon.thumbnail_key.as_ref().map(|_| format!("{origin}/api/beacons/{id}/thumbnail?ticket={token}")),
    })))
}
/// Search results alongside channels and categories: title or category matches, newest first.
pub async fn search(
    app: &App,
    db: &mut PgConnection,
    viewer: Option<&auth::User>,
    pattern: &str,
) -> Res<Vec<Value>> {
    let sql = format!(
        "SELECT b.* FROM beacons b WHERE {} AND (b.title ILIKE $3 OR b.category ILIKE $3) ORDER BY b.published_at DESC,b.id DESC LIMIT 8",
        visible("b")
    );
    let rows: Vec<Beacon> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(viewer.map(|u| u.id.as_str()).unwrap_or(""))
        .bind(false)
        .bind(pattern)
        .fetch_all(&mut *db)
        .await?;
    present(app, db, rows, viewer, false).await
}
pub fn routes() -> axum::Router<App> {
    axum::Router::new()
        .route("/api/beacons/feed", get(feed))
        .route("/api/beacons/shelf", get(shelf))
        .route("/api/beacons/{id}/share", get(share))
        .route("/api/channels/{name}/beacons", get(channel))
}
