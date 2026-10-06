//! Module 3 channel moderation: roles, chat timeouts/bans, message deletion, chat rules and the
//! channel audit log. Every action rechecks the actor's role, so a removed moderator loses
//! access on their next request. Channel roles never grant /admin access.
use crate::{
    App, auth,
    profiles::{self, Fail, Res},
    safety, security as sec,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    Owner,
    Moderator,
    Staff,
}
impl Role {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Moderator => "moderator",
            Role::Staff => "staff",
        }
    }
}

/// The user's moderation role in this channel, if any. Moderators must still be verified and in
/// good standing; staff must have the admin role and MFA.
pub async fn role_of(app: &App, channel: &str, user: &auth::User) -> Res<Option<Role>> {
    if user.id == channel {
        return Ok(Some(Role::Owner));
    }
    let moderator: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_moderators m JOIN channel_users u ON u.id=m.user_id WHERE m.channel_id=$1 AND m.user_id=$2 AND u.eligible AND u.email_verified)")
        .bind(channel).bind(&user.id).fetch_one(&app.db).await?;
    if moderator {
        return Ok(Some(Role::Moderator));
    }
    let mut conn = app.db.acquire().await?;
    if user.mfa_enabled && safety::is_staff(&mut conn, &user.id).await? {
        return Ok(Some(Role::Staff));
    }
    Ok(None)
}
pub(crate) async fn actor(app: &App, jar: &CookieJar, channel: &str) -> Res<(auth::User, Role)> {
    let user = profiles::signed_in(app, jar).await?;
    let role = role_of(app, channel, &user)
        .await?
        .ok_or_else(|| Fail::denied("You can't moderate this channel."))?;
    Ok((user, role))
}
async fn channel(app: &App, name: &str) -> Res<String> {
    let mut conn = app.db.acquire().await?;
    Ok(profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id)
}
async fn user_id(app: &App, name: &str) -> Res<String> {
    sqlx::query_scalar(
        "SELECT id FROM channel_users WHERE lower(username)=lower($1) AND deleted_at IS NULL",
    )
    .bind(name)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Fail::missing)
}
/// Nobody sanctions the owner, themselves, a current moderator or platform staff in chat.
/// The owner removes a moderator before sanctioning them.
pub async fn protected(app: &App, channel: &str, actor: &str, target: &str) -> Res<bool> {
    if target == channel || target == actor {
        return Ok(true);
    }
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_moderators WHERE channel_id=$1 AND user_id=$2) OR EXISTS(SELECT 1 FROM staff_roles WHERE user_id=$2)")
        .bind(channel).bind(target).fetch_one(&app.db).await?)
}
pub(crate) fn reason(text: &str) -> Res<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 500 {
        return Err(Fail::field("reason", "Give a reason of 1–500 characters."));
    }
    Ok(text.to_string())
}
/// When spike protection's followers-only chat ends, if it's on now.
pub async fn followers_only(app: &App, channel: &str) -> Res<Option<DateTime<Utc>>> {
    Ok(sqlx::query_scalar("SELECT followers_only_until FROM chat_settings WHERE channel_id=$1 AND followers_only_until>now()")
        .bind(channel)
        .fetch_optional(&app.db)
        .await?)
}

/// Whether chat is limited to subscribers (docs/SUPPORT.md "Subscriptions").
pub async fn subs_only(app: &App, channel: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT subs_only FROM chat_settings WHERE channel_id=$1),false)",
    )
    .bind(channel)
    .fetch_one(&app.db)
    .await?)
}

