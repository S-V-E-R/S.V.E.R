//! Lights and Shine Moments (docs/SUPPORT.md "Shine", PROGRESSION.md Question 4, built by Joe's
//! decision on October 10, 2026). Shine Moments: up to 10 of a channel's own public clips and
//! Highlights pinned on its profile. Light It Up: recognition worth more than a like, free and
//! limited (3 a day, once per person per day), with an optional short message. Neither is money,
//! a score or a ranking signal (legacy's paid lights and Beacon Mode stay out).
use crate::{
    App,
    profiles::{self, Fail, Res},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_MOMENTS: usize = 10;
const LIGHTS_PER_DAY: i64 = 3;
/// A pinnable, showable clip or Highlight: the channel's own, public, ready, approved, not held.
const SHOWABLE: &str = "v.kind IN ('CLIP','HIGHLIGHT') AND v.status='READY' AND v.visibility='PUBLIC' AND v.approval='APPROVED' AND NOT EXISTS(SELECT 1 FROM video_holds h WHERE h.video_id=v.id)";

async fn channel(app: &App, name: &str) -> Res<String> {
    sqlx::query_scalar("SELECT id FROM channel_users WHERE lower(username)=lower($1) AND eligible AND deleted_at IS NULL")
        .bind(name).fetch_optional(&app.db).await?.ok_or_else(Fail::missing)
}

/// GET /api/channels/{name}/moments
async fn moments(State(app): State<App>, Path(name): Path<String>) -> Res<Json<Value>> {
    let id = channel(&app, &name).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT jsonb_build_object('video_id',v.id,'kind',v.kind,'title',coalesce(nullif(m.title,''),v.title),'description',m.description)
        FROM shine_moments m JOIN videos v ON v.id=m.video_id WHERE m.channel_id=$1 AND v.owner_id=$1 AND {SHOWABLE} ORDER BY m.position"
    )))
    .bind(&id).fetch_all(&app.db).await?;
    Ok(Json(json!({"moments": items})))
}
#[derive(Deserialize)]
pub struct Moment {
    video_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
}
#[derive(Deserialize)]
pub struct Moments {
    items: Vec<Moment>,
}
/// PUT /api/me/moments: replaces the pinned list, in order.
async fn set_moments(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Moments>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    if input.items.len() > MAX_MOMENTS {
        return Err(Fail::field("items", "Pin up to 10 Shine Moments."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("DELETE FROM shine_moments WHERE channel_id=$1")
        .bind(&me.id)
        .execute(&mut *tx)
        .await?;
    for (position, m) in input.items.iter().enumerate() {
        let title = if m.title.trim().is_empty() {
            String::new()
        } else {
            crate::text::plain(&m.title, "title", 1, 100, 0, false)?
        };
        let description = if m.description.trim().is_empty() {
            String::new()
        } else {
            crate::text::plain(&m.description, "description", 1, 500, 3, false)?
        };
        let ok: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT EXISTS(SELECT 1 FROM videos v WHERE v.id=$1 AND v.owner_id=$2 AND {SHOWABLE})"
        )))
        .bind(&m.video_id)
        .bind(&me.id)
        .fetch_one(&mut *tx)
        .await?;
        if !ok {
            return Err(Fail::field(
                "items",
                "Pin your own public clips and Highlights.",
            ));
        }
        sqlx::query("INSERT INTO shine_moments(channel_id,video_id,position,title,description) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
            .bind(&me.id).bind(&m.video_id).bind(position as i16).bind(title).bind(description).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    moments(State(app), Path(me.username)).await
}

#[derive(Deserialize)]
pub struct Light {
    to: String,
    #[serde(default)]
    message: String,
}
/// POST /api/lights: gives a light (3 a day, once per person per day, never yourself).
async fn give(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Light>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    if !me.email_verified {
        return Err(Fail::denied("Verify your email address to give lights."));
    }
    let to = channel(&app, &input.to).await?;
    if to == me.id {
        return Err(Fail::bad("You can't light yourself up."));
    }
    let mut tx = app.db.begin().await?;
    // One give at a time per person, so parallel requests can't beat the daily count below.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('lights:'||$1,0))")
        .bind(&me.id)
        .execute(&mut *tx)
        .await?;
    profiles::ensure_unrestricted(&mut tx, &me.id).await?;
    if profiles::blocked_between(&mut tx, &me.id, &to).await? {
        return Err(Fail::denied("You can't light up this person."));
    }
    let message = input.message.trim();
    if !message.is_empty() {
        crate::text::plain(message, "message", 1, 200, 1, false)?;
        let words: Vec<String> = sqlx::query_scalar(
            "SELECT coalesce((SELECT banned_words FROM chat_settings WHERE channel_id=$1),'{}')",
        )
        .bind(&to)
        .fetch_one(&mut *tx)
        .await?;
        crate::moderation::check_content(message, &words, true, false)?;
    }
    let today: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM lights WHERE from_id=$1 AND day=(now() AT TIME ZONE 'UTC')::date",
    )
    .bind(&me.id)
    .fetch_one(&mut *tx)
    .await?;
    if today >= LIGHTS_PER_DAY {
        return Err(Fail::conflict(
            "You've given your 3 lights today. More tomorrow.",
        ));
    }
    let id = profiles::new_id();
    let added = sqlx::query(
        "INSERT INTO lights(id,from_id,to_id,message) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(&id)
    .bind(&me.id)
    .bind(&to)
    .bind((!message.is_empty()).then_some(message))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if added == 0 {
        return Err(Fail::conflict("You already lit them up today."));
    }
    crate::alerts::community(&mut tx, &to, &me.id, "light", &id, None,
        &json!({"title": "You were lit up", "body": format!("{} lit you up.", me.username), "url": format!("/{}", input.to)})).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"given": true, "left_today": LIGHTS_PER_DAY - today - 1}),
    ))
}
/// GET /api/channels/{name}/lights: the count and the latest lights with messages.
async fn received(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let id = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM lights WHERE to_id=$1 AND NOT hidden")
            .bind(&id)
            .fetch_one(&app.db)
            .await?;
    let mut recent: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT jsonb_build_object('id',l.id,'from',{},'message',l.message,'at',l.created_at) FROM lights l JOIN channel_users c ON c.id=l.from_id
        WHERE l.to_id=$1 AND NOT l.hidden AND c.eligible AND l.message IS NOT NULL
        AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE (k.blocker_id=$2 AND k.blocked_id=l.from_id) OR (k.blocker_id=l.from_id AND k.blocked_id=$2))
        ORDER BY l.created_at DESC LIMIT 10",
        profiles::chip_sql("c")
    )))
    .bind(&id).bind(viewer.as_ref().map(|v| v.id.as_str()).unwrap_or("")).fetch_all(&app.db).await?;
    recent.iter_mut().for_each(|v| profiles::hydrate(&app, v));
    Ok(Json(json!({"count": count, "recent": recent})))
}
/// DELETE /api/me/lights/{id}: the recipient hides a light (its message leaves the profile).
async fn hide(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let hidden = sqlx::query("UPDATE lights SET hidden=true WHERE id=$1 AND to_id=$2")
        .bind(&id)
        .bind(&me.id)
        .execute(&app.db)
        .await?
        .rows_affected();
    if hidden == 0 {
        return Err(Fail::missing());
    }
    Ok(Json(json!({"hidden": true})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{name}/moments", get(moments))
        .route("/api/me/moments", put(set_moments))
        .route("/api/lights", post(give))
        .route("/api/channels/{name}/lights", get(received))
        .route("/api/me/lights/{id}", delete(hide))
}
