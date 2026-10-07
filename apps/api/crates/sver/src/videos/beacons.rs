//! Module 9: Beacons (docs/BEACONS.md). Short vertical videos that lead viewers to creators and
//! their live streams. A Beacon is a video row (kind BEACON) made from a channel's approved clip.
//! Feed order never reads money, views or likes.
use super::*;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};

/// Beacons per creator per day.
const DAILY: i64 = 10;

async fn creator(app: &App, jar: &CookieJar) -> Res<auth::User> {
    let user = profiles::signed_in(app, jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    Ok(user)
}
/// Anyone who has streamed on S.V.E.R at least once can post.
async fn eligible(db: &mut PgConnection, user: &str) -> Res<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1)")
            .bind(user)
            .fetch_one(db)
            .await?,
    )
}
/// A per-Beacon watermark schedule: corner order and seconds per corner (docs/BEACONS.md).
fn schedule(app: &App) -> Value {
    let seed = *uuid::Uuid::new_v4().as_bytes();
    let mut order = vec![0u64, 1, 2, 3];
    for i in (1..4).rev() {
        order.swap(i, seed[i] as usize % (i + 1));
    }
    let t = &app.config.videos.tuning;
    let shift = (f64::from(seed[8]) / 255.0) * 2.0 - 1.0;
    json!({"order":order,"period":t.beacon_mark_seconds + t.beacon_mark_jitter * shift})
}

/// GET /api/me/beacons: eligibility, clips that can become Beacons, and the creator's Beacons.
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let can_post = eligible(&mut db, &user.id).await?;
    let today: i64 = sqlx::query_scalar("SELECT count(*) FROM videos WHERE owner_id=$1 AND kind='BEACON' AND created_at>now()-interval '1 day'")
        .bind(&user.id).fetch_one(&mut *db).await?;
    let clips: Vec<Video> = sqlx::query_as("SELECT * FROM videos v WHERE owner_id=$1 AND kind='CLIP' AND status='READY' AND approval='APPROVED' AND beacon_approved AND NOT EXISTS(SELECT 1 FROM video_holds h WHERE h.video_id=v.id) ORDER BY created_at DESC LIMIT 50")
        .bind(&user.id).fetch_all(&mut *db).await?;
    let mut sources = Vec::new();
    for clip in &clips {
        let token = ticket(&app, clip, Some(&user), "play", true)?;
        sources.push(json!({"id":clip.id,"title":clip.title,"duration_ms":clip.duration_ms,"category":clip.category,
            "thumbnail":clip.thumbnail_key.as_ref().map(|_|format!("/api/videos/{}/thumbnail?ticket={token}",clip.id)),
            "preview":format!("/api/videos/{}/file?ticket={token}",clip.id)}));
    }
    let crop: Option<f32> = sqlx::query_scalar("SELECT b.crop FROM beacons b JOIN videos v ON v.id=b.video_id WHERE v.owner_id=$1 ORDER BY v.created_at DESC LIMIT 1")
        .bind(&user.id).fetch_optional(&mut *db).await?;
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',v.id,'title',v.title,'status',v.status,'published',b.published_at IS NOT NULL,'failure',b.failure,'held',EXISTS(SELECT 1 FROM video_holds h WHERE h.video_id=v.id),'views',v.views,'likes',b.likes,'completions',b.completions,'follows',b.follows,'live_joins',b.live_joins,'created_at',v.created_at,'duration_ms',v.duration_ms) FROM videos v JOIN beacons b ON b.video_id=v.id WHERE v.owner_id=$1 AND v.status NOT IN ('DELETING','DELETED','EXPIRED') ORDER BY v.created_at DESC LIMIT 100")
        .bind(&user.id).fetch_all(&mut *db).await?;
    let mut beacons = Vec::new();
    for mut row in rows {
        let id = row["id"].as_str().unwrap_or_default().to_string();
        let video = load(&mut db, &id).await?;
        if video.status == "READY" {
            let token = ticket(&app, &video, Some(&user), "play", true)?;
            row["thumbnail"] = json!(format!("/api/videos/{id}/thumbnail?ticket={token}"));
            row["play"] = json!(format!("/api/videos/{id}/file?ticket={token}&q=sd"));
        }
        beacons.push(row);
    }
    Ok(Json(
        json!({"eligible":can_post,"today":today,"limit":DAILY,"crop":crop.unwrap_or(0.5),"clips":sources,"beacons":beacons}),
    ))
}

