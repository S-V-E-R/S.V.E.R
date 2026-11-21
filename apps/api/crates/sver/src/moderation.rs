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

/// A log row `l`'s chat evidence as the team reads it: without each line's author account ID.
macro_rules! evidence_shown {
    () => {
        "coalesce((SELECT jsonb_agg(e-'author' ORDER BY i) FROM jsonb_array_elements(l.detail->'chat') WITH ORDINALITY t(e,i)),'[]')"
    };
}

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
    if user.mfa_enabled && safety::is_staff(&mut *app.db.acquire().await?, &user.id).await? {
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
/// `actor` for an app-callable endpoint: the person from a bearer token (with `scope`) or cookie.
pub(crate) async fn actor_any(
    app: &App,
    headers: &axum::http::HeaderMap,
    jar: &CookieJar,
    channel: &str,
    scope: &str,
) -> Res<(auth::User, Role)> {
    let user = crate::devapps::actor(app, headers, jar, scope).await?;
    let role = role_of(app, channel, &user)
        .await?
        .ok_or_else(|| Fail::denied("You can't moderate this channel."))?;
    // An app acts for the owner or an appointed moderator only; staff moderate other channels from
    // their own signed-in session, never through a token they gave an app.
    if headers.contains_key("authorization") && role == Role::Staff {
        return Err(Fail::denied("You can't moderate this channel."));
    }
    Ok((user, role))
}
pub(crate) async fn channel(app: &App, name: &str) -> Res<String> {
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
/// Only timeouts and bans need a reason (Joe, October 12, 2026); elsewhere a blank one is fine.
pub(crate) fn optional_reason(text: &str, default: &str) -> Res<String> {
    if text.trim().is_empty() {
        return Ok(default.into());
    }
    reason(text)
}
pub(crate) fn delete_reason(text: &str) -> Res<String> {
    optional_reason(text, "Message deleted")
}
/// What a timeout or ban given without a reason is logged with until it's reviewed.
pub(crate) const NO_REASON_YET: &str = "No reason given yet";
/// When spike protection's followers-only chat ends, if it's on now.
pub async fn followers_only(app: &App, channel: &str) -> Res<Option<DateTime<Utc>>> {
    Ok(sqlx::query_scalar("SELECT followers_only_until FROM chat_settings WHERE channel_id=$1 AND followers_only_until>now()")
        .bind(channel)
        .fetch_optional(&app.db)
        .await?)
}

/// Floors shield mode puts on chat for everyone without a channel role (docs/LIVE_STREAMS.md
/// "Shield mode"): slow mode of at least this many seconds and accounts at least this many days old.
pub const SHIELD_SLOW_SECONDS: i32 = 10;
pub const SHIELD_ACCOUNT_DAYS: i16 = 7;
/// How long shield mode can run, in minutes; it always ends by itself.
const SHIELD_MINUTES: [i32; 3] = [10, 30, 60];

#[derive(Deserialize)]
pub struct Protect {
    on: bool,
    #[serde(default)]
    minutes: Option<i32>,
}
/// PUT /api/channels/{username}/chat/protect: shield mode, also the one-click spike prompt. Owners
/// and moderators raise it for 10, 30 or 60 minutes (it always ends by itself) or end it early.
/// While it's up: followers-only chat, links blocked, slow mode and account age at least the
/// shield floors, and incoming raids refused (`viewer_send_refusal`, migration 0153).
pub async fn protect(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Protect>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let minutes = input.minutes.unwrap_or(SHIELD_MINUTES[0]);
    if !SHIELD_MINUTES.contains(&minutes) {
        return Err(Fail::field(
            "minutes",
            "Shield mode runs for 10, 30 or 60 minutes.",
        ));
    }
    let mut tx = app.db.begin().await?;
    let until: Option<DateTime<Utc>> = sqlx::query_scalar("INSERT INTO chat_settings(channel_id,followers_only_until) VALUES($1,CASE WHEN $2 THEN now()+make_interval(mins=>$3) END) ON CONFLICT(channel_id) DO UPDATE SET followers_only_until=EXCLUDED.followers_only_until RETURNING followers_only_until")
        .bind(&channel)
        .bind(input.on)
        .bind(minutes)
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
        json!({"until": until, "minutes": input.on.then_some(minutes)}),
        "Shield mode",
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
) -> Res<String> {
    let id = profiles::new_id();
    sqlx::query("INSERT INTO channel_moderation_log(id,channel_id,actor_id,actor_role,action,target_id,message_id,detail,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(&id).bind(channel).bind(actor).bind(role.name()).bind(action).bind(target).bind(message).bind(detail).bind(reason)
        .execute(&mut *db).await?;
    // Live events: the channel's private moderation topic, by username (never internal IDs).
    let (by, user): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT (SELECT username FROM users WHERE id=$1),(SELECT username FROM users WHERE id=$2)",
    )
    .bind(actor)
    .bind(target)
    .fetch_one(&mut *db)
    .await?;
    crate::events::emit(
        db,
        channel,
        "moderation",
        json!({"action": action, "by": by, "role": role.name(), "user": user, "message": message, "reason": reason}),
    )
    .await?;
    Ok(id)
}
/// Logs a timeout or ban with evidence for the team: the chat leading up to it and the stream's
/// last minute. One given without a reason (`review`) waits for review. Run it before the
/// person's lines are cleared, so the evidence still shows them.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn log_restriction(
    db: &mut PgConnection,
    channel: &str,
    actor: &str,
    role: Role,
    kind: &str,
    target: &str,
    detail: Value,
    reason: &str,
    review: bool,
) -> Res<()> {
    let entry = log(
        db,
        channel,
        actor,
        role,
        kind,
        Some(target),
        None,
        detail,
        reason,
    )
    .await?;
    let chat = evidence(db, channel).await?;
    let clip = crate::videos::evidence(db, channel, &entry).await?;
    sqlx::query("UPDATE channel_moderation_log SET needs_review=$2, detail=detail||jsonb_build_object('chat',$3::jsonb,'clip',$4::text) WHERE id=$1")
        .bind(&entry).bind(review).bind(json!(chat)).bind(&clip).execute(db).await?;
    Ok(())
}

/// Case-folded, whitespace-normalized text for banned-phrase matching (`text::unmasked`).
fn fold(text: &str) -> String {
    crate::text::unmasked(text)
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
/// Plain-text URL/domain detection, including mixed case, `www.` and scheme-less forms. Never
/// fetches anything and is not a complete obfuscation detector.
fn has_link(body: &str) -> bool {
    crate::text::unmasked(body)
        .to_lowercase()
        .split_whitespace()
        .any(|token| {
            let token = token.trim_matches(|c: char| {
                matches!(
                    c,
                    '(' | ')'
                        | '['
                        | ']'
                        | '<'
                        | '>'
                        | '"'
                        | '\''
                        | ','
                        | '!'
                        | '?'
                        | ':'
                        | ';'
                        | '.'
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
    if let Some((links, words)) = rules
        && !code_allowed(&words, links, code)
    {
        return Err(Fail::bad("That code isn't allowed in this chat."));
    }
    Ok(())
}
/// Whether an emote code or a name passes a channel's banned words and link rule. Codes and names
/// glue words together ("SlurWordFan"), so a banned word matches inside them, wildcards aside.
pub fn code_allowed(words: &[String], links: bool, code: &str) -> bool {
    let folded = fold(code);
    !(words.iter().any(|w| {
        let w = w.trim_matches('*');
        !w.is_empty() && folded.contains(w)
    }) || (links && has_link(code)))
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
/// A channel ban or an active timeout; shared by chat and board presses (docs/CROWDSYNC.md).
pub(crate) async fn check_restriction(app: &App, channel: &str, user: &str) -> Res<()> {
    let restriction: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as("SELECT kind, until FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND (kind='ban' OR until>now()) ORDER BY kind='ban' DESC LIMIT 1")
        .bind(channel).bind(user).fetch_optional(&app.db).await?;
    match restriction {
        Some((kind, _)) if kind == "ban" => Err(Fail::denied("You're banned from this chat.")),
        Some((_, Some(until))) => Err(Fail {
            retry: Some((until - Utc::now()).num_seconds().max(1)),
            ..Fail::denied("You're timed out in this chat.")
        }),
        _ => Ok(()),
    }
}
/// The chat rules one send checks, with what they need to know about the sender: one query, with
/// defaults when the channel never saved settings.
#[derive(sqlx::FromRow)]
struct SendRules {
    slow: i32,
    links: bool,
    words: Vec<String>,
    min_days: i16,
    min_loyalty: i16,
    subs_only: bool,
    /// Shield mode (followers-only chat) is up.
    shield: bool,
    earned: i64,
    joined: Option<DateTime<Utc>>,
}
async fn send_rules(app: &App, channel: &str, user: &str) -> Res<SendRules> {
    Ok(sqlx::query_as("SELECT coalesce(s.slow_mode_seconds,0) AS slow, coalesce(s.block_links,false) AS links, coalesce(s.banned_words,'{}') AS words,
        coalesce(s.min_account_days,0::smallint) AS min_days, coalesce(s.min_loyalty,0::smallint) AS min_loyalty, coalesce(s.subs_only,false) AS subs_only,
        coalesce(s.followers_only_until>now(),false) AS shield,
        coalesce((SELECT earned FROM engagement WHERE channel_id=$1 AND user_id=$2),0) AS earned, (SELECT created_at FROM users WHERE id=$2) AS joined
        FROM (SELECT 1) x LEFT JOIN chat_settings s ON s.channel_id=$1")
        .bind(channel)
        .bind(user)
        .fetch_one(&app.db)
        .await?)
}
/// The sender's channel role, read the first time a rule needs it and then reused.
struct LazyRole<'a> {
    app: &'a App,
    channel: &'a str,
    user: &'a auth::User,
    role: Option<Option<Role>>,
}
impl LazyRole<'_> {
    async fn get(&mut self) -> Res<Option<Role>> {
        if let Some(role) = self.role {
            return Ok(role);
        }
        let role = role_of(self.app, self.channel, self.user).await?;
        self.role = Some(role);
        Ok(role)
    }
}
/// Banned words (no role exemption) and the link rule (channel roles exempt), then the slow-mode
/// seconds that apply. Shield mode raises the link block and slow mode to its floors.
fn words_and_slow(rules: &SendRules, body: &str, exempt: bool) -> Res<i32> {
    check_content(body, &rules.words, rules.links || rules.shield, exempt)?;
    Ok(match (exempt, rules.shield) {
        (true, _) => 0,
        (false, true) => rules.slow.max(SHIELD_SLOW_SECONDS),
        (false, false) => rules.slow,
    })
}
/// `words_and_slow` for board text inputs (docs/CROWDSYNC.md).
pub(crate) async fn check_words(
    app: &App,
    channel: &str,
    user: &auth::User,
    body: &str,
) -> Res<()> {
    let rules = send_rules(app, channel, &user.id).await?;
    let exempt = role_of(app, channel, user).await?.is_some();
    words_and_slow(&rules, body, exempt).map(drop)
}
/// Whether the person may chat here at all, whatever they say: no ban or timeout, and past shield
/// mode's followers-only gate, the minimum account age and the chat rank gate (channel roles are
/// exempt from all but the first). Also gates what else needs chat standing, like emote reports.
async fn standing<'a>(
    app: &'a App,
    channel: &'a str,
    user: &'a auth::User,
) -> Res<(SendRules, LazyRole<'a>)> {
    check_restriction(app, channel, &user.id).await?;
    let rules = send_rules(app, channel, &user.id).await?;
    let mut role = LazyRole {
        app,
        channel,
        user,
        role: None,
    };
    // Shield mode (and the spike prompt): while it is up, only people who followed at least 10
    // minutes ago (and channel roles) can chat, so a burst of new accounts can't flood it.
    if rules.shield && role.get().await?.is_none() {
        let follower: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2 AND created_at<=now()-interval '10 minutes')")
            .bind(&user.id)
            .bind(channel)
            .fetch_one(&app.db)
            .await?;
        if !follower {
            return Err(Fail::denied(
                "Shield mode is on: for a few minutes only followers of at least 10 minutes can chat.",
            ));
        }
    }
    // Minimum account age (docs/LIVE_STREAMS.md "Hate-raid defenses").
    let min_days = if rules.shield {
        rules.min_days.max(SHIELD_ACCOUNT_DAYS)
    } else {
        rules.min_days
    };
    let new = rules
        .joined
        .is_some_and(|at| at > Utc::now() - Duration::days(min_days.into()));
    if min_days > 0 && new && role.get().await?.is_none() {
        return Err(Fail::denied(format!(
            "Chat here needs an account at least {min_days} day{} old.",
            if min_days == 1 { "" } else { "s" }
        )));
    }
    // The chat rank gate (docs/PROGRESSION.md section 5).
    if crate::progression::loyalty(rules.earned) < rules.min_loyalty && role.get().await?.is_none()
    {
        return Err(Fail::denied(format!(
            "Chat here needs the {} loyalty rank. Watch and chat on this channel to earn it.",
            crate::progression::LOYALTY[rules.min_loyalty as usize].0
        )));
    }
    Ok((rules, role))
}
/// `standing` on its own.
pub(crate) async fn check_standing(app: &App, channel: &str, user: &auth::User) -> Res<()> {
    standing(app, channel, user).await.map(drop)
}
/// What a message that passed `check_send` still needs: the sender's channel role, and the
/// slow-mode seconds to reserve with `reserve_slow` once every other check has passed.
pub struct Checked {
    pub role: Option<Role>,
    pub slow: i32,
}
pub async fn check_send(app: &App, channel: &str, user: &auth::User, body: &str) -> Res<Checked> {
    let (rules, mut role) = standing(app, channel, user).await?;
    // Subscriber-only chat and subscriber emotes; channel roles are exempt.
    let tier = crate::subs::active_tier(app, channel, &user.id).await?;
    let tokens: Vec<&str> = body.split_whitespace().collect();
    let locked: Option<i16> = sqlx::query_scalar("SELECT max(tier) FROM channel_emotes WHERE channel_id=$1 AND status='VISIBLE' AND tier>coalesce($2,0::smallint) AND code=ANY($3)")
        .bind(channel).bind(tier).bind(&tokens).fetch_one(&app.db).await?;
    if (locked.is_some() || (tier.is_none() && rules.subs_only)) && role.get().await?.is_none() {
        return Err(match locked {
            Some(t) => Fail::bad(format!("That emote is for Tier {t} subscribers.")),
            None => Fail::denied("Chat is subscribers-only right now."),
        });
    }
    // AutoMod (caps, repeats, spam) with the channel's warn-and-timeout ladder; roles exempt.
    crate::bot::automod(app, channel, user, body).await?;
    let role = role.get().await?;
    let slow = words_and_slow(&rules, body, role.is_some())?;
    Ok(Checked { role, slow })
}
/// Takes the sender's slow-mode turn in each channel (`(channel, seconds)`), all or none. Call it
/// after every other check, and give the turns back with `sec::release` if the send still fails.
pub(crate) async fn reserve_slow(
    app: &App,
    user: &str,
    slows: &[(&str, i32)],
) -> Res<Vec<(String, DateTime<Utc>)>> {
    let mut reserved = Vec::new();
    for (channel, slow) in slows.iter().filter(|s| s.1 > 0) {
        match sec::reserve(
            app,
            vec![format!("chat-slow:{channel}:{user}")],
            1,
            (*slow).into(),
        )
        .await
        {
            Ok(turn) => reserved.extend(turn),
            Err(e) => {
                let _ = sec::release(app, reserved).await;
                return Err(Fail {
                    message: "Slow mode is on. You can send again shortly.".into(),
                    ..Fail::from(e)
                });
            }
        }
    }
    Ok(reserved)
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
/// `text` as words for banned-word matching: compatibility forms and leetspeak folded ("a$$" and
/// "h3ll0" read as "ass" and "hello"), split at anything that isn't a letter or digit (or `*` in a
/// pattern).
fn words_of(text: &str, pattern: bool) -> Vec<String> {
    let folded = crate::text::unmasked(text).to_lowercase();
    let mut words = Vec::new();
    for token in folded.split_whitespace() {
        let token: String = token
            .chars()
            .map(|c| match c {
                '0' => 'o',
                '1' => 'i',
                '3' => 'e',
                '4' | '@' => 'a',
                '5' | '$' => 's',
                '7' => 't',
                c => c,
            })
            .collect();
        words.extend(
            token
                .split(|c: char| !(c.is_alphanumeric() || (pattern && c == '*')))
                .filter(|w| !w.is_empty())
                .map(str::to_owned),
        );
    }
    words
}
/// `words` with each run of 3 or more single letters joined, so "f u c k" and "f.u.c.k" read as
/// one word.
fn joined(words: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut run: Vec<&String> = Vec::new();
    let flush = |run: &mut Vec<&String>, out: &mut Vec<String>| {
        if run.len() >= 3 {
            out.push(run.iter().map(|s| s.as_str()).collect());
        } else {
            out.extend(run.iter().map(|s| (*s).clone()));
        }
        run.clear();
    };
    for word in words {
        if word.chars().count() == 1 {
            run.push(word);
        } else {
            flush(&mut run, &mut out);
            out.push(word.clone());
        }
    }
    flush(&mut run, &mut out);
    out
}
/// One banned word against one message word: `*` at either end is a wildcard ("ass*" is
/// "assassin"); otherwise the whole word, also when stretched ("fuuuck"), never shortened.
fn word_matches(pattern: &str, word: &str) -> bool {
    let squeeze = |s: &str| {
        let mut out: Vec<char> = s.chars().collect();
        out.dedup();
        out
    };
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(rest), Some(_)) => word.contains(rest.trim_end_matches('*')),
        (None, Some(stem)) => word.starts_with(stem),
        (Some(stem), None) => word.ends_with(stem),
        (None, None) => {
            pattern == word || (word.len() >= pattern.len() && squeeze(pattern) == squeeze(word))
        }
    }
}
/// Whether `body` says one of a channel's banned words or phrases, as whole words. A pattern
/// with no letters or digits (an emoji, "!!!") is matched anywhere in the text.
pub(crate) fn says_banned(body: &str, banned: &[String]) -> bool {
    let plain = words_of(body, false);
    let readings = [joined(&plain), plain];
    banned.iter().any(|entry| {
        let pattern: Vec<String> = words_of(entry, true)
            .into_iter()
            .filter(|w| w.chars().any(char::is_alphanumeric))
            .collect();
        if pattern.is_empty() {
            return !entry.trim().is_empty() && fold(body).contains(entry.as_str());
        }
        readings.iter().any(|words| {
            words.windows(pattern.len()).any(|at| {
                at.iter()
                    .zip(&pattern)
                    .all(|(word, p)| word_matches(p, word))
            })
        })
    })
}
pub(crate) fn check_content(body: &str, words: &[String], links: bool, exempt: bool) -> Res<()> {
    if says_banned(body, words) {
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
    let (me, role) = actor(&app, &jar, &channel).await?;
    let settings: Option<(i32, bool, Vec<String>, i16, bool, i16)> = sqlx::query_as(
        "SELECT slow_mode_seconds, block_links, banned_words, overlay_fade_seconds, allow_signatures, min_loyalty FROM chat_settings WHERE channel_id=$1",
    )
    .bind(&channel)
    .fetch_optional(&app.db)
    .await?;
    let (slow, links, words, fade, signatures, min_loyalty) =
        settings.unwrap_or((0, false, vec![], 30, true, 0));
    let min_account_days: i16 = sqlx::query_scalar(
        "SELECT coalesce((SELECT min_account_days FROM chat_settings WHERE channel_id=$1),0::smallint)",
    )
    .bind(&channel)
    .fetch_one(&app.db)
    .await?;
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
    // Timeouts and bans waiting for a reason: a moderator sees their own, the owner and staff all
    // (`theirs` flags the ones another moderator left).
    let reviews: Vec<Value> = sqlx::query_scalar(concat!("SELECT jsonb_build_object('id',l.id,'action',l.action,'actor',a.username,'target',t.username,'created_at',l.created_at,'chat',", evidence_shown!(), ",'clip',l.detail->>'clip','theirs',l.actor_id<>$2) FROM channel_moderation_log l LEFT JOIN users a ON a.id=l.actor_id LEFT JOIN users t ON t.id=l.target_id WHERE l.channel_id=$1 AND l.needs_review AND ($3 OR l.actor_id=$2) ORDER BY l.created_at"))
        .bind(&channel).bind(&me.id).bind(role != Role::Moderator).fetch_all(&app.db).await?;
    let log: Vec<Value> = sqlx::query_scalar(concat!("SELECT jsonb_build_object('id',l.id,'action',l.action,'actor_role',l.actor_role,'actor',a.username,'target',t.username,'reason',l.reason,'detail',CASE WHEN l.detail ? 'chat' THEN jsonb_set(l.detail,'{chat}',", evidence_shown!(), ") ELSE l.detail END,'created_at',l.created_at) FROM channel_moderation_log l LEFT JOIN users a ON a.id=l.actor_id LEFT JOIN users t ON t.id=l.target_id WHERE l.channel_id=$1 ORDER BY l.created_at DESC LIMIT 50"))
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
        "settings": {"slow_mode_seconds": slow, "block_links": links, "banned_words": words, "overlay_fade_seconds": fade, "allow_signatures": signatures, "min_loyalty": min_loyalty, "min_account_days": min_account_days, "gifs": crate::chat::gif_setting(&app, &channel).await?},
        "moderators": moderators, "restrictions": restrictions, "log": log, "reviews": reviews,
    })))
}

#[derive(Deserialize)]
pub struct Reason {
    #[serde(default)]
    reason: String,
}
pub async fn delete_message(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
    Json(input): Json<Reason>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor_any(&app, &headers, &jar, &channel, "channel:moderate").await?;
    let reason = delete_reason(&input.reason)?;
    let (author, origin): (String, Option<String>) = sqlx::query_as(
        "SELECT author_id,origin FROM chat_messages WHERE id=$1 AND channel_id=$2 AND squad_id IS NULL",
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
        crate::chat::publish_delete(&app, Some(&channel), origin.as_deref(), &id);
    }
    Ok(Json(json!({"deleted": true})))
}

#[derive(Deserialize)]
pub struct Restrict {
    username: String,
    kind: String,
    seconds: Option<i64>,
    #[serde(default)]
    reason: String,
}
pub async fn restrict(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Restrict>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor_any(&app, &headers, &jar, &channel, "channel:moderate").await?;
    // Act first, review later: without a reason it takes effect now and waits for review.
    let review = input.reason.trim().is_empty();
    let reason = optional_reason(&input.reason, NO_REASON_YET)?;
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
    log_restriction(
        &mut tx,
        &channel,
        &user.id,
        role,
        &input.kind,
        &target,
        json!({"seconds": input.seconds}),
        &reason,
        review,
    )
    .await?;
    // After the evidence, which still shows them: their recent lines leave every chat.
    let cleared = clear_recent(&mut tx, &channel, &target).await?;
    tx.commit().await?;
    if input.kind == "ban" {
        // Shared chats this channel is in re-check their viewers' access.
        app.chat.access_changed();
    }
    publish_cleared(&app, cleared);
    Ok(Json(
        json!({"kind": input.kind, "until": until, "review": review}),
    ))
}
/// The chat a moderation action can see in `$1`'s channel: its own room and, while it's in a
/// co-stream, the shared room (whose lines carry the host's channel ID). Rows are aliased `m`.
macro_rules! rooms_of {
    () => {
        "((m.channel_id=$1 AND m.squad_id IS NULL) OR m.squad_id IN (SELECT squad_id FROM squad_members WHERE user_id=$1))"
    };
}
/// Evidence for the team: up to 30 lines of the last 10 minutes of the channel's chat. Each keeps
/// its author's account ID, so erasing that account removes exactly its lines (names get reused);
/// `evidence_shown!` leaves it out of what the team reads.
async fn evidence(db: &mut PgConnection, channel: &str) -> Res<Vec<Value>> {
    Ok(sqlx::query_scalar(concat!("SELECT jsonb_build_object('user',u.username,'author',m.author_id,'body',m.body,'at',m.created_at,'deleted',m.deleted_at IS NOT NULL) FROM (SELECT m.* FROM chat_messages m WHERE ", rooms_of!(), " AND m.created_at>now()-interval '10 minutes' ORDER BY m.created_at DESC LIMIT 30) m JOIN users u ON u.id=m.author_id ORDER BY m.created_at"))
        .bind(channel).fetch_all(db).await?)
}
/// Messages to tell open chats about: each one's room, Hype lane and ID.
pub(crate) type Cleared = Vec<(String, Option<String>, String)>;
/// A timeout or ban removes the person's lines from the last 10 minutes of the channel's chat
/// (and its shared co-stream room, unless they're protected there: a member streamer, a member's
/// moderator or staff, as the shared chat's own restrict refuses). Publish the result after
/// commit with `publish_cleared`.
pub(crate) async fn clear_recent(
    db: &mut PgConnection,
    channel: &str,
    target: &str,
) -> Res<Cleared> {
    Ok(sqlx::query_as(concat!("UPDATE chat_messages m SET deleted_at=now() WHERE ", rooms_of!(), " AND m.author_id=$2 AND m.deleted_at IS NULL AND m.created_at>now()-interval '10 minutes'
        AND (m.squad_id IS NULL OR NOT (EXISTS(SELECT 1 FROM squad_members q WHERE q.squad_id=m.squad_id AND (q.user_id=$2 OR EXISTS(SELECT 1 FROM channel_moderators d WHERE d.channel_id=q.user_id AND d.user_id=$2))) OR EXISTS(SELECT 1 FROM staff_roles WHERE user_id=$2)))
        RETURNING coalesce(m.squad_id,m.channel_id),m.origin,m.id"))
        .bind(channel).bind(target).fetch_all(db).await?)
}
/// Chat, pop-outs, docks, the overlay and MAGNet drop each removed line.
pub(crate) fn publish_cleared(app: &App, cleared: Cleared) {
    for (room, origin, id) in cleared {
        crate::chat::publish_delete(app, Some(&room), origin.as_deref(), &id);
    }
}
#[derive(Deserialize)]
pub struct Sweep {
    /// Chatted or followed here within the last `minutes` (1–60).
    minutes: i32,
    /// Accounts younger than this many days (1–90).
    account_days: i32,
    #[serde(default)]
    reason: String,
}
/// Most accounts one sweep bans.
const SWEEP_LIMIT: i64 = 500;
/// POST /api/channels/{u}/chat/sweep: after a raid, bans every account younger than
/// `account_days` that chatted or followed here in the last `minutes`, and removes their follows
/// and those messages (docs/LIVE_STREAMS.md "Hate-raid defenses"). Channel roles, staff and
/// people already banned are left alone. One evidence capture covers the sweep; each ban is logged
/// and reviewed like any other, so undoing one lifts it.
pub async fn sweep(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Sweep>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    if !(1..=60).contains(&input.minutes) {
        return Err(Fail::field("minutes", "Look back 1–60 minutes."));
    }
    if !(1..=90).contains(&input.account_days) {
        return Err(Fail::field("account_days", "Choose 1–90 days."));
    }
    let review = input.reason.trim().is_empty();
    let reason = optional_reason(&input.reason, NO_REASON_YET)?;
    let mut tx = app.db.begin().await?;
    // Co-stream partners and their moderators are as protected as this channel's own.
    let targets: Vec<String> = sqlx::query_scalar(concat!("WITH partners AS (SELECT user_id FROM squad_members WHERE squad_id IN (SELECT squad_id FROM squad_members WHERE user_id=$1))
        SELECT u.id FROM users u WHERE u.created_at>now()-make_interval(days=>$3)
        AND (EXISTS(SELECT 1 FROM chat_messages m WHERE ", rooms_of!(), " AND m.author_id=u.id AND m.created_at>now()-make_interval(mins=>$2))
          OR EXISTS(SELECT 1 FROM follows f WHERE f.following_id=$1 AND f.follower_id=u.id AND f.created_at>now()-make_interval(mins=>$2)))
        AND u.id<>$1 AND u.id<>$4 AND u.id NOT IN (SELECT user_id FROM partners)
        AND NOT EXISTS(SELECT 1 FROM channel_moderators WHERE (channel_id=$1 OR channel_id IN (SELECT user_id FROM partners)) AND user_id=u.id)
        AND NOT EXISTS(SELECT 1 FROM staff_roles WHERE user_id=u.id)
        AND NOT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=$1 AND user_id=u.id AND kind='ban')
        ORDER BY u.id LIMIT $5"))
        .bind(&channel).bind(input.minutes).bind(input.account_days).bind(&user.id).bind(SWEEP_LIMIT)
        .fetch_all(&mut *tx).await?;
    if targets.is_empty() {
        return Ok(Json(json!({"banned": 0, "review": false})));
    }
    let sweep = profiles::new_id();
    for target in &targets {
        sqlx::query("INSERT INTO channel_restrictions(channel_id,user_id,kind,until) VALUES($1,$2,'ban',NULL) ON CONFLICT(channel_id,user_id,kind) DO UPDATE SET until=NULL, created_at=now()")
            .bind(&channel).bind(target).execute(&mut *tx).await?;
        log(
            &mut tx,
            &channel,
            &user.id,
            role,
            "ban",
            Some(target),
            None,
            json!({"sweep": sweep, "minutes": input.minutes, "account_days": input.account_days}),
            &reason,
        )
        .await?;
    }
    // Evidence before the messages go: the chat leading up to it and the stream's last minute.
    let chat = evidence(&mut tx, &channel).await?;
    let clip = crate::videos::evidence(&mut tx, &channel, &sweep).await?;
    sqlx::query("UPDATE channel_moderation_log SET needs_review=$2, detail=detail||jsonb_build_object('chat',$3::jsonb,'clip',$4::text) WHERE channel_id=$1 AND detail->>'sweep'=$5")
        .bind(&channel).bind(review).bind(json!(chat)).bind(&clip).bind(&sweep).execute(&mut *tx).await?;
    let removed: Cleared = sqlx::query_as(concat!("UPDATE chat_messages m SET deleted_at=now() WHERE ", rooms_of!(), " AND m.author_id=ANY($2) AND m.deleted_at IS NULL AND m.created_at>now()-make_interval(mins=>$3) RETURNING coalesce(m.squad_id,m.channel_id),m.origin,m.id"))
        .bind(&channel).bind(&targets).bind(input.minutes).fetch_all(&mut *tx).await?;
    sqlx::query("DELETE FROM follows WHERE following_id=$1 AND follower_id=ANY($2)")
        .bind(&channel)
        .bind(&targets)
        .execute(&mut *tx)
        .await?;
    let mut counted: Vec<&str> = targets.iter().map(String::as_str).collect();
    counted.push(&channel);
    crate::social::refresh_counts(&mut tx, &counted).await?;
    tx.commit().await?;
    app.chat.access_changed();
    publish_cleared(&app, removed);
    Ok(Json(json!({"banned": targets.len(), "review": review})))
}
#[derive(Deserialize)]
pub struct Review {
    keep: bool,
    #[serde(default)]
    reason: String,
}
/// POST /api/channels/{u}/chat/reviews/{id}: keep a timeout or ban with a reason, or undo it.
/// The moderator who gave it reviews it; the owner and staff can review anyone's.
pub async fn review(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
    Json(input): Json<Review>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let reason = if input.keep {
        Some(reason(&input.reason)?)
    } else {
        None
    };
    let mut tx = app.db.begin().await?;
    let (actor_id, target, kind, squad): (String, Option<String>, String, Option<String>) = sqlx::query_as("SELECT actor_id,target_id,action,detail->>'squad' FROM channel_moderation_log WHERE id=$1 AND channel_id=$2 AND needs_review FOR UPDATE")
        .bind(&id).bind(&channel).fetch_optional(&mut *tx).await?.ok_or_else(Fail::missing)?;
    if role == Role::Moderator && actor_id != user.id {
        return Err(Fail::denied(
            "Only the moderator who gave it, or the owner, can review this.",
        ));
    }
    // A co-stream restriction is logged in every member's channel at once; reviewing one reviews
    // all, while the reviewing channel is still in that shared chat.
    sqlx::query("UPDATE channel_moderation_log l SET needs_review=false, reason=coalesce($2,l.reason) FROM channel_moderation_log e WHERE e.id=$1 AND (l.id=e.id OR (e.detail->>'squad' IS NOT NULL AND l.detail->>'squad'=e.detail->>'squad' AND l.created_at=e.created_at AND l.target_id=e.target_id AND l.action=e.action AND EXISTS(SELECT 1 FROM squad_members s WHERE s.squad_id=e.detail->>'squad' AND s.user_id=e.channel_id)))")
        .bind(&id).bind(&reason).execute(&mut *tx).await?;
    // Undo lifts only this action's restriction, never a newer one someone gave since (a shared
    // chat's has no time of its own, so its end must still match, and only a channel still in
    // that shared chat can lift it).
    let mut lifted = false;
    if !input.keep {
        let undo = if squad.is_some() {
            "DELETE FROM squad_restrictions WHERE squad_id=$1 AND user_id=$2 AND kind=$3 AND until IS NOT DISTINCT FROM (SELECT (detail->>'until')::timestamptz FROM channel_moderation_log WHERE id=$4)
             AND EXISTS(SELECT 1 FROM squad_members s JOIN channel_moderation_log l ON l.channel_id=s.user_id WHERE s.squad_id=$1 AND l.id=$4)"
        } else {
            "DELETE FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND kind=$3 AND created_at<=(SELECT created_at FROM channel_moderation_log WHERE id=$4)"
        };
        lifted = sqlx::query(undo)
            .bind(squad.as_ref().unwrap_or(&channel))
            .bind(&target)
            .bind(&kind)
            .bind(&id)
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
                target.as_deref(),
                None,
                json!({"squad": squad}),
                "Undone on review",
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(Json(
        json!({"reviewed": true, "lifted": (!input.keep).then_some(lifted)}),
    ))
}
/// When a stream ends, everyone with timeouts or bans still waiting for a reason gets one
/// notification pointing at the review list (called by streams in its end transaction).
pub async fn stream_ended(db: &mut PgConnection, channel: &str) -> Res<()> {
    sqlx::query("INSERT INTO notifications(id,user_id,kind,channel_id,event_key,payload) SELECT gen_random_uuid()::text,l.actor_id,'mod_review',$1,'mod_review:'||$1||':'||extract(epoch FROM now())::bigint,jsonb_build_object('title','Review your timeouts and bans','body',count(*)||' from the stream on '||c.display_name||' need a reason, or you can undo them.','url','/studio/chat?channel='||c.username) FROM channel_moderation_log l JOIN channel_users c ON c.id=l.channel_id WHERE l.channel_id=$1 AND l.needs_review GROUP BY l.actor_id,c.display_name,c.username ON CONFLICT DO NOTHING")
        .bind(channel).execute(db).await?;
    Ok(())
}
pub async fn lift(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, target, kind)): Path<(String, String, String)>,
    Json(input): Json<Reason>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let reason = optional_reason(&input.reason, "Lifted")?;
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
    // Co-stream restrictions are lifted from the shared chat, not here.
    sqlx::query("UPDATE channel_moderation_log SET needs_review=false WHERE channel_id=$1 AND target_id=$2 AND action=$3 AND needs_review AND detail->>'squad' IS NULL")
        .bind(&channel).bind(&target).bind(&kind).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"lifted": lifted})))
}

