//! Achievements and cosmetics (docs/ACHIEVEMENTS.md): milestones earned from records S.V.E.R
//! already keeps (checked every 10 minutes, never from a client report), and the cosmetics levels
//! unlock: a chat name style, a chosen badge frame and a showcase of up to 3 achievements.
use crate::{
    App,
    profiles::{self, Fail, Res},
    progression,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// Key, name, what earns it.
pub const CATALOG: [(&str, &str, &str); 21] = [
    ("first_follow", "First Follow", "Followed a channel"),
    (
        "voice",
        "Voice of the Crowd",
        "Chatted in 10 different channels",
    ),
    ("scout", "Scout", "Scouted a new stream"),
    ("pathfinder", "Pathfinder", "Scouted 10 new streams"),
    (
        "order_keeper",
        "Order Keeper",
        "21 daily orders done in one week",
    ),
    ("legend", "Legend", "Legend loyalty rank in a channel"),
    ("level_10", "Level 10", "Reached level 10"),
    ("level_25", "Level 25", "Reached level 25"),
    ("level_50", "Level 50", "Reached level 50"),
    ("level_100", "Level 100", "Reached level 100"),
    ("first_light", "First Light", "Went live for the first time"),
    ("hours_10", "Ten Hours Lit", "Streamed 10 hours"),
    ("hours_100", "Hundred Hours Lit", "Streamed 100 hours"),
    ("followers_100", "Gathering", "Reached 100 followers"),
    (
        "followers_1000",
        "Host of a Thousand",
        "Reached 1,000 followers",
    ),
    ("raider", "Raider", "Led a raid"),
    ("supported", "Supported", "Got a first subscriber"),
    ("first_tribute", "Patron", "Paid a first tribute"),
    (
        "faithful",
        "Faithful",
        "Subscribed to a channel for 6 months",
    ),
    ("signatory", "Signatory", "Signed someone's Wall"),
    (
        "council_of_war",
        "Council of War",
        "Filled all 8 War Council seats",
    ),
];
/// Chat name styles and the level each unlocks at (always in the faction's colors).
pub const STYLES: [(&str, &str, i64); 5] = [
    ("plain", "Plain", 1),
    ("bold", "Bold", 10),
    ("smallcaps", "Small caps", 20),
    ("accent", "Accent underline", 30),
    ("outlined", "Outlined", 50),
];

/// The level achievements, granted together from one pass over `xp_days`.
const LEVELS: [(&str, i64); 4] = [
    ("level_10", 10),
    ("level_25", 25),
    ("level_50", 50),
    ("level_100", 100),
];
/// Each other achievement's rule: a query of user IDs that have earned it, with one bound number.
fn rules() -> Vec<(&'static str, &'static str, i64)> {
    vec![
        (
            "first_follow",
            "SELECT DISTINCT follower_id FROM follows WHERE $1=$1",
            0,
        ),
        (
            "voice",
            "SELECT user_id FROM engagement WHERE last_chat_at IS NOT NULL GROUP BY user_id HAVING count(*)>=$1",
            10,
        ),
        (
            "scout",
            "SELECT user_id FROM scout_awards GROUP BY user_id HAVING count(*)>=$1",
            1,
        ),
        (
            "pathfinder",
            "SELECT user_id FROM scout_awards GROUP BY user_id HAVING count(*)>=$1",
            10,
        ),
        (
            "order_keeper",
            "SELECT DISTINCT user_id FROM order_milestones WHERE orders>=$1",
            21,
        ),
        (
            "legend",
            "SELECT DISTINCT user_id FROM engagement WHERE earned>=$1",
            progression::LOYALTY[4].1,
        ),
        (
            "first_light",
            "SELECT DISTINCT owner_id FROM broadcasts WHERE confirmed_live_at IS NOT NULL AND $1=$1",
            0,
        ),
        (
            "hours_10",
            "SELECT owner_id FROM broadcasts WHERE confirmed_live_at IS NOT NULL GROUP BY owner_id HAVING sum(extract(epoch FROM coalesce(ended_at,now())-started_at))>=$1*3600",
            10,
        ),
        (
            "hours_100",
            "SELECT owner_id FROM broadcasts WHERE confirmed_live_at IS NOT NULL GROUP BY owner_id HAVING sum(extract(epoch FROM coalesce(ended_at,now())-started_at))>=$1*3600",
            100,
        ),
        (
            "followers_100",
            "SELECT following_id FROM follows GROUP BY following_id HAVING count(*)>=$1",
            100,
        ),
        (
            "followers_1000",
            "SELECT following_id FROM follows GROUP BY following_id HAVING count(*)>=$1",
            1000,
        ),
        (
            "raider",
            "SELECT DISTINCT raider_id FROM raids WHERE status='moved' AND $1=$1",
            0,
        ),
        (
            "supported",
            "SELECT DISTINCT channel_id FROM channel_subs WHERE $1=$1",
            0,
        ),
        (
            "first_tribute",
            "SELECT DISTINCT (detail->>'from') FROM ledger_transactions WHERE kind='tribute' AND detail->>'from' IN (SELECT id FROM users) AND $1=$1",
            0,
        ),
        (
            "faithful",
            "SELECT DISTINCT user_id FROM channel_subs WHERE months>=$1",
            6,
        ),
        (
            "signatory",
            "SELECT DISTINCT author_id FROM wall_posts WHERE status='APPROVED' AND deleted_at IS NULL AND author_id<>wall_owner_id AND $1=$1",
            0,
        ),
        (
            "council_of_war",
            "SELECT user_id FROM war_council GROUP BY user_id HAVING count(*)>=$1",
            8,
        ),
    ]
}

/// Every 10 minutes: grants every achievement whose rule now holds, skipping people who already
/// have it. Earned ones are kept.
pub async fn award(app: &App) -> Res<()> {
    for (key, sql, n) in rules() {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO user_achievements(user_id,key) SELECT s.u,$2 FROM ({sql}) s(u)
            WHERE s.u IS NOT NULL AND NOT EXISTS(SELECT 1 FROM user_achievements a WHERE a.user_id=s.u AND a.key=$2) ON CONFLICT DO NOTHING"
        )))
        .bind(n)
        .bind(key)
        .execute(&app.db)
        .await?;
    }
    let (keys, needs): (Vec<&str>, Vec<i64>) = LEVELS
        .iter()
        .map(|(key, level)| (*key, progression::xp_for(*level)))
        .unzip();
    sqlx::query("INSERT INTO user_achievements(user_id,key)
        SELECT t.user_id,k.key FROM (SELECT user_id,sum(xp) AS xp FROM xp_days GROUP BY user_id HAVING sum(xp)>=$3) t
        JOIN unnest($1::text[],$2::bigint[]) k(key,need) ON t.xp>=k.need
        WHERE NOT EXISTS(SELECT 1 FROM user_achievements a WHERE a.user_id=t.user_id AND a.key=k.key) ON CONFLICT DO NOTHING")
        .bind(&keys)
        .bind(&needs)
        .bind(needs[0])
        .execute(&app.db)
        .await?;
    Ok(())
}