#[derive(Deserialize)]
pub struct Protect {
    on: bool,
}
/// PUT /api/channels/{username}/chat/protect: the one-click spike prompt. Owners and moderators
/// turn on followers-only chat for 10 minutes (it always ends by itself) or end it early.
pub async fn protect(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Protect>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let mut tx = app.db.begin().await?;
    let until: Option<DateTime<Utc>> = sqlx::query_scalar("INSERT INTO chat_settings(channel_id,followers_only_until) VALUES($1,CASE WHEN $2 THEN now()+interval '10 minutes' END) ON CONFLICT(channel_id) DO UPDATE SET followers_only_until=EXCLUDED.followers_only_until RETURNING followers_only_until")
        .bind(&channel)
        .bind(input.on)
        .fetch_one(&mut *tx)
        .await?;
    log(
        &mut tx,
        &channel,
        &user.id,
        role,
        if input.on {
            "followers_only_on"
        } else {
            "followers_only_off"
        },
        None,
        None,
        json!({"until": until}),
        "Spike protection",
    )
    .await?;
    tx.commit().await?;
    app.chat
        .publish(&channel, None, 0, json!({"type":"protect","until":until}));
    Ok(Json(json!({"followers_only_until": until})))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn log(
    db: &mut PgConnection,
    channel: &str,
    actor: &str,
    role: Role,
    action: &str,
    target: Option<&str>,
    message: Option<&str>,
    detail: Value,
    reason: &str,
) -> Res<()> {
    sqlx::query("INSERT INTO channel_moderation_log(id,channel_id,actor_id,actor_role,action,target_id,message_id,detail,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(profiles::new_id()).bind(channel).bind(actor).bind(role.name()).bind(action).bind(target).bind(message).bind(detail).bind(reason)
        .execute(db).await?;
    Ok(())
}

/// Case-folded, whitespace-normalized text for banned-phrase matching.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
/// Plain-text URL/domain detection, including mixed case, `www.` and scheme-less forms. Never
/// fetches anything and is not a complete obfuscation detector.
fn has_link(body: &str) -> bool {
    body.to_lowercase().split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| {
            matches!(
                c,
                '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | ',' | '!' | '?' | ':' | ';' | '.'
            )
        });
        if token.contains("://") || token.starts_with("www.") {
            return true;
        }
        let host = token.split(['/', '?', '#']).next().unwrap_or("");
        match host.rsplit_once('.') {
            Some((name, tld)) => {
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
                    && (2..=24).contains(&tld.chars().count())
                    && tld.chars().all(char::is_alphabetic)
            }
            None => false,
        }
    })
}

/// Upload codes obey the channel's content rules, without consuming chat slow-mode windows.
pub async fn check_emote_code(db: &mut sqlx::PgConnection, channel: &str, code: &str) -> Res<()> {
    let rules: Option<(bool, Vec<String>)> =
        sqlx::query_as("SELECT block_links,banned_words FROM chat_settings WHERE channel_id=$1")
            .bind(channel)
            .fetch_optional(db)
            .await?;
    if let Some((links, words)) = rules {
        let folded = fold(code);
        if words.iter().any(|w| folded.contains(w)) || (links && has_link(code)) {
            return Err(Fail::bad("That code isn't allowed in this chat."));
        }
    }
    Ok(())
}

