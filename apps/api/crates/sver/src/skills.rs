//! Module 7 CrowdSync, phase 3 (docs/CROWDSYNC.md "Skills"): premium effects a viewer buys with
//! Purchased Valor at fixed S.V.E.R prices. A Skill is a chat message paid like a tribute (the
//! same Valor spend, 0.8¢ per Valor to the streamer, ledger entry and refund rules; under-18 limits
//! apply when the Valor is bought), and its effect plays on stream. Streamers can switch
//! categories off. Skills never affect MAGNet, influence or tiers beyond the earnings they bring.
//! Streamers add up to 10 custom Skills with their own image and/or sound, priced only at the
//! built-in price points (Joe's decision, October 10, 2026).
use crate::{
    App, auth, boards,
    media::{self, Kind},
    profiles::{self, Fail, Res},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    routing::{delete, get, post},
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

/// The only prices a custom Skill can have: the built-in Skills' price points.
pub const PRICES: [i64; 7] = [30, 50, 75, 100, 150, 200, 300];
const MAX_CUSTOM: i64 = 10;

/// A Skill being bought: a built-in one or the channel's own.
#[derive(Clone)]
pub struct Chosen {
    /// The catalog key the viewer sent (built-in id or custom Skill id).
    pub id: String,
    /// What chat and events show: the built-in id (as before) or the custom Skill's name.
    pub shown: String,
    pub name: String,
    pub category: &'static str,
    pub valor: i64,
    pub effect: &'static str,
    pub image: Option<String>,
    pub audio: Option<String>,
}
impl From<&Skill> for Chosen {
    fn from(s: &Skill) -> Self {
        Chosen {
            id: s.id.into(),
            shown: s.id.into(),
            name: s.name.into(),
            category: s.category,
            valor: s.valor,
            effect: s.effect,
            image: None,
            audio: None,
        }
    }
}

fn skill_json(s: &Skill, enabled: bool) -> Value {
    json!({"id": s.id, "name": s.name, "category": s.category, "valor": s.valor, "effect": s.effect, "enabled": enabled})
}
/// A custom Skill shows as a sticker when it has an image, otherwise as a sound.
fn custom_category(image: &Option<String>) -> &'static str {
    if image.is_some() { "sticker" } else { "sound" }
}
/// id, name, price, image key, sound key.
type CustomRow = (String, String, i32, Option<String>, Option<String>);
/// The channel's custom Skills; `ready` leaves out any without an image or sound yet.
async fn customs(app: &App, channel: &str, ready: bool) -> Res<Vec<Chosen>> {
    let rows: Vec<CustomRow> = sqlx::query_as("SELECT id,name,valor,image_key,audio_key FROM channel_skills WHERE channel_id=$1 AND (NOT $2 OR image_key IS NOT NULL OR audio_key IS NOT NULL) ORDER BY created_at")
        .bind(channel).bind(ready).fetch_all(&app.db).await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, valor, image, audio)| Chosen {
            category: custom_category(&image),
            shown: name.clone(),
            image: image.map(|k| profiles::media_url(app, &k)),
            audio: audio.map(|k| profiles::media_url(app, &k)),
            id,
            name,
            valor: valor.into(),
            effect: "custom",
        })
        .collect())
}
fn chosen_json(c: &Chosen, enabled: bool) -> Value {
    json!({"id": c.id, "name": c.name, "category": c.category, "valor": c.valor, "effect": c.effect, "enabled": enabled, "custom": true, "image": c.image, "audio": c.audio})
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
pub(crate) async fn check(app: &App, channel: &str, id: &str) -> Res<Chosen> {
    let skill = match SKILLS.iter().find(|s| s.id == id) {
        Some(s) => Chosen::from(s),
        None => customs(app, channel, true)
            .await?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| Fail::field("skill", "Choose a Skill from the list."))?,
    };
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
pub(crate) async fn played(app: &App, channel: &str, user: &auth::User, skill: &Chosen) -> Res<()> {
    boards::play(
        app,
        channel,
        Some(&user.id),
        json!({"skill": skill.id, "label": skill.name, "effect": skill.effect, "image": skill.image, "audio": skill.audio,
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
    let mut skills: Vec<Value> = SKILLS
        .iter()
        .map(|s| skill_json(s, !off.iter().any(|c| c == s.category)))
        .collect();
    skills.extend(
        customs(&app, &channel, true)
            .await?
            .iter()
            .map(|c| chosen_json(c, !off.iter().any(|o| o == c.category))),
    );
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
    let custom: Vec<Value> = customs(&app, &user.id, false)
        .await?
        .iter()
        .map(|c| chosen_json(c, true))
        .collect();
    Ok(Json(
        json!({"skills": skills, "custom": custom, "prices": PRICES, "max_custom": MAX_CUSTOM, "categories": CATEGORIES, "disabled": off}),
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

#[derive(Deserialize)]
pub struct Custom {
    name: String,
    valor: i64,
}
/// POST /api/me/skills/custom: a new custom Skill (it goes on sale once it has an image or sound).
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Custom>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let name = input.name.trim();
    if !(1..=30).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err(Fail::field("name", "Name it in 1–30 characters."));
    }
    crate::text::filter(name, "name")?;
    if !PRICES.contains(&input.valor) {
        return Err(Fail::field("valor", "Choose one of the Skill prices."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,92))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_skills WHERE channel_id=$1")
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_CUSTOM {
        return Err(Fail::conflict("You can have up to 10 custom Skills."));
    }
    sqlx::query("INSERT INTO channel_skills(id,channel_id,name,valor) VALUES($1,$2,$3,$4)")
        .bind(profiles::new_id())
        .bind(&user.id)
        .bind(name)
        .bind(input.valor as i32)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    mine(State(app), jar).await
}
/// The signed-in owner's custom Skill, locked for a change.
async fn owned(
    tx: &mut sqlx::PgConnection,
    owner: &str,
    id: &str,
) -> Res<(Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT image_key,audio_key FROM channel_skills WHERE id=$1 AND channel_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Fail::missing)
}
/// DELETE /api/me/skills/custom/{id}: past messages keep the name they showed.
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let (image, audio) = owned(&mut tx, &user.id, &id).await?;
    sqlx::query("DELETE FROM channel_skills WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    media::queue_delete(&mut tx, image.as_deref(), None).await?;
    media::queue_delete(&mut tx, audio.as_deref(), None).await?;
    tx.commit().await?;
    mine(State(app), jar).await
}
/// Stores a custom Skill's new image or sound key and queues the old one for deletion.
async fn set_media(app: &App, owner: &str, id: &str, image: bool, key: &str) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let (old_image, old_audio) = owned(&mut tx, owner, id).await?;
    let column = if image { "image_key" } else { "audio_key" };
    // column is one of the two literal media columns above; values are bound.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE channel_skills SET {column}=$2 WHERE id=$1"
    )))
    .bind(id)
    .bind(key)
    .execute(&mut *tx)
    .await?;
    let old = if image { old_image } else { old_audio };
    media::queue_delete(&mut tx, old.as_deref(), Some(key)).await?;
    tx.commit().await?;
    Ok(())
}
async fn uploader(app: &App, jar: &CookieJar, id: &str) -> Res<String> {
    let user = profiles::signed_in(app, jar).await?;
    let mut tx = app.db.begin().await?;
    profiles::ensure_unrestricted(&mut tx, &user.id).await?;
    owned(&mut tx, &user.id, id).await?;
    tx.commit().await?;
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Uploads aren't available yet."));
    }
    Ok(user.id)
}
/// POST /api/me/skills/custom/{id}/image: the Skill's sticker (the image pipeline, fingerprinted).
async fn image(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let owner = uploader(&app, &jar, &id).await?;
    let (bytes, _, _) = media::read_upload(multipart, Kind::AlertImage).await?;
    profiles::rate(&app, format!("image-upload:{owner}"), 20, 3600).await?;
    let processed = media::process_async(bytes, Kind::AlertImage, None).await?;
    media::store(&app, &processed).await?;
    let mut tx = app.db.begin().await?;
    media::record(&mut tx, &owner, Kind::AlertImage, &processed).await?;
    tx.commit().await?;
    set_media(&app, &owner, &id, true, &processed.stored).await?;
    mine(State(app), jar).await
}
/// POST /api/me/skills/custom/{id}/sound: the Skill's sound (re-encoded, 10 seconds at most).
async fn sound(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let owner = uploader(&app, &jar, &id).await?;
    let key = crate::overlays::store_sound(&app, &owner, multipart).await?;
    set_media(&app, &owner, &id, false, &key).await?;
    mine(State(app), jar).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/skills", get(list))
        .route("/api/me/skills", get(mine).put(save))
        .route("/api/me/skills/custom", post(create))
        .route("/api/me/skills/custom/{id}", delete(remove))
        .route(
            "/api/me/skills/custom/{id}/image",
            post(image).layer(DefaultBodyLimit::max(2 * 1024 * 1024 + 16 * 1024)),
        )
        .route(
            "/api/me/skills/custom/{id}/sound",
            post(sound).layer(DefaultBodyLimit::max(1024 * 1024 + 16 * 1024)),
        )
}