#[derive(Deserialize)]
struct Create {
    clip_id: String,
    crop: f32,
    title: Option<String>,
}
/// POST /api/me/beacons: turn an approved clip into a Beacon. Rendering runs as one leased job.
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Create>,
) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    if !input.crop.is_finite() || !(0.0..=1.0).contains(&input.crop) {
        return Err(Fail::field("crop", "Choose where the 9:16 frame sits."));
    }
    security::reserve(&app, vec![format!("beacon-create:{}", user.id)], 20, 3600).await?;
    let mut tx = app.db.begin().await?;
    if !eligible(&mut tx, &user.id).await? {
        return Err(Fail::denied(
            "Stream on S.V.E.R once to start posting Beacons.",
        ));
    }
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let today: i64 = sqlx::query_scalar("SELECT count(*) FROM videos WHERE owner_id=$1 AND kind='BEACON' AND created_at>now()-interval '1 day'")
        .bind(&user.id).fetch_one(&mut *tx).await?;
    if today >= DAILY {
        return Err(Fail::conflict("You can post up to 10 Beacons a day."));
    }
    let clip = load(&mut tx, &input.clip_id).await?;
    if clip.kind != "CLIP"
        || clip.owner_id.as_deref() != Some(&user.id)
        || clip.status != "READY"
        || clip.approval != "APPROVED"
        || !clip.beacon_approved
        || held(&mut tx, &clip.id).await?
    {
        return Err(Fail::bad("Choose one of your channel's approved clips."));
    }
    let title = match input.title.as_deref().map(str::trim) {
        None | Some("") => clip.title.clone(),
        Some(title) => {
            let title = crate::text::clean(title, "title", false)?;
            if title.chars().count() > 120 {
                return Err(Fail::field("title", "Use a title of up to 120 characters."));
            }
            title
        }
    };
    moderation::check_clip(&app, &user.id, &user, &title).await?;
    let id = profiles::new_id();
    sqlx::query("INSERT INTO videos(id,owner_id,clipper_id,broadcast_id,parent_id,kind,status,approval,visibility,mature,title,category_id,category,genre,faction,started_at,ended_at,duration_ms,source_start_ms,source_end_ms) VALUES($1,$2,$3,$4,$5,'BEACON','PROCESSING','APPROVED','PUBLIC',$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
        .bind(&id).bind(&user.id).bind(&clip.clipper_id).bind(&clip.broadcast_id).bind(&clip.id).bind(clip.mature).bind(&title).bind(&clip.category_id).bind(&clip.category).bind(&clip.genre).bind(&clip.faction).bind(clip.started_at).bind(clip.ended_at).bind(clip.duration_ms).bind(clip.source_start_ms).bind(clip.source_end_ms).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO beacons(video_id,source_id,crop) VALUES($1,$2,$3)")
        .bind(&id)
        .bind(&clip.id)
        .bind(input.crop)
        .execute(&mut *tx)
        .await?;
    enqueue(
        &mut tx,
        &id,
        "BEACON",
        Some(&format!("{id}:render:0")),
        schedule(&app),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}

async fn owned(app: &App, user: &auth::User, id: &str) -> Res<Video> {
    let video = load(&mut *app.db.acquire().await?, id).await?;
    if video.kind != "BEACON" || video.owner_id.as_deref() != Some(&user.id) {
        return Err(Fail::missing());
    }
    Ok(video)
}
/// POST /api/me/beacons/{id}/publish
async fn publish(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    let video = owned(&app, &user, &id).await?;
    let mut tx = app.db.begin().await?;
    if video.status != "READY" || held(&mut tx, &id).await? {
        return Err(Fail::conflict("This Beacon isn't ready to publish."));
    }
    sqlx::query("UPDATE beacons SET published_at=coalesce(published_at,now()) WHERE video_id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE videos SET revision=revision+1 WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"published":true})))
}
/// POST /api/me/beacons/{id}/retry: run processing again after a failure.
async fn retry(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    let video = owned(&app, &user, &id).await?;
    if video.status != "FAILED" {
        return Err(Fail::conflict("Only a failed Beacon can be retried."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query(
        "UPDATE videos SET status='PROCESSING',revision=revision+1 WHERE id=$1 AND status='FAILED'",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE beacons SET failure=NULL WHERE video_id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    enqueue(
        &mut tx,
        &id,
        "BEACON",
        Some(&format!("{id}:render:{}", profiles::new_id())),
        schedule(&app),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"queued":true})))
}
/// DELETE /api/me/beacons/{id}: removes every rendition, the thumbnail and the clean copy.
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    owned(&app, &user, &id).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE beacons SET published_at=NULL WHERE video_id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    request_delete(&mut tx, &id, false).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}
/// POST /api/me/beacons/{id}/clean: a short download link to the clean copy (creator only).
async fn clean(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    let user = creator(&app, &jar).await?;
    let video = owned(&app, &user, &id).await?;
    if video.status != "READY" {
        return Err(Fail::conflict("This Beacon is still processing."));
    }
    let token = ticket(&app, &video, Some(&user), "download", true)?;
    Ok(Json(
        json!({"url":format!("/api/videos/{id}/file?ticket={token}")}),
    ))
}

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    owner_id: String,
    title: String,
    views: i64,
    duration_ms: i64,
    category: Option<String>,
    likes: i64,
    published_at: Option<DateTime<Utc>>,
    charity: Option<String>,
    donate_url: Option<String>,
}
/// Published, playable Beacons the viewer may see. $1 viewer, $2 viewer faction, $3 adult.
/// Nothing here reads views, likes or money.
const BASE: &str = "SELECT v.id,v.owner_id,v.title,v.views,v.duration_ms,v.category,b.likes,b.published_at,
    cs.charity_name AS charity,cs.donate_url
    FROM videos v JOIN beacons b ON b.video_id=v.id JOIN channel_users c ON c.id=v.owner_id
    LEFT JOIN charity_streams cs ON cs.broadcast_id=v.broadcast_id
    WHERE v.kind='BEACON' AND v.status='READY' AND b.published_at IS NOT NULL AND c.eligible
    AND NOT EXISTS(SELECT 1 FROM video_holds h WHERE h.video_id=v.id)
    AND (NOT v.mature OR $3)
    AND ($1::text IS NULL OR (NOT EXISTS(SELECT 1 FROM beacon_mutes m WHERE m.user_id=$1 AND m.creator_id=v.owner_id)
        AND NOT EXISTS(SELECT 1 FROM user_blocks u WHERE (u.blocker_id=$1 AND u.blocked_id=v.owner_id) OR (u.blocker_id=v.owner_id AND u.blocked_id=$1))
        AND NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=v.owner_id AND r.user_id=$1 AND r.kind='ban')))";