fn name(key: &str) -> Option<(&'static str, &'static str)> {
    CATALOG
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, n, d)| (*n, *d))
}
async fn earned(db: &mut PgConnection, user: &str) -> Res<Vec<Value>> {
    let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT key,earned_at FROM user_achievements WHERE user_id=$1 ORDER BY earned_at",
    )
    .bind(user)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(key, at)| {
            name(&key)
                .map(|(n, d)| json!({"key": key, "name": n, "description": d, "earned_at": at}))
        })
        .collect())
}
/// The badge frame to draw: the chosen one if still unlocked, else the newest unlocked.
pub async fn frame(db: &mut PgConnection, user: &str, level: i64) -> Res<i64> {
    let chosen: Option<i16> =
        sqlx::query_scalar("SELECT frame FROM user_cosmetics WHERE user_id=$1")
            .bind(user)
            .fetch_optional(db)
            .await?
            .flatten();
    let newest = progression::frame(level);
    Ok(chosen.map_or(newest, |f| i64::from(f).min(newest)))
}
/// Up to 3 showcased achievements, for the profile and user card, with when each was earned.
pub async fn showcase(db: &mut PgConnection, user: &str) -> Res<Vec<Value>> {
    let rows: Vec<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT s.key,a.earned_at FROM user_cosmetics c CROSS JOIN LATERAL unnest(c.showcase) WITH ORDINALITY s(key,n) LEFT JOIN user_achievements a ON a.user_id=c.user_id AND a.key=s.key WHERE c.user_id=$1 ORDER BY s.n",
    )
    .bind(user)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(k, at)| {
            name(&k).map(|(n, d)| json!({"key": k, "name": n, "description": d, "earned_at": at}))
        })
        .collect())
}

