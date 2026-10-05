use super::{FACTIONS, engine, valid};
use crate::{
    App,
    profiles::{self, Fail, Res},
    safety,
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

pub(super) async fn war_data(db: &mut PgConnection, app: &App, at: DateTime<Utc>) -> Res<Value> {
    let Some(season) = engine::latest(db).await? else {
        return Ok(
            json!({"season":null,"week":null,"genres":[],"factions":[],"scoreboard":[],"history":[],"previous_winners":[]}),
        );
    };
    let week: Option<engine::Week> = sqlx::query_as(
        "SELECT * FROM faction_weeks WHERE season_id=$1 ORDER BY starts_at DESC LIMIT 1",
    )
    .bind(season.id)
    .fetch_optional(&mut *db)
    .await?;
    let mut data = match &week {
        Some(w) => engine::standings(db, &app.config.factions, w, at.min(w.ends_at)).await?,
        None => json!({"genres":[],"factions":[]}),
    };
    data["as_of"] = json!(at);
    data["season"] = json!({"number":season.number,"starts_at":season.starts_at,"ends_at":season.ends_at,"next_starts_at":season.next_starts_at,"finished":season.finished_at.is_some(),"winners":season.winners});
    data["week"]=week.map_or(Value::Null,|w|json!({"id":w.id,"starts_at":w.starts_at,"ends_at":w.ends_at,"completed":w.completed_at.is_some()}));
    let counts:Vec<(String,i64)>=sqlx::query_as("SELECT holder,count(*) FROM faction_territories WHERE season_id=$1 AND holder IS NOT NULL GROUP BY holder")
        .bind(season.id).fetch_all(&mut *db).await?;
    let held:Vec<(String,i64)>=sqlx::query_as("SELECT g->>'holder',count(*) FROM faction_weeks w CROSS JOIN LATERAL jsonb_array_elements(w.result->'genres') g WHERE w.season_id=$1 AND g->>'holder' IS NOT NULL GROUP BY 1")
        .bind(season.id).fetch_all(&mut *db).await?;
    data["scoreboard"]=json!(FACTIONS.iter().map(|f|json!({"faction":f,"territories":counts.iter().find(|(a,_)|a==f).map_or(0,|(_,n)|*n),"genre_weeks":held.iter().find(|(a,_)|a==f).map_or(0,|(_,n)|*n)})).collect::<Vec<_>>());
    let previous:Option<Vec<String>>=sqlx::query_scalar("SELECT winners FROM faction_seasons WHERE finished_at IS NOT NULL AND number<$1 ORDER BY number DESC LIMIT 1")
        .bind(season.number).fetch_optional(&mut *db).await?;
    data["previous_winners"] = json!(previous.unwrap_or_default());
    data["history"]=json!(sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('ends_at',ends_at,'genres',result->'genres') FROM faction_weeks WHERE season_id=$1 AND completed_at IS NOT NULL ORDER BY ends_at DESC LIMIT 14")
        .bind(season.id).fetch_all(db).await?);
    Ok(data)
}
pub async fn war(State(app): State<App>) -> Res<Json<Value>> {
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    let data = war_data(&mut tx, &app, at).await?;
    tx.commit().await?;
    Ok(Json(data))
}
pub async fn hub(
    State(app): State<App>,
    jar: CookieJar,
    Path(faction): Path<String>,
) -> Res<Json<Value>> {
    if !valid(&faction) {
        return Err(Fail::missing());
    }
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    let mut data = war_data(&mut tx, &app, at).await?;
    data["faction"] = json!(faction);
    data["is_member"] = json!(match &viewer {
        Some(v) =>
            profiles::channel_user_by_id(&mut tx, &v.id)
                .await?
                .is_some_and(|u| u.eligible)
                && super::membership(&mut tx, &v.id).await?.as_deref() == Some(&faction),
        None => false,
    });
    let season = engine::latest(&mut tx).await?;
    let week = engine::current(&mut tx, at).await?;
    for (name, weekly) in [("weekly_leaders", true), ("season_leaders", false)] {
        let rows:Vec<(String,i64)>=sqlx::query_as("SELECT i.user_id,sum(i.points)::bigint FROM faction_influence i JOIN faction_weeks w ON w.id=i.week_id WHERE i.faction=$1 AND i.user_id IS NOT NULL AND CASE WHEN $2 THEN w.id=$3 ELSE w.season_id=$4 END GROUP BY i.user_id ORDER BY sum(i.points) DESC,i.user_id LIMIT 50")
            .bind(&faction).bind(weekly).bind(week.as_ref().map(|w|w.id)).bind(season.as_ref().map(|s|s.id)).fetch_all(&mut *tx).await?;
        let ids = rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
        let users = profiles::public_channels(&mut tx, &ids).await?;
        data[name] = json!(
            rows.iter()
                .filter_map(|(id, n)| users
                    .iter()
                    .find(|u| &u.id == id)
                    .map(|u| json!({"user":profiles::chip(&app,u),"influence":n})))
                .collect::<Vec<_>>()
        );
    }
    data["live"] = json!(crate::playback::faction_streams(&app, &mut tx, &faction, at).await?);
    tx.commit().await?;
    Ok(Json(data))
}
pub async fn members(
    State(app): State<App>,
    Path(faction): Path<String>,
    query: profiles::CursorQuery,
) -> Res<Json<Value>> {
    if !valid(&faction) {
        return Err(Fail::missing());
    }
    let cursor = profiles::parse_cursor(&query.cursor)?;
    let mut db = app.db.acquire().await?;
    let rows:Vec<(DateTime<Utc>,String)>=sqlx::query_as("SELECT joined_at,user_id FROM faction_members WHERE faction=$1 AND ($2::timestamptz IS NULL OR (joined_at,user_id)>($2,$3)) ORDER BY joined_at,user_id LIMIT 51")
        .bind(&faction).bind(cursor.as_ref().map(|c|c.0)).bind(cursor.as_ref().map(|c|c.1.as_str()).unwrap_or("")).fetch_all(&mut *db).await?;
    let next = (rows.len() > 50).then(|| profiles::make_cursor(rows[49].0, &rows[49].1));
    let users = profiles::public_channels(
        &mut db,
        &rows
            .iter()
            .take(50)
            .map(|(_, id)| id.clone())
            .collect::<Vec<_>>(),
    )
    .await?;
    let items = rows
        .iter()
        .take(50)
        .filter_map(|(at, id)| {
            users
                .iter()
                .find(|u| &u.id == id)
                .map(|u| json!({"user":profiles::chip(&app,u),"joined_at":at}))
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"items":items,"next_cursor":next})))
}
pub async fn admin(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let seasons:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('number',number,'starts_at',starts_at,'ends_at',ends_at,'next_starts_at',next_starts_at,'finished_at',finished_at,'winners',winners) FROM faction_seasons ORDER BY number DESC LIMIT 20").fetch_all(&mut *db).await?;
    let jobs:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',w.id,'season',s.number,'starts_at',w.starts_at,'ends_at',w.ends_at,'completed_at',w.completed_at,'attempts',w.attempts,'error',w.last_error) FROM faction_weeks w JOIN faction_seasons s ON s.id=w.season_id ORDER BY w.ends_at DESC LIMIT 100").fetch_all(&mut *db).await?;
    type SwitchRow = (
        i64,
        Option<String>,
        Option<String>,
        String,
        DateTime<Utc>,
        String,
    );
    let rows:Vec<SwitchRow>=sqlx::query_as("SELECT id,user_id,from_faction,to_faction,happened_at,reason FROM faction_switches ORDER BY id DESC LIMIT 100").fetch_all(&mut *db).await?;
    let users = profiles::public_channels(
        &mut db,
        &rows.iter().filter_map(|r| r.1.clone()).collect::<Vec<_>>(),
    )
    .await?;
    let switches=rows.into_iter().map(|(id,user,from,to,at,reason)|json!({"id":id,"user":users.iter().find(|u|Some(&u.id)==user.as_ref()).map(|u|profiles::chip(&app,u)),"from":from,"to":to,"at":at,"reason":reason})).collect::<Vec<_>>();
    Ok(Json(
        json!({"seasons":seasons,"jobs":jobs,"switches":switches}),
    ))
}
pub async fn genres(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'home',home,'neighbors',neighbors) FROM faction_genres ORDER BY position,id").fetch_all(&app.db).await?;
    Ok(Json(json!({"items":rows})))
}
#[derive(Deserialize)]
pub struct GenreInput {
    name: String,
    note: String,
}
fn name(input: &str) -> Res<String> {
    let name = input.trim();
    if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
        return Err(Fail::field("name", "Names are 1–60 characters."));
    }
    Ok(name.into())
}
pub async fn create_genre(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<GenreInput>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    let name = name(&input.name)?;
    let id = name
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if !(2..=40).contains(&id.len()) {
        return Err(Fail::field(
            "name",
            "Use a name with 2–40 letters or numbers.",
        ));
    }
    let note = safety::note(Some(&input.note), "note", true)?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    let at = engine::now(&mut tx).await?;
    engine::advance(&mut tx, &app.config.factions, at).await?;
    if sqlx::query("INSERT INTO faction_genres(id,name,position) VALUES($1,$2,(SELECT coalesce(max(position),0)+1 FROM faction_genres)) ON CONFLICT DO NOTHING")
        .bind(&id).bind(&name).execute(&mut *tx).await?.rows_affected()==0 {
        return Err(Fail::conflict("That genre already exists."));
    }
    sqlx::query("INSERT INTO faction_territories(season_id,genre) SELECT id,$1 FROM faction_seasons WHERE finished_at IS NULL")
        .bind(&id).execute(&mut *tx).await?;
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "create_genre",
        "genre",
        &id,
        &[],
        &note,
        json!({"name":name}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true,"id":id})))
}
pub async fn edit_genre(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<GenreInput>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    let name = name(&input.name)?;
    let note = safety::note(Some(&input.note), "note", true)?;
    let mut tx = app.db.begin().await?;
    engine::lock(&mut tx).await?;
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM faction_genres WHERE name=$1 AND id<>$2)",
    )
    .bind(&name)
    .bind(&id)
    .fetch_one(&mut *tx)
    .await?
    {
        return Err(Fail::conflict("That genre name is already in use."));
    }
    if sqlx::query("UPDATE faction_genres SET name=$2 WHERE id=$1")
        .bind(&id)
        .bind(&name)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        == 0
    {
        return Err(Fail::missing());
    }
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "rename_genre",
        "genre",
        &id,
        &[],
        &note,
        json!({"name":name}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
