//! Module 2 profiles: shared helpers, channel resolution, public channel reads and profile
//! identity settings (docs/PROFILES.md).
// Row tuples from runtime sqlx queries read more clearly inline than as aliases.
#![allow(clippy::type_complexity)]
use crate::{App, Error, auth, auth::User, reserved, security as sec, text};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};
use std::borrow::Cow;

/// Module 2 error: Login's `{"error"}` shape plus the optional `field` key and `Retry-After`.
#[derive(Debug)]
pub struct Fail {
    pub status: StatusCode,
    pub message: Cow<'static, str>,
    pub field: Option<&'static str>,
    pub retry: Option<i64>,
}
impl Fail {
    pub fn new(status: StatusCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            status,
            message: message.into(),
            field: None,
            retry: None,
        }
    }
    pub fn bad(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    pub fn field(field: &'static str, message: &'static str) -> Self {
        Self {
            field: Some(field),
            ..Self::bad(message)
        }
    }
    pub fn field_owned(field: &'static str, message: String) -> Self {
        Self {
            field: Some(field),
            ..Self::bad(message)
        }
    }
    pub fn denied(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }
    pub fn conflict(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
    pub fn missing() -> Self {
        Self::new(StatusCode::NOT_FOUND, "Not found.")
    }
    pub fn channel_missing() -> Self {
        Self::new(StatusCode::NOT_FOUND, "This channel doesn't exist.")
    }
    pub fn stale() -> Self {
        Self::conflict("This changed in another tab. Reload to see the latest.")
    }
    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The request could not be completed.",
        )
    }
    pub fn unavailable(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }
}
impl From<Error> for Fail {
    fn from(e: Error) -> Self {
        Self {
            status: e.0,
            message: Cow::Borrowed(e.1),
            field: None,
            retry: e.2,
        }
    }
}
impl From<sqlx::Error> for Fail {
    fn from(e: sqlx::Error) -> Self {
        if e.as_database_error()
            .is_some_and(|d| d.is_unique_violation())
        {
            return Self::stale();
        }
        Error::from(e).into()
    }
}
impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        let mut body = json!({"error": self.message});
        if let Some(field) = self.field {
            body["field"] = json!(field);
        }
        let mut response = (self.status, Json(body)).into_response();
        if let Some(seconds) = self.retry {
            response
                .headers_mut()
                .insert("retry-after", seconds.to_string().parse().unwrap());
        }
        response
    }
}
pub type Res<T> = std::result::Result<T, Fail>;

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The signed-in viewer, or None for visitors (including expired or pending-deletion sessions).
pub async fn viewer(app: &App, jar: &CookieJar) -> Res<Option<User>> {
    match auth::session(app, jar, false).await {
        Ok((tx, user, _)) => {
            tx.commit().await?;
            Ok(Some(user))
        }
        Err(e) if e.0 == StatusCode::UNAUTHORIZED || e.0 == StatusCode::FORBIDDEN => Ok(None),
        Err(e) => Err(e.into()),
    }
}
/// A signed-in user for a mutation; commits the session transaction immediately.
pub async fn signed_in(app: &App, jar: &CookieJar) -> Res<User> {
    let (tx, user, _) = auth::session(app, jar, false).await?;
    tx.commit().await?;
    Ok(user)
}

