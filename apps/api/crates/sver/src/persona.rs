//! The persona pass (docs/PROFILES.md "Persona pass", Joe, October 16, 2026): what a channel page
//! shows about who someone is (level, title, frame, showcase, Currently), how its Home is laid out
//! (owner-ordered sections, stream first or persona first, a theme variant) and how the visitor
//! relates to it. Nothing here ranks people against each other.

use crate::profiles::{Fail, Res, bump_section, ensure_profile, ensure_unrestricted, signed_in};
use crate::{App, progression, text};
use axum::{Json, extract::State};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// Home's sections, in the default order. The owner can reorder them; none can be dropped,
/// and empty ones are hidden from visitors.
pub const HOME_SECTIONS: [&str; 8] = [
    "about_me",
    "war_council",
    "wall",
    "up_next",
    "videos",
    "shine",
    "achievements",
    "activity",
];
/// The words "Currently" can start with.
pub const CURRENTLY: [(&str, &str); 6] = [
    ("PLAYING", "Playing"),
    ("LISTENING", "Listening to"),
    ("WATCHING", "Watching"),
    ("READING", "Reading"),
    ("BUILDING", "Building"),
    ("LEARNING", "Learning"),
];
/// Each faction's variants besides its default (docs/PROFILES.md "Theme variants").
fn variants(faction: Option<&str>) -> &'static [&'static str] {
    match faction {
        Some("myria") => &["forge", "ash"],
        Some("aetheron") => &["moon", "deep"],
        Some("glint") => &["crown", "dawn"],
        _ => &[],
    }
}

/// The stored order, then any section it doesn't name, so new sections always appear.
pub fn home_order(stored: &[String]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for s in stored {
        if let Some(k) = HOME_SECTIONS.iter().find(|k| **k == s.as_str())
            && !out.contains(k)
        {
            out.push(k);
        }
    }
    for k in HOME_SECTIONS {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

pub fn currently_label(verb: &str) -> Option<&'static str> {
    CURRENTLY.iter().find(|(k, _)| *k == verb).map(|(_, l)| *l)
}

type Row = (String, String, String, String, String, Vec<String>);
async fn row(db: &mut PgConnection, user: &str) -> Res<Row> {
    Ok(sqlx::query_as("SELECT currently_verb,currently_text,song_why,theme_variant,page_layout,home_order FROM profiles WHERE user_id=$1")
        .bind(user)
        .fetch_optional(db)
        .await?
        .unwrap_or_default())
}

/// The persona fields of GET /api/channels/{username}, merged into its `channel` object.
pub async fn channel_fields(
    db: &mut PgConnection,
    user: &str,
    faction: Option<&str>,
) -> Res<Value> {
    let (verb, current, why, variant, layout, order) = row(db, user).await?;
    let level = progression::level(progression::total(db, user).await?);
    let (streamed, has_videos, has_beacons, live_category): (bool, bool, bool, Option<String>) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1 AND confirmed_live_at IS NOT NULL),
                EXISTS(SELECT 1 FROM videos WHERE owner_id=$1 AND status='READY' AND visibility<>'PRIVATE'),
                EXISTS(SELECT 1 FROM beacons WHERE owner_id=$1 AND status='PUBLISHED'),
                (SELECT c.name FROM broadcasts b JOIN stream_settings s ON s.owner_id=b.owner_id
                   LEFT JOIN stream_categories c ON c.id=s.category_id
                  WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') LIMIT 1)",
    )
    .bind(user)
    .fetch_one(&mut *db)
    .await?;
    let variant = variants(faction)
        .contains(&variant.as_str())
        .then_some(variant);
    let layout = match layout.as_str() {
        "stream" | "persona" => layout,
        _ if streamed => "stream".into(),
        _ => "persona".into(),
    };
    Ok(json!({
        "level": level,
        "title": progression::title(level, faction),
        "frame": crate::achievements::frame(db, user, level).await?,
        "showcase": crate::achievements::showcase(db, user).await?,
        "currently": currently_label(&verb).map(|l| json!({"verb": verb, "label": l, "text": current})),
        "streaming_category": live_category,
        "streamed": streamed,
        "has_videos": has_videos,
        "has_beacons": has_beacons,
        "song_why": why,
        "theme_variant": variant,
        "layout": layout,
        "home_order": home_order(&order),
    }))
}

