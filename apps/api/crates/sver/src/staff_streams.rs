//! Staff operations for live streams: the live list with health, counts and open reports, an
//! audited stop, and the category catalog (docs/LIVE_STREAMS.md "Staff operations").
use crate::{
    App,
    profiles::{Fail, Res},
    safety,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

type Row = (
    String,
    String,
    DateTime<Utc>,
    Value,
    String,
    String,
    String,
    Option<String>,
    i64,
    Option<DateTime<Utc>>,
);
/// GET /api/admin/streams: every open broadcast, oldest first.
async fn list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let rows: Vec<Row> = sqlx::query_as("SELECT b.id,b.state,b.started_at,b.health,c.username,c.display_name,coalesce(s.title,c.username||'''s stream'),k.name,(SELECT count(*) FROM reports r WHERE r.target_type='live_stream' AND r.target_id=b.id AND r.status='OPEN'),(SELECT followers_only_until FROM chat_settings WHERE channel_id=b.owner_id AND followers_only_until>now()) FROM broadcasts b JOIN channel_users c ON c.id=b.owner_id LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories k ON k.id=s.category_id WHERE b.state<>'ENDED' ORDER BY b.started_at LIMIT 200")
        .fetch_all(&app.db)
        .await?;
    let mut db = app.db.acquire().await?;
    let mut items = Vec::new();
    for (
        id,
        state,
        started_at,
        health,
        username,
        display_name,
        title,
        category,
        reports,
        followers_only,
    ) in rows
    {
        let (raw, counted, trusted, excluded, pending) =
            crate::integrity::counts(&mut db, &id).await?;
        let mature: (bool, bool) = sqlx::query_as("SELECT coalesce((SELECT mature FROM stream_settings WHERE owner_id=b.owner_id),false),b.mature_locked FROM broadcasts b WHERE b.id=$1")
            .bind(&id).fetch_one(&mut *db).await?;
        items.push(json!({"id":id,"state":state,"started_at":started_at,"health":health,
            "mature":mature.0,"mature_locked":mature.1,
            "channel":{"username":username,"display_name":display_name},"title":title,"category":category,
            "open_reports":reports,"followers_only_until":followers_only,
            "counts":{"sessions":raw,"public":counted,"trusted":trusted,"excluded":excluded,"pending":pending}}));
    }
    Ok(Json(json!({"items":items})))
}

#[derive(Deserialize)]
struct Stop {
    reason: String,
}
/// POST /api/admin/streams/{id}/mature: labels the channel mature and locks it on until this
/// broadcast ends (docs/CHANNEL_ADDITIONS.md "Enforcement"). Audited.
async fn lock_mature(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Stop>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let reason = input.reason.trim();
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err(Fail::field("reason", "Give a reason of 1–500 characters."));
    }
    let mut tx = app.db.begin().await?;
    let owner: Option<String> = sqlx::query_scalar("UPDATE broadcasts SET mature=true,mature_locked=true WHERE id=$1 AND state<>'ENDED' RETURNING owner_id")
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await?;
    let owner = owner.ok_or_else(|| Fail::conflict("That stream has already ended."))?;
    sqlx::query("INSERT INTO stream_settings(owner_id,title,mature) SELECT id,username||'''s stream',true FROM users WHERE id=$1 ON CONFLICT(owner_id) DO UPDATE SET mature=true,revision=stream_settings.revision+1,updated_at=clock_timestamp()")
        .bind(&owner)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "lock_mature",
        "live_stream",
        &id,
        &[],
        reason,
        json!({"owner": owner}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"locked":true})))
}
/// POST /api/admin/streams/{id}/stop: ends the broadcast and revokes the key (the owner makes a
/// new one), exactly like a report's remove-content action. Audited; never undone.
async fn stop(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Stop>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let reason = input.reason.trim();
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err(Fail::field("reason", "Give a reason of 1–500 characters."));
    }
    let mut tx = app.db.begin().await?;
    let owner: Option<String> =
        sqlx::query_scalar("SELECT owner_id FROM broadcasts WHERE id=$1 AND state<>'ENDED'")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?;
    let owner = owner.ok_or_else(|| Fail::conflict("That stream has already ended."))?;
    crate::streams::revoke(&mut tx, &owner).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "stop_stream",
        "live_stream",
        &id,
        &[],
        reason,
        json!({"owner": owner}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"stopped":true})))
}

