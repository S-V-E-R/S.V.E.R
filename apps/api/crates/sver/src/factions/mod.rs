//! Module 4: faction identity and the seasonal genre war. Other modules use these interfaces.
mod community;
mod engine;
mod influence;
mod read;
#[cfg(test)]
mod tests;
mod tuning;
use crate::{
    App,
    profiles::{self, Fail, Res},
    safety,
};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
pub(crate) use community::{owner, remove, restore, snapshot, target};
pub(crate) use engine::lock;
pub use engine::tick;
pub use influence::support;
pub(crate) use influence::{chat, stream, watch};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
pub use tuning::Tuning;

pub const FACTIONS: [&str; 3] = ["myria", "aetheron", "glint"];
pub fn valid(value: &str) -> bool {
    FACTIONS.contains(&value)
}
pub async fn membership(db: &mut PgConnection, user: &str) -> Res<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT faction FROM faction_members WHERE user_id=$1")
            .bind(user)
            .fetch_optional(db)
            .await?,
    )
}
/// Projection for Profiles' SQL chips. `user` must be a fixed SQL expression, never user input.
pub fn membership_sql(user: &str) -> String {
    format!("(SELECT fm.faction FROM faction_members fm WHERE fm.user_id={user})")
}
pub(super) async fn member(
    db: &mut PgConnection,
    user: &str,
    faction: &str,
    verified: bool,
) -> Res<()> {
    if !valid(faction) {
        return Err(Fail::missing());
    }
    let account = profiles::channel_user_by_id(db, user)
        .await?
        .ok_or_else(Fail::missing)?;
    if !account.eligible || (verified && !account.email_verified) {
        return Err(Fail::denied(
            "A verified account in good standing is required.",
        ));
    }
    if membership(db, user).await?.as_deref() != Some(faction) {
        return Err(Fail::denied("Only this faction's members can access this."));
    }
    Ok(())
}
pub(super) async fn status(db: &mut PgConnection, user: &str, at: DateTime<Utc>) -> Res<Value> {
    let row: Option<(String, DateTime<Utc>, bool)> = sqlx::query_as(
        "SELECT faction,chosen_at,free_switch_used FROM faction_members WHERE user_id=$1",
    )
    .bind(user)
    .fetch_optional(&mut *db)
    .await?;
    let active = engine::current(db, at).await?.is_some();
    let season = engine::latest(db).await?;
    let between = season
        .as_ref()
        .is_some_and(|s| s.finished_at.is_some() && at < s.next_starts_at);
    let (faction, chosen, free) = match row {
        Some((f, c, used)) => (Some(f), Some(c), !used && at < c + Duration::days(7)),
        None => (None, None, false),
    };
    Ok(
        json!({"faction":faction,"chosen_at":chosen,"free_switch_available":free,
        "free_switch_until":chosen.map(|c|c+Duration::days(7)),"between_seasons":between,
        "can_choose":faction.is_none() || free || between,
        "next_switch_at":if active {season.map(|s|s.ends_at)} else {None}}),
    )
}
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    let value = status(&mut tx, &user.id, at).await?;
    tx.commit().await?;
    Ok(Json(value))
}
#[derive(Deserialize)]
struct Choose {
    faction: String,
}
async fn choose(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Choose>,
) -> Res<Json<Value>> {
    if !valid(&input.faction) {
        return Err(Fail::field("faction", "Choose Myria, Aetheron or Glint."));
    }
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    profiles::ensure_unrestricted(&mut tx, &user.id).await?;
    let state = status(&mut tx, &user.id, at).await?;
    if state["faction"] == input.faction {
        tx.commit().await?;
        return Ok(Json(state));
    }
    if state["can_choose"] != true {
        return Err(Fail::conflict(
            "Your faction is locked until the break between seasons.",
        ));
    }
    let reason = if state["faction"].is_null() {
        "join"
    } else if state["free_switch_available"] == true {
        "free_switch"
    } else {
        "season_break"
    };
    sqlx::query("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at,free_switch_used) VALUES($1,$2,$3,$3,false) ON CONFLICT(user_id) DO UPDATE SET faction=$2,joined_at=$3,free_switch_used=faction_members.free_switch_used OR $4,moderator_candidate=false")
        .bind(&user.id).bind(&input.faction).bind(at).bind(reason=="free_switch").execute(&mut *tx).await?;
    sqlx::query("INSERT INTO faction_switches(user_id,from_faction,to_faction,happened_at,reason) VALUES($1,$2,$3,$4,$5)")
        .bind(&user.id).bind(state["faction"].as_str()).bind(&input.faction).bind(at).bind(reason).execute(&mut *tx).await?;
    let value = status(&mut tx, &user.id, at).await?;
    tx.commit().await?;
    Ok(Json(value))
}
/// Imports the preserved selection exactly once. The importer resolves legacy numeric IDs from
/// the restored Faction table, never from a hard-coded guess or the live legacy database.
pub async fn import_membership(
    db: &mut PgConnection,
    user: &str,
    faction: &str,
    chosen: DateTime<Utc>,
) -> Res<bool> {
    if !valid(faction) {
        return Err(Fail::bad("Unknown legacy faction."));
    }
    let inserted=sqlx::query("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at) VALUES($1,$2,$3,$3) ON CONFLICT DO NOTHING")
        .bind(user).bind(faction).bind(chosen).execute(&mut *db).await?.rows_affected()>0;
    if inserted {
        sqlx::query("INSERT INTO faction_switches(user_id,to_faction,happened_at,reason) VALUES($1,$2,$3,'legacy_import')")
            .bind(user).bind(faction).bind(chosen).execute(db).await?;
    }
    Ok(inserted)
}
pub async fn rewards(db: &mut PgConnection, user: &str) -> Res<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('season',s.number,'faction',r.faction,'awarded_at',r.awarded_at,'valor_pending',r.valor_pending) FROM faction_rewards r JOIN faction_seasons s ON s.id=r.season_id WHERE r.user_id=$1 ORDER BY s.number DESC")
        .bind(user).fetch_all(db).await?)
}
pub async fn erase(db: &mut PgConnection, user: &str) -> Res<()> {
    sqlx::query("DELETE FROM faction_posts WHERE author_id=$1")
        .bind(user)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn validate_genre(db: &mut PgConnection, id: &str) -> Res<()> {
    if !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM faction_genres WHERE id=$1)")
        .bind(id)
        .fetch_one(db)
        .await?
    {
        return Err(Fail::field("genre", "Choose a genre from the catalog."));
    }
    Ok(())
}
pub async fn category_move_allowed(app: &App, db: &mut PgConnection) -> Res<()> {
    engine::lock(db).await?;
    let at = engine::now(db).await?;
    engine::advance(db, &app.config.factions, at).await?;
    if engine::current(db, at).await?.is_some() {
        return Err(Fail::conflict(
            "Categories can change genres only between seasons.",
        ));
    }
    Ok(())
}
#[derive(Deserialize)]
struct Retry {
    note: String,
}
async fn retry(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Retry>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    let note = safety::note(Some(&input.note), "note", true)?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "retry_faction_checkpoints",
        "factions",
        "timeline",
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/faction", get(mine).put(choose))
        .route("/api/factions/war", get(read::war))
        .route("/api/factions/{faction}", get(read::hub))
        .route("/api/factions/{faction}/members", get(read::members))
        .route(
            "/api/factions/{faction}/council",
            get(community::council).put(community::vote),
        )
        .route(
            "/api/factions/{faction}/board",
            get(community::board).post(community::post),
        )
        .route(
            "/api/factions/{faction}/board/{id}",
            axum::routing::delete(community::remove_post),
        )
        .route(
            "/api/factions/{faction}/election",
            get(community::election).put(community::election_vote),
        )
        .route(
            "/api/factions/{faction}/candidate",
            put(community::candidate),
        )
        .route("/api/admin/factions", get(read::admin))
        .route("/api/admin/factions/retry", post(retry))
        .route(
            "/api/admin/genres",
            get(read::genres).post(read::create_genre),
        )
        .route("/api/admin/genres/{id}", put(read::edit_genre))
}
