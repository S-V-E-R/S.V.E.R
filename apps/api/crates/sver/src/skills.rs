//! Module 7 CrowdSync, phase 3 (docs/CROWDSYNC.md "Skills"): premium effects a viewer buys with
//! Purchased Valor at fixed S.V.E.R prices. A Skill is a chat message paid like a tribute (the
//! same Valor spend, 0.8¢ per Valor to the streamer, ledger entry and refund rules; under-18 limits
//! apply when the Valor is bought), and its effect plays on stream. Streamers can switch
//! categories off. Skills never affect MAGNet, influence or tiers beyond the earnings they bring.
use crate::{
    App, auth, boards,
    profiles::{self, Fail, Res},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};

pub struct Skill {
    pub id: &'static str,
    pub name: &'static str,
    /// "sticker", "fullscreen" or "sound": the categories a streamer can switch off.
    pub category: &'static str,
    /// Price in Purchased Valor (**Proposed** defaults set by S.V.E.R).
    pub valor: i64,
    pub effect: &'static str,
}
pub const CATEGORIES: &[&str] = &["sticker", "fullscreen", "sound"];
pub const SKILLS: &[Skill] = &[
    Skill {
        id: "crown",
        name: "Crown",
        category: "sticker",
        valor: 50,
        effect: "sticker-crown",
    },
    Skill {
        id: "heart",
        name: "Big heart",
        category: "sticker",
        valor: 50,
        effect: "sticker-heart",
    },
    Skill {
        id: "trophy",
        name: "Trophy",
        category: "sticker",
        valor: 75,
        effect: "sticker-trophy",
    },
    Skill {
        id: "starfall",
        name: "Starfall",
        category: "fullscreen",
        valor: 200,
        effect: "stars",
    },
    Skill {
        id: "quake",
        name: "Quake",
        category: "fullscreen",
        valor: 150,
        effect: "shake",
    },
    Skill {
        id: "fireworks",
        name: "Fireworks show",
        category: "fullscreen",
        valor: 300,
        effect: "fireworks",
    },
    Skill {
        id: "chime",
        name: "Chime",
        category: "sound",
        valor: 30,
        effect: "sound-chime",
    },
    Skill {
        id: "fanfare",
        name: "Fanfare",
        category: "sound",
        valor: 100,
        effect: "sound-fanfare",
    },
];

fn skill_json(s: &Skill, enabled: bool) -> Value {
    json!({"id": s.id, "name": s.name, "category": s.category, "valor": s.valor, "effect": s.effect, "enabled": enabled})
}
async fn disabled(db: &mut sqlx::PgConnection, channel: &str) -> Res<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT disabled FROM skill_settings WHERE channel_id=$1),'{}')",
    )
    .bind(channel)
    .fetch_one(db)
    .await?)
}

/// The Skill a chat message plays, if its category is on and the channel's effects aren't paused
/// (a paused board means no effect would play, so nothing is sold).
pub(crate) async fn check(app: &App, channel: &str, id: &str) -> Res<&'static Skill> {
    let skill = SKILLS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| Fail::field("skill", "Choose a Skill from the list."))?;
    let mut db = app.db.acquire().await?;
    if disabled(&mut db, channel)
        .await?
        .iter()
        .any(|c| c == skill.category)
    {
        return Err(Fail::field(
            "skill",
            "This channel has switched that kind of Skill off.",
        ));
    }
    if boards::effects_state(&mut db, channel).await?.0 {
        return Err(Fail::field(
            "skill",
            "Effects are paused on this channel right now.",
        ));
    }
    Ok(skill)
}
/// After the paid message commits, the Skill plays on stream with the sender's name.
pub(crate) async fn played(app: &App, channel: &str, user: &auth::User, skill: &Skill) -> Res<()> {
    boards::play(
        app,
        channel,
        Some(&user.id),
        json!({"skill": skill.id, "label": skill.name, "effect": skill.effect,
            "user": {"username": user.username}, "caption": format!("{} played {}", user.username, skill.name)}),
    )
    .await
}

/// GET /api/channels/{username}/skills: the catalog with what this channel allows, and the
/// viewer's Purchased Valor.
async fn list(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    let channel = profiles::eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id;
    let off = disabled(&mut db, &channel).await?;
    let paused = boards::effects_state(&mut db, &channel).await?.0;
    let skills: Vec<Value> = SKILLS
        .iter()
        .map(|s| skill_json(s, !off.iter().any(|c| c == s.category)))
        .collect();
    let valor = match profiles::viewer(&app, &jar).await? {
        Some(v) if v.id != channel => {
            Some(crate::ledger::balance(&mut db, &format!("valor:{}", v.id), "valor").await?)
        }
        _ => None,
    };
    Ok(Json(
        json!({"skills": skills, "valor": valor, "paused": paused}),
    ))
}

/// GET /api/me/skills
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let off = disabled(&mut *app.db.acquire().await?, &user.id).await?;
    let skills: Vec<Value> = SKILLS.iter().map(|s| skill_json(s, true)).collect();
    Ok(Json(
        json!({"skills": skills, "categories": CATEGORIES, "disabled": off}),
    ))
}
#[derive(Deserialize)]
pub struct Settings {
    disabled: Vec<String>,
}
/// PUT /api/me/skills: switch Skill categories off (or back on) for the channel.
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut off: Vec<String> = input.disabled;
    off.sort();
    off.dedup();
    if off.iter().any(|c| !CATEGORIES.contains(&c.as_str())) {
        return Err(Fail::field("disabled", "Unknown Skill category."));
    }
    sqlx::query("INSERT INTO skill_settings(channel_id,disabled) VALUES($1,$2) ON CONFLICT(channel_id) DO UPDATE SET disabled=EXCLUDED.disabled")
        .bind(&user.id)
        .bind(&off)
        .execute(&app.db)
        .await?;
    mine(State(app), jar).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/skills", get(list))
        .route("/api/me/skills", get(mine).put(save))
}
