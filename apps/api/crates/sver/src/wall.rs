//! The Wall: posts, replies, likes, owner review and pins (docs/PROFILES.md, "The Wall").
// Row tuples from runtime sqlx queries read more clearly inline than as aliases.
#![allow(clippy::type_complexity)]
use crate::{
    App,
    auth::User,
    profiles::{
        ChannelUser, CursorQuery, Fail, Res, blocked_between, bump_section, chip_sql,
        eligible_by_name, ensure_profile, ensure_unrestricted, hydrate, make_cursor, new_id,
        parse_cursor, rate, section_revision, signed_in, viewer,
    },
    social, text,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};

const PAGE: i64 = 20;
const REMOVED: &str = "Removed by S.V.E.R moderators.";

#[derive(FromRow)]
struct Settings {
    who_can_post: String,
    require_approval: bool,
    hold_links: bool,
    hold_new_accounts: bool,
}
async fn settings(db: &mut PgConnection, owner: &str) -> Res<Settings> {
    Ok(sqlx::query_as("SELECT who_can_post,require_approval,hold_links,hold_new_accounts FROM profiles WHERE user_id=$1")
        .bind(owner)
        .fetch_optional(&mut *db)
        .await?
        .unwrap_or(Settings { who_can_post: "ANYONE".into(), require_approval: false, hold_links: false, hold_new_accounts: false }))
}

/// Why a user can't post (or reply/react) on a wall, or None when they can.
async fn denial(
    db: &mut PgConnection,
    owner: &ChannelUser,
    user: &User,
    mode: &str,
    posting: bool,
) -> Res<Option<&'static str>> {
    let me = crate::profiles::channel_user_by_id(db, &user.id)
        .await?
        .ok_or_else(Fail::missing)?;
    if me.internal || me.restricted {
        return Ok(Some("You can't post on this wall."));
    }
    if user.id != owner.id && blocked_between(db, &owner.id, &user.id).await? {
        return Ok(Some("You can't post on this wall."));
    }
    if !user.email_verified {
        return Ok(Some("Verify your email to post."));
    }
    if !posting || user.id == owner.id {
        return Ok(None);
    }
    let (owner_follows, follows_owner): (bool, bool) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2),EXISTS(SELECT 1 FROM follows WHERE follower_id=$2 AND following_id=$1)")
        .bind(&owner.id)
        .bind(&user.id)
        .fetch_one(&mut *db)
        .await?;
    let allowed = match mode {
        "ANYONE" => true,
        "FOLLOWING" => owner_follows,
        "MUTUAL" => owner_follows && follows_owner,
        _ => false,
    };
    Ok(if allowed {
        None
    } else {
        Some("You can't post on this wall.")
    })
}
fn deny(message: &'static str) -> Fail {
    Fail::denied(message)
}

