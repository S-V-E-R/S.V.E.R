//! Emergency switches and the site banner (docs/ADMIN.md "Operations"). A switch turns one risky
//! feature off in seconds, with a short message where the feature would be, and back on, with no
//! deploy. Every flip and banner change needs staff step-up and a note, and is audited.
use crate::{
    App,
    profiles::{Fail, Res},
    safety,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::{get, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// Each switch and the name shown to people while it's off.
pub const SWITCHES: [(&str, &str); 11] = [
    ("signups", "Sign-ups"),
    ("going_live", "Going live"),
    ("restreaming", "Restreaming"),
    ("linked_chat_twitch", "Linked chat from Twitch"),
    ("linked_chat_youtube", "Linked chat from YouTube"),
    ("linked_chat_kick", "Linked chat from Kick"),
    ("clipping", "Clipping"),
    ("beacon_uploads", "Beacon uploads"),
    ("dms", "Direct messages"),
    ("purchases", "Purchases"),
    ("payouts", "Payouts"),
];
fn label(name: &str) -> &'static str {
    SWITCHES
        .iter()
        .find(|(n, _)| *n == name)
        .map_or("This feature", |s| s.1)
}

/// Whether a feature is switched off right now.
pub async fn off(db: &mut PgConnection, name: &str) -> Res<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feature_switches WHERE name=$1 AND off)")
            .bind(name)
            .fetch_one(db)
            .await?,
    )
}
/// Refuses with the feature's message while it's switched off.
pub async fn guard(db: &mut PgConnection, name: &str) -> Res<()> {
    if off(db, name).await? {
        return Err(Fail::unavailable(format!(
            "Paused right now: {}. Please try again soon.",
            label(name)
        )));
    }
    Ok(())
}

type Banner = Option<(String, Option<DateTime<Utc>>, DateTime<Utc>)>;
async fn banner(db: &mut PgConnection) -> Res<Value> {
    let row: Banner = sqlx::query_as("SELECT message,ends_at,updated_at FROM site_banner WHERE id=1 AND (ends_at IS NULL OR ends_at>now())")
        .fetch_optional(db)
        .await?;
    Ok(row.map_or(Value::Null, |(message, ends_at, updated_at)| {
        json!({"message": message, "ends_at": ends_at, "updated_at": updated_at})
    }))
}
/// GET /api/site: the banner and which features are paused, for every page.
async fn site(State(app): State<App>) -> Res<impl IntoResponse> {
    let mut db = app.db.acquire().await?;
    let paused: Vec<String> =
        sqlx::query_scalar("SELECT name FROM feature_switches WHERE off ORDER BY name")
            .fetch_all(&mut *db)
            .await?;
    let paused: Vec<Value> = paused
        .iter()
        .map(|n| json!({"name": n, "label": label(n)}))
        .collect();
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=15")],
        Json(json!({"banner": banner(&mut db).await?, "paused": paused})),
    ))
}

type SwitchRow = (String, bool, Option<String>, DateTime<Utc>);
async fn admin_view(app: &App) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    let rows: Vec<SwitchRow> = sqlx::query_as("SELECT f.name,f.off,u.username,f.changed_at FROM feature_switches f LEFT JOIN users u ON u.id=f.changed_by")
        .fetch_all(&mut *db)
        .await?;
    let switches: Vec<Value> = SWITCHES
        .iter()
        .map(|(name, label)| {
            let row = rows.iter().find(|r| r.0 == *name);
            json!({"name": name, "label": label, "off": row.is_some_and(|r| r.1),
                "changed_by": row.and_then(|r| r.2.clone()), "changed_at": row.map(|r| r.3)})
        })
        .collect();
    let banner: Option<(String, Option<DateTime<Utc>>)> =
        sqlx::query_as("SELECT message,ends_at FROM site_banner WHERE id=1")
            .fetch_optional(&mut *db)
            .await?;
    Ok(Json(json!({"switches": switches,
        "banner": banner.map(|(message, ends_at)| json!({"message": message, "ends_at": ends_at}))})))
}
/// GET /api/admin/switches: every switch and the banner, for staff.
async fn admin(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    admin_view(&app).await
}
#[derive(Deserialize)]
pub struct Flip {
    off: bool,
    note: Option<String>,
}
/// PUT /api/admin/switches/{name}: turn a feature off or back on.
async fn flip(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Flip>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    if !SWITCHES.iter().any(|(n, _)| *n == name) {
        return Err(Fail::missing());
    }
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO feature_switches(name,off,changed_by) VALUES($1,$2,$3) ON CONFLICT(name) DO UPDATE SET off=EXCLUDED.off,changed_by=EXCLUDED.changed_by,changed_at=now()")
        .bind(&name).bind(input.off).bind(&staff.id).execute(&mut *tx).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        if input.off { "switch_off" } else { "switch_on" },
        "switch",
        &name,
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    admin_view(&app).await
}
#[derive(Deserialize)]
pub struct SetBanner {
    /// None or empty clears the banner.
    message: Option<String>,
    ends_at: Option<DateTime<Utc>>,
    note: Option<String>,
}
/// PUT /api/admin/banner: one message across the top of every page, with an optional end time.
async fn set_banner(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<SetBanner>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let message = input
        .message
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty());
    if let Some(message) = message
        && (message.chars().count() > 280 || message.chars().any(char::is_control))
    {
        return Err(Fail::field(
            "message",
            "Use up to 280 characters on one line.",
        ));
    }
    if input.ends_at.is_some_and(|at| at <= Utc::now()) {
        return Err(Fail::field("ends_at", "Choose an end time in the future."));
    }
    let mut tx = app.db.begin().await?;
    match message {
        Some(message) => {
            sqlx::query("INSERT INTO site_banner(id,message,ends_at,updated_by) VALUES(1,$1,$2,$3) ON CONFLICT(id) DO UPDATE SET message=EXCLUDED.message,ends_at=EXCLUDED.ends_at,updated_by=EXCLUDED.updated_by,updated_at=now()")
                .bind(message).bind(input.ends_at).bind(&staff.id).execute(&mut *tx).await?;
        }
        None => {
            sqlx::query("DELETE FROM site_banner WHERE id=1")
                .execute(&mut *tx)
                .await?;
        }
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        if message.is_some() {
            "banner_set"
        } else {
            "banner_clear"
        },
        "banner",
        "site",
        &[],
        &note,
        json!({"message": message, "ends_at": input.ends_at}),
        false,
    )
    .await?;
    tx.commit().await?;
    admin_view(&app).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/site", get(site))
        .route("/api/admin/switches", get(admin))
        .route("/api/admin/switches/{name}", put(flip))
        .route("/api/admin/banner", put(set_banner))
}