/// Creates the user's profile row with defaults if it doesn't exist yet.
pub async fn ensure_profile(db: &mut PgConnection, user_id: &str) -> Res<()> {
    sqlx::query("INSERT INTO profiles(user_id,display_name) SELECT id,username FROM users WHERE id=$1 ON CONFLICT (user_id) DO NOTHING")
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Locks a section's revision and advances it; a stale expected revision returns 409.
pub async fn bump_section(
    db: &mut PgConnection,
    user_id: &str,
    section: &str,
    expected: Option<i64>,
) -> Res<i64> {
    sqlx::query(
        "INSERT INTO profile_sections(user_id,section) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(section)
    .execute(&mut *db)
    .await?;
    let current: i64 = sqlx::query_scalar(
        "SELECT revision FROM profile_sections WHERE user_id=$1 AND section=$2 FOR UPDATE",
    )
    .bind(user_id)
    .bind(section)
    .fetch_one(&mut *db)
    .await?;
    if expected.is_some_and(|e| e != current) {
        return Err(Fail::stale());
    }
    Ok(sqlx::query_scalar("UPDATE profile_sections SET revision=revision+1 WHERE user_id=$1 AND section=$2 RETURNING revision")
        .bind(user_id)
        .bind(section)
        .fetch_one(&mut *db)
        .await?)
}
pub async fn section_revision(db: &mut PgConnection, user_id: &str, section: &str) -> Res<i64> {
    Ok(
        sqlx::query_scalar("SELECT revision FROM profile_sections WHERE user_id=$1 AND section=$2")
            .bind(user_id)
            .bind(section)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or(0),
    )
}

/// Restricted users can't edit public profile fields, post, reply, react, submit fan art or report.
pub async fn ensure_unrestricted(db: &mut PgConnection, user_id: &str) -> Res<()> {
    let restricted: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT restricted_until>now() FROM profiles WHERE user_id=$1),false)",
    )
    .bind(user_id)
    .fetch_one(&mut *db)
    .await?;
    if restricted {
        return Err(Fail::denied(
            "Your channel is restricted. You can still view your account standing and appeal.",
        ));
    }
    Ok(())
}
pub fn ensure_verified(user: &User, message: &'static str) -> Res<()> {
    if user.email_verified {
        Ok(())
    } else {
        Err(Fail::denied(message))
    }
}
/// True when either user has blocked the other.
pub async fn blocked_between(db: &mut PgConnection, a: &str, b: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1))")
        .bind(a)
        .bind(b)
        .fetch_one(&mut *db)
        .await?)
}
pub async fn rate(app: &App, key: String, limit: i32, seconds: i64) -> Res<()> {
    sec::reserve(app, vec![key], limit, seconds).await?;
    Ok(())
}