#[derive(FromRow)]
struct PostRow {
    id: String,
    body: String,
    status: String,
    pinned_position: Option<i32>,
    created_at: DateTime<Utc>,
    author_id: String,
    author: Value,
    author_deleted: bool,
    likes: i64,
    liked: bool,
    reply_count: i64,
}
#[derive(FromRow)]
struct ReplyRow {
    id: String,
    post_id: String,
    body: String,
    status: String,
    created_at: DateTime<Utc>,
    author_id: String,
    author: Value,
    author_deleted: bool,
}
fn visible(alias: &'static str) -> String {
    // $1 owner, $2 viewer (nullable).
    format!(
        "{a}.deleted_at IS NULL AND ({a}.status='APPROVED' OR ({a}.status='PENDING' AND ($2::text={a}.author_id OR $2::text=$1)) OR ({a}.status IN ('REJECTED','REMOVED') AND $2::text={a}.author_id))",
        a = alias
    )
}
fn post_select() -> String {
    format!(
        "SELECT p.id,p.body,p.status,p.pinned_position,p.created_at,p.author_id,{chip} AS author,a.deleted_at IS NOT NULL AS author_deleted, \
         (SELECT count(*) FROM wall_likes l WHERE l.post_id=p.id) AS likes, \
         ($2::text IS NOT NULL AND EXISTS(SELECT 1 FROM wall_likes l WHERE l.post_id=p.id AND l.user_id=$2)) AS liked, \
         (SELECT count(*) FROM wall_replies r WHERE r.post_id=p.id AND {rv}) AS reply_count \
         FROM wall_posts p JOIN channel_users a ON a.id=p.author_id WHERE p.wall_owner_id=$1 AND {pv}",
        chip = chip_sql("a"),
        rv = visible("r"),
        pv = visible("p")
    )
}
fn status_label(status: &str) -> Option<&'static str> {
    match status {
        "PENDING" => Some("Waiting for approval"),
        "REJECTED" => Some("Not approved"),
        "REMOVED" => Some(REMOVED),
        _ => None,
    }
}
fn reply_json(app: &App, row: &ReplyRow, owner_id: &str, viewer: Option<&str>) -> Value {
    let hidden = row.author_deleted || row.status == "REMOVED";
    let mut author = row.author.clone();
    hydrate(app, &mut author);
    json!({
        "id": row.id,
        "author": author,
        "body": if hidden { Value::Null } else { json!(row.body) },
        "status": row.status,
        "status_label": status_label(&row.status),
        "created_at": row.created_at,
        "can_delete": viewer.is_some_and(|v| v == row.author_id || v == owner_id) && row.status != "REMOVED",
        "can_report": viewer.is_some_and(|v| v != row.author_id) && row.status == "APPROVED" && !row.author_deleted,
    })
}
async fn load_replies(
    app: &App,
    db: &mut PgConnection,
    owner_id: &str,
    viewer: Option<&str>,
    post_ids: &[String],
    after: Option<(DateTime<Utc>, String)>,
    limit: i64,
) -> Res<Vec<(ReplyRow, i64)>> {
    // Only fixed wall SQL, literal aliases or the selected literal table are interpolated; values are bound.
    let rows: Vec<(String, String, String, String, DateTime<Utc>, String, Value, bool, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT * FROM (SELECT r.id,r.post_id,r.body,r.status,r.created_at,r.author_id,{chip},a.deleted_at IS NOT NULL, row_number() OVER (PARTITION BY r.post_id ORDER BY r.created_at,r.id) AS n \
         FROM wall_replies r JOIN channel_users a ON a.id=r.author_id WHERE r.post_id=ANY($3) AND {rv} AND ($4::timestamptz IS NULL OR (r.created_at,r.id)>($4,$5))) x WHERE n<=$6 ORDER BY created_at,id",
        chip = chip_sql("a"),
        rv = visible("r")
    )))
    .bind(owner_id)
    .bind(viewer)
    .bind(post_ids)
    .bind(after.as_ref().map(|a| a.0))
    .bind(after.as_ref().map(|a| a.1.clone()).unwrap_or_default())
    .bind(limit)
    .fetch_all(&mut *db)
    .await?;
    let _ = app;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                ReplyRow {
                    id: r.0,
                    post_id: r.1,
                    body: r.2,
                    status: r.3,
                    created_at: r.4,
                    author_id: r.5,
                    author: r.6,
                    author_deleted: r.7,
                },
                r.8,
            )
        })
        .collect())
}
async fn posts_json(
    app: &App,
    db: &mut PgConnection,
    owner_id: &str,
    viewer: Option<&str>,
    rows: Vec<PostRow>,
) -> Res<Vec<Value>> {
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let replies = load_replies(app, db, owner_id, viewer, &ids, None, 3).await?;
    Ok(rows
        .iter()
        .map(|row| {
            let hidden = row.author_deleted || row.status == "REMOVED";
            let mut author = row.author.clone();
            hydrate(app, &mut author);
            let first: Vec<Value> = replies.iter().filter(|(r, _)| r.post_id == row.id).map(|(r, _)| reply_json(app, r, owner_id, viewer)).collect();
            json!({
                "id": row.id,
                "author": author,
                "body": if hidden { Value::Null } else { json!(row.body) },
                "status": row.status,
                "status_label": status_label(&row.status),
                "pinned_position": row.pinned_position,
                "created_at": row.created_at,
                "like_count": row.likes,
                "liked": row.liked,
                "reply_count": row.reply_count,
                "replies": first,
                "more_replies": row.reply_count > first.len() as i64,
                "can_delete": viewer.is_some_and(|v| v == row.author_id || v == owner_id) && row.status != "REMOVED",
                "can_report": viewer.is_some_and(|v| v != row.author_id) && row.status == "APPROVED" && !row.author_deleted,
                "can_pin": viewer == Some(owner_id) && row.status == "APPROVED",
            })
        })
        .collect())
}
async fn viewer_state(
    db: &mut PgConnection,
    owner: &ChannelUser,
    viewer: Option<&User>,
) -> Res<Value> {
    let settings = settings(db, &owner.id).await?;
    Ok(match viewer {
        None => json!({"can_post": false, "reason": "Log in to sign the Wall.", "is_owner": false}),
        Some(user) => {
            let reason = denial(db, owner, user, &settings.who_can_post, true).await?;
            json!({"can_post": reason.is_none(), "reason": reason, "is_owner": user.id == owner.id, "can_react": denial(db, owner, user, &settings.who_can_post, false).await?.is_none()})
        }
    })
}

