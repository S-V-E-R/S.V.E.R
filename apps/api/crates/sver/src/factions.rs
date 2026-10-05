//! Faction choice (docs/FACTIONS.md, "Membership"), brought forward from Module 4 so sign-up can
//! ask "Choose your side" and the site can wear the viewer's faction colors. The seasonal war,
//! influence and the map remain Module 4.

use crate::{
    App,
    profiles::{Fail, Res, signed_in},
};
use axum::{Json, extract::State};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

pub const FACTIONS: [&str; 3] = ["myria", "aetheron", "glint"];
/// One free switch during the first 7 days after choosing; after that, only between seasons.
const FREE_SWITCH_DAYS: i64 = 7;

/// The account's faction, if it has chosen one.
pub async fn of(db: &mut PgConnection, user_id: &str) -> Res<Option<String>> {
    Ok(sqlx::query_scalar("SELECT faction FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_optional(&mut *db)
        .await?
        .flatten())
}

struct Standing {
    faction: Option<String>,
    chosen_at: Option<DateTime<Utc>>,
    switches: i64,
}

async fn standing(db: &mut PgConnection, user_id: &str, lock: bool) -> Res<Standing> {
    let sql = if lock {
        "SELECT faction,faction_chosen_at FROM users WHERE id=$1 FOR UPDATE"
    } else {
        "SELECT faction,faction_chosen_at FROM users WHERE id=$1"
    };
    let (faction, chosen_at): (Option<String>, Option<DateTime<Utc>>) = sqlx::query_as(sql)
        .bind(user_id)
        .fetch_one(&mut *db)
        .await?;
    let switches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM faction_changes WHERE user_id=$1 AND from_faction IS NOT NULL",
    )
    .bind(user_id)
    .fetch_one(&mut *db)
    .await?;
    Ok(Standing {
        faction,
        chosen_at,
        switches,
    })
}

/// When the free switch ends, if it's still available.
fn switch_until(s: &Standing, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let until = s.chosen_at? + Duration::days(FREE_SWITCH_DAYS);
    (s.faction.is_some() && s.switches == 0 && now < until).then_some(until)
}

fn body(s: &Standing) -> Value {
    let until = switch_until(s, Utc::now());
    json!({
        "faction": s.faction,
        "chosen_at": s.chosen_at,
        "can_choose": s.faction.is_none() || until.is_some(),
        "free_switch_until": until,
    })
}

/// GET /api/me/faction
pub async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    Ok(Json(body(&standing(&mut db, &user.id, false).await?)))
}

#[derive(Deserialize)]
pub struct Choice {
    faction: String,
}

/// PUT /api/me/faction: the first choice, or the one free switch within 7 days of it.
pub async fn choose(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Choice>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let to = input.faction.trim().to_ascii_lowercase();
    if !FACTIONS.contains(&to.as_str()) {
        return Err(Fail::field("faction", "Pick Myria, Aetheron or Glint."));
    }
    let mut tx = app.db.begin().await?;
    let now = Utc::now();
    let current = standing(&mut tx, &user.id, true).await?;
    if current.faction.as_deref() == Some(to.as_str()) {
        tx.commit().await?;
        return Ok(Json(body(&current)));
    }
    if current.faction.is_some() && switch_until(&current, now).is_none() {
        return Err(Fail::denied(
            "Your free switch is used or has ended. You can switch between seasons.",
        ));
    }
    // The 7-day window counts from the first choice, so a switch keeps the original date.
    sqlx::query(
        "UPDATE users SET faction=$2,faction_chosen_at=COALESCE(faction_chosen_at,now()) WHERE id=$1",
    )
    .bind(&user.id)
    .bind(&to)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO faction_changes(user_id,from_faction,to_faction) VALUES($1,$2,$3)")
        .bind(&user.id)
        .bind(&current.faction)
        .bind(&to)
        .execute(&mut *tx)
        .await?;
    let saved = standing(&mut tx, &user.id, false).await?;
    tx.commit().await?;
    Ok(Json(body(&saved)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(faction: Option<&str>, days_ago: i64, switches: i64) -> Standing {
        Standing {
            faction: faction.map(str::to_string),
            chosen_at: faction.map(|_| Utc::now() - Duration::days(days_ago)),
            switches,
        }
    }

    #[test]
    fn free_switch_rules() {
        let now = Utc::now();
        assert!(switch_until(&s(None, 0, 0), now).is_none());
        assert!(switch_until(&s(Some("myria"), 2, 0), now).is_some());
        assert!(switch_until(&s(Some("myria"), 2, 1), now).is_none());
        assert!(switch_until(&s(Some("myria"), 8, 0), now).is_none());
        assert_eq!(body(&s(None, 0, 0))["can_choose"], true);
        assert_eq!(body(&s(Some("glint"), 9, 0))["can_choose"], false);
    }
}