/// Chat rules applied by `chat::send` before a message is stored.
/// PUT /api/channels/{username}/chat/subs-only: owners and moderators limit chat to subscribers.
pub async fn set_subs_only(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Protect>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO chat_settings(channel_id,subs_only) VALUES($1,$2) ON CONFLICT(channel_id) DO UPDATE SET subs_only=EXCLUDED.subs_only")
        .bind(&channel)
        .bind(input.on)
        .execute(&mut *tx)
        .await?;
    log(
        &mut tx,
        &channel,
        &user.id,
        role,
        if input.on {
            "subs_only_on"
        } else {
            "subs_only_off"
        },
        None,
        None,
        json!({}),
        "Subscriber-only chat",
    )
    .await?;
    tx.commit().await?;
    app.chat
        .publish(&channel, None, 0, json!({"type":"subs_only","on":input.on}));
    Ok(Json(json!({"subs_only": input.on})))
}
pub async fn check_send(app: &App, channel: &str, user: &auth::User, body: &str) -> Res<()> {
    let restriction: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as("SELECT kind, until FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND (kind='ban' OR until>now()) ORDER BY kind='ban' DESC LIMIT 1")
        .bind(channel).bind(&user.id).fetch_optional(&app.db).await?;
    match restriction {
        Some((kind, _)) if kind == "ban" => {
            return Err(Fail::denied("You're banned from this chat."));
        }
        Some((_, Some(until))) => {
            return Err(Fail {
                retry: Some((until - Utc::now()).num_seconds().max(1)),
                ..Fail::denied("You're timed out in this chat.")
            });
        }
        _ => {}
    }
    // Spike protection: while followers-only chat is on, only people who followed at least 10
    // minutes ago (and channel roles) can chat, so a burst of new accounts can't flood it.
    if followers_only(app, channel).await?.is_some() && role_of(app, channel, user).await?.is_none()
    {
        let follower: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2 AND created_at<=now()-interval '10 minutes')")
            .bind(&user.id)
            .bind(channel)
            .fetch_one(&app.db)
            .await?;
        if !follower {
            return Err(Fail::denied(
                "Chat is followers-only for a few minutes (followers of at least 10 minutes can chat).",
            ));
        }
    }
    // Subscriber-only chat and subscriber emotes; channel roles are exempt.
    let tier = crate::subs::active_tier(app, channel, &user.id).await?;
    let tokens: Vec<&str> = body.split_whitespace().collect();
    let locked: Option<i16> = sqlx::query_scalar("SELECT max(tier) FROM channel_emotes WHERE channel_id=$1 AND status='VISIBLE' AND tier>coalesce($2,0::smallint) AND code=ANY($3)")
        .bind(channel).bind(tier).bind(&tokens).fetch_one(&app.db).await?;
    if (locked.is_some() || (tier.is_none() && subs_only(app, channel).await?))
        && role_of(app, channel, user).await?.is_none()
    {
        return Err(match locked {
            Some(t) => Fail::bad(format!("That emote is for Tier {t} subscribers.")),
            None => Fail::denied("Chat is subscribers-only right now."),
        });
    }
    let settings: Option<(i32, bool, Vec<String>)> = sqlx::query_as(
        "SELECT slow_mode_seconds, block_links, banned_words FROM chat_settings WHERE channel_id=$1",
    )
    .bind(channel)
    .fetch_optional(&app.db)
    .await?;
    let Some((slow, links, words)) = settings else {
        return Ok(());
    };
    let exempt = role_of(app, channel, user).await?.is_some();
    check_content(body, &words, links, exempt)?;
    if slow > 0 && !exempt {
        sec::reserve(
            app,
            vec![format!("chat-slow:{channel}:{}", user.id)],
            1,
            slow.into(),
        )
        .await
        .map_err(|e| Fail {
            message: "Slow mode is on. You can send again shortly.".into(),
            ..Fail::from(e)
        })?;
    }
    Ok(())
}
/// A channel ban refuses signed-in playback as well as chat.
pub async fn banned(app: &App, channel: &str, user: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND kind='ban')")
        .bind(channel).bind(user).fetch_one(&app.db).await?)
}
/// Clip titles share chat's ban/timeout, banned-word and link checks, without consuming a
/// chat slow-mode turn or inheriting subscribers-only chat as a separate clipping permission.
pub async fn check_clip(app: &App, channel: &str, user: &auth::User, title: &str) -> Res<()> {
    let restricted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND (kind='ban' OR until>now()))").bind(channel).bind(&user.id).fetch_one(&app.db).await?;
    if restricted {
        return Err(Fail::denied(
            "You can't clip while banned or timed out on this channel.",
        ));
    }
    let settings: Option<(bool, Vec<String>)> =
        sqlx::query_as("SELECT block_links,banned_words FROM chat_settings WHERE channel_id=$1")
            .bind(channel)
            .fetch_optional(&app.db)
            .await?;
    if let Some((links, words)) = settings {
        check_content(
            title,
            &words,
            links,
            role_of(app, channel, user).await?.is_some(),
        )?;
    }
    Ok(())
}
fn check_content(body: &str, words: &[String], links: bool, exempt: bool) -> Res<()> {
    let folded = fold(body);
    if words.iter().any(|word| folded.contains(word)) {
        return Err(Fail::bad("That message isn't allowed in this chat."));
    }
    if links && !exempt && has_link(body) {
        return Err(Fail::bad("Links aren't allowed in this chat."));
    }
    Ok(())
}