/// Home preview: pinned posts, then the latest 3 approved posts.
pub async fn preview(
    app: &App,
    db: &mut PgConnection,
    owner: &ChannelUser,
    viewer: Option<&User>,
) -> Res<Value> {
    let viewer_id = viewer.map(|v| v.id.clone());
    // Only fixed wall SQL, literal aliases or the selected literal table are interpolated; values are bound.
    let pinned: Vec<PostRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} AND p.status='APPROVED' AND p.pinned_position IS NOT NULL ORDER BY p.pinned_position",
        post_select()
    )))
    .bind(&owner.id)
    .bind(&viewer_id)
    .fetch_all(&mut *db)
    .await?;
    // Only fixed wall SQL, literal aliases or the selected literal table are interpolated; values are bound.
    let latest: Vec<PostRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{} AND p.status='APPROVED' AND p.pinned_position IS NULL ORDER BY p.created_at DESC,p.id DESC LIMIT 3", post_select())))
        .bind(&owner.id)
        .bind(&viewer_id)
        .fetch_all(&mut *db)
        .await?;
    Ok(json!({
        "pinned": posts_json(app, db, &owner.id, viewer_id.as_deref(), pinned).await?,
        "latest": posts_json(app, db, &owner.id, viewer_id.as_deref(), latest).await?,
        "viewer": viewer_state(db, owner, viewer).await?,
    }))
}

