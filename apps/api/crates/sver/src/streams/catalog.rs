//! Wikidata is background enrichment only. Selection and publishing use Postgres exclusively.
use crate::{
    App,
    profiles::{Fail, Res},
    safety, text,
};
use axum::{
    Json,
    extract::{Path, State},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::BTreeSet;

const ENDPOINT: &str = "https://query.wikidata.org/sparql";
const PAGE_SIZE: usize = 200;

#[cfg(test)]
mod tests;

/// Only explicit genre terms, never arbitrary words from the title. Broad "action" and
/// "adventure" tags don't override a specific shooter/RPG/etc. Cross-bucket hybrids need review.
fn genre_label(label: &str) -> Option<&'static str> {
    match text::part_norm(label).as_str() {
        "first person shooter"
        | "third person shooter"
        | "shooter game"
        | "tactical shooter"
        | "hero shooter"
        | "battle royale game"
        | "looter shooter" => Some("fps_battle_royale"),
        "massively multiplayer online role playing game"
        | "massively multiplayer online game"
        | "role playing video game"
        | "action role playing game"
        | "tactical role playing game" => Some("mmos_rpgs"),
        "real time strategy"
        | "real time strategy video game"
        | "multiplayer online battle arena" => Some("rts_moba"),
        "strategy video game"
        | "turn based strategy video game"
        | "turn based strategy"
        | "4x"
        | "grand strategy wargame" => Some("strategy_4x"),
        "fighting game" | "platform fighter" => Some("fighting"),
        "racing video game" | "racing game" | "sports video game" | "sports game"
        | "driving video game" => Some("sports_racing"),
        "digital collectible card game"
        | "collectible card game"
        | "digital board game"
        | "board game"
        | "deck building game" => Some("card_board"),
        "puzzle video game"
        | "puzzle game"
        | "simulation video game"
        | "vehicle simulation game"
        | "business simulation game" => Some("puzzle_simulation"),
        "sandbox game"
        | "sandbox video game"
        | "life simulation game"
        | "farming simulation game"
        | "cozy game" => Some("cozy_sandbox"),
        "party video game" | "party game" => Some("coop_party"),
        _ => None,
    }
}

fn classify(genres: &[String], description: &str) -> Option<&'static str> {
    let mapped: BTreeSet<_> = genres.iter().filter_map(|g| genre_label(g)).collect();
    if !mapped.is_empty() {
        return (mapped.len() == 1).then(|| *mapped.first().unwrap());
    }
    // ponytail: exact genre phrases only; ambiguous prose stays in review instead of adding an LLM.
    let description = format!(" {} ", text::part_norm(description));
    if [" not ", " unlike ", " inspired ", " elements "]
        .iter()
        .any(|s| description.contains(s))
    {
        return None;
    }
    let phrases = [
        "first person shooter",
        "third person shooter",
        "massively multiplayer online role playing game",
        "role playing video game",
        "multiplayer online battle arena",
        "real time strategy",
        "fighting game",
        "racing video game",
        "sports video game",
        "puzzle video game",
        "life simulation game",
        "party video game",
    ];
    let matches: BTreeSet<_> = phrases
        .iter()
        .filter(|p| description.contains(&format!(" {p} ")))
        .filter_map(|p| genre_label(p))
        .collect();
    (matches.len() == 1).then(|| *matches.first().unwrap())
}

fn source_id(uri: &str) -> Res<&str> {
    let id = uri
        .strip_prefix("http://www.wikidata.org/entity/")
        .ok_or_else(Fail::internal)?;
    if !valid_id(id) {
        return Err(Fail::internal());
    }
    Ok(id)
}
fn valid_id(id: &str) -> bool {
    id.starts_with('Q')
        && (2..=19).contains(&id.len())
        && !id.starts_with("Q0")
        && id[1..].bytes().all(|c| c.is_ascii_digit())
}