pub async fn view(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (_, role) = actor(&app, &jar, &channel).await?;
    let settings: Option<(i32, bool, Vec<String>)> = sqlx::query_as(
        "SELECT slow_mode_seconds, block_links, banned_words FROM chat_settings WHERE channel_id=$1",
    )
    .bind(&channel)
    .fetch_optional(&app.db)
    .await?;
    let (slow, links, words) = settings.unwrap_or((0, false, vec![]));
    // Only chip_sql with a literal alias is interpolated; channel values are bound.
    let mut moderators: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT {} FROM channel_moderators m JOIN channel_users c ON c.id=m.user_id WHERE m.channel_id=$1 ORDER BY m.appointed_at", profiles::chip_sql("c"))))
        .bind(&channel).fetch_all(&app.db).await?;
    // Only chip_sql with a literal alias is interpolated; channel values are bound.
    let mut restrictions: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('user',{},'kind',r.kind,'until',r.until) FROM channel_restrictions r JOIN channel_users c ON c.id=r.user_id WHERE r.channel_id=$1 AND (r.kind='ban' OR r.until>now()) ORDER BY r.created_at DESC", profiles::chip_sql("c"))))
        .bind(&channel).fetch_all(&app.db).await?;
    moderators
        .iter_mut()
        .chain(restrictions.iter_mut())
        .for_each(|v| profiles::hydrate(&app, v));
    let log: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('action',l.action,'actor_role',l.actor_role,'actor',a.username,'target',t.username,'reason',l.reason,'detail',l.detail,'created_at',l.created_at) FROM channel_moderation_log l LEFT JOIN users a ON a.id=l.actor_id LEFT JOIN users t ON t.id=l.target_id WHERE l.channel_id=$1 ORDER BY l.created_at DESC LIMIT 50")
        .bind(&channel).fetch_all(&app.db).await?;
    // A provisional (spike) window on the live broadcast offers the followers-only prompt.
    let spike: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') AND l.provisional_until>now())")
        .bind(&channel)
        .fetch_one(&app.db)
        .await?;
    Ok(Json(json!({
        "role": role.name(),
        "spike": spike,
        "followers_only_until": followers_only(&app, &channel).await?,
        "settings": {"slow_mode_seconds": slow, "block_links": links, "banned_words": words},
        "moderators": moderators, "restrictions": restrictions, "log": log,
    })))
}

#[derive(Deserialize)]
pub struct Reason {
    reason: String,
}
pub async fn delete_message(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
    Json(input): Json<Reason>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let reason = reason(&input.reason)?;
    let author: String = sqlx::query_scalar(
        "SELECT author_id FROM chat_messages WHERE id=$1 AND channel_id=$2 AND squad_id IS NULL",
    )
    .bind(&id)
    .bind(&channel)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Fail::missing)?;
    if role == Role::Moderator
        && author != user.id
        && protected(&app, &channel, &user.id, &author).await?
    {
        return Err(Fail::denied("You can't delete this message."));
    }
    let mut tx = app.db.begin().await?;
    // Retrying is idempotent: only the first delete is logged and broadcast.
    let deleted =
        sqlx::query("UPDATE chat_messages SET deleted_at=now() WHERE id=$1 AND deleted_at IS NULL")
            .bind(&id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            == 1;
    if deleted {
        log(
            &mut tx,
            &channel,
            &user.id,
            role,
            "delete_message",
            Some(&author),
            Some(&id),
            json!({}),
            &reason,
        )
        .await?;
    }
    tx.commit().await?;
    if deleted {
        app.chat
            .publish(&channel, None, 0, json!({"type":"delete","id":id}));
    }
    Ok(Json(json!({"deleted": true})))
}