/// GET /api/channels/{username}/wall?cursor=
pub async fn wall(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let viewer_id = viewer.as_ref().map(|v| v.id.clone());
    let mut db = app.db.acquire().await?;
    let owner = eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let after = parse_cursor(&q.cursor)?;
    let pinned: Vec<PostRow> = if after.is_none() {
        // Only fixed wall SQL, literal aliases or the selected literal table are interpolated; values are bound.
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} AND p.status='APPROVED' AND p.pinned_position IS NOT NULL ORDER BY p.pinned_position", post_select())))
            .bind(&owner.id)
            .bind(&viewer_id)
            .fetch_all(&mut *db)
            .await?
    } else {
        Vec::new()
    };
    // Only fixed wall SQL, literal aliases or the selected literal table are interpolated; values are bound.
    let rows: Vec<PostRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} AND NOT (p.status='APPROVED' AND p.pinned_position IS NOT NULL) AND ($3::timestamptz IS NULL OR (p.created_at,p.id)<($3,$4)) ORDER BY p.created_at DESC,p.id DESC LIMIT $5",
        post_select()
    )))
    .bind(&owner.id)
    .bind(&viewer_id)
    .bind(after.as_ref().map(|a| a.0))
    .bind(after.as_ref().map(|a| a.1.clone()).unwrap_or_default())
    .bind(PAGE + 1)
    .fetch_all(&mut *db)
    .await?;
    let more = rows.len() as i64 > PAGE;
    let rows: Vec<PostRow> = rows.into_iter().take(PAGE as usize).collect();
    let next = if more {
        rows.last().map(|r| make_cursor(r.created_at, &r.id))
    } else {
        None
    };
    Ok(Json(json!({
        "owner": {"username": owner.username, "display_name": owner.display_name},
        "pinned": posts_json(&app, &mut db, &owner.id, viewer_id.as_deref(), pinned).await?,
        "items": posts_json(&app, &mut db, &owner.id, viewer_id.as_deref(), rows).await?,
        "next_cursor": next,
        "viewer": viewer_state(&mut db, &owner, viewer.as_ref()).await?,
    })))
}
/// GET /api/wall/posts/{id}/replies?cursor=: the replies behind "Show more" (oldest first).
pub async fn replies(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let viewer_id = viewer.as_ref().map(|v| v.id.clone());
    let mut db = app.db.acquire().await?;
    let row: Option<(String, String, String)> = sqlx::query_as("SELECT p.wall_owner_id,p.status,p.author_id FROM wall_posts p JOIN channel_users o ON o.id=p.wall_owner_id WHERE p.id=$1 AND o.eligible AND p.deleted_at IS NULL")
        .bind(&id)
        .fetch_optional(&mut *db)
        .await?;
    let (owner_id, status, author) = row.ok_or_else(Fail::missing)?;
    let v = viewer_id.as_deref();
    let can_see = status == "APPROVED"
        || (status == "PENDING" && (v == Some(author.as_str()) || v == Some(owner_id.as_str())))
        || v == Some(author.as_str());
    if !can_see {
        return Err(Fail::missing());
    }
    let after = parse_cursor(&q.cursor)?;
    let rows = load_replies(
        &app,
        &mut db,
        &owner_id,
        viewer_id.as_deref(),
        std::slice::from_ref(&id),
        after,
        PAGE + 1,
    )
    .await?;
    let more = rows.len() as i64 > PAGE;
    let rows: Vec<&ReplyRow> = rows.iter().take(PAGE as usize).map(|(r, _)| r).collect();
    Ok(Json(json!({
        "items": rows.iter().map(|r| reply_json(&app, r, &owner_id, viewer_id.as_deref())).collect::<Vec<_>>(),
        "next_cursor": if more { rows.last().map(|r| make_cursor(r.created_at, &r.id)) } else { None },
    })))
}