const FOLLOWED: &str =
    "EXISTS(SELECT 1 FROM follows f WHERE f.follower_id=$1 AND f.following_id=v.owner_id)";
/// The three feed sources (docs/BEACONS.md "The feed").
fn source(kind: &str) -> String {
    match kind {
        "followed" => format!(
            "{BASE} AND $1::text IS NOT NULL AND {FOLLOWED} ORDER BY b.published_at DESC LIMIT $4 OFFSET $5"
        ),
        "faction" => format!(
            "{BASE} AND $2::text IS NOT NULL AND (SELECT fm.faction FROM faction_members fm WHERE fm.user_id=v.owner_id)=$2 AND NOT ($1::text IS NOT NULL AND {FOLLOWED}) ORDER BY b.published_at DESC LIMIT $4 OFFSET $5"
        ),
        // Fair rotation: every creator's newest Beacon before anyone's second, in an order that
        // changes every hour.
        _ => format!(
            "{BASE} AND NOT ($1::text IS NOT NULL AND {FOLLOWED}) AND NOT ($2::text IS NOT NULL AND (SELECT fm.faction FROM faction_members fm WHERE fm.user_id=v.owner_id) IS NOT DISTINCT FROM $2)
            ORDER BY row_number() OVER (PARTITION BY v.owner_id ORDER BY b.published_at DESC), md5(v.owner_id||to_char(now(),'YYYYMMDDHH24')), b.published_at DESC LIMIT $4 OFFSET $5"
        ),
    }
}
struct Viewer {
    user: Option<auth::User>,
    faction: Option<String>,
    adult: bool,
}
async fn viewer(app: &App, jar: &CookieJar) -> Res<Viewer> {
    let user = profiles::viewer(app, jar).await?;
    let mut db = app.db.acquire().await?;
    let (faction, adult) = match &user {
        Some(u) => (
            profiles::channel_user_by_id(&mut db, &u.id)
                .await?
                .and_then(|c| c.faction),
            crate::auth::is_adult(&mut db, &u.id).await?,
        ),
        None => (None, false),
    };
    Ok(Viewer {
        user,
        faction,
        adult,
    })
}
async fn rows(app: &App, sql: String, viewer: &Viewer, limit: i64, offset: i64) -> Res<Vec<Row>> {
    // The SQL comes only from the fixed fragments above; every value is bound.
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(viewer.user.as_ref().map(|u| u.id.as_str()))
        .bind(viewer.faction.as_deref())
        .bind(viewer.adult)
        .bind(limit)
        .bind(offset)
        .fetch_all(&app.db)
        .await?)
}
async fn card(app: &App, row: &Row, viewer: &Viewer, from: &str) -> Res<Value> {
    let mut db = app.db.acquire().await?;
    let video = load(&mut db, &row.id).await?;
    let token = ticket(app, &video, viewer.user.as_ref(), "play", viewer.adult)?;
    let owner = profiles::channel_user_by_id(&mut db, &row.owner_id)
        .await?
        .ok_or_else(Fail::missing)?;
    let liked = match &viewer.user {
        Some(u) => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM beacon_likes WHERE beacon_id=$1 AND user_id=$2)",
            )
            .bind(&row.id)
            .bind(&u.id)
            .fetch_one(&mut *db)
            .await?
        }
        None => false,
    };
    let id = &row.id;
    Ok(
        json!({"id":id,"title":row.title,"channel":profiles::chip(app,&owner),"views":row.views,"likes":row.likes,"liked":liked,
        "live":crate::playback::is_live(&mut db,&row.owner_id).await?,"category":row.category,"duration_ms":row.duration_ms,
        "published_at":row.published_at,"from":from,
        "charity":row.charity.as_ref().map(|name|json!({"name":name,"donate_url":row.donate_url})),
        "play":format!("/api/videos/{id}/file?ticket={token}&q=sd"),"play_hd":format!("/api/videos/{id}/file?ticket={token}"),
        "thumbnail":format!("/api/videos/{id}/thumbnail?ticket={token}")}),
    )
}
#[derive(Deserialize, Default)]
struct Page {
    #[serde(default)]
    page: i64,
}
/// GET /api/beacons: followed, faction and fair-rotation Beacons, interleaved. Signed-out
/// visitors get the fair rotation only.
async fn feed(
    State(app): State<App>,
    jar: CookieJar,
    Query(page): Query<Page>,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let offset = page.page.clamp(0, 100) * 10;
    let mut lists = Vec::new();
    for kind in ["followed", "faction", "rotation"] {
        let found = if viewer.user.is_none() && kind != "rotation" {
            Vec::new()
        } else {
            rows(&app, source(kind), &viewer, 10, offset).await?
        };
        lists.push((kind, found.into_iter()));
    }
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    loop {
        let mut any = false;
        for (kind, list) in &mut lists {
            if let Some(row) = list.next() {
                any = true;
                if seen.insert(row.id.clone()) {
                    items.push(card(&app, &row, &viewer, kind).await?);
                }
            }
        }
        if !any {
            break;
        }
    }
    let more = items.len() >= 10;
    Ok(Json(
        json!({"items":items,"next":more.then_some(page.page + 1)}),
    ))
}
/// GET /api/beacons/live: live creators with a Beacon from the last week (the Live now row).
async fn live(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let sql = format!(
        "SELECT * FROM ({BASE} AND b.published_at>now()-interval '7 days' AND {}) x ORDER BY md5(owner_id||to_char(now(),'YYYYMMDDHH24')) LIMIT $4 OFFSET $5",
        crate::playback::live_sql("v.owner_id")
    );
    let found = rows(&app, sql, &viewer, 50, 0).await?;
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for row in found {
        if seen.insert(row.owner_id.clone()) && items.len() < 12 {
            items.push(card(&app, &row, &viewer, "live").await?);
        }
    }
    Ok(Json(json!({"items":items})))
}
/// The viewer must be able to see the Beacon before it is shown or any counter moves.
async fn visible(app: &App, jar: &CookieJar, id: &str) -> Res<(Viewer, Row)> {
    let viewer = viewer(app, jar).await?;
    let sql = format!("{BASE} AND v.id=$6 LIMIT $4 OFFSET $5");
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(viewer.user.as_ref().map(|u| u.id.as_str()))
        .bind(viewer.faction.as_deref())
        .bind(viewer.adult)
        .bind(1i64)
        .bind(0i64)
        .bind(id)
        .fetch_optional(&app.db)
        .await?;
    Ok((viewer, row.ok_or_else(Fail::missing)?))
}
/// GET /api/beacons/{id}: one Beacon for its own page and link previews.
async fn one(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    let (viewer, row) = visible(&app, &jar, &id).await?;
    Ok(Json(card(&app, &row, &viewer, "link").await?))
}
/// GET /api/channels/{name}/beacons: the channel's Beacons tab, newest first.
async fn channel(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(page): Query<Page>,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let owner = profiles::eligible_by_name(&mut *app.db.acquire().await?, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let sql = format!("{BASE} AND v.owner_id=$6 ORDER BY b.published_at DESC LIMIT $4 OFFSET $5");
    let found: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(viewer.user.as_ref().map(|u| u.id.as_str()))
        .bind(viewer.faction.as_deref())
        .bind(viewer.adult)
        .bind(24i64)
        .bind(page.page.clamp(0, 100) * 24)
        .bind(&owner.id)
        .fetch_all(&app.db)
        .await?;
    let mut items = Vec::new();
    for row in &found {
        items.push(card(&app, row, &viewer, "channel").await?);
    }
    Ok(Json(
        json!({"items":items,"next":(found.len()==24).then_some(page.page+1)}),
    ))
}
/// POST|DELETE /api/beacons/{id}/like: signed in, one per account, can be taken back.
async fn like(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    set_like(&app, &jar, &id, true).await
}
async fn unlike(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    set_like(&app, &jar, &id, false).await
}
async fn set_like(app: &App, jar: &CookieJar, id: &str, on: bool) -> Res<Json<Value>> {
    let user = profiles::signed_in(app, jar).await?;
    security::reserve(app, vec![format!("beacon-like:{}", user.id)], 30, 60).await?;
    visible(app, jar, id).await?;
    let mut tx = app.db.begin().await?;
    let changed = if on {
        sqlx::query(
            "INSERT INTO beacon_likes(beacon_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
    } else {
        sqlx::query("DELETE FROM beacon_likes WHERE beacon_id=$1 AND user_id=$2")
            .bind(id)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
    };
    let delta: i64 = match (changed, on) {
        (0, _) => 0,
        (_, true) => 1,
        (_, false) => -1,
    };
    let likes: i64 = sqlx::query_scalar(
        "UPDATE beacons SET likes=greatest(0,likes+$2) WHERE video_id=$1 RETURNING likes",
    )
    .bind(id)
    .bind(delta)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"liked":on,"likes":likes})))
}
#[derive(Deserialize)]
struct Event {
    kind: String,
    #[serde(default)]
    browser_id: String,
}
/// POST /api/beacons/{id}/events: creator-only measures. A completion needs 90% of the Beacon
/// watched in a counted heartbeat; a follow needs a real follow made in the last 15 minutes.
async fn event(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Event>,
) -> Res<Json<Value>> {
    let (viewer, row) = visible(&app, &jar, &id).await?;
    let key = match &viewer.user {
        Some(u) => format!("u:{}", u.id),
        None if (16..=64).contains(&input.browser_id.len()) => {
            format!("b:{}", security::digest(&input.browser_id))
        }
        None => return Err(Fail::bad("Invalid viewer.")),
    };
    security::reserve(&app, vec![format!("beacon-event:{key}")], 30, 60).await?;
    let ok: bool = match input.kind.as_str() {
        "complete" => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_playback WHERE video_id=$1 AND viewer_key=$2 AND counted AND watched_seconds>=$3)")
            .bind(&id).bind(&key).bind((row.duration_ms as f64 / 1000.0 * 0.9 - 1.0).max(3.0)).fetch_one(&app.db).await?,
        "follow" => match &viewer.user {
            Some(u) => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2 AND created_at>now()-interval '15 minutes')")
                .bind(&u.id).bind(&row.owner_id).fetch_one(&app.db).await?,
            None => false,
        },
        _ => return Err(Fail::bad("Unknown event.")),
    };
    if ok {
        let kind = if input.kind == "complete" {
            "COMPLETE"
        } else {
            "FOLLOW"
        };
        record(&app, &id, &key, kind).await?;
    }
    Ok(Json(json!({"recorded":ok})))
}
async fn record(app: &App, beacon: &str, key: &str, kind: &str) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let added = sqlx::query("INSERT INTO beacon_events(beacon_id,viewer_key,kind) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(beacon).bind(key).bind(kind).execute(&mut *tx).await?.rows_affected();
    if added == 1 {
        let column = match kind {
            "COMPLETE" => "completions",
            "FOLLOW" => "follows",
            _ => "live_joins",
        };
        // The column name comes from the fixed match above.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE beacons SET {column}={column}+1 WHERE video_id=$1"
        )))
        .bind(beacon)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