/// The signed-in visitor's own relationship with this channel: their loyalty rank here (once
/// they've earned some Engagement Valor), when they followed, and War Council membership both
/// ways. Only about them, never a comparison.
pub async fn relationship(db: &mut PgConnection, channel: &str, viewer: &str) -> Res<Value> {
    let (earned, since, on_theirs, on_mine): (
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
        bool,
        bool,
    ) = sqlx::query_as(
        "SELECT (SELECT earned FROM engagement WHERE channel_id=$1 AND user_id=$2),
                (SELECT created_at FROM follows WHERE follower_id=$2 AND following_id=$1),
                EXISTS(SELECT 1 FROM war_council WHERE user_id=$1 AND member_id=$2),
                EXISTS(SELECT 1 FROM war_council WHERE user_id=$2 AND member_id=$1)",
    )
    .bind(channel)
    .bind(viewer)
    .fetch_one(db)
    .await?;
    let rank = earned
        .filter(|e| *e > 0)
        .map(|e| progression::LOYALTY[progression::loyalty(e).max(0) as usize].0);
    Ok(json!({
        "loyalty": rank,
        "following_since": since,
        "on_their_council": on_theirs,
        "on_your_council": on_mine,
    }))
}

/// GET /api/me/persona: Currently, the song note, theme variant, layout and Home order.
pub async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let (verb, current, why, variant, layout, order) = row(&mut db, &user.id).await?;
    let faction = crate::factions::membership(&mut db, &user.id).await?;
    let revision: i64 = sqlx::query_scalar(
        "SELECT coalesce((SELECT revision FROM profile_sections WHERE user_id=$1 AND section='persona'),0)",
    )
    .bind(&user.id)
    .fetch_one(&mut *db)
    .await?;
    Ok(Json(json!({
        "currently_verb": verb,
        "currently_text": current,
        "song_why": why,
        "theme_variant": variant,
        "variants": variants(faction.as_deref()),
        "faction": faction,
        "page_layout": layout,
        "home_order": home_order(&order),
        "sections": HOME_SECTIONS,
        "verbs": CURRENTLY.iter().map(|(k, l)| json!({"verb": k, "label": l})).collect::<Vec<_>>(),
        "revision": revision,
    })))
}

#[derive(Deserialize)]
pub struct PersonaInput {
    currently_verb: Option<String>,
    currently_text: Option<String>,
    song_why: Option<String>,
    theme_variant: Option<String>,
    page_layout: Option<String>,
    home_order: Option<Vec<String>>,
    revision: Option<i64>,
}

/// PUT /api/me/persona: any of the fields; the rest are kept.
pub async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<PersonaInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if let Some(v) = &input.currently_verb
        && !v.is_empty()
        && currently_label(v).is_none()
    {
        return Err(Fail::field(
            "currently_verb",
            "Choose one of the listed words.",
        ));
    }
    let current = input
        .currently_text
        .as_deref()
        .map(|v| text::plain(v, "currently_text", 0, 60, 0, true))
        .transpose()?;
    let why = input
        .song_why
        .as_deref()
        .map(|v| text::plain(v, "song_why", 0, 80, 0, true))
        .transpose()?;
    if let Some(l) = &input.page_layout
        && !matches!(l.as_str(), "" | "stream" | "persona")
    {
        return Err(Fail::field(
            "page_layout",
            "Choose stream first or persona first.",
        ));
    }
    if let Some(order) = &input.home_order
        && (order.len() > HOME_SECTIONS.len()
            || order.iter().any(|s| !HOME_SECTIONS.contains(&s.as_str())))
    {
        return Err(Fail::field("home_order", "Unknown section."));
    }
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    if let Some(v) = &input.theme_variant
        && !v.is_empty()
    {
        let faction = crate::factions::membership(&mut tx, &user.id).await?;
        if !variants(faction.as_deref()).contains(&v.as_str()) {
            return Err(Fail::field(
                "theme_variant",
                "That variant isn't available for your faction.",
            ));
        }
    }
    let revision = bump_section(&mut tx, &user.id, "persona", input.revision).await?;
    sqlx::query("UPDATE profiles SET currently_verb=coalesce($2,currently_verb),currently_text=coalesce($3,currently_text),song_why=coalesce($4,song_why),theme_variant=coalesce($5,theme_variant),page_layout=coalesce($6,page_layout),home_order=coalesce($7,home_order),updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(&input.currently_verb)
        .bind(current)
        .bind(why)
        .bind(&input.theme_variant)
        .bind(&input.page_layout)
        .bind(&input.home_order)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "revision": revision})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order_keeps_every_section_once() {
        let stored = vec![
            "wall".to_string(),
            "bogus".into(),
            "wall".into(),
            "about_me".into(),
        ];
        let order = home_order(&stored);
        assert_eq!(&order[..2], &["wall", "about_me"]);
        assert_eq!(order.len(), HOME_SECTIONS.len());
    }
}