fn review_status(
    settings: &Settings,
    author: &ChannelUser,
    body: &str,
    is_owner: bool,
) -> &'static str {
    if is_owner {
        return "APPROVED";
    }
    if settings.require_approval
        || (settings.hold_links && text::contains_url(body))
        || (settings.hold_new_accounts && author.created_at > Utc::now() - Duration::days(7))
    {
        "PENDING"
    } else {
        "APPROVED"
    }
}
#[derive(Deserialize)]
pub struct Body {
    body: String,
}
/// POST /api/channels/{username}/wall
pub async fn create_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Body>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let owner = eligible_by_name(&mut tx, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let settings = settings(&mut tx, &owner.id).await?;
    if let Some(reason) = denial(&mut tx, &owner, &user, &settings.who_can_post, true).await? {
        return Err(deny(reason));
    }
    let body = text::plain(&input.body, "body", 1, 500, 8, true)?;
    rate(&app, format!("wall-post:min:{}", user.id), 10, 60).await?;
    rate(&app, format!("wall-post:day:{}", user.id), 50, 86400).await?;
    let author = crate::profiles::channel_user_by_id(&mut tx, &user.id)
        .await?
        .ok_or_else(Fail::missing)?;
    let status = review_status(&settings, &author, &body, owner.id == user.id);
    let id = new_id();
    ensure_profile(&mut tx, &owner.id).await?;
    sqlx::query(
        "INSERT INTO wall_posts(id,wall_owner_id,author_id,body,status) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(&id)
    .bind(&owner.id)
    .bind(&user.id)
    .bind(&body)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    if status == "APPROVED" {
        crate::activity::record(
            &mut tx,
            &user.id,
            "wall_post",
            Some(&owner.id),
            Some(&id),
            json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"id": id, "status": status, "status_label": status_label(status)}),
    ))
}
/// Loads a post with its wall owner for a mutation (deleted posts are not found).
async fn post_owner(db: &mut PgConnection, id: &str) -> Res<(String, String, String, ChannelUser)> {
    let row: Option<(String, String, String)> = sqlx::query_as("SELECT author_id,status,wall_owner_id FROM wall_posts WHERE id=$1 AND deleted_at IS NULL FOR UPDATE").bind(id).fetch_optional(&mut *db).await?;
    let (author, status, owner_id) = row.ok_or_else(Fail::missing)?;
    let owner = crate::profiles::channel_user_by_id(db, &owner_id)
        .await?
        .ok_or_else(Fail::missing)?;
    Ok((author, status, owner_id, owner))
}
/// POST /api/wall/posts/{id}/replies
pub async fn create_reply(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Body>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let (_, status, _, owner) = post_owner(&mut tx, &id).await?;
    if !owner.eligible || status != "APPROVED" {
        return Err(Fail::missing());
    }
    let settings = settings(&mut tx, &owner.id).await?;
    if let Some(reason) = denial(&mut tx, &owner, &user, &settings.who_can_post, true).await? {
        return Err(deny(reason));
    }
    let body = text::plain(&input.body, "body", 1, 300, 8, true)?;
    rate(&app, format!("wall-reply:{}", user.id), 20, 60).await?;
    let author = crate::profiles::channel_user_by_id(&mut tx, &user.id)
        .await?
        .ok_or_else(Fail::missing)?;
    let status = review_status(&settings, &author, &body, owner.id == user.id);
    let reply = new_id();
    sqlx::query(
        "INSERT INTO wall_replies(id,post_id,author_id,body,status) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(&reply)
    .bind(&id)
    .bind(&user.id)
    .bind(&body)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id": reply, "status": status, "status_label": status_label(status)}),
    ))
}
async fn like(app: App, jar: CookieJar, id: String, on: bool) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let (_, status, _, owner) = post_owner(&mut tx, &id).await?;
    if !owner.eligible || status != "APPROVED" {
        return Err(Fail::missing());
    }
    let settings = settings(&mut tx, &owner.id).await?;
    if let Some(reason) = denial(&mut tx, &owner, &user, &settings.who_can_post, false).await? {
        return Err(deny(reason));
    }
    let changed = if on {
        sqlx::query("INSERT INTO wall_likes(post_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(&id)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?
    } else {
        sqlx::query("DELETE FROM wall_likes WHERE post_id=$1 AND user_id=$2")
            .bind(&id)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?
    };
    if changed.rows_affected() > 0 {
        rate(&app, format!("wall-react:{}", user.id), 30, 60).await?;
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wall_likes WHERE post_id=$1")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"liked": on, "like_count": count})))
}
pub async fn like_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    like(app, jar, id, true).await
}
pub async fn unlike_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    like(app, jar, id, false).await
}
/// DELETE /api/wall/posts/{id}: author or wall owner; soft delete hides replies and likes too.
pub async fn delete_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let (author, status, owner_id, _) = post_owner(&mut tx, &id).await?;
    if (user.id != author && user.id != owner_id) || status == "REMOVED" {
        return Err(Fail::missing());
    }
    sqlx::query("UPDATE wall_posts SET deleted_at=now(),pinned_position=NULL WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted": true})))
}
/// DELETE /api/wall/replies/{id}
pub async fn delete_reply(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String, String)> = sqlx::query_as("SELECT r.author_id,p.wall_owner_id,r.status FROM wall_replies r JOIN wall_posts p ON p.id=r.post_id WHERE r.id=$1 AND r.deleted_at IS NULL FOR UPDATE OF r").bind(&id).fetch_optional(&mut *tx).await?;
    let (author, owner, status) = row.ok_or_else(Fail::missing)?;
    if (user.id != author && user.id != owner) || status == "REMOVED" {
        return Err(Fail::missing());
    }
    sqlx::query("UPDATE wall_replies SET deleted_at=now() WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted": true})))
}

