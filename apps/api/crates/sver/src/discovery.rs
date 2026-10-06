//! Module 5 discovery (docs/MAGNET.md "Discovery" and "Spotlights"): fair rotation for the home
//! page and browse, search, watch-page suggestions and spotlights. Never ordered by viewer count,
//! followers or money; viewer counts are labels only.
use crate::{
    App,
    profiles::{self, Fail, Res},
    safety,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::HashSet;

/// The rotation advances one place this often, so each stream spends time in the top row.
pub const ROTATE_SECONDS: i64 = 180;
/// A stream started this recently is "just went live".
const FRESH_MINUTES: i64 = 30;

/// One public live stream, as discovery sees it.
#[derive(sqlx::FromRow, Clone, Debug)]
pub struct Stream {
    pub broadcast_id: String,
    pub owner_id: String,
    pub username: String,
    pub display_name: String,
    pub avatar_key: Option<String>,
    pub faction: Option<String>,
    pub title: String,
    pub category_id: Option<String>,
    pub category: Option<String>,
    pub genre: Option<String>,
    pub started_at: DateTime<Utc>,
    pub viewers: i64,
    pub thumbnail_key: Option<String>,
    /// The owner's first broadcast ever.
    pub first_stream: bool,
    /// Back after 30 or more days away.
    pub returning: bool,
    /// A charity stream's charity (Shine).
    pub charity: Option<String>,
}

/// Every public live stream: LIVE, or RECONNECTING inside its grace window, on an eligible channel.
pub async fn live_streams(db: &mut PgConnection) -> Res<Vec<Stream>> {
    Ok(sqlx::query_as(
        "SELECT b.id AS broadcast_id,b.owner_id,cu.username,cu.display_name,cu.avatar_key,
          (SELECT fm.faction FROM faction_members fm WHERE fm.user_id=b.owner_id) AS faction,
          coalesce(s.title,cu.username||'''s stream') AS title,c.id AS category_id,c.name AS category,c.genre,b.started_at,
          (SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now() AND l.level IN ('counted','trusted')) AS viewers,
          b.thumbnail_key,
          NOT EXISTS(SELECT 1 FROM broadcasts p WHERE p.owner_id=b.owner_id AND p.id<>b.id AND p.started_at<b.started_at) AS first_stream,
          coalesce((SELECT max(p.ended_at) FROM broadcasts p WHERE p.owner_id=b.owner_id AND p.id<>b.id AND p.started_at<b.started_at)<b.started_at-interval '30 days',false) AS returning,
          (SELECT cs.charity_name FROM charity_streams cs WHERE cs.broadcast_id=b.id) AS charity
         FROM broadcasts b JOIN channel_users cu ON cu.id=b.owner_id AND cu.eligible
         LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id
         WHERE b.state='LIVE' OR (b.state='RECONNECTING' AND b.reconnect_deadline>now())",
    )
    .fetch_all(db)
    .await?)
}

/// The rotation slot for a moment in time.
pub fn slot(at: DateTime<Utc>) -> i64 {
    at.timestamp().div_euclid(ROTATE_SECONDS)
}

/// Fair rotation. Streams sit in a fixed cycle (by start time); each slot the cycle advances one
/// place, so every stream reaches the first position within one cycle. Streams in the viewer's
/// home genres take a second place in the cycle (the home-turf boost), so they come round twice
/// as often. Nothing about size, followers or money is an input.
pub fn rotate(mut streams: Vec<Stream>, home_genres: &[String], slot: i64) -> Vec<Stream> {
    streams.sort_by(|a, b| (a.started_at, &a.broadcast_id).cmp(&(b.started_at, &b.broadcast_id)));
    let mut cycle: Vec<usize> = (0..streams.len()).collect();
    cycle.extend((0..streams.len()).filter(|&i| {
        streams[i]
            .genre
            .as_ref()
            .is_some_and(|g| home_genres.contains(g))
    }));
    if cycle.is_empty() {
        return streams;
    }
    let start = slot.rem_euclid(cycle.len() as i64) as usize;
    let mut seen = HashSet::new();
    let order: Vec<usize> = cycle[start..]
        .iter()
        .chain(&cycle[..start])
        .copied()
        .filter(|i| seen.insert(*i))
        .collect();
    order.into_iter().map(|i| streams[i].clone()).collect()
}

/// The home genres of a faction (Module 4's territories at season start).
async fn home_genres(db: &mut PgConnection, faction: Option<&str>) -> Res<Vec<String>> {
    let Some(faction) = faction else {
        return Ok(Vec::new());
    };
    Ok(
        sqlx::query_scalar("SELECT id FROM faction_genres WHERE home=$1")
            .bind(faction)
            .fetch_all(db)
            .await?,
    )
}

/// The card shape used everywhere in discovery.
pub fn card(app: &App, s: &Stream, now: DateTime<Utc>) -> Value {
    let label = if s.charity.is_some() {
        Some("Charity stream")
    } else if s.first_stream {
        Some("New creator")
    } else if s.returning {
        Some("Returning creator")
    } else {
        None
    };
    json!({"broadcast_id":s.broadcast_id,"username":s.username,"display_name":s.display_name,
        "avatar":profiles::avatar_json(app,s.avatar_key.as_deref()),"faction":s.faction,"title":s.title,
        "category":s.category,"category_id":s.category_id,"genre":s.genre,"started_at":s.started_at,"viewers":s.viewers,
        "thumbnail":s.thumbnail_key.as_deref().map(|k|profiles::media_url(app,k)),"label":label,"charity":s.charity,
        "fresh":now-s.started_at<Duration::minutes(FRESH_MINUTES)})
}

/// The signed-in viewer's id and faction, if any.
async fn viewer(app: &App, jar: &CookieJar) -> Res<(Option<String>, Option<String>)> {
    let Some(user) = profiles::viewer(app, jar).await? else {
        return Ok((None, None));
    };
    let mut db = app.db.acquire().await?;
    let faction = crate::factions::membership(&mut db, &user.id).await?;
    Ok((Some(user.id), faction))
}

/// The viewer's rotation of every public live stream, excluding channels they've blocked or that
/// blocked them.
async fn rotation_for(
    app: &App,
    viewer: Option<&str>,
    faction: Option<&str>,
    at: DateTime<Utc>,
) -> Res<Vec<Stream>> {
    let mut db = app.db.acquire().await?;
    let mut streams = live_streams(&mut db).await?;
    if let Some(viewer) = viewer {
        let hidden: Vec<String> = sqlx::query_scalar("SELECT blocked_id FROM user_blocks WHERE blocker_id=$1 UNION SELECT blocker_id FROM user_blocks WHERE blocked_id=$1")
            .bind(viewer).fetch_all(&mut *db).await?;
        streams.retain(|s| !hidden.contains(&s.owner_id));
    }
    let home = home_genres(&mut db, faction).await?;
    Ok(rotate(streams, &home, slot(at)))
}

/// Channels that were live in the last 14 days (newest first), for empty states.
async fn recently_live(app: &App, db: &mut PgConnection) -> Res<Vec<Value>> {
    let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as("SELECT owner_id,max(ended_at) FROM broadcasts b WHERE state='ENDED' AND end_reason IS DISTINCT FROM 'revoked' AND ended_at>now()-interval '14 days' GROUP BY owner_id ORDER BY max(ended_at) DESC LIMIT 12")
        .fetch_all(&mut *db)
        .await?;
    let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let users = profiles::public_channels(db, &ids).await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, ended)| {
            let u = users.iter().find(|u| u.id == id)?;
            Some(json!({"user":profiles::chip(app,u),"ended_at":ended}))
        })
        .collect())
}

/// Staff spotlights in force, whether or not the channel is live.
async fn staff_spotlights(app: &App, db: &mut PgConnection) -> Res<Vec<(String, Value)>> {
    let rows: Vec<(String, String, DateTime<Utc>)> = sqlx::query_as("SELECT channel_id,reason,ends_at FROM spotlights WHERE ended_early_at IS NULL AND starts_at<=now() AND ends_at>now() ORDER BY starts_at")
        .fetch_all(&mut *db)
        .await?;
    let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let users = profiles::public_channels(db, &ids).await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, reason, ends)| {
            let u = users.iter().find(|u| u.id == id)?;
            Some((
                id,
                json!({"user":profiles::chip(app,u),"reason":reason,"ends_at":ends}),
            ))
        })
        .collect())
}

/// GET /api/discovery/home: the home shelves (docs/MAGNET.md "Homepage").
async fn home(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let now = Utc::now();
    let (viewer, faction) = viewer(&app, &jar).await?;
    let rotation = rotation_for(&app, viewer.as_deref(), faction.as_deref(), now).await?;
    let mut db = app.db.acquire().await?;
    let followed: HashSet<String> = match &viewer {
        Some(v) => sqlx::query_scalar("SELECT following_id FROM follows WHERE follower_id=$1")
            .bind(v)
            .fetch_all(&mut *db)
            .await?
            .into_iter()
            .collect(),
        None => HashSet::new(),
    };
    let cards = |keep: &dyn Fn(&Stream) -> bool| -> Vec<Value> {
        rotation
            .iter()
            .filter(|s| keep(s))
            .map(|s| card(&app, s, now))
            .collect()
    };
    let mut fresh: Vec<&Stream> = rotation
        .iter()
        .filter(|s| now - s.started_at < Duration::minutes(FRESH_MINUTES))
        .collect();
    fresh.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    // Spotlights: staff picks, then automatic first streams and returns (live ones only).
    let staff = staff_spotlights(&app, &mut db).await?;
    let mut spotlights: Vec<Value> = staff
        .iter()
        .map(|(id, v)| {
            let mut v = v.clone();
            v["kind"] = json!("staff");
            v["stream"] = rotation
                .iter()
                .find(|s| &s.owner_id == id)
                .map_or(Value::Null, |s| card(&app, s, now));
            v
        })
        .collect();
    for s in rotation.iter().filter(|s| s.first_stream || s.returning) {
        if !staff.iter().any(|(id, _)| id == &s.owner_id) {
            spotlights.push(json!({"kind":if s.first_stream {"first_stream"} else {"returning"},"reason":if s.first_stream {"First stream on S.V.E.R"} else {"Back after a while"},"stream":card(&app, s, now)}));
        }
    }
    spotlights.truncate(6);
    let recent = if rotation.is_empty() {
        recently_live(&app, &mut db).await?
    } else {
        Vec::new()
    };
    Ok(Json(json!({
        "as_of": now,
        "rotates_at": DateTime::<Utc>::from_timestamp((slot(now) + 1) * ROTATE_SECONDS, 0),
        "following": cards(&|s| followed.contains(&s.owner_id)),
        "live": cards(&|_| true),
        "faction": if faction.is_some() { json!(cards(&|s| s.faction == faction)) } else { Value::Null },
        "fresh": fresh.into_iter().take(10).map(|s| card(&app, s, now)).collect::<Vec<_>>(),
        "spotlights": spotlights,
        "recent": recent,
    })))
}

#[derive(Deserialize, Default)]
struct LiveFilter {
    genre: Option<String>,
    category: Option<String>,
    faction: Option<String>,
}
/// GET /api/discovery/live?genre=&category=&faction=: one browse list, in fair rotation.
async fn live(
    State(app): State<App>,
    jar: CookieJar,
    Query(filter): Query<LiveFilter>,
) -> Res<Json<Value>> {
    let now = Utc::now();
    let (viewer, faction) = viewer(&app, &jar).await?;
    if filter
        .faction
        .as_deref()
        .is_some_and(|f| !crate::factions::valid(f))
    {
        return Err(Fail::bad("Unknown faction."));
    }
    let rotation = rotation_for(&app, viewer.as_deref(), faction.as_deref(), now).await?;
    let items: Vec<Value> = rotation
        .iter()
        .filter(|s| filter.genre.is_none() || s.genre == filter.genre)
        .filter(|s| filter.category.is_none() || s.category_id == filter.category)
        .filter(|s| filter.faction.is_none() || s.faction == filter.faction)
        .map(|s| card(&app, s, now))
        .collect();
    let mut db = app.db.acquire().await?;
    let recent = if items.is_empty() {
        recently_live(&app, &mut db).await?
    } else {
        Vec::new()
    };
    Ok(Json(json!({"as_of":now,"items":items,"recent":recent})))
}

/// GET /api/discovery/browse: genres with their categories and how many are live in each.
async fn browse(State(app): State<App>) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    let live = live_streams(&mut db).await?;
    let genres: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT id,name,home FROM faction_genres ORDER BY position,name")
            .fetch_all(&mut *db)
            .await?;
    let categories: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id,name,genre FROM stream_categories WHERE active ORDER BY lower(name)",
    )
    .fetch_all(&mut *db)
    .await?;
    let count = |f: &dyn Fn(&Stream) -> bool| live.iter().filter(|s| f(s)).count();
    let items: Vec<Value> = genres.into_iter().filter_map(|(id, name, home)| {
        let cats: Vec<Value> = categories.iter().filter(|c| c.2 == id)
            .map(|c| json!({"id":c.0,"name":c.1,"live":count(&|s| s.category_id.as_deref() == Some(c.0.as_str()))})).collect();
        (!cats.is_empty()).then(|| json!({"id":id,"name":name,"home":home,"live":count(&|s| s.genre.as_deref() == Some(id.as_str())),"categories":cats}))
    }).collect();
    Ok(Json(json!({"genres":items,"live":live.len()})))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}
/// Escapes LIKE wildcards so a search is always a plain substring match.
fn like(text: &str) -> String {
    let mut out = String::from("%");
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}
/// GET /api/search?q=: channels (live first) and categories by name.
async fn search(
    State(app): State<App>,
    jar: CookieJar,
    Query(input): Query<SearchQuery>,
) -> Res<Json<Value>> {
    let q = input.q.trim();
    let len = q.chars().count();
    if !(2..=50).contains(&len) || q.chars().any(char::is_control) {
        return Err(Fail::bad("Search for 2–50 characters."));
    }
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let pattern = like(q);
    // ponytail: substring scan over channels; add a trigram index once the user table is large.
    let ids: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT c.id FROM channel_users c WHERE c.eligible AND (c.username ILIKE $1 OR c.display_name ILIKE $1)
         AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE (k.blocker_id=$2 AND k.blocked_id=c.id) OR (k.blocker_id=c.id AND k.blocked_id=$2))
         ORDER BY {} DESC, lower(c.username)=lower($3) DESC, length(c.username), lower(c.username) LIMIT 20",
        crate::playback::live_sql("c.id")
    )))
    .bind(&pattern)
    .bind(viewer.as_ref().map(|v| v.id.as_str()).unwrap_or(""))
    .bind(q)
    .fetch_all(&mut *db)
    .await?;
    let users = profiles::public_channels(&mut db, &ids).await?;
    let mut channels = Vec::new();
    for id in &ids {
        if let Some(u) = users.iter().find(|u| &u.id == id) {
            let mut chip = profiles::chip(&app, u);
            chip["faction"] = json!(u.faction);
            channels.push(chip);
        }
    }
    let categories: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'genre',genre) FROM stream_categories WHERE active AND name ILIKE $1 ORDER BY lower(name)=lower($2) DESC, lower(name) LIMIT 10")
        .bind(&pattern)
        .bind(q)
        .fetch_all(&mut *db)
        .await?;
    Ok(Json(
        json!({"query":q,"channels":channels,"categories":categories}),
    ))
}

/// Ordered suggestions while watching `current`: same genre, then same faction, then anything
/// live; each group keeps the viewer's rotation order.
pub fn suggest(rotation: &[Stream], current: &Stream, limit: usize) -> Vec<Stream> {
    let others: Vec<&Stream> = rotation
        .iter()
        .filter(|s| s.owner_id != current.owner_id)
        .collect();
    let mut out: Vec<Stream> = Vec::new();
    let mut take = |keep: &dyn Fn(&Stream) -> bool| {
        for s in others.iter().filter(|s| keep(s)) {
            if !out.iter().any(|o| o.owner_id == s.owner_id) {
                out.push((*s).clone());
            }
        }
    };
    take(&|s| current.genre.is_some() && s.genre == current.genre);
    take(&|s| current.faction.is_some() && s.faction == current.faction);
    take(&|_| true);
    out.truncate(limit);
    out
}
/// GET /api/channels/{username}/suggestions: other live streams for the watch page and stream end.
async fn suggestions(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let now = Utc::now();
    let mut db = app.db.acquire().await?;
    let owner = profiles::eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    // The channel's own context, even when it has just gone offline (the stream-end case).
    let (genre, owner_faction): (Option<String>, Option<String>) = sqlx::query_as("SELECT (SELECT c.genre FROM stream_settings s JOIN stream_categories c ON c.id=s.category_id WHERE s.owner_id=$1),(SELECT faction FROM faction_members WHERE user_id=$1)")
        .bind(&owner.id).fetch_one(&mut *db).await?;
    drop(db);
    let (viewer, faction) = viewer(&app, &jar).await?;
    let rotation = rotation_for(&app, viewer.as_deref(), faction.as_deref(), now).await?;
    let current = Stream {
        broadcast_id: String::new(),
        owner_id: owner.id.clone(),
        username: owner.username.clone(),
        display_name: owner.display_name.clone(),
        avatar_key: None,
        faction: owner_faction,
        title: String::new(),
        category_id: None,
        category: None,
        genre,
        started_at: now,
        viewers: 0,
        thumbnail_key: None,
        first_stream: false,
        returning: false,
        charity: None,
    };
    let items: Vec<Value> = suggest(&rotation, &current, 8)
        .iter()
        .map(|s| card(&app, s, now))
        .collect();
    Ok(Json(json!({"items":items})))
}

/// GET /api/admin/spotlights: staff spotlights, newest first.
async fn admin_list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'username',c.username,'reason',s.reason,'starts_at',s.starts_at,'ends_at',s.ends_at,'ended_early_at',s.ended_early_at,'active',s.ended_early_at IS NULL AND s.ends_at>now()) FROM spotlights s JOIN channel_users c ON c.id=s.channel_id ORDER BY s.starts_at DESC LIMIT 100")
        .fetch_all(&app.db)
        .await?;
    Ok(Json(json!({"items":items})))
}
#[derive(Deserialize)]
struct NewSpotlight {
    username: String,
    reason: String,
    days: i64,
}
/// POST /api/admin/spotlights: at most 14 days, one active per channel, then a 7-day cooldown.
async fn admin_create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewSpotlight>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let reason = input.reason.trim();
    if reason.is_empty() || reason.chars().count() > 120 || reason.chars().any(char::is_control) {
        return Err(Fail::field(
            "reason",
            "Give a public reason of 1–120 characters.",
        ));
    }
    if !(1..=14).contains(&input.days) {
        return Err(Fail::field("days", "Spotlights last 1–14 days."));
    }
    let mut tx = app.db.begin().await?;
    let channel =
        profiles::eligible_by_name(&mut tx, input.username.trim().trim_start_matches('@'))
            .await?
            .ok_or_else(Fail::channel_missing)?;
    // Serialize spotlights for one channel so the one-active and cooldown rules hold under races.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('spotlight:'||$1))")
        .bind(&channel.id)
        .execute(&mut *tx)
        .await?;
    let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM spotlights WHERE channel_id=$1 AND coalesce(ended_early_at,ends_at)>now()-interval '7 days')")
        .bind(&channel.id)
        .fetch_one(&mut *tx)
        .await?;
    if blocked {
        return Err(Fail::conflict(
            "This channel has an active spotlight or one that ended in the last 7 days.",
        ));
    }
    let id = profiles::new_id();
    sqlx::query("INSERT INTO spotlights(id,channel_id,reason,ends_at,created_by) VALUES($1,$2,$3,now()+make_interval(days=>$4),$5)")
        .bind(&id).bind(&channel.id).bind(reason).bind(input.days as i32).bind(&staff.id).execute(&mut *tx).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "create_spotlight",
        "profile",
        &channel.id,
        &[],
        reason,
        json!({"spotlight": id, "days": input.days}),
        false,
    )
    .await?;
    tx.commit().await?;
    admin_list(State(app), jar).await
}
/// POST /api/admin/spotlights/{id}/end
async fn admin_end(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let channel: Option<String> = sqlx::query_scalar("UPDATE spotlights SET ended_early_at=now() WHERE id=$1 AND ended_early_at IS NULL AND ends_at>now() RETURNING channel_id")
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await?;
    let channel = channel.ok_or_else(|| Fail::conflict("That spotlight isn't active."))?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "end_spotlight",
        "profile",
        &channel,
        &[],
        "Ended early",
        json!({"spotlight": id}),
        false,
    )
    .await?;
    tx.commit().await?;
    admin_list(State(app), jar).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/discovery/home", get(home))
        .route("/api/discovery/live", get(live))
        .route("/api/discovery/browse", get(browse))
        .route("/api/search", get(search))
        .route("/api/channels/{username}/suggestions", get(suggestions))
        .route("/api/admin/spotlights", get(admin_list).post(admin_create))
        .route("/api/admin/spotlights/{id}/end", post(admin_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, minutes_ago: i64, genre: &str, faction: &str) -> Stream {
        Stream {
            broadcast_id: id.into(),
            owner_id: format!("o-{id}"),
            username: id.into(),
            display_name: id.into(),
            avatar_key: None,
            faction: Some(faction.into()),
            title: id.into(),
            category_id: None,
            category: None,
            genre: Some(genre.into()),
            started_at: Utc::now() - Duration::minutes(minutes_ago),
            viewers: 0,
            thumbnail_key: None,
            first_stream: false,
            returning: false,
            charity: None,
        }
    }
    fn ids(v: &[Stream]) -> Vec<String> {
        v.iter().map(|s| s.broadcast_id.clone()).collect()
    }

    #[test]
    fn every_stream_reaches_the_top_within_a_cycle() {
        let streams: Vec<Stream> = (0..7)
            .map(|i| s(&format!("s{i}"), 100 - i, "art", "glint"))
            .collect();
        for (genres, cycle) in [(Vec::new(), 7), (vec!["art".to_string()], 14)] {
            let firsts: HashSet<String> = (0..cycle)
                .map(|slot| {
                    rotate(streams.clone(), &genres, slot)[0]
                        .broadcast_id
                        .clone()
                })
                .collect();
            assert_eq!(firsts.len(), 7, "every stream is first within one cycle");
        }
        let order = rotate(streams, &[], 3);
        assert_eq!(order.len(), 7);
        assert_eq!(ids(&order).into_iter().collect::<HashSet<_>>().len(), 7);
    }

    #[test]
    fn home_turf_comes_round_more_often_and_size_never_matters() {
        let mut streams = vec![
            s("art", 50, "art", "aetheron"),
            s("fps1", 40, "fps_battle_royale", "myria"),
            s("fps2", 30, "fps_battle_royale", "myria"),
        ];
        let home = vec!["art".to_string()];
        let leads = (0..4)
            .filter(|&slot| rotate(streams.clone(), &home, slot)[0].broadcast_id == "art")
            .count();
        assert_eq!(leads, 2, "the boosted stream leads 2 of the 4 slots");
        let orders = |streams: &[Stream]| -> Vec<Vec<String>> {
            (0..4)
                .map(|slot| ids(&rotate(streams.to_vec(), &home, slot)))
                .collect()
        };
        let before = orders(&streams);
        streams[1].viewers = 100_000;
        assert_eq!(before, orders(&streams), "viewer counts change nothing");
    }

    #[test]
    fn suggestions_prefer_genre_then_faction() {
        let current = s("me", 10, "art", "glint");
        let rotation = vec![
            s("other", 1, "music", "myria"),
            s("ally", 2, "music", "glint"),
            s("same", 3, "art", "myria"),
            current.clone(),
        ];
        assert_eq!(
            ids(&suggest(&rotation, &current, 8)),
            ["same", "ally", "other"]
        );
        assert_eq!(suggest(&rotation, &current, 1).len(), 1);
    }

    #[test]
    fn search_patterns_are_literal() {
        assert_eq!(like("a_b%c"), "%a\\_b\\%c%");
        assert_eq!(like("x\\y"), "%x\\\\y%");
    }
}
