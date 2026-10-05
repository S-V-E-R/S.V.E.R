use super::{engine, member};
use crate::{
    App,
    profiles::{self, Fail, Res},
    safety, text,
};
use axum::{
    Json,
    extract::{Path, State},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

async fn week(db: &mut PgConnection, app: &App) -> Res<engine::Week> {
    engine::lock(db).await?;
    let at = engine::now(db).await?;
    engine::advance(db, &app.config.factions, at).await?;
    engine::current(db, at)
        .await?
        .ok_or_else(|| Fail::conflict("Voting opens when the season starts."))
}
pub async fn council(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let current = week(&mut tx, &app).await?;
    member(&mut tx, &user.id, &faction, false).await?;
    let votes:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('genre',v.genre,'votes',count(*)) FROM faction_votes v JOIN faction_members m ON m.user_id=v.user_id AND m.faction=v.faction WHERE v.week_id=$1 AND v.faction=$2 GROUP BY v.genre ORDER BY count(*) DESC,v.genre")
        .bind(current.id).bind(&faction).fetch_all(&mut *tx).await?;
    let mine: Option<String> = sqlx::query_scalar(
        "SELECT genre FROM faction_votes WHERE week_id=$1 AND user_id=$2 AND faction=$3",
    )
    .bind(current.id)
    .bind(&user.id)
    .bind(&faction)
    .fetch_optional(&mut *tx)
    .await?;
    let target: Option<String> =
        sqlx::query_scalar("SELECT genre FROM faction_targets WHERE week_id=$1 AND faction=$2")
            .bind(current.id)
            .bind(&faction)
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"votes":votes,"my_vote":mine,"target":target,"closes_at":current.ends_at}),
    ))
}
#[derive(Deserialize)]
pub struct Vote {
    genre: String,
}
pub async fn vote(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
    Json(input): Json<Vote>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let current = week(&mut tx, &app).await?;
    member(&mut tx, &user.id, &faction, true).await?;
    if !sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM faction_territories WHERE season_id=$1 AND genre=$2)",
    )
    .bind(current.season_id)
    .bind(&input.genre)
    .fetch_one(&mut *tx)
    .await?
    {
        return Err(Fail::field("genre", "Choose a genre on the map."));
    }
    sqlx::query("INSERT INTO faction_votes(week_id,user_id,faction,genre) VALUES($1,$2,$3,$4) ON CONFLICT(week_id,user_id) DO UPDATE SET faction=$3,genre=$4")
        .bind(current.id).bind(&user.id).bind(&faction).bind(&input.genre).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub async fn board(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
    query: profiles::CursorQuery,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let cursor = profiles::parse_cursor(&query.cursor)?;
    let mut db = app.db.acquire().await?;
    member(&mut db, &user.id, &faction, false).await?;
    let rows:Vec<(String,String,String,DateTime<Utc>)>=sqlx::query_as("SELECT id,author_id,body,created_at FROM faction_posts WHERE faction=$1 AND status='VISIBLE' AND author_id IS NOT NULL AND ($2::timestamptz IS NULL OR (created_at,id)<($2,$3)) ORDER BY created_at DESC,id DESC LIMIT 31")
        .bind(&faction).bind(cursor.as_ref().map(|c|c.0)).bind(cursor.as_ref().map(|c|c.1.as_str()).unwrap_or("")).fetch_all(&mut *db).await?;
    let next = (rows.len() > 30).then(|| profiles::make_cursor(rows[29].3, &rows[29].0));
    let users = profiles::public_channels(
        &mut db,
        &rows
            .iter()
            .take(30)
            .map(|r| r.1.clone())
            .collect::<Vec<_>>(),
    )
    .await?;
    let moderator = is_moderator(&mut db, &user.id, &faction).await?;
    let mut items = Vec::new();
    for (id, author, body, at) in rows.into_iter().take(30) {
        let Some(profile) = users.iter().find(|u| u.id == author) else {
            continue;
        };
        if profiles::blocked_between(&mut db, &user.id, &author).await? {
            continue;
        }
        items.push(json!({"id":id,"author":profiles::chip(&app,profile),"body":body,"created_at":at,"can_delete":author==user.id || moderator,"can_report":author!=user.id}));
    }
    Ok(Json(
        json!({"items":items,"next_cursor":next,"can_post":user.email_verified,"slow_seconds":app.config.factions.board_slow_seconds}),
    ))
}
#[derive(Deserialize)]
pub struct Post {
    id: String,
    body: String,
}
pub async fn post(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
    Json(input): Json<Post>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if uuid::Uuid::parse_str(&input.id).is_err() {
        return Err(Fail::bad("Invalid post ID."));
    }
    let body = text::plain(&input.body, "body", 1, 500, 10, true)?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    member(&mut tx, &user.id, &faction, true).await?;
    let existing: Option<(String, String, String)> =
        sqlx::query_as("SELECT author_id,faction,body FROM faction_posts WHERE id=$1")
            .bind(&input.id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((author, side, old)) = existing {
        if author == user.id && side == faction && old == body {
            return Ok(Json(json!({"saved":true})));
        }
        return Err(Fail::conflict("That post ID is already in use."));
    }
    let recent:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_posts WHERE author_id=$1 AND created_at>clock_timestamp()-make_interval(secs=>$2))")
        .bind(&user.id).bind(app.config.factions.board_slow_seconds as f64).fetch_one(&mut *tx).await?;
    if recent {
        return Err(Fail::conflict(
            "Slow mode is on. Wait before posting again.",
        ));
    }
    sqlx::query("INSERT INTO faction_posts(id,faction,author_id,body) VALUES($1,$2,$3,$4)")
        .bind(&input.id)
        .bind(&faction)
        .bind(&user.id)
        .bind(body)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
async fn is_moderator(db: &mut PgConnection, user: &str, faction: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_moderators m JOIN faction_weeks w ON w.id=m.week_id JOIN faction_members f ON f.user_id=m.user_id AND f.faction=m.faction WHERE m.user_id=$1 AND m.faction=$2 AND w.starts_at<=now() AND w.ends_at>now())")
        .bind(user).bind(faction).fetch_one(db).await?)
}
#[derive(Deserialize)]
pub struct Remove {
    #[serde(default)]
    note: String,
}
pub async fn remove_post(
    State(app): State<App>,
    jar: CookieJar,
    Path((faction, id)): Path<(String, String)>,
    Json(input): Json<Remove>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    let author: Option<String> = sqlx::query_scalar(
        "SELECT author_id FROM faction_posts WHERE id=$1 AND faction=$2 AND author_id IS NOT NULL",
    )
    .bind(&id)
    .bind(&faction)
    .fetch_optional(&mut *tx)
    .await?;
    let author = author.ok_or_else(Fail::missing)?;
    member(&mut tx, &user.id, &faction, true).await?;
    if author != user.id && !is_moderator(&mut tx, &user.id, &faction).await? {
        return Err(Fail::denied(
            "Only the author or an elected faction moderator can remove this post.",
        ));
    }
    let note = if author == user.id {
        "Author deleted their post".to_string()
    } else {
        safety::note(Some(&input.note), "note", true)?
    };
    sqlx::query("UPDATE faction_posts SET status=$2 WHERE id=$1 AND status='VISIBLE'")
        .bind(&id)
        .bind(if author == user.id {
            "DELETED"
        } else {
            "REMOVED"
        })
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&user.id),
        "remove_faction_post",
        "faction_post",
        &id,
        &[],
        &note,
        json!({"faction":faction}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub async fn election(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let current = week(&mut tx, &app).await?;
    member(&mut tx, &user.id, &faction, false).await?;
    let rows:Vec<(String,i64)>=sqlx::query_as("SELECT c.user_id,(SELECT count(*) FROM faction_moderator_votes v JOIN faction_members m ON m.user_id=v.user_id AND m.faction=v.faction WHERE v.week_id=$2 AND v.faction=$1 AND v.candidate_id=c.user_id) FROM faction_members c WHERE c.faction=$1 AND c.moderator_candidate ORDER BY c.joined_at,c.user_id LIMIT 100")
        .bind(&faction).bind(current.id).fetch_all(&mut *tx).await?;
    let users = profiles::public_channels(
        &mut tx,
        &rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
    )
    .await?;
    let candidates = rows
        .into_iter()
        .filter_map(|(id, n)| {
            users
                .iter()
                .find(|u| u.id == id && u.email_verified)
                .map(|u| json!({"user":profiles::chip(&app,u),"votes":n}))
        })
        .collect::<Vec<_>>();
    let mine:Option<String>=sqlx::query_scalar("SELECT candidate_id FROM faction_moderator_votes WHERE week_id=$1 AND user_id=$2 AND faction=$3")
        .bind(current.id).bind(&user.id).bind(&faction).fetch_optional(&mut *tx).await?;
    let candidate: bool =
        sqlx::query_scalar("SELECT moderator_candidate FROM faction_members WHERE user_id=$1")
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    let selected = match mine {
        Some(id) => profiles::channel_user_by_id(&mut tx, &id)
            .await?
            .map(|u| u.username),
        None => None,
    };
    tx.commit().await?;
    Ok(Json(
        json!({"candidates":candidates,"my_vote":selected,"candidate":candidate,"closes_at":current.ends_at}),
    ))
}
#[derive(Deserialize)]
pub struct Candidacy {
    enabled: bool,
}
pub async fn candidate(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
    Json(input): Json<Candidacy>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    member(&mut tx, &user.id, &faction, true).await?;
    sqlx::query("UPDATE faction_members SET moderator_candidate=$2 WHERE user_id=$1")
        .bind(&user.id)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
pub struct ElectionVote {
    username: String,
}
pub async fn election_vote(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
    Json(input): Json<ElectionVote>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let current = week(&mut tx, &app).await?;
    member(&mut tx, &user.id, &faction, true).await?;
    let candidate = profiles::eligible_by_name(&mut tx, &input.username)
        .await?
        .ok_or_else(Fail::missing)?;
    member(&mut tx, &candidate.id, &faction, true).await?;
    let consent: bool =
        sqlx::query_scalar("SELECT moderator_candidate FROM faction_members WHERE user_id=$1")
            .bind(&candidate.id)
            .fetch_one(&mut *tx)
            .await?;
    if !consent {
        return Err(Fail::bad("That member is not standing for election."));
    }
    sqlx::query("INSERT INTO faction_moderator_votes(week_id,user_id,candidate_id,faction) VALUES($1,$2,$3,$4) ON CONFLICT(week_id,user_id) DO UPDATE SET candidate_id=$3,faction=$4")
        .bind(current.id).bind(&user.id).bind(&candidate.id).bind(&faction).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}

pub async fn target(
    db: &mut PgConnection,
    id: &str,
    reporter: &str,
) -> Res<Option<(String, String, Value)>> {
    let row:Option<(String,String,String)>=sqlx::query_as("SELECT faction,author_id,body FROM faction_posts WHERE id=$1 AND status='VISIBLE' AND author_id IS NOT NULL")
        .bind(id).fetch_optional(&mut *db).await?;
    let Some((faction, author, body)) = row else {
        return Ok(None);
    };
    member(db, reporter, &faction, false).await?;
    let Some(user) = profiles::channel_user_by_id(db, &author).await? else {
        return Ok(None);
    };
    if !user.eligible || profiles::blocked_between(db, reporter, &author).await? {
        return Ok(None);
    }
    Ok(Some((
        author,
        user.username,
        json!({"body":body,"faction":faction}),
    )))
}
pub async fn owner(db: &mut PgConnection, id: &str) -> Res<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT author_id FROM faction_posts WHERE id=$1 AND author_id IS NOT NULL",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}
pub async fn snapshot(db: &mut PgConnection, id: &str) -> Res<Value> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('body',body,'status',status,'faction',faction) FROM faction_posts WHERE id=$1")
        .bind(id).fetch_optional(db).await?.unwrap_or(Value::Null))
}
pub async fn remove(db: &mut PgConnection, id: &str) -> Res<Value> {
    let previous: Option<String> =
        sqlx::query_scalar("SELECT status FROM faction_posts WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?;
    let previous = previous.ok_or_else(Fail::missing)?;
    sqlx::query("UPDATE faction_posts SET status='REMOVED' WHERE id=$1 AND status='VISIBLE'")
        .bind(id)
        .execute(db)
        .await?;
    Ok(json!({"type":"faction_post","id":id,"previous":previous}))
}
pub async fn restore(db: &mut PgConnection, id: &str, previous: &str) -> Res<()> {
    if previous == "VISIBLE" {
        sqlx::query("UPDATE faction_posts SET status='VISIBLE' WHERE id=$1 AND status='REMOVED' AND author_id IS NOT NULL").bind(id).execute(db).await?;
    }
    Ok(())
}