#[derive(Deserialize)]
pub struct Restrict {
    username: String,
    kind: String,
    seconds: Option<i64>,
    reason: String,
}
pub async fn restrict(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Restrict>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let reason = reason(&input.reason)?;
    let until = match (input.kind.as_str(), input.seconds) {
        ("timeout", Some(s)) if (60..=1_209_600).contains(&s) => {
            Some(Utc::now() + Duration::seconds(s))
        }
        ("timeout", _) => {
            return Err(Fail::field(
                "seconds",
                "Timeouts last from one minute to 14 days.",
            ));
        }
        ("ban", None) => None,
        _ => return Err(Fail::bad("Choose a timeout or a ban.")),
    };
    let target = user_id(&app, &input.username).await?;
    if protected(&app, &channel, &user.id, &target).await? {
        return Err(Fail::denied("You can't restrict this person here."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO channel_restrictions(channel_id,user_id,kind,until) VALUES($1,$2,$3,$4) ON CONFLICT(channel_id,user_id,kind) DO UPDATE SET until=EXCLUDED.until, created_at=now()")
        .bind(&channel).bind(&target).bind(&input.kind).bind(until).execute(&mut *tx).await?;
    log(
        &mut tx,
        &channel,
        &user.id,
        role,
        &input.kind,
        Some(&target),
        None,
        json!({"seconds": input.seconds}),
        &reason,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"kind": input.kind, "until": until})))
}
pub async fn lift(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, target, kind)): Path<(String, String, String)>,
    Json(input): Json<Reason>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let reason = reason(&input.reason)?;
    let target = user_id(&app, &target).await?;
    let mut tx = app.db.begin().await?;
    let lifted = sqlx::query(
        "DELETE FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND kind=$3",
    )
    .bind(&channel)
    .bind(&target)
    .bind(&kind)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if lifted {
        log(
            &mut tx,
            &channel,
            &user.id,
            role,
            &format!("lift_{kind}"),
            Some(&target),
            None,
            json!({}),
            &reason,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"lifted": lifted})))
}

#[derive(Deserialize)]
pub struct Settings {
    slow_mode_seconds: i32,
    block_links: bool,
    banned_words: Vec<String>,
    reason: String,
}
pub async fn save_settings(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let reason = reason(&input.reason)?;
    if !(input.slow_mode_seconds == 0 || (3..=120).contains(&input.slow_mode_seconds)) {
        return Err(Fail::field(
            "slow_mode_seconds",
            "Slow mode is off, or 3–120 seconds.",
        ));
    }
    let mut words: Vec<String> = input
        .banned_words
        .iter()
        .map(|w| fold(w))
        .filter(|w| !w.is_empty())
        .collect();
    words.sort();
    words.dedup();
    if words.len() > 200 || words.iter().any(|w| w.chars().count() > 64) {
        return Err(Fail::field(
            "banned_words",
            "Use up to 200 banned words or phrases of 1–64 characters.",
        ));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO chat_settings(channel_id,slow_mode_seconds,block_links,banned_words) VALUES($1,$2,$3,$4) ON CONFLICT(channel_id) DO UPDATE SET slow_mode_seconds=$2,block_links=$3,banned_words=$4")
        .bind(&channel).bind(input.slow_mode_seconds).bind(input.block_links).bind(&words).execute(&mut *tx).await?;
    log(&mut tx, &channel, &user.id, role, "settings", None, None, json!({"slow_mode_seconds": input.slow_mode_seconds, "block_links": input.block_links, "banned_words": words.len()}), &reason).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"slow_mode_seconds": input.slow_mode_seconds, "block_links": input.block_links, "banned_words": words}),
    ))
}

