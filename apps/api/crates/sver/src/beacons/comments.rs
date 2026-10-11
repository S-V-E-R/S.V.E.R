//! Beacon comments (docs/BEACONS.md "Comments"; Joe's decision October 10, 2026). Short text under
//! a Beacon from signed-in, verified accounts, held to the owner's chat rules (banned words, no
//! links), never across a block or a channel ban. The author or the Beacon's owner deletes one;
//! reports go to staff like wall replies (`beacon_comment`), and a removal can be restored on
//! appeal. Comments never affect the feed's order.
use super::*;
use axum::{
    Json,
    extract::{Path, State},
    routing::{delete, get},
};

const MAX_CHARS: usize = 300;
const LIST: i64 = 50;

/// A published, visible Beacon the viewer may see, and its owner.
async fn visible(db: &mut PgConnection, id: &str, viewer: &str) -> Res<String> {
    let owner: Option<String> = sqlx::query_scalar("SELECT b.owner_id FROM beacons b JOIN channel_users c ON c.id=b.owner_id AND c.eligible WHERE b.id=$1 AND b.status='PUBLISHED' AND NOT b.hidden")
        .bind(id).fetch_optional(&mut *db).await?;
    let owner = owner.ok_or_else(Fail::missing)?;
    if !viewer.is_empty() && profiles::blocked_between(db, viewer, &owner).await? {
        return Err(Fail::missing());
    }
    Ok(owner)
}

/// GET /api/beacons/{id}/comments: the newest 50, leaving out people the viewer blocked or who
/// blocked them.
async fn list(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err(Fail::missing());
    }
    let viewer = profiles::viewer(&app, &jar).await?;
    let me = viewer.as_ref().map(|v| v.id.clone()).unwrap_or_default();
    let mut db = app.db.acquire().await?;
    let owner = visible(&mut db, &id, &me).await?;
    // Only chip_sql with a literal alias is interpolated; values are bound.
    let mut items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT jsonb_build_object('id',m.id,'body',m.body,'created_at',m.created_at,'author',{},'mine',m.author_id=$2) FROM beacon_comments m JOIN channel_users c ON c.id=m.author_id AND c.eligible
        WHERE m.beacon_id=$1 AND m.status='VISIBLE' AND m.deleted_at IS NULL
        AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE (k.blocker_id=$2 AND k.blocked_id=m.author_id) OR (k.blocker_id=m.author_id AND k.blocked_id=$2))
        ORDER BY m.created_at DESC LIMIT $3",
        profiles::chip_sql("c")
    )))
    .bind(&id)
    .bind(&me)
    .bind(LIST)
    .fetch_all(&mut *db)
    .await?;
    items
        .iter_mut()
        .for_each(|v| profiles::hydrate(&app, &mut v["author"]));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM beacon_comments WHERE beacon_id=$1 AND status='VISIBLE' AND deleted_at IS NULL")
        .bind(&id).fetch_one(&mut *db).await?;
    Ok(Json(json!({
        "comments": items,
        "count": count,
        "can_comment": viewer.as_ref().is_some_and(|v| v.email_verified),
        "can_moderate": me == owner,
    })))
}

#[derive(Deserialize)]
pub struct Post {
    body: String,
}
/// POST /api/beacons/{id}/comments
async fn post(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Post>,
) -> Res<Json<Value>> {
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err(Fail::missing());
    }
    let user = profiles::signed_in(&app, &jar).await?;
    if !user.email_verified {
        return Err(Fail::denied("Verify your email address to comment."));
    }
    let body = input.body.trim();
    if body.is_empty() || body.chars().count() > MAX_CHARS {
        return Err(Fail::field("body", "Write a comment of 1–300 characters."));
    }
    crate::text::plain(body, "body", 1, MAX_CHARS, 2, false)?;
    profiles::rate(&app, format!("beacon-comment:{}", user.id), 10, 60).await?;
    let mut tx = app.db.begin().await?;
    profiles::ensure_unrestricted(&mut tx, &user.id).await?;
    let owner = visible(&mut tx, &id, &user.id).await?;
    let banned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND kind='ban')")
        .bind(&owner).bind(&user.id).fetch_one(&mut *tx).await?;
    if banned {
        return Err(Fail::denied("You can't comment on this channel's Beacons."));
    }
    let words: Vec<String> = sqlx::query_scalar(
        "SELECT coalesce((SELECT banned_words FROM chat_settings WHERE channel_id=$1),'{}')",
    )
    .bind(&owner)
    .fetch_one(&mut *tx)
    .await?;
    moderation::check_content(body, &words, true, false)?;
    let comment = profiles::new_id();
    sqlx::query("INSERT INTO beacon_comments(id,beacon_id,author_id,body) VALUES($1,$2,$3,$4)")
        .bind(&comment)
        .bind(&id)
        .bind(&user.id)
        .bind(body)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    list(State(app), jar, Path(id)).await
}

/// DELETE /api/beacons/{id}/comments/{comment}: by its author or the Beacon's owner.
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path((id, comment)): Path<(String, String)>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let removed = sqlx::query("UPDATE beacon_comments m SET deleted_at=now() FROM beacons b WHERE m.id=$1 AND m.beacon_id=$2 AND b.id=m.beacon_id AND m.deleted_at IS NULL AND (m.author_id=$3 OR b.owner_id=$3)")
        .bind(&comment).bind(&id).bind(&user.id).execute(&app.db).await?.rows_affected();
    if removed == 0 {
        return Err(Fail::missing());
    }
    list(State(app), jar, Path(id)).await
}

/// Report support (`safety.rs`): the comment if the reporter can see it, as (author, username, snapshot).
pub async fn target(
    db: &mut PgConnection,
    id: &str,
    reporter: &str,
) -> Res<Option<(String, String, Value)>> {
    Ok(sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('body',m.body,'beacon',m.beacon_id) FROM beacon_comments m JOIN channel_users c ON c.id=m.author_id JOIN beacons b ON b.id=m.beacon_id
        WHERE m.id=$1 AND m.status='VISIBLE' AND m.deleted_at IS NULL AND b.status='PUBLISHED' AND NOT b.hidden AND c.deleted_at IS NULL
        AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE (k.blocker_id=$2 AND k.blocked_id IN (m.author_id,b.owner_id)) OR (k.blocked_id=$2 AND k.blocker_id IN (m.author_id,b.owner_id)))")
        .bind(id).bind(reporter).fetch_optional(db).await?)
}

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/beacons/{id}/comments", get(list).post(post))
        .route("/api/beacons/{id}/comments/{comment}", delete(remove))
}