/// GET /api/me/wall/pending?cursor=: pending posts and replies on the owner's wall, oldest first.
pub async fn pending(
    State(app): State<App>,
    jar: CookieJar,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let after = parse_cursor(&q.cursor)?;
    let mut db = app.db.acquire().await?;
    // Only fixed wall SQL, literal aliases or the selected literal table are interpolated; values are bound.
    let rows: Vec<(String, String, String, DateTime<Utc>, Value, Option<String>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT * FROM (SELECT 'post' AS kind,p.id,p.body,p.created_at,{chip} AS author,NULL::text AS post_id FROM wall_posts p JOIN channel_users a ON a.id=p.author_id WHERE p.wall_owner_id=$1 AND p.status='PENDING' AND p.deleted_at IS NULL \
         UNION ALL SELECT 'reply',r.id,r.body,r.created_at,{chip},r.post_id FROM wall_replies r JOIN wall_posts p ON p.id=r.post_id JOIN channel_users a ON a.id=r.author_id WHERE p.wall_owner_id=$1 AND r.status='PENDING' AND r.deleted_at IS NULL AND p.deleted_at IS NULL) q \
         WHERE ($2::timestamptz IS NULL OR (created_at,id)>($2,$3)) ORDER BY created_at,id LIMIT $4",
        chip = chip_sql("a")
    )))
    .bind(&user.id)
    .bind(after.as_ref().map(|a| a.0))
    .bind(after.as_ref().map(|a| a.1.clone()).unwrap_or_default())
    .bind(PAGE + 1)
    .fetch_all(&mut *db)
    .await?;
    let more = rows.len() as i64 > PAGE;
    let rows = &rows[..rows.len().min(PAGE as usize)];
    let mut items: Vec<Value> = rows.iter().map(|(kind, id, body, at, author, post)| json!({"kind": kind, "id": id, "body": body, "created_at": at, "author": author, "post_id": post})).collect();
    items.iter_mut().for_each(|v| hydrate(&app, v));
    let total: i64 = pending_count(&mut db, &user.id).await?;
    Ok(Json(
        json!({"items": items, "pending_count": total, "next_cursor": if more { rows.last().map(|r| make_cursor(r.3, &r.1)) } else { None }}),
    ))
}
pub async fn pending_count(db: &mut PgConnection, owner: &str) -> Res<i64> {
    Ok(sqlx::query_scalar("SELECT (SELECT count(*) FROM wall_posts WHERE wall_owner_id=$1 AND status='PENDING' AND deleted_at IS NULL)+(SELECT count(*) FROM wall_replies r JOIN wall_posts p ON p.id=r.post_id WHERE p.wall_owner_id=$1 AND r.status='PENDING' AND r.deleted_at IS NULL AND p.deleted_at IS NULL)")
        .bind(owner)
        .fetch_one(&mut *db)
        .await?)
}
/// POST /api/wall/{posts|replies}/{id}/{approve|reject|block-author}: wall owner review.
pub async fn review(
    State(app): State<App>,
    jar: CookieJar,
    Path((kind, id, action)): Path<(String, String, String)>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let (author, status, owner): (String, String, String) = match kind.as_str() {
        "posts" => sqlx::query_as("SELECT author_id,status,wall_owner_id FROM wall_posts WHERE id=$1 AND deleted_at IS NULL FOR UPDATE").bind(&id).fetch_optional(&mut *tx).await?,
        "replies" => sqlx::query_as("SELECT r.author_id,r.status,p.wall_owner_id FROM wall_replies r JOIN wall_posts p ON p.id=r.post_id WHERE r.id=$1 AND r.deleted_at IS NULL FOR UPDATE OF r").bind(&id).fetch_optional(&mut *tx).await?,
        _ => None,
    }
    .ok_or_else(Fail::missing)?;
    if owner != user.id {
        return Err(Fail::missing());
    }
    let table = if kind == "posts" {
        "wall_posts"
    } else {
        "wall_replies"
    };
    match action.as_str() {
        "approve" | "reject" => {
            if status != "PENDING" {
                return Err(Fail::conflict("This was already reviewed."));
            }
            let next = if action == "approve" {
                "APPROVED"
            } else {
                "REJECTED"
            };
            // Only fixed wall SQL, literal aliases or the selected literal table are interpolated; values are bound.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET status=$2,moderated_at=now() WHERE id=$1"
            )))
            .bind(&id)
            .bind(next)
            .execute(&mut *tx)
            .await?;
            if kind == "posts" && next == "APPROVED" {
                crate::activity::record(
                    &mut tx,
                    &author,
                    "wall_post",
                    Some(&owner),
                    Some(&id),
                    json!({}),
                )
                .await?;
            }
        }
        "block-author" => {
            if author == user.id {
                return Err(Fail::bad("You can't block yourself."));
            }
            let internal: bool =
                sqlx::query_scalar("SELECT internal FROM channel_users WHERE id=$1")
                    .bind(&author)
                    .fetch_one(&mut *tx)
                    .await?;
            if !internal {
                social::apply_block(&mut tx, &user.id, &author).await?;
            } else {
                // Internal accounts can't be blocked; still reject their pending content.
                sqlx::query("UPDATE wall_posts SET status='REJECTED',moderated_at=now() WHERE wall_owner_id=$1 AND author_id=$2 AND status='PENDING'").bind(&user.id).bind(&author).execute(&mut *tx).await?;
                sqlx::query("UPDATE wall_replies r SET status='REJECTED',moderated_at=now() FROM wall_posts p WHERE r.post_id=p.id AND p.wall_owner_id=$1 AND r.author_id=$2 AND r.status='PENDING'").bind(&user.id).bind(&author).execute(&mut *tx).await?;
            }
        }
        _ => return Err(Fail::missing()),
    }
    tx.commit().await?;
    Ok(Json(json!({"done": true})))
}