/// GET /api/users/{username}/achievements: earned achievements and the showcase.
async fn public(State(app): State<App>, Path(username): Path<String>) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    let id: String = sqlx::query_scalar("SELECT id FROM channel_users WHERE lower(username)=lower($1) AND eligible AND deleted_at IS NULL")
        .bind(&username).fetch_optional(&mut *db).await?.ok_or_else(Fail::missing)?;
    Ok(Json(
        json!({"achievements": earned(&mut db, &id).await?, "showcase": showcase(&mut db, &id).await?, "total": CATALOG.len()}),
    ))
}
/// GET /api/me/cosmetics: what's unlocked and chosen.
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let level = progression::level(progression::total(&mut db, &me.id).await?);
    let (style, chosen_frame, show): (Option<String>, Option<i16>, Option<Vec<String>>) =
        sqlx::query_as("SELECT name_style,frame,showcase FROM user_cosmetics WHERE user_id=$1")
            .bind(&me.id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or((None, None, None));
    let styles: Vec<Value> = STYLES
        .iter()
        .map(|(k, n, l)| json!({"key": k, "name": n, "level": l, "unlocked": level >= *l}))
        .collect();
    Ok(Json(json!({
        "level": level, "name_style": style.unwrap_or_else(|| "plain".into()), "frame": chosen_frame,
        "max_frame": progression::frame(level), "showcase": show.unwrap_or_default(),
        "styles": styles, "earned": earned(&mut db, &me.id).await?,
    })))
}
#[derive(Deserialize)]
pub struct Choice {
    name_style: String,
    frame: Option<i64>,
    #[serde(default)]
    showcase: Vec<String>,
}
/// PUT /api/me/cosmetics: only what the account has unlocked or earned.
async fn choose(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Choice>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    {
        let mut db = app.db.acquire().await?;
        let level = progression::level(progression::total(&mut db, &me.id).await?);
        let style = STYLES
            .iter()
            .find(|(k, _, _)| *k == input.name_style)
            .ok_or_else(|| Fail::field("name_style", "Choose a name style."))?;
        if level < style.2 {
            return Err(Fail::field(
                "name_style",
                "That style unlocks at a higher level.",
            ));
        }
        if input
            .frame
            .is_some_and(|f| !(0..=progression::frame(level)).contains(&f))
        {
            return Err(Fail::field(
                "frame",
                "That frame unlocks at a higher level.",
            ));
        }
        let show = input.showcase.clone();
        let unique: std::collections::HashSet<&String> = show.iter().collect();
        if show.len() > 3 || unique.len() != show.len() {
            return Err(Fail::field(
                "showcase",
                "Showcase up to 3 different achievements.",
            ));
        }
        let have: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM user_achievements WHERE user_id=$1 AND key=ANY($2)",
        )
        .bind(&me.id)
        .bind(&show)
        .fetch_one(&mut *db)
        .await?;
        if have != show.len() as i64 {
            return Err(Fail::field(
                "showcase",
                "Showcase only achievements you've earned.",
            ));
        }
        sqlx::query("INSERT INTO user_cosmetics(user_id,name_style,frame,showcase) VALUES($1,$2,$3,$4) ON CONFLICT(user_id) DO UPDATE SET name_style=EXCLUDED.name_style,frame=EXCLUDED.frame,showcase=EXCLUDED.showcase")
            .bind(&me.id).bind(&input.name_style).bind(input.frame.map(|f| f as i16)).bind(&show).execute(&mut *db).await?;
    }
    mine(State(app), jar).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/users/{username}/achievements", get(public))
        .route("/api/me/cosmetics", get(mine).put(choose))
}