#[derive(Deserialize)]
pub struct Settings {
    slow_mode_seconds: i32,
    block_links: bool,
    banned_words: Vec<String>,
    #[serde(default)]
    reason: String,
    /// The OBS chat overlay's message lifetime (10–120 seconds).
    #[serde(default)]
    overlay_fade_seconds: Option<i16>,
    /// Other channels' signature emotes render in this chat (default on).
    #[serde(default)]
    allow_signatures: Option<bool>,
    /// The chat rank gate: the loyalty rank (0 off, 1-4) needed to chat.
    #[serde(default)]
    min_loyalty: Option<i16>,
    /// Who can send GIFs: off, everyone, followers or subscribers.
    #[serde(default)]
    gifs: Option<String>,
    /// Minimum account age to chat, in days (0 off, up to 90).
    #[serde(default)]
    min_account_days: Option<i16>,
}
pub async fn save_settings(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = actor(&app, &jar, &channel).await?;
    let reason = optional_reason(&input.reason, "Chat rules changed")?;
    if !(input.slow_mode_seconds == 0 || (3..=120).contains(&input.slow_mode_seconds)) {
        return Err(Fail::field(
            "slow_mode_seconds",
            "Slow mode is off, or 3–120 seconds.",
        ));
    }
    if input
        .overlay_fade_seconds
        .is_some_and(|s| !(10..=120).contains(&s))
    {
        return Err(Fail::field(
            "overlay_fade_seconds",
            "Overlay messages last 10–120 seconds.",
        ));
    }
    if input.min_loyalty.is_some_and(|r| !(0..=4).contains(&r)) {
        return Err(Fail::field("min_loyalty", "Choose a loyalty rank."));
    }
    if input
        .min_account_days
        .is_some_and(|d| !(0..=90).contains(&d))
    {
        return Err(Fail::field(
            "min_account_days",
            "Account age is off, or 1–90 days.",
        ));
    }
    if input
        .gifs
        .as_deref()
        .is_some_and(|g| !["off", "everyone", "followers", "subscribers"].contains(&g))
    {
        return Err(Fail::field("gifs", "Choose who can send GIFs."));
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
    // The row first, so a channel's first save keeps every optional field too (left out = column
    // default or unchanged).
    sqlx::query(
        "INSERT INTO chat_settings(channel_id) VALUES($1) ON CONFLICT(channel_id) DO NOTHING",
    )
    .bind(&channel)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE chat_settings SET slow_mode_seconds=$2,block_links=$3,banned_words=$4,overlay_fade_seconds=coalesce($5,overlay_fade_seconds),allow_signatures=coalesce($6,allow_signatures),min_loyalty=coalesce($7,min_loyalty),gifs=coalesce($8,gifs),min_account_days=coalesce($9,min_account_days) WHERE channel_id=$1")
        .bind(&channel).bind(input.slow_mode_seconds).bind(input.block_links).bind(&words).bind(input.overlay_fade_seconds).bind(input.allow_signatures).bind(input.min_loyalty).bind(&input.gifs).bind(input.min_account_days).execute(&mut *tx).await?;
    log(&mut tx, &channel, &user.id, role, "settings", None, None, json!({"slow_mode_seconds": input.slow_mode_seconds, "block_links": input.block_links, "banned_words": words.len(), "min_loyalty": input.min_loyalty, "gifs": input.gifs, "min_account_days": input.min_account_days}), &reason).await?;
    tx.commit().await?;
    if input.allow_signatures.is_some() {
        // Open chats pick up the signature setting now, with their emotes.
        app.chat
            .publish(&channel, None, 0, json!({"type":"emotes"}));
    }
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

/// GET /api/channels/{username}/chatters: the owner's or a moderator's viewer list for the live
/// stream (S.V.E.R Desktop's dock, `channel:read`). Like Twitch's, it names only people who
/// chatted during this broadcast, which chat already shows; everyone else is a count.
async fn chatters(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    actor_any(&app, &headers, &jar, &channel, "channel:read").await?;
    let rows: Vec<(String, String, i64, DateTime<Utc>, bool)> = sqlx::query_as(
        "SELECT u.username, u.display_name, count(*), max(m.created_at),
            EXISTS(SELECT 1 FROM channel_moderators d WHERE d.channel_id=$1 AND d.user_id=u.id)
         FROM broadcasts b
         JOIN chat_messages m ON m.channel_id=$1 AND m.created_at>=b.started_at
         JOIN channel_users u ON u.id=m.author_id
         WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING')
           AND m.author_id<>$1 AND m.origin IS NULL AND u.deleted_at IS NULL
         GROUP BY u.id, u.username, u.display_name ORDER BY max(m.created_at) DESC LIMIT 500",
    )
    .bind(&channel)
    .fetch_all(&app.db)
    .await?;
    let watching: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM broadcasts b JOIN playback_leases l ON l.broadcast_id=b.id
         WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') AND l.expires_at>now()
           AND l.level IN ('counted','trusted')",
    )
    .bind(&channel)
    .fetch_one(&app.db)
    .await?;
    let chatters: Vec<Value> = rows
        .into_iter()
        .map(|(username, display, messages, last, moderator)| {
            json!({"username": username, "display_name": display, "messages": messages,
                "last_at": last, "role": if moderator { "moderator" } else { "viewer" }})
        })
        .collect();
    Ok(Json(json!({"chatters": chatters, "watching": watching})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/chatters", get(chatters))
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
        .route("/api/channels/{username}/chat/sweep", post(sweep))
        .route("/api/channels/{username}/chat/reviews/{id}", post(review))
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
    use super::{fold, has_link, says_banned};

    #[test]
    fn banned_words_are_whole_words() {
        let banned = |list: &[&str], text: &str| {
            says_banned(text, &list.iter().map(|w| fold(w)).collect::<Vec<_>>())
        };
        // Whole words: "ass" no longer blocks "class" or "pass", or "as".
        assert!(banned(&["ass"], "what an ass"));
        for fine in ["first class", "pass the ball", "as if", "assassin"] {
            assert!(!banned(&["ass"], fine), "{fine}");
        }
        // Disguises: leetspeak, spacing, dots, stretching, fullwidth, mentions.
        for said in [
            "a$$",
            "@ss",
            "A S S",
            "a.s.s",
            "asssss",
            "ａｓｓ",
            "you're an a55!",
        ] {
            assert!(banned(&["ass"], said), "{said}");
        }
        assert!(banned(&["hello"], "h3ll0 there"));
        // Wildcards at either end, and phrases.
        assert!(banned(&["ass*"], "assassin"));
        assert!(!banned(&["ass*"], "ask"));
        assert!(banned(&["*hole"], "a pothole"));
        assert!(banned(&["*ss*"], "glasses"));
        assert!(banned(&["bad word"], "such a BAD   word"));
        assert!(!banned(&["bad word"], "bad, then a word"));
        // Patterns with no letters still match anywhere.
        assert!(banned(&["\u{1F346}"], "look \u{1F346}"));
        assert!(!banned(&[], "anything"));
    }

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
        // Variation selectors, tags, Hangul fillers and fullwidth forms can't disguise either.
        for masked in ["s\u{FE0F}lur", "s\u{E0041}lur", "s\u{3164}lur", "ｓｌｕｒ"] {
            assert_eq!(fold(masked), "slur", "{masked:?}");
        }
        assert!(has_link("ｗｗｗ．example．com"));
        // Chat refuses selectors and tags outside emoji, and keeps them in emoji.
        assert!(crate::text::clean("s\u{FE0F}lur", "body", false).is_err());
        assert!(crate::text::clean("s\u{E0041}lur", "body", false).is_err());
        let emoji = "\u{2764}\u{FE0F} 1\u{FE0F}\u{20E3} \u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";
        assert!(crate::text::clean(emoji, "body", false).is_ok());
    }
}