fn query(cursor: &str, recent: bool) -> Res<String> {
    if !cursor.is_empty() && !valid_id(cursor) {
        return Err(Fail::internal());
    }
    let releases = if recent {
        let today = chrono::Utc::now().date_naive();
        format!(
            r#"?game wdt:P577 ?release. FILTER(?release >= "{}T00:00:00Z"^^xsd:dateTime && ?release < "{}T00:00:00Z"^^xsd:dateTime)"#,
            today - chrono::Duration::days(30),
            today + chrono::Duration::days(730)
        )
    } else {
        String::new()
    };
    Ok(format!(
        r#"SELECT ?game ?name ?description
        (GROUP_CONCAT(DISTINCT ?genreName;separator="|") AS ?genres)
        (GROUP_CONCAT(DISTINCT ?alias;separator="|") AS ?aliases) WHERE {{
        {{ SELECT DISTINCT ?game WHERE {{ ?game wdt:P31 wd:Q7889.
            {releases}
            FILTER(STR(?game) > "http://www.wikidata.org/entity/{cursor}")
        }} ORDER BY ?game LIMIT {PAGE_SIZE} }}
        OPTIONAL {{ ?game rdfs:label ?en FILTER(LANG(?en)="en") }}
        OPTIONAL {{ ?game rdfs:label ?mul FILTER(LANG(?mul)="mul") }}
        BIND(COALESCE(?en,?mul,"") AS ?name)
        OPTIONAL {{ ?game schema:description ?description FILTER(LANG(?description)="en") }}
        OPTIONAL {{ ?game wdt:P136 ?genre. ?genre rdfs:label ?genreName FILTER(LANG(?genreName)="en") }}
        OPTIONAL {{ ?game skos:altLabel ?alias FILTER(LANG(?alias) IN ("en","mul")) }}
    }} GROUP BY ?game ?name ?description ORDER BY ?game"#
    ))
}

#[derive(Deserialize, Default)]
struct Binding {
    value: String,
}
#[derive(Deserialize)]
struct Game {
    game: Binding,
    name: Binding,
    #[serde(default)]
    description: Binding,
    genres: Binding,
    aliases: Binding,
}
#[derive(Deserialize)]
struct Results {
    bindings: Vec<Game>,
}
#[derive(Deserialize)]
struct Page {
    results: Results,
}

fn terms(value: &str) -> Vec<String> {
    value
        .split('|')
        .filter_map(|s| text::clean(s, "catalog", false).ok())
        .filter(|s| !s.is_empty() && s.chars().count() <= 160)
        .take(40)
        .collect()
}

async fn import(db: &mut PgConnection, game: &Game) -> Res<()> {
    let id = source_id(&game.game.value)?;
    let Ok(name) = text::clean(&game.name.value, "name", false) else {
        return Ok(());
    };
    if name.is_empty() || name == id || name.chars().count() > 160 {
        return Ok(());
    }
    let aliases = terms(&game.aliases.value);
    let genres = terms(&game.genres.value);
    let description: String = game.description.value.chars().take(1000).collect();
    let genre = classify(&genres, &description);
    sqlx::query("INSERT INTO game_catalog(source_id,name,aliases,genres,description,suggested_genre) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(source_id) DO UPDATE SET name=$2,aliases=$3,genres=$4,description=$5,suggested_genre=$6,refreshed_at=now()")
        .bind(id).bind(&name).bind(&aliases).bind(&genres).bind(description).bind(genre).execute(&mut *db).await?;
    let (category, reviewed): (Option<String>, bool) = sqlx::query_as(
        "SELECT category_id,reviewed FROM game_catalog WHERE source_id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_one(&mut *db)
    .await?;
    if category.is_some() || reviewed {
        return Ok(());
    }
    // Match staff-created categories (including hidden ones) before considering a new row.
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM stream_categories c WHERE lower(name)=lower($1) AND NOT EXISTS(SELECT 1 FROM game_catalog g WHERE g.category_id=c.id AND g.source_id<>$2) ORDER BY id LIMIT 1",
    )
    .bind(&name)
    .bind(id)
    .fetch_optional(&mut *db)
    .await?;
    sqlx::query("UPDATE game_catalog SET category_id=$2 WHERE source_id=$1")
        .bind(id)
        .bind(existing)
        .execute(&mut *db)
        .await?;
    // Remakes can share a title. Never silently merge distinct provider identities.
    sqlx::query("UPDATE game_catalog SET suggested_genre=NULL WHERE category_id IS NULL AND NOT reviewed AND lower(name)=lower($1) AND (SELECT count(*) FROM game_catalog WHERE lower(name)=lower($1))>1")
        .bind(name).execute(db).await?;
    Ok(())
}