/// A row of the `channel_users` view.
#[derive(FromRow, Clone)]
pub struct ChannelUser {
    pub id: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub email_verified: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    pub display_name: String,
    pub bio: String,
    pub mood_emoji: String,
    pub status_text: String,
    pub avatar_key: Option<String>,
    pub banner_key: Option<String>,
    pub internal: bool,
    pub restricted: bool,
    pub restricted_until: Option<DateTime<Utc>>,
    pub eligible: bool,
}
pub async fn channel_user_by_id(db: &mut PgConnection, id: &str) -> Res<Option<ChannelUser>> {
    Ok(sqlx::query_as("SELECT * FROM channel_users WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *db)
        .await?)
}
/// An eligible channel by case-insensitive username (reserved route names never resolve).
pub async fn eligible_by_name(db: &mut PgConnection, name: &str) -> Res<Option<ChannelUser>> {
    if !reserved::is_username_shaped(name) || reserved::is_listed(name) {
        return Ok(None);
    }
    Ok(
        sqlx::query_as("SELECT * FROM channel_users WHERE lower(username)=lower($1) AND eligible")
            .bind(name)
            .fetch_optional(&mut *db)
            .await?,
    )
}
pub enum Resolved {
    Found(Box<ChannelUser>),
    Redirect(String),
    Missing,
}
/// Channel resolution: exact owner first, then an active rename hold (by user ID, so a chain of
/// renames lands on the current name). Unknown, internal, deleted, held and restricted are Missing.
pub async fn resolve(db: &mut PgConnection, name: &str) -> Res<Resolved> {
    if !reserved::is_username_shaped(name) || reserved::is_listed(name) {
        return Ok(Resolved::Missing);
    }
    let owner: Option<ChannelUser> =
        sqlx::query_as("SELECT * FROM channel_users WHERE lower(username)=lower($1)")
            .bind(name)
            .fetch_optional(&mut *db)
            .await?;
    if let Some(owner) = owner {
        return Ok(if owner.eligible {
            Resolved::Found(Box::new(owner))
        } else {
            Resolved::Missing
        });
    }
    let held: Option<(String, bool)> = sqlx::query_as("SELECT c.username,c.eligible FROM username_holds h JOIN channel_users c ON c.id=h.user_id WHERE h.handle_canonical=lower($1) AND h.released_at>now() AND h.redirect")
        .bind(name)
        .fetch_optional(&mut *db)
        .await?;
    Ok(match held {
        Some((current, true)) => Resolved::Redirect(current),
        _ => Resolved::Missing,
    })
}

pub const AVATAR_SIZES: [u32; 3] = [64, 160, 400];
pub const BANNER_WIDTHS: [u32; 3] = [750, 1500, 3000];
pub fn media_url(app: &App, key: &str) -> String {
    format!("{}/{}", app.config.media.public_base, key)
}
/// Avatar URLs by size; `null` means the static default avatar.
pub fn avatar_json(app: &App, key: Option<&str>) -> Value {
    match key {
        Some(prefix) => json!(
            AVATAR_SIZES
                .iter()
                .map(|s| (
                    s.to_string(),
                    json!(media_url(app, &format!("{prefix}/{s}.webp")))
                ))
                .collect::<serde_json::Map<_, _>>()
        ),
        None => Value::Null,
    }
}
/// Banner URLs by width; stored keys look like `banners/<hash>@<largest width>`.
pub fn banner_json(app: &App, key: Option<&str>) -> Value {
    let Some((prefix, max)) = key.and_then(|k| k.split_once('@')) else {
        return Value::Null;
    };
    let max: u32 = max.parse().unwrap_or(750);
    json!(
        BANNER_WIDTHS
            .iter()
            .filter(|w| **w <= max)
            .map(|w| (
                w.to_string(),
                json!(media_url(app, &format!("{prefix}/{w}.webp")))
            ))
            .collect::<serde_json::Map<_, _>>()
    )
}
/// The display chip used in every list. Ineligible users are unlinked; deleted ones are anonymous.
pub fn chip(app: &App, user: &ChannelUser) -> Value {
    if user.deleted_at.is_some() {
        return json!({"username": null, "display_name": "Deleted user", "avatar": null, "linked": false, "deleted": true});
    }
    if !user.eligible {
        return json!({"username": user.username, "display_name": user.username, "avatar": null, "linked": false, "deleted": false});
    }
    json!({"username": user.username, "display_name": user.display_name, "avatar": avatar_json(app, user.avatar_key.as_deref()), "linked": true, "deleted": false})
}
pub fn chip_sql(alias: &'static str) -> String {
    // jsonb chip built in SQL for list queries; the web maps avatar keys through `media_base`.
    format!(
        "jsonb_build_object('username',CASE WHEN {a}.deleted_at IS NULL THEN {a}.username END,'display_name',CASE WHEN {a}.deleted_at IS NOT NULL THEN 'Deleted user' WHEN {a}.eligible THEN {a}.display_name ELSE {a}.username END,'avatar_key',CASE WHEN {a}.eligible THEN {a}.avatar_key END,'linked',{a}.eligible,'deleted',{a}.deleted_at IS NOT NULL,'live',{a}.eligible AND {live})",
        a = alias,
        live = crate::playback::live_sql(&format!("{alias}.id"))
    )
}
/// Replaces `avatar_key` inside SQL-built chips with avatar URL maps.
pub fn hydrate(app: &App, value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(key) = map.remove("avatar_key") {
                map.insert("avatar".into(), avatar_json(app, key.as_str()));
            }
            for v in map.values_mut() {
                hydrate(app, v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| hydrate(app, v)),
        _ => {}
    }
}

pub async fn follower_counts(db: &mut PgConnection, user_id: &str) -> Res<(i64, i64)> {
    // Exact counts excluding internal, deleted and held accounts.
    Ok(sqlx::query_as("SELECT (SELECT count(*) FROM follows f JOIN channel_users c ON c.id=f.follower_id WHERE f.following_id=$1 AND NOT c.internal AND c.deleted_at IS NULL),(SELECT count(*) FROM follows f JOIN channel_users c ON c.id=f.following_id WHERE f.follower_id=$1 AND NOT c.internal AND c.deleted_at IS NULL)")
        .bind(user_id)
        .fetch_one(&mut *db)
        .await?)
}

pub async fn links_json(db: &mut PgConnection, user_id: &str) -> Res<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('platform',platform,'url',url) FROM social_links WHERE user_id=$1 ORDER BY position")
        .bind(user_id)
        .fetch_all(&mut *db)
        .await?)
}