#[derive(Deserialize)]
pub struct Appoint {
    username: String,
}
/// Owner only, with a recent sign-in (step-up).
pub async fn appoint(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Appoint>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (tx, user, session) = auth::session(&app, &jar, false).await?;
    tx.commit().await?;
    if user.id != channel {
        return Err(Fail::denied(
            "Only the channel owner can appoint moderators.",
        ));
    }
    auth::recent(&session)?;
    let target: String = sqlx::query_scalar("SELECT id FROM channel_users WHERE lower(username)=lower($1) AND eligible AND email_verified")
        .bind(&input.username).fetch_optional(&app.db).await?
        .ok_or_else(|| Fail::field("username", "Moderators need a verified account in good standing."))?;
    if target == channel {
        return Err(Fail::field(
            "username",
            "You already moderate your own channel.",
        ));
    }
    let mut tx = app.db.begin().await?;
    let added = sqlx::query(
        "INSERT INTO channel_moderators(channel_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(&channel)
    .bind(&target)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if added {
        log(
            &mut tx,
            &channel,
            &user.id,
            Role::Owner,
            "appoint_moderator",
            Some(&target),
            None,
            json!({}),
            "Appointed by the owner",
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"moderator": true})))
}
/// The owner (with step-up) manages appointments; staff may remove one for safety, audited.
pub async fn dismiss(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, target)): Path<(String, String)>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (tx, user, session) = auth::session(&app, &jar, false).await?;
    tx.commit().await?;
    let role = if user.id == channel {
        auth::recent(&session)?;
        Role::Owner
    } else if role_of(&app, &channel, &user).await? == Some(Role::Staff) {
        Role::Staff
    } else {
        return Err(Fail::denied(
            "Only the channel owner can remove moderators.",
        ));
    };
    let target = user_id(&app, &target).await?;
    let mut tx = app.db.begin().await?;
    let removed = sqlx::query("DELETE FROM channel_moderators WHERE channel_id=$1 AND user_id=$2")
        .bind(&channel)
        .bind(&target)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        == 1;
    if removed {
        log(
            &mut tx,
            &channel,
            &user.id,
            role,
            "remove_moderator",
            Some(&target),
            None,
            json!({}),
            "Removed",
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"moderator": false})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/chat/moderation", get(view))
        .route("/api/channels/{username}/chat/settings", put(save_settings))
        .route("/api/channels/{username}/chat/protect", put(protect))
        .route(
            "/api/channels/{username}/chat/subs-only",
            put(set_subs_only),
        )
        .route(
            "/api/channels/{username}/chat/messages/{id}",
            delete(delete_message),
        )
        .route("/api/channels/{username}/chat/restrictions", post(restrict))
        .route(
            "/api/channels/{username}/chat/restrictions/{target}/{kind}",
            delete(lift),
        )
        .route("/api/channels/{username}/moderators", post(appoint))
        .route(
            "/api/channels/{username}/moderators/{target}",
            delete(dismiss),
        )
}

#[cfg(test)]
mod tests {
    use super::{fold, has_link};

    #[test]
    fn links_and_phrases() {
        for link in [
            "example.com",
            "Visit WWW.Example.org now",
            "https://x",
            "(sub.domain.co.uk/path)",
            "EVIL.IO!",
        ] {
            assert!(has_link(link), "{link}");
        }
        for text in ["e.g. this", "pi is 3.14", "hello there", "wait...", "a.b"] {
            assert!(!has_link(text), "{text}");
        }
        assert_eq!(fold("  Bad   WORD\n here "), "bad word here");
    }
}