/// GET /api/admin/categories: the whole catalog, including inactive categories.
async fn categories(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',k.id,'name',k.name,'genre',k.genre,'active',k.active,'channels',(SELECT count(*) FROM stream_settings s WHERE s.category_id=k.id)) FROM stream_categories k ORDER BY k.active DESC,lower(k.name)")
        .fetch_all(&app.db)
        .await?;
    Ok(Json(json!({"items":items})))
}
fn name(text: &str) -> Res<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 60 || text.chars().any(char::is_control) {
        return Err(Fail::field("name", "Names are 1–60 characters."));
    }
    Ok(text.to_string())
}
/// A stable ID from the name: lowercase ASCII words joined by hyphens.
fn slug(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
#[derive(Deserialize)]
struct NewCategory {
    name: String,
    genre: String,
    note: String,
}
/// POST /api/admin/categories. Genres come from the war's catalog.
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewCategory>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let name = name(&input.name)?;
    let id = slug(&name);
    let genre = input.genre.trim();
    let note = safety::note(Some(&input.note), "note", true)?;
    if id.is_empty() || id.len() > 60 {
        return Err(Fail::field("name", "Use letters or numbers in the name."));
    }
    let mut tx = app.db.begin().await?;
    crate::factions::validate_genre(&mut tx, genre).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('stream-catalog'))")
        .execute(&mut *tx)
        .await?;
    let inserted = sqlx::query(
        "INSERT INTO stream_categories(id,name,genre) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
    )
    .bind(&id)
    .bind(&name)
    .bind(genre)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(Fail::conflict("That category already exists."));
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "create_category",
        "category",
        &id,
        &[],
        &note,
        json!({"genre": genre}),
        false,
    )
    .await?;
    tx.commit().await?;
    categories(State(app), jar).await
}
#[derive(Deserialize)]
struct EditCategory {
    name: Option<String>,
    active: Option<bool>,
    genre: Option<String>,
    note: String,
}
/// PATCH /api/admin/categories/{id}: rename, or hide from new choices. Channels keep their current
/// category until they change it.
async fn edit(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<EditCategory>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let new_name = input.name.as_deref().map(name).transpose()?;
    let note = safety::note(Some(&input.note), "note", true)?;
    let mut tx = app.db.begin().await?;
    if let Some(genre) = &input.genre {
        crate::factions::validate_genre(&mut tx, genre).await?;
        let old: Option<String> =
            sqlx::query_scalar("SELECT genre FROM stream_categories WHERE id=$1")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
        if old.as_deref() != Some(genre) {
            crate::factions::category_move_allowed(&app, &mut tx).await?;
        }
    }
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM stream_categories WHERE name=$2 AND id<>$1)",
    )
    .bind(&id)
    .bind(&new_name)
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(Fail::conflict("Another category has that name."));
    }
    let updated = sqlx::query("UPDATE stream_categories SET name=coalesce($2,name),active=coalesce($3,active),genre=coalesce($4,genre) WHERE id=$1")
        .bind(&id)
        .bind(&new_name)
        .bind(input.active)
        .bind(&input.genre)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(Fail::missing());
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "edit_category",
        "category",
        &id,
        &[],
        &note,
        json!({"active": input.active,"genre":input.genre,"name":new_name}),
        false,
    )
    .await?;
    tx.commit().await?;
    categories(State(app), jar).await
}

#[derive(Deserialize)]
struct MergeCategory {
    into: String,
    note: String,
}
async fn merge_category(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<MergeCategory>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    if id == input.into {
        return Err(Fail::bad("Choose a different destination category."));
    }
    let note = safety::note(Some(&input.note), "note", true)?;
    let mut tx = app.db.begin().await?;
    crate::factions::lock(&mut tx).await?;
    let source: Option<String> =
        sqlx::query_scalar("SELECT genre FROM stream_categories WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?;
    let target: Option<String> =
        sqlx::query_scalar("SELECT genre FROM stream_categories WHERE id=$1 AND active FOR UPDATE")
            .bind(&input.into)
            .fetch_optional(&mut *tx)
            .await?;
    let (Some(source), Some(target)) = (source, target) else {
        return Err(Fail::missing());
    };
    if source != target {
        crate::factions::category_move_allowed(&app, &mut tx).await?;
    }
    sqlx::query(
        "UPDATE stream_settings SET category_id=$2,revision=revision+1 WHERE category_id=$1",
    )
    .bind(&id)
    .bind(&input.into)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE stream_categories SET active=false WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "merge_category",
        "category",
        &id,
        &[],
        &note,
        json!({"into":input.into}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/admin/game-catalog",
            get(crate::streams::catalog::review_queue),
        )
        .route(
            "/api/admin/game-catalog/{id}",
            post(crate::streams::catalog::review),
        )
        .route("/api/admin/streams", get(list))
        .route("/api/admin/streams/{id}/stop", post(stop))
        .route("/api/admin/streams/{id}/mature", post(lock_mature))
        .route("/api/admin/categories", get(categories).post(create))
        .route("/api/admin/categories/{id}", patch(edit))
        .route("/api/admin/categories/{id}/merge", post(merge_category))
}

#[cfg(test)]
mod tests {
    #[test]
    fn category_ids_are_stable_slugs() {
        assert_eq!(super::slug("Counter-Strike 2"), "counter-strike-2");
        assert_eq!(super::slug("  Just Chatting!! "), "just-chatting");
        assert_eq!(super::slug("Pokémon"), "pok-mon");
        assert_eq!(super::slug("ポケモン"), "");
    }
}