/// GET /api/channels/{username}/resolve: canonical casing or a hold redirect (used by the web proxy).
pub async fn resolve_channel(State(app): State<App>, Path(name): Path<String>) -> Res<Json<Value>> {
    let mut db = app.db.acquire().await?;
    match resolve(&mut db, &name).await? {
        Resolved::Found(user) => Ok(Json(json!({"username": user.username}))),
        Resolved::Redirect(current) => Ok(Json(json!({"redirect_to": current}))),
        Resolved::Missing => Err(Fail::channel_missing()),
    }
}

/// GET /api/channels/{username}: header, counts, song, War Council, Home previews and the
/// viewer relationship.
pub async fn channel(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let user = match resolve(&mut db, &name).await? {
        Resolved::Found(user) => *user,
        Resolved::Redirect(current) => return Ok(Json(json!({"redirect_to": current}))),
        Resolved::Missing => return Err(Fail::channel_missing()),
    };
    let viewer_id = viewer.as_ref().map(|v| v.id.as_str());
    let is_owner = viewer_id == Some(user.id.as_str());
    let (followers, following) = follower_counts(&mut db, &user.id).await?;
    let relation = match viewer_id {
        Some(v) if !is_owner => {
            let (following, blocked, blocked_by): (bool, bool, bool) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2),EXISTS(SELECT 1 FROM user_blocks WHERE blocker_id=$1 AND blocked_id=$2),EXISTS(SELECT 1 FROM user_blocks WHERE blocker_id=$2 AND blocked_id=$1)")
                .bind(v)
                .bind(&user.id)
                .fetch_one(&mut *db)
                .await?;
            json!({"signed_in": true, "is_owner": false, "following": following, "blocked": blocked, "interaction_blocked": blocked || blocked_by})
        }
        Some(_) => {
            json!({"signed_in": true, "is_owner": true, "following": false, "blocked": false, "interaction_blocked": false})
        }
        None => {
            json!({"signed_in": false, "is_owner": false, "following": false, "blocked": false, "interaction_blocked": false})
        }
    };
    let profile: Option<(Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i32, Option<String>, bool)> = sqlx::query_as("SELECT song_provider,song_media_id,song_title,song_artist,song_thumb_key,song_volume,song_notice,fan_art_enabled FROM profiles WHERE user_id=$1")
        .bind(&user.id)
        .fetch_optional(&mut *db)
        .await?;
    let (song, fan_art_enabled) = match &profile {
        // Imported SoundCloud songs stay hidden until the post-import job finds the track ID.
        Some((Some(provider), Some(media), title, artist, thumb, volume, _, fan))
            if !media.is_empty() =>
        {
            (
                json!({"provider": provider, "media_id": media, "title": title, "artist": artist, "thumbnail": thumb.as_ref().map(|k| media_url(&app, k)), "volume": volume}),
                *fan,
            )
        }
        Some((_, _, _, _, _, _, _, fan)) => (Value::Null, *fan),
        None => (Value::Null, false),
    };
    let approved_fan_art: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM fan_art f JOIN channel_users c ON c.id=f.submitter_id WHERE f.channel_id=$1 AND f.status='APPROVED' AND c.deleted_at IS NULL)")
        .bind(&user.id)
        .fetch_one(&mut *db)
        .await?;
    let war_council =
        crate::social::war_council_read(&app, &mut db, &user.id, viewer_id, is_owner).await?;
    let wall_preview = crate::wall::preview(&app, &mut db, &user, viewer.as_ref()).await?;
    let schedule_next = crate::studio::next_occurrences(&mut db, &user.id, 3).await?;
    let about_has_content: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM profile_blocks WHERE user_id=$1 AND enabled) OR EXISTS(SELECT 1 FROM sponsors WHERE user_id=$1 AND active) OR EXISTS(SELECT 1 FROM setup_items WHERE user_id=$1) OR EXISTS(SELECT 1 FROM setup_photos WHERE user_id=$1 AND status='VISIBLE') OR EXISTS(SELECT 1 FROM profiles WHERE user_id=$1 AND (setup_title<>'' OR setup_description<>''))")
        .bind(&user.id)
        .fetch_one(&mut *db)
        .await?;
    let song_notice = if is_owner {
        profile.as_ref().and_then(|p| p.6.clone())
    } else {
        None
    };
    Ok(Json(json!({
        "channel": {
            "username": user.username,
            "display_name": user.display_name,
            "bio": user.bio,
            "mood_emoji": user.mood_emoji,
            "status_text": user.status_text,
            "avatar": avatar_json(&app, user.avatar_key.as_deref()),
            "banner": banner_json(&app, user.banner_key.as_deref()),
            "joined_at": user.created_at,
            "follower_count": followers,
            "following_count": following,
            "links": links_json(&mut db, &user.id).await?,
            "song": song,
            "song_notice": song_notice,
            "live": crate::playback::is_live(&mut db, &user.id).await?,
            "faction": null,
        },
        "tabs": {
            "wall": true,
            // The Schedule tab always exists; an empty week renders an empty state.
            "schedule": true,
            "about": about_has_content || !user.bio.is_empty() || is_owner,
            "fan_art": fan_art_enabled && (approved_fan_art || is_owner),
        },
        "fan_art_enabled": fan_art_enabled,
        "header": crate::studio::channel_header(&mut db, &user.id).await?,
        "war_council": war_council,
        "wall_preview": wall_preview,
        "schedule_next": schedule_next,
        "viewer": relation,
    })))
}