/// Materialize a selected game from our saved metadata. Never call a provider while saving.
pub async fn select(db: &mut PgConnection, category: &str) -> Res<String> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('stream-catalog'))")
        .execute(&mut *db)
        .await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_categories WHERE id=$1)")
            .bind(category)
            .fetch_one(&mut *db)
            .await?;
    if exists {
        return Ok(category.into());
    }
    let source = category
        .strip_prefix("wikidata-q")
        .map(|n| format!("Q{n}"))
        .ok_or_else(|| Fail::bad("Choose an available stream category."))?;
    let row: Option<(String, String, Option<String>)> = sqlx::query_as("SELECT name,suggested_genre,category_id FROM game_catalog WHERE source_id=$1 AND suggested_genre IS NOT NULL AND NOT reviewed FOR UPDATE")
        .bind(&source).fetch_optional(&mut *db).await?;
    let (name, genre, linked) =
        row.ok_or_else(|| Fail::bad("Choose an available stream category."))?;
    if let Some(linked) = linked {
        return Ok(linked);
    }
    crate::factions::validate_genre(db, &genre).await?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM stream_categories WHERE lower(name)=lower($1) ORDER BY id LIMIT 1",
    )
    .bind(&name)
    .fetch_optional(&mut *db)
    .await?;
    let chosen = existing.unwrap_or_else(|| category.into());
    sqlx::query(
        "INSERT INTO stream_categories(id,name,genre) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
    )
    .bind(&chosen)
    .bind(name)
    .bind(genre)
    .execute(&mut *db)
    .await?;
    sqlx::query("UPDATE game_catalog SET category_id=$2 WHERE source_id=$1")
        .bind(source)
        .bind(&chosen)
        .execute(db)
        .await?;
    Ok(chosen)
}

/// One leased page per pass. Network access never holds a DB transaction or a media worker.
pub async fn tick(app: &App) -> Res<()> {
    tick_at(app, ENDPOINT).await
}
async fn tick_at(app: &App, endpoint: &str) -> Res<()> {
    let token = uuid::Uuid::new_v4().to_string();
    let cursor: Option<(String, bool)> = sqlx::query_as("UPDATE game_catalog_sync SET lease_token=$1,lease_until=now()+interval '2 minutes' WHERE next_run_at<=now() AND (lease_until IS NULL OR lease_until<now()) RETURNING cursor,recent")
        .bind(&token).fetch_optional(&app.db).await?;
    let Some((cursor, recent)) = cursor else {
        return Ok(());
    };
    let result = fetch(app, endpoint, &cursor, recent).await;
    match result {
        Ok(games) => {
            let mut tx = app.db.begin().await?;
            let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM game_catalog_sync WHERE lease_token=$1 AND lease_until>now() FOR UPDATE)")
                .bind(&token).fetch_one(&mut *tx).await?;
            if !owned {
                return Ok(());
            }
            sqlx::query("SELECT pg_advisory_xact_lock(hashtext('stream-catalog'))")
                .execute(&mut *tx)
                .await?;
            for game in &games {
                import(&mut tx, game).await?;
            }
            let next = games
                .last()
                .map(|g| source_id(&g.game.value))
                .transpose()?
                .unwrap_or("");
            let complete = games.len() < PAGE_SIZE;
            let finished = complete && !recent;
            sqlx::query("UPDATE game_catalog_sync SET cursor=$2,next_run_at=now()+make_interval(secs=>$3),lease_token=NULL,lease_until=NULL,last_success_at=now(),last_complete_at=CASE WHEN $4 THEN now() ELSE last_complete_at END,failures=0,recent=$5 WHERE lease_token=$1")
                .bind(token).bind(if complete { "" } else { next }).bind(if finished {86400.0} else {2.0}).bind(finished).bind(if complete {!recent} else {recent}).execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(())
        }
        Err(retry) => {
            sqlx::query("UPDATE game_catalog_sync SET next_run_at=now()+make_interval(secs=>$2),lease_token=NULL,lease_until=NULL,failures=least(failures+1,1000) WHERE lease_token=$1")
                .bind(token).bind(retry as f64).execute(&app.db).await?;
            Err(Fail::unavailable(
                "Game catalog refresh will retry; saved games remain available.",
            ))
        }
    }
}