/// GET /api/me/wall: settings, pins and the pending count for Studio.
pub async fn my_wall(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let s = settings(&mut db, &user.id).await?;
    let pins: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'body',left(body,120),'position',pinned_position) FROM wall_posts WHERE wall_owner_id=$1 AND pinned_position IS NOT NULL AND deleted_at IS NULL ORDER BY pinned_position")
        .bind(&user.id)
        .fetch_all(&mut *db)
        .await?;
    Ok(Json(json!({
        "settings": {"who_can_post": s.who_can_post, "require_approval": s.require_approval, "hold_links": s.hold_links, "hold_new_accounts": s.hold_new_accounts},
        "pins": pins,
        "pending_count": pending_count(&mut db, &user.id).await?,
        "revision": section_revision(&mut db, &user.id, "wall").await?,
    })))
}
#[derive(Deserialize)]
pub struct SettingsInput {
    who_can_post: String,
    require_approval: bool,
    hold_links: bool,
    hold_new_accounts: bool,
    revision: Option<i64>,
}
/// PUT /api/me/wall/settings (SUBSCRIBERS stays hidden until subscriptions exist).
pub async fn save_settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<SettingsInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if !["ANYONE", "FOLLOWING", "MUTUAL", "NONE"].contains(&input.who_can_post.as_str()) {
        return Err(Fail::field("who_can_post", "Choose who can post."));
    }
    let mut tx = app.db.begin().await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "wall", input.revision).await?;
    sqlx::query("UPDATE profiles SET who_can_post=$2,require_approval=$3,hold_links=$4,hold_new_accounts=$5,updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(&input.who_can_post)
        .bind(input.require_approval)
        .bind(input.hold_links)
        .bind(input.hold_new_accounts)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "revision": revision})))
}
#[derive(Deserialize)]
pub struct PinsInput {
    post_ids: Vec<String>,
}
/// PUT /api/me/wall/pins: up to 3 approved posts on the owner's wall, in pin order.
pub async fn save_pins(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<PinsInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if input.post_ids.len() > 3 {
        return Err(Fail::conflict("You can pin up to 3 posts."));
    }
    let mut unique = input.post_ids.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != input.post_ids.len() {
        return Err(Fail::bad("Each post can be pinned once."));
    }
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    let valid: i64 = sqlx::query_scalar("SELECT count(*) FROM wall_posts WHERE id=ANY($1) AND wall_owner_id=$2 AND status='APPROVED' AND deleted_at IS NULL").bind(&input.post_ids).bind(&user.id).fetch_one(&mut *tx).await?;
    if valid as usize != input.post_ids.len() {
        return Err(Fail::bad("Only approved posts on your wall can be pinned."));
    }
    sqlx::query("UPDATE wall_posts SET pinned_position=NULL WHERE wall_owner_id=$1 AND pinned_position IS NOT NULL").bind(&user.id).execute(&mut *tx).await?;
    for (i, id) in input.post_ids.iter().enumerate() {
        sqlx::query("UPDATE wall_posts SET pinned_position=$2 WHERE id=$1")
            .bind(id)
            .bind(i as i32 + 1)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