/// GET /api/me/profile: the owner's editable identity fields with section revisions.
pub async fn my_profile(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let me = channel_user_by_id(&mut db, &user.id)
        .await?
        .ok_or_else(Fail::missing)?;
    let (song_notice, email_updates, show_linked): (Option<String>, bool, bool) =
        sqlx::query_as("SELECT song_notice,email_report_updates,show_linked_accounts FROM profiles WHERE user_id=$1")
            .bind(&user.id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or((None, false, false));
    let rename = crate::rename::status(&mut db, &user).await?;
    Ok(Json(json!({
        "username": me.username,
        "display_name": me.display_name,
        "bio": me.bio,
        "mood_emoji": me.mood_emoji,
        "status_text": me.status_text,
        "avatar": avatar_json(&app, me.avatar_key.as_deref()),
        "banner": banner_json(&app, me.banner_key.as_deref()),
        "links": links_json(&mut db, &user.id).await?,
        "platforms": text::PLATFORMS.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        "revisions": {
            "profile": section_revision(&mut db, &user.id, "profile").await?,
            "links": section_revision(&mut db, &user.id, "links").await?,
        },
        "rename": rename,
        "internal": me.internal,
        "restricted_until": me.restricted_until.filter(|_| me.restricted),
        "song_notice": song_notice,
        "email_report_updates": email_updates,
        "mfa_enabled": user.mfa_enabled,
        "mood_presets": text::MOOD_PRESETS,
        "show_linked_accounts": show_linked,
    })))
}

/// GET /api/me/link-suggestions: links derived from linked Twitch and Discord identities, for
/// the user to add and confirm (decision P5). Nothing is saved here.
pub async fn link_suggestions(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let identities: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT provider,subject,handle FROM identities WHERE user_id=$1 AND provider IN ('twitch','discord') ORDER BY provider DESC",
    )
    .bind(&user.id)
    .fetch_all(&mut *db)
    .await?;
    let existing: Vec<String> =
        sqlx::query_scalar("SELECT platform FROM social_links WHERE user_id=$1")
            .bind(&user.id)
            .fetch_all(&mut *db)
            .await?;
    let mut suggestions = Vec::new();
    let mut missing_handle = Vec::new();
    for (provider, subject, handle) in identities {
        if existing.contains(&provider) {
            continue;
        }
        let url = match (provider.as_str(), handle.as_deref()) {
            ("twitch", Some(h)) => Some(format!("https://twitch.tv/{h}")),
            ("discord", _) => Some(format!("https://discord.com/users/{subject}")),
            _ => None,
        };
        // Only suggest what the link validator accepts.
        match url.filter(|u| text::social_link(&provider, u).is_ok()) {
            Some(url) => suggestions.push(
                json!({"platform": provider, "url": url, "label": handle.unwrap_or_default()}),
            ),
            None => missing_handle.push(provider),
        }
    }
    Ok(Json(
        json!({"suggestions": suggestions, "missing_handle": missing_handle, "link_count": existing.len()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardSettings {
    show_linked_accounts: bool,
}
/// PUT /api/me/card-settings: the opt-in "Also known as" line (decision P6).
pub async fn card_settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<CardSettings>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    ensure_profile(&mut tx, &user.id).await?;
    sqlx::query("UPDATE profiles SET show_linked_accounts=$2,updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(input.show_linked_accounts)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"saved": true, "show_linked_accounts": input.show_linked_accounts}),
    ))
}

#[derive(Deserialize)]
pub struct ProfilePatch {
    display_name: Option<String>,
    bio: Option<String>,
    mood_emoji: Option<String>,
    status_text: Option<String>,
    revision: Option<i64>,
}
/// PATCH /api/me/profile: display name, bio, mood and status, saved atomically.
pub async fn update_profile(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<ProfilePatch>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let display = input
        .display_name
        .as_deref()
        .map(|v| text::display_name(v, &user.username))
        .transpose()?;
    let bio = input
        .bio
        .as_deref()
        .map(|v| text::plain(v, "bio", 0, 300, 4, true))
        .transpose()?;
    let mood = input.mood_emoji.as_deref().map(text::mood).transpose()?;
    let status = input
        .status_text
        .as_deref()
        .map(|v| text::plain(v, "status_text", 0, 80, 0, true))
        .transpose()?;
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "profile", input.revision).await?;
    sqlx::query("UPDATE profiles SET display_name=coalesce($2,display_name),bio=coalesce($3,bio),mood_emoji=coalesce($4,mood_emoji),status_text=coalesce($5,status_text),updated_at=now() WHERE user_id=$1")
        .bind(&user.id)
        .bind(display)
        .bind(bio)
        .bind(mood)
        .bind(status)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true, "revision": revision})))
}