/// Called by the live heartbeat once a viewer who tapped Live now becomes a counted session.
pub async fn live_join(app: &App, beacon: &str, channel: &str, viewer_key: &str) -> Res<()> {
    let ours: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM videos v JOIN beacons b ON b.video_id=v.id WHERE v.id=$1 AND v.owner_id=$2 AND b.published_at IS NOT NULL)")
        .bind(beacon).bind(channel).fetch_one(&app.db).await?;
    if ours {
        record(app, beacon, viewer_key, "LIVE").await?;
    }
    Ok(())
}
/// POST|DELETE /api/beacons/mutes/{username}
async fn mute(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    set_mute(&app, &jar, &name, true).await
}
async fn unmute(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    set_mute(&app, &jar, &name, false).await
}
async fn set_mute(app: &App, jar: &CookieJar, name: &str, on: bool) -> Res<Json<Value>> {
    let user = profiles::signed_in(app, jar).await?;
    security::reserve(app, vec![format!("beacon-mute:{}", user.id)], 30, 60).await?;
    let target = profiles::eligible_by_name(&mut *app.db.acquire().await?, name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    if on {
        sqlx::query(
            "INSERT INTO beacon_mutes(user_id,creator_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(&user.id)
        .bind(&target.id)
        .execute(&app.db)
        .await?;
    } else {
        sqlx::query("DELETE FROM beacon_mutes WHERE user_id=$1 AND creator_id=$2")
            .bind(&user.id)
            .bind(&target.id)
            .execute(&app.db)
            .await?;
    }
    Ok(Json(json!({"muted":on})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/beacons", get(mine).post(create))
        .route("/api/me/beacons/{id}", axum::routing::delete(remove))
        .route("/api/me/beacons/{id}/publish", post(publish))
        .route("/api/me/beacons/{id}/retry", post(retry))
        .route("/api/me/beacons/{id}/clean", post(clean))
        .route("/api/beacons", get(feed))
        .route("/api/beacons/live", get(live))
        .route("/api/beacons/mutes/{username}", post(mute).delete(unmute))
        .route("/api/beacons/{id}", get(one))
        .route("/api/beacons/{id}/like", post(like).delete(unlike))
        .route("/api/beacons/{id}/events", post(event))
        .route("/api/channels/{name}/beacons", get(channel))
}