async fn fetch(app: &App, endpoint: &str, cursor: &str, recent: bool) -> Result<Vec<Game>, u64> {
    let query = query(cursor, recent).map_err(|_| 3600u64)?;
    let mut response = app
        .http
        .get(endpoint)
        .header("User-Agent", "SVER-Catalog/1.0 (https://sver.tv/contact)")
        .header("Accept", "application/sparql-results+json")
        .query(&[("format", "json"), ("query", &query)])
        .timeout(std::time::Duration::from_secs(25))
        .send()
        .await
        .map_err(|_| 3600u64)?;
    if !response.status().is_success() {
        return Err(response
            .headers()
            .get("retry-after")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(3600)
            .clamp(60, 86400));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| 3600u64)? {
        if body.len() + chunk.len() > 2_000_000 {
            return Err(3600);
        }
        body.extend_from_slice(&chunk);
    }
    let page: Page = serde_json::from_slice(&body).map_err(|_| 3600u64)?;
    if page.results.bindings.len() > PAGE_SIZE {
        return Err(3600);
    }
    let mut previous = cursor;
    for game in &page.results.bindings {
        let id = source_id(&game.game.value).map_err(|_| 3600u64)?;
        if id <= previous {
            return Err(3600);
        }
        previous = id;
    }
    Ok(page.results.bindings)
}

pub async fn review_queue(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',source_id,'name',name,'genres',genres,'description',description) FROM game_catalog WHERE category_id IS NULL AND suggested_genre IS NULL AND NOT reviewed ORDER BY name LIMIT 50").fetch_all(&app.db).await?;
    let status: Value = sqlx::query_scalar("SELECT jsonb_build_object('last_success_at',last_success_at,'last_complete_at',last_complete_at,'failures',failures,'pending',(SELECT count(*) FROM game_catalog WHERE category_id IS NULL AND suggested_genre IS NULL AND NOT reviewed)) FROM game_catalog_sync").fetch_one(&app.db).await?;
    Ok(Json(json!({"items":items,"status":status})))
}

#[derive(Deserialize)]
pub struct Decision {
    genre: Option<String>,
    name: Option<String>,
    note: String,
}
pub async fn review(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(Some(&input.note), "note", true)?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('stream-catalog'))")
        .execute(&mut *tx)
        .await?;
    let name: String = sqlx::query_scalar("SELECT name FROM game_catalog WHERE source_id=$1 AND category_id IS NULL AND NOT reviewed FOR UPDATE")
        .bind(&id).fetch_optional(&mut *tx).await?.ok_or_else(Fail::missing)?;
    let name = text::clean(input.name.as_deref().unwrap_or(&name), "name", false)?;
    if name.is_empty() || name.chars().count() > 160 {
        return Err(Fail::field("name", "Use 1–160 characters."));
    }
    let mut category = None;
    if let Some(genre) = &input.genre {
        crate::factions::validate_genre(&mut tx, genre).await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM stream_categories WHERE lower(name)=lower($1) ORDER BY id LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(&mut *tx)
        .await?;
        let chosen = if let Some(existing) = existing {
            existing
        } else {
            let id = format!("wikidata-{}", id.to_lowercase());
            sqlx::query("INSERT INTO stream_categories(id,name,genre) VALUES($1,$2,$3)")
                .bind(&id)
                .bind(name)
                .bind(genre)
                .execute(&mut *tx)
                .await?;
            id
        };
        category = Some(chosen);
    }
    sqlx::query("UPDATE game_catalog SET reviewed=true,category_id=$2 WHERE source_id=$1")
        .bind(&id)
        .bind(category)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "review_game",
        "game",
        &id,
        &[],
        &note,
        json!({"genre":input.genre}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