#[derive(Deserialize)]
pub struct LinkInput {
    platform: String,
    url: String,
}
#[derive(Deserialize)]
pub struct LinksInput {
    links: Vec<LinkInput>,
    revision: Option<i64>,
}
/// Validates an ordered social link list: up to 5, one per platform except Website (2).
pub fn validate_links(links: &[LinkInput]) -> Res<Vec<(String, String)>> {
    if links.len() > 5 {
        return Err(Fail::field("links", "Add up to 5 links."));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for link in links {
        let platform = link.platform.trim().to_ascii_lowercase();
        let url = text::social_link(&platform, &link.url)?;
        let same = out.iter().filter(|(p, _)| *p == platform).count();
        if (platform == "website" && same >= 2) || (platform != "website" && same >= 1) {
            return Err(Fail::field(
                "links",
                "Add each platform once (Website up to twice).",
            ));
        }
        out.push((platform, url));
    }
    Ok(out)
}
/// PUT /api/me/links: replaces the ordered link list.
pub async fn update_links(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<LinksInput>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let links = validate_links(&input.links)?;
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    let revision = bump_section(&mut tx, &user.id, "links", input.revision).await?;
    sqlx::query("DELETE FROM social_links WHERE user_id=$1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    for (i, (platform, url)) in links.iter().enumerate() {
        sqlx::query(
            "INSERT INTO social_links(id,user_id,position,platform,url) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(new_id())
        .bind(&user.id)
        .bind(i as i32 + 1)
        .bind(platform)
        .bind(url)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"saved": true, "revision": revision, "links": links.iter().map(|(p, u)| json!({"platform": p, "url": u})).collect::<Vec<_>>()}),
    ))
}

#[derive(Deserialize)]
pub struct Cursor {
    pub cursor: Option<String>,
}
/// Cursor format shared by list endpoints: `<rfc3339 timestamp>|<id>`.
pub fn parse_cursor(cursor: &Option<String>) -> Res<Option<(DateTime<Utc>, String)>> {
    match cursor.as_deref().filter(|c| !c.is_empty()) {
        None => Ok(None),
        Some(c) => {
            let (at, id) = c
                .split_once('|')
                .ok_or_else(|| Fail::bad("Invalid cursor."))?;
            let at = DateTime::parse_from_rfc3339(at)
                .map_err(|_| Fail::bad("Invalid cursor."))?
                .with_timezone(&Utc);
            if id.len() > 200 {
                return Err(Fail::bad("Invalid cursor."));
            }
            Ok(Some((at, id.to_string())))
        }
    }
}
pub fn make_cursor(at: DateTime<Utc>, id: &str) -> String {
    format!(
        "{}|{}",
        at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        id
    )
}
pub type CursorQuery = Query<Cursor>;
