//! Strikes, the escalation ladder, manual actions (/mod, /modpanel), lockdowns, anti-raid, the
//! new-account filter and the ban trap. Every action is a row in `discord_cases` and a mod log post.
use super::{
    BAN, By, Home, KICK, MANAGE_CHANNELS, MANAGE_GUILD, MANAGE_MESSAGES, MODERATE, STAFF, ago,
    alert_team, button, created, dm, done, embed, field, form, log, minutes, missing, needs, note,
    post, row, update,
};
use crate::{
    App,
    discord::{
        call,
        commands::{Interaction, private},
        home,
    },
    profiles::Res,
};
use chrono::{Duration, Utc};
use reqwest::Method;
use serde_json::{Map, Value, json};

/// Discord's longest timeout: 28 days.
const LONGEST_MUTE: i64 = 28 * 1440;
/// The mute length when the ladder doesn't set one.
const DEFAULT_MUTE: i64 = 60;
/// Send Messages and Send Messages in Threads.
const SEND: u64 = 1 << 11 | 1 << 38;
const ACTION: u32 = 0xE7_4C3C;

/// A strike: an active 'warn'. The member's active strikes after it.
pub(super) async fn warn_case(app: &App, user: &str, by: Option<&str>, reason: &str) -> Res<i64> {
    sqlx::query(
        "INSERT INTO discord_cases(user_id,moderator_id,action,reason) VALUES($1,$2,'warn',$3)",
    )
    .bind(user)
    .bind(by)
    .bind(reason)
    .execute(&app.db)
    .await?;
    strikes(app, user).await
}
async fn strikes(app: &App, user: &str) -> Res<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM discord_cases WHERE user_id=$1 AND action='warn' AND active",
    )
    .bind(user)
    .fetch_one(&app.db)
    .await?)
}

/// The member as Discord has them: Ok(None) only when Discord says they aren't in the server
/// (Unknown Member), Err when it didn't say (a rate limit, an outage), so "unknown" is never
/// taken for "absent".
pub(super) async fn member(app: &App, h: &Home, user: &str) -> Result<Option<Value>, ()> {
    match call(
        app,
        Method::GET,
        &format!("/guilds/{}/members/{user}", h.guild),
        None,
    )
    .await
    {
        Some((200, m)) => Ok(Some(m)),
        Some((404, body)) if body["code"] == 10007 => Ok(None),
        _ => Err(()),
    }
}
/// The restricted role: members keep seeing channels but can't take part until staff review.
/// Never staff or protected members (nor anyone, while Discord can't say who they are).
pub(super) async fn restrict(app: &App, h: &Home, user: &str) -> bool {
    let Some(role) = home::id(&h.s, "restricted") else {
        return false;
    };
    if !matches!(untouchable(app, h, user, None).await, Ok(None)) {
        return false;
    }
    done(
        app,
        Method::PUT,
        &format!("/guilds/{}/members/{user}/roles/{role}", h.guild),
        None,
    )
    .await
}
async fn unrestrict(app: &App, h: &Home, user: &str) {
    if let Some(role) = home::id(&h.s, "restricted") {
        done(
            app,
            Method::DELETE,
            &format!("/guilds/{}/members/{user}/roles/{role}", h.guild),
            None,
        )
        .await;
    }
}
/// A timeout for `minutes` (at most 28 days), or None to end one.
async fn timeout(app: &App, h: &Home, user: &str, minutes: Option<i64>) -> bool {
    let until = minutes.map(|m| (Utc::now() + Duration::minutes(m.min(LONGEST_MUTE))).to_rfc3339());
    done(
        app,
        Method::PATCH,
        &format!("/guilds/{}/members/{user}", h.guild),
        Some(json!({"communication_disabled_until": until})),
    )
    .await
}
/// Mass spam: a 24-hour timeout pending review. Why not, when it wasn't given.
pub(super) async fn quarantine(app: &App, h: &Home, user: &str) -> Res<Option<String>> {
    if let Some(why) = untouchable(app, h, user, None).await? {
        return Ok(Some(why));
    }
    if !timeout(app, h, user, Some(1440)).await {
        return Ok(Some("Discord refused the timeout.".into()));
    }
    sqlx::query("INSERT INTO discord_cases(user_id,action,reason,expires_at) VALUES($1,'mute','Automod: mass spam',now()+interval '24 hours')")
        .bind(user).execute(&app.db).await?;
    Ok(None)
}

/// Holds the member's ban lock until the transaction ends, so a ban and the lifting of an
/// expired timed ban for the same member never interleave.
async fn ban_lock(app: &App, user: &str) -> Res<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('discord-ban:'||$1, 0))")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

#[derive(Clone, Copy)]
pub(super) enum Act {
    Warn,
    /// Minutes.
    Mute(i64),
    Unmute,
    Kick,
    /// Minutes; None for good.
    Ban(Option<i64>),
    Softban,
}
impl Act {
    fn name(self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Mute(_) => "mute",
            Self::Unmute => "unmute",
            Self::Kick => "kick",
            Self::Ban(_) => "ban",
            Self::Softban => "softban",
        }
    }
    fn minutes(self) -> Option<i64> {
        match self {
            Self::Mute(m) => Some(m.min(LONGEST_MUTE)),
            Self::Ban(m) => m,
            _ => None,
        }
    }
    /// The permission a moderator needs for it.
    fn needs(self) -> u64 {
        match self {
            Self::Kick => KICK,
            Self::Ban(_) | Self::Softban => BAN,
            _ => MODERATE,
        }
    }
}

/// Why `by` may not act on `user` (or Discord couldn't say): the bot itself, themselves, a
/// protected role, staff (staff permissions or the mod team role), or someone whose highest role
/// is at or above theirs.
async fn untouchable(app: &App, h: &Home, user: &str, by: Option<&By>) -> Res<Option<String>> {
    let bot: Option<String> = sqlx::query_scalar("SELECT bot_id FROM discord_gateway")
        .fetch_optional(&app.db)
        .await?
        .flatten();
    if bot.as_deref() == Some(user) {
        return Ok(Some("That's the bot.".into()));
    }
    if by.is_some_and(|b| b.id == user) {
        return Ok(Some("You can't moderate yourself.".into()));
    }
    let m = match member(app, h, user).await {
        Ok(Some(m)) => m,
        Ok(None) => return Ok(None),
        Err(()) => return Ok(Some("Discord didn't answer. Try again in a minute.".into())),
    };
    let theirs: Vec<String> = m["roles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().map(String::from))
        .collect();
    let protected = home::ids(&h.s, "protected");
    if let Some(role) = theirs.iter().find(|r| protected.contains(r)) {
        return Ok(Some(format!(
            "<@{user}> has a protected role (<@&{role}>) and can't be moderated."
        )));
    }
    let Some((200, roles)) = call(
        app,
        Method::GET,
        &format!("/guilds/{}/roles", h.guild),
        None,
    )
    .await
    else {
        return Ok(Some("Discord didn't answer. Try again in a minute.".into()));
    };
    let roles = roles.as_array().cloned().unwrap_or_default();
    // A member's roles, @everyone (whose id is the server's) included.
    let held = |ids: &[String]| -> Vec<Value> {
        roles
            .iter()
            .filter(|r| r["id"] == h.guild.as_str() || ids.iter().any(|i| r["id"] == i.as_str()))
            .cloned()
            .collect()
    };
    let power = held(&theirs)
        .iter()
        .filter_map(|r| r["permissions"].as_str()?.parse::<u64>().ok())
        .fold(0, |all, p| all | p);
    let team = home::id(&h.s, "mod_team").is_some_and(|t| theirs.contains(&t));
    if power & STAFF != 0 || team {
        return Ok(Some(format!(
            "<@{user}> is staff and can't be moderated with the bot."
        )));
    }
    if let Some(b) = by {
        let top = |ids: &[String]| {
            held(ids)
                .iter()
                .filter_map(|r| r["position"].as_i64())
                .max()
                .unwrap_or(0)
        };
        if top(&theirs) >= top(&b.roles) {
            return Ok(Some(format!(
                "<@{user}>'s highest role is at or above yours."
            )));
        }
    }
    Ok(None)
}

/// Marks earlier bans of the member over, so an older timed ban can't lift a newer one.
async fn close_bans(app: &App, user: &str) -> Res<()> {
    sqlx::query("UPDATE discord_cases SET lifted_at=now() WHERE user_id=$1 AND action='ban' AND lifted_at IS NULL")
        .bind(user).execute(&app.db).await?;
    Ok(())
}

/// One moderation action: checks the moderator's permission and the target, tells the member
/// (when staff want that), acts on Discord, records the case and logs it. Ok(Err(why)) when it
/// couldn't be done.
pub(super) async fn act(
    app: &App,
    h: &Home,
    user: &str,
    by: Option<&By>,
    act: Act,
    reason: &str,
) -> Res<Result<String, String>> {
    if let Some(b) = by
        && !b.can(act.needs())
    {
        return Ok(Err(missing(act.needs())));
    }
    if let Some(why) = untouchable(app, h, user, by).await? {
        return Ok(Err(why));
    }
    let _lock = match act {
        Act::Ban(_) | Act::Softban => Some(ban_lock(app, user).await?),
        _ => None,
    };
    let by = by.map(|b| b.id.as_str());
    let length = act
        .minutes()
        .map_or(String::new(), |m| format!(" for {}", span(m)));
    let told = match act {
        Act::Warn => Some("⚠️ You've been warned on the S.V.E.R server.".to_string()),
        Act::Mute(_) => Some(format!("🔇 You've been muted on the S.V.E.R server{length}.")),
        Act::Kick => Some("👢 You've been kicked from the S.V.E.R server.".into()),
        Act::Ban(m) => Some(format!(
            "⛔ You've been banned from the S.V.E.R server{}.",
            if m.is_some() { &length } else { " for good" }
        )),
        Act::Softban => Some(
            "👢 You've been removed from the S.V.E.R server and your recent messages deleted. You may rejoin.".into(),
        ),
        Act::Unmute => None,
    };
    if let Some(text) = told.filter(|_| home::on(&h.s, "dm_on_action")) {
        dm(app, user, json!({"content": format!("{text}\n**Reason:** {reason}\n\nIf you think this was a mistake, open a ticket by messaging this bot.")})).await;
    }
    let g = &h.guild;
    let bans = format!("/guilds/{g}/bans/{user}");
    let ban = |seconds: u32| {
        done(
            app,
            Method::PUT,
            &bans,
            Some(json!({"delete_message_seconds": seconds})),
        )
    };
    let ok = match act {
        Act::Warn => true,
        Act::Mute(m) => timeout(app, h, user, Some(m)).await,
        Act::Unmute => timeout(app, h, user, None).await,
        Act::Kick => {
            done(
                app,
                Method::DELETE,
                &format!("/guilds/{g}/members/{user}"),
                None,
            )
            .await
        }
        Act::Ban(_) => ban(0).await,
        Act::Softban => ban(7 * 86_400).await && done(app, Method::DELETE, &bans, None).await,
    };
    if !ok {
        return Ok(Err(
            "Discord refused. Check the bot's permissions, and that its role sits above the member's."
                .into(),
        ));
    }
    if let Act::Warn = act {
        let count = warn_case(app, user, by, reason).await?;
        log_action(
            app,
            h,
            user,
            by,
            act,
            reason,
            &format!("\n**Strikes:** {count}"),
        )
        .await;
        escalate(app, h, user, count).await?;
        return Ok(Ok(format!(
            "<@{user}> warned. Active strikes: **{count}**."
        )));
    }
    if let Act::Unmute = act {
        sqlx::query("UPDATE discord_cases SET lifted_at=now() WHERE user_id=$1 AND action='mute' AND lifted_at IS NULL")
            .bind(user).execute(&app.db).await?;
    }
    if let Act::Ban(_) | Act::Softban = act {
        close_bans(app, user).await?;
    }
    let expires = act.minutes().map(|m| Utc::now() + Duration::minutes(m));
    sqlx::query("INSERT INTO discord_cases(user_id,moderator_id,action,reason,expires_at) VALUES($1,$2,$3,$4,$5)")
        .bind(user).bind(by).bind(act.name()).bind(reason).bind(expires).execute(&app.db).await?;
    let ends = expires.map_or(String::new(), |at| format!("\n**Ends:** {}", ago(at)));
    log_action(app, h, user, by, act, reason, &ends).await;
    Ok(Ok(format!("Done: {} for <@{user}>{length}.", act.name())))
}
async fn log_action(
    app: &App,
    h: &Home,
    user: &str,
    by: Option<&str>,
    act: Act,
    reason: &str,
    extra: &str,
) {
    let by = by.map_or("Automod".into(), |b| format!("<@{b}>"));
    note(
        app,
        h,
        &format!("Mod action: {}", act.name()),
        ACTION,
        format!("**Member:** <@{user}>\n**Moderator:** {by}\n**Reason:** {reason}{extra}"),
    )
    .await;
}
fn span(minutes: i64) -> String {
    match minutes {
        m if m % 10_080 == 0 => format!("{} week(s)", m / 10_080),
        m if m % 1440 == 0 => format!("{} day(s)", m / 1440),
        m if m % 60 == 0 => format!("{} hour(s)", m / 60),
        m => format!("{m} minute(s)"),
    }
}

/// The ladder's step for this many strikes (at least, not exactly, its number), proposed to staff
/// in the mod log with the member restricted meanwhile. One open proposal per member.
pub(super) async fn escalate(app: &App, h: &Home, user: &str, strikes: i64) -> Res<()> {
    let step = [("ban", "ban_at"), ("kick", "kick_at"), ("mute", "mute_at")]
        .into_iter()
        .find(|(_, key)| home::number(&h.s, key).is_some_and(|n| strikes >= n));
    let Some((action, _)) = step else {
        return Ok(());
    };
    let length = (action == "mute")
        .then(|| home::number(&h.s, "mute_minutes").unwrap_or(DEFAULT_MUTE) as i32);
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO discord_escalations(user_id,strikes,action,minutes) VALUES($1,$2,$3,$4)
        ON CONFLICT (user_id) WHERE resolved_at IS NULL DO NOTHING RETURNING id",
    )
    .bind(user)
    .bind(strikes as i32)
    .bind(action)
    .bind(length)
    .fetch_optional(&app.db)
    .await?;
    let Some(id) = id else {
        return Ok(());
    };
    restrict(app, h, user).await;
    let message = log(app, h, json!({
        "embeds": [embed("Escalation: approval needed", 0x9B_59B6, format!(
            "**Member:** <@{user}>\n**Strikes:** {strikes}\n**Recommended:** {action}\n**Length:** {}\n\nThe member is restricted until a moderator approves or dismisses this.",
            length.map_or("Permanent".into(), |m| span(m.into()))))],
        "components": [row(vec![
            button("Approve", 4, format!("mod:esc:approve:{id}")),
            button("Custom length", 2, format!("mod:esc:custom:{id}")),
            button("Dismiss", 2, format!("mod:esc:dismiss:{id}")),
        ])]})).await;
    sqlx::query("UPDATE discord_escalations SET message_id=$2 WHERE id=$1")
        .bind(id)
        .bind(message)
        .execute(&app.db)
        .await?;
    Ok(())
}

async fn approve(app: &App, h: &Home, i: &Interaction, id: i64, length: Option<i64>) -> Res<Value> {
    let claimed: Option<(String, String, Option<i32>)> = sqlx::query_as("UPDATE discord_escalations SET resolved_at=now(),resolved_by=$2,outcome='approved' WHERE id=$1 AND resolved_at IS NULL RETURNING user_id,action,minutes")
        .bind(id).bind(&i.user).fetch_optional(&app.db).await?;
    let Some((user, action, minutes)) = claimed else {
        return Ok(private("Someone already handled this escalation."));
    };
    let minutes = length.or(minutes.map(i64::from));
    let step = match action.as_str() {
        "mute" => Act::Mute(minutes.unwrap_or(DEFAULT_MUTE)),
        "kick" => Act::Kick,
        _ => Act::Ban(minutes),
    };
    match act(
        app,
        h,
        &user,
        Some(&By::of(app, h, i).await),
        step,
        "Escalation approved",
    )
    .await?
    {
        Ok(_) => {
            unrestrict(app, h, &user).await;
            Ok(update(
                json!({"content": format!("**{action}** approved by <@{}> for <@{user}>.", i.user), "components": []}),
            ))
        }
        Err(why) => {
            sqlx::query("UPDATE discord_escalations SET resolved_at=NULL,resolved_by=NULL,outcome=NULL WHERE id=$1")
                .bind(id).execute(&app.db).await?;
            Ok(private(why))
        }
    }
}

/// Lockdown: denies sending in every text channel (or one) to `role`, remembering each channel's
/// overwrite so unlocking restores it. Err(why) when there's nothing to lock.
async fn lock(
    app: &App,
    h: &Home,
    channel: Option<&str>,
    role: &str,
    raid: Option<i64>,
    by: Option<&str>,
    minutes: Option<i64>,
) -> Res<Result<(i64, usize), &'static str>> {
    // A server lockdown blocks any other; channels already locked on their own are skipped.
    let locked: Vec<Option<String>> =
        sqlx::query_scalar("SELECT channel_id FROM discord_lockdowns WHERE unlocked_at IS NULL")
            .fetch_all(&app.db)
            .await?;
    if locked
        .iter()
        .any(|l| l.is_none() || l.as_deref() == channel)
    {
        return Ok(Err("That's already locked. Unlock it first."));
    }
    let Some((200, list)) = call(
        app,
        Method::GET,
        &format!("/guilds/{}/channels", h.guild),
        None,
    )
    .await
    else {
        return Ok(Err("Discord didn't answer. Try again in a minute."));
    };
    let mut previous = Map::new();
    for c in list.as_array().into_iter().flatten() {
        let Some(id) = c["id"].as_str() else { continue };
        if !matches!(c["type"].as_i64(), Some(0 | 5))
            || channel.is_some_and(|ch| ch != id)
            || locked.iter().any(|l| l.as_deref() == Some(id))
        {
            continue;
        }
        let before = c["permission_overwrites"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|o| o["id"] == role)
            .map(|o| json!({"allow": o["allow"], "deny": o["deny"]}))
            .unwrap_or(Value::Null);
        let bits = |k: &str| {
            before[k]
                .as_str()
                .and_then(|b| b.parse::<u64>().ok())
                .unwrap_or(0)
        };
        let body = json!({"type": 0, "allow": (bits("allow") & !SEND).to_string(), "deny": (bits("deny") | SEND).to_string()});
        if done(
            app,
            Method::PUT,
            &format!("/channels/{id}/permissions/{role}"),
            Some(body),
        )
        .await
        {
            previous.insert(id.into(), before);
        }
    }
    if previous.is_empty() {
        return Ok(Err(
            "No channel could be locked. Check the bot's permissions.",
        ));
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO discord_lockdowns(raid_joins,channel_id,role_id,previous,locked_by,unlock_at) VALUES($1,$2,$3,$4,$5,now()+make_interval(mins=>$6)) RETURNING id")
        .bind(raid.map(|r| r as i32)).bind(channel).bind(role).bind(Value::Object(previous.clone())).bind(by).bind(minutes.map(|m| m as i32))
        .fetch_one(&app.db).await?;
    Ok(Ok((id, previous.len())))
}
/// Lifts a lockdown; how many channels were restored, or None when it was already lifted.
async fn unlock(app: &App, id: i64, by: Option<&str>) -> Res<Option<usize>> {
    let row: Option<(String, Value)> = sqlx::query_as("UPDATE discord_lockdowns SET unlocked_at=now(),unlocked_by=$2 WHERE id=$1 AND unlocked_at IS NULL RETURNING role_id,previous")
        .bind(id).bind(by).fetch_optional(&app.db).await?;
    let Some((role, previous)) = row else {
        return Ok(None);
    };
    let previous = previous.as_object().cloned().unwrap_or_default();
    for (channel, before) in &previous {
        let path = format!("/channels/{channel}/permissions/{role}");
        if before.is_null() {
            done(app, Method::DELETE, &path, None).await;
        } else {
            done(
                app,
                Method::PUT,
                &path,
                Some(json!({"type": 0, "allow": before["allow"], "deny": before["deny"]})),
            )
            .await;
        }
    }
    Ok(Some(previous.len()))
}

/// A member joined: logged; held when a raid lockdown is on, when the joins make a raid, or when
/// the account is too new (restricted, with approve/kick/ban for staff). True when held.
pub(super) async fn joined(app: &App, h: &Home, d: &Value) -> Res<bool> {
    let Some(user) = d["user"]["id"].as_str() else {
        return Ok(false);
    };
    if d["user"]["bot"] == true {
        return Ok(false);
    }
    sqlx::query("INSERT INTO discord_joins(user_id) VALUES($1)")
        .bind(user)
        .execute(&app.db)
        .await?;
    let made = created(user);
    note(
        app,
        h,
        "Member joined",
        0x2E_CC71,
        format!(
            "**Member:** <@{user}>\n**Account created:** {}",
            made.map_or("unknown".into(), ago)
        ),
    )
    .await;
    let raid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_lockdowns WHERE raid_joins IS NOT NULL AND unlocked_at IS NULL)")
        .fetch_one(&app.db).await?;
    if raid {
        return Ok(true);
    }
    if let (Some(joins), Some(seconds)) = (
        home::number(&h.s, "raid_joins"),
        home::number(&h.s, "raid_seconds"),
    ) {
        let recent: Vec<String> = sqlx::query_scalar("SELECT user_id FROM discord_joins WHERE joined_at>now()-make_interval(secs=>$1) ORDER BY joined_at DESC")
            .bind(seconds as f64).fetch_all(&app.db).await?;
        if recent.len() as i64 >= joins {
            // The raid lockdown takes @everyone (whose id is the server's) out of every channel.
            let count = recent.len() as i64;
            let (id, state) = match lock(app, h, None, &h.guild, Some(count), None, None).await? {
                Ok((id, _)) => (id, "Nobody can send messages".to_string()),
                // Already locked by hand, or Discord refused: the raid is still recorded (it
                // holds new members and keeps "ban all"), and staff still hear about it.
                Err(why) => {
                    let id: i64 = sqlx::query_scalar("INSERT INTO discord_lockdowns(raid_joins,role_id) VALUES($1,$2) RETURNING id")
                        .bind(count as i32).bind(&h.guild).fetch_one(&app.db).await?;
                    (id, format!("The raid lockdown didn't happen ({why})"))
                }
            };
            // "Ban all" means exactly these accounts, whenever staff press it.
            sqlx::query("UPDATE discord_lockdowns SET raiders=$2 WHERE id=$1")
                .bind(id)
                .bind(&recent)
                .execute(&app.db)
                .await?;
            let list: Vec<String> = recent
                .iter()
                .take(15)
                .map(|u| {
                    format!(
                        "<@{u}> (account created {})",
                        created(u).map_or("unknown".into(), ago)
                    )
                })
                .collect();
            log(app, h, json!({
                "embeds": [embed("Raid detected", 0xFF_0000, format!(
                    "**{count}** accounts joined in **{seconds}** seconds.\n{state}, and new members are held.\n\n**Recent joins:**\n{}",
                    list.join("\n")))],
                "components": [row(vec![
                    button("Unlock server", 3, format!("mod:raid:unlock:{id}")),
                    button("Ban all recent joins", 4, format!("mod:raid:banall:{id}")),
                    button("Unlock in 30 min", 2, format!("mod:raid:auto:{id}")),
                ])]})).await;
            alert_team(app, h, "Raid detected. Review it in the mod log.").await;
            return Ok(true);
        }
    }
    if let (Some(days), Some(made)) = (home::number(&h.s, "new_account_days"), made)
        && Utc::now() - made < Duration::days(days)
    {
        restrict(app, h, user).await;
        dm(app, user, json!({"content": "Welcome to the S.V.E.R server! Your account is new, so a moderator will review your access shortly."})).await;
        log(app, h, json!({
            "embeds": [embed("New account held", 0xFF_A500, format!(
                "<@{user}> joined with an account created {}.\n**Action:** restricted until a moderator decides.", ago(made)))],
            "components": [row(vec![
                button("Approve", 3, format!("mod:new:approve:{user}")),
                button("Kick", 2, format!("mod:new:kick:{user}")),
                button("Ban", 4, format!("mod:new:ban:{user}")),
            ])]})).await;
        return Ok(true);
    }
    Ok(false)
}
pub(super) async fn left(app: &App, h: &Home, d: &Value) -> Res<()> {
    if let Some(user) = d["user"]["id"]
        .as_str()
        .filter(|_| d["user"]["bot"] != true)
    {
        note(
            app,
            h,
            "Member left",
            0x95_A5A6,
            format!("**Member:** <@{user}>"),
        )
        .await;
    }
    Ok(())
}

/// Someone posted in the ban trap: banned, with a week of their messages deleted. Staff, the mod
/// team and protected members (and anyone, while Discord can't say who they are) only lose the
/// message; so does the server owner, whom Discord refuses to ban.
pub(super) async fn trapped(
    app: &App,
    h: &Home,
    channel: &str,
    message: &str,
    user: &str,
) -> Res<()> {
    let spared = untouchable(app, h, user, None).await?;
    let lock = ban_lock(app, user).await?;
    let banned = spared.is_none()
        && done(
            app,
            Method::PUT,
            &format!("/guilds/{}/bans/{user}", h.guild),
            Some(json!({"delete_message_seconds": 7 * 86_400})),
        )
        .await;
    if banned {
        close_bans(app, user).await?;
        sqlx::query("INSERT INTO discord_cases(user_id,action,reason) VALUES($1,'ban','Posted in the ban trap')")
            .bind(user).execute(&app.db).await?;
    } else {
        done(
            app,
            Method::DELETE,
            &format!("/channels/{channel}/messages/{message}"),
            None,
        )
        .await;
    }
    drop(lock);
    let outcome = match (banned, spared) {
        (true, _) => "Banned, with a week of messages deleted.".to_string(),
        (false, Some(why)) => format!("Not banned ({why}); the message was deleted."),
        (false, None) => "The ban failed; the message was deleted.".into(),
    };
    note(
        app,
        h,
        "Ban trap",
        ACTION,
        format!("<@{user}> posted in <#{channel}>. {outcome}"),
    )
    .await;
    Ok(())
}

/// Timed bans that are over, escalations nobody touched for 30 minutes, and lockdowns due to lift.
pub(super) async fn tick(app: &App, h: &Home) -> Res<()> {
    let due: Vec<(i64, String)> = sqlx::query_as("UPDATE discord_cases SET lifted_at=now() WHERE id IN (SELECT id FROM discord_cases WHERE action='ban' AND lifted_at IS NULL AND expires_at<=now() ORDER BY expires_at LIMIT 20 FOR UPDATE SKIP LOCKED) RETURNING id,user_id")
        .fetch_all(&app.db).await?;
    for (id, user) in due {
        // Under the member's ban lock: if a newer ban landed meanwhile, it stands and this one is
        // simply over.
        let mut lock = ban_lock(app, &user).await?;
        let newer: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_cases WHERE user_id=$1 AND action='ban' AND lifted_at IS NULL AND id<>$2)")
            .bind(&user).bind(id).fetch_one(&mut *lock).await?;
        if newer {
            continue;
        }
        match call(
            app,
            Method::DELETE,
            &format!("/guilds/{}/bans/{user}", h.guild),
            None,
        )
        .await
        {
            // Lifted, or already unbanned by hand.
            Some((status, _)) if status < 300 || status == 404 => {
                sqlx::query("INSERT INTO discord_cases(user_id,action,reason) VALUES($1,'unban','The timed ban ended')")
                    .bind(&user).execute(&app.db).await?;
                note(
                    app,
                    h,
                    "Ban lifted",
                    0x2E_CC71,
                    format!("<@{user}>'s timed ban ended."),
                )
                .await;
            }
            _ => {
                sqlx::query("UPDATE discord_cases SET lifted_at=NULL WHERE id=$1")
                    .bind(id)
                    .execute(&app.db)
                    .await?;
            }
        }
    }
    // Discord ends timeouts itself; the cases just catch up.
    sqlx::query("UPDATE discord_cases SET lifted_at=expires_at WHERE action='mute' AND lifted_at IS NULL AND expires_at<=now()")
        .execute(&app.db).await?;
    let waiting: Vec<(String, Option<String>)> = sqlx::query_as("UPDATE discord_escalations SET repinged_at=now() WHERE resolved_at IS NULL AND repinged_at IS NULL AND created_at<=now()-interval '30 minutes' RETURNING user_id,message_id")
        .fetch_all(&app.db).await?;
    for (user, message) in waiting {
        let link = match (home::id(&h.s, "mod_log"), message) {
            (Some(channel), Some(m)) => {
                format!(" https://discord.com/channels/{}/{channel}/{m}", h.guild)
            }
            _ => String::new(),
        };
        alert_team(
            app,
            h,
            &format!("The escalation for <@{user}> has waited 30 minutes for a decision.{link}"),
        )
        .await;
    }
    let lifting: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM discord_lockdowns WHERE unlocked_at IS NULL AND unlock_at<=now()",
    )
    .fetch_all(&app.db)
    .await?;
    for id in lifting {
        if let Some(n) = unlock(app, id, None).await? {
            note(
                app,
                h,
                "Lockdown lifted",
                0x2E_CC71,
                format!("The lockdown ended on schedule; {n} channel(s) unlocked."),
            )
            .await;
        }
    }
    Ok(())
}

/// Buttons and forms: escalations, raids, held accounts and the mod panel.
pub(super) async fn component(
    app: &App,
    h: &Home,
    i: &Interaction,
    area: &str,
    action: &str,
    target: &str,
) -> Res<Value> {
    let who = By::of(app, h, i).await;
    let by = i.user.as_str();
    let number: i64 = target.parse().unwrap_or(0);
    // Buttons that don't go through `act` check their permission here.
    let bit = match (area, action) {
        ("esc", "dismiss") | ("new", "approve") | ("form" | "panel", "note") => MODERATE,
        ("raid", "unlock" | "auto") => MANAGE_CHANNELS,
        ("raid", "banall") => BAN,
        _ => 0,
    };
    if bit != 0 && !who.can(bit) {
        return Ok(needs(bit));
    }
    Ok(match (area, action) {
        ("esc", "approve") => approve(app, h, i, number, None).await?,
        ("esc", "custom") => form(
            format!("mod:esc:length:{number}"),
            "Custom length",
            &[("duration", "Length, like 30m, 2h, 7d or 2w", true, false)],
        ),
        ("esc", "length") => match field(i, "duration").and_then(minutes) {
            Some(m) => approve(app, h, i, number, Some(m)).await?,
            None => private("Lengths look like 30m, 2h, 7d or 2w."),
        },
        ("esc", "dismiss") => {
            let user: Option<String> = sqlx::query_scalar("UPDATE discord_escalations SET resolved_at=now(),resolved_by=$2,outcome='dismissed' WHERE id=$1 AND resolved_at IS NULL RETURNING user_id")
                .bind(number).bind(by).fetch_optional(&app.db).await?;
            match user {
                Some(user) => {
                    unrestrict(app, h, &user).await;
                    update(
                        json!({"content": format!("Escalation for <@{user}> dismissed by <@{by}>."), "embeds": [], "components": []}),
                    )
                }
                None => private("Someone already handled this escalation."),
            }
        }
        ("raid", "unlock") => match unlock(app, number, Some(by)).await? {
            Some(n) => update(
                json!({"content": format!("🔓 Server unlocked by <@{by}> ({n} channel(s))."), "components": []}),
            ),
            None => private("The server is already unlocked."),
        },
        ("raid", "auto") => {
            let at: Option<chrono::DateTime<Utc>> = sqlx::query_scalar("UPDATE discord_lockdowns SET unlock_at=now()+interval '30 minutes' WHERE id=$1 AND unlocked_at IS NULL AND unlock_at IS NULL RETURNING unlock_at")
                .bind(number).fetch_optional(&app.db).await?;
            match at {
                Some(at) => update(
                    json!({"content": format!("⏰ The server unlocks {}; set by <@{by}>.", ago(at)), "components": []}),
                ),
                None => private("The server is already unlocked, or an unlock is already set."),
            }
        }
        ("raid", "banall") => {
            // Exactly the joins that set the raid off; each ban goes through the usual checks, so
            // protected and staff members are skipped.
            let joined: Vec<String> =
                sqlx::query_scalar("SELECT unnest(raiders) FROM discord_lockdowns WHERE id=$1")
                    .bind(number)
                    .fetch_all(&app.db)
                    .await?;
            // Many bans take longer than Discord waits for an answer: answer first, report after.
            let (app, h) = (app.clone(), h.clone());
            let count = joined.len();
            tokio::spawn(async move {
                let mut banned = 0;
                let reason = format!("Raid ban (lockdown {number})");
                for user in &joined {
                    if let Ok(Ok(_)) =
                        act(&app, &h, user, Some(&who), Act::Ban(None), &reason).await
                    {
                        banned += 1;
                    }
                }
                note(
                    &app,
                    &h,
                    "Raid accounts banned",
                    ACTION,
                    format!(
                        "<@{}> banned {banned} of {} raid joins.",
                        who.id,
                        joined.len()
                    ),
                )
                .await;
            });
            update(
                json!({"content": format!("🔨 Banning {count} recent join(s), started by <@{}>. The server stays locked until unlocked.", i.user),
                "components": [row(vec![
                    button("Unlock server", 3, format!("mod:raid:unlock:{number}")),
                    button("Unlock in 30 min", 2, format!("mod:raid:auto:{number}")),
                ])]}),
            )
        }
        ("new", "approve") => {
            match member(app, h, target).await {
                Ok(Some(_)) => {}
                Ok(None) => return Ok(private("They're no longer in the server.")),
                Err(()) => return Ok(private("Discord didn't answer. Try again in a minute.")),
            }
            unrestrict(app, h, target).await;
            update(
                json!({"content": format!("✅ <@{target}> approved by <@{by}>."), "components": []}),
            )
        }
        ("new", "kick" | "ban") => {
            let step = if action == "kick" {
                Act::Kick
            } else {
                Act::Ban(None)
            };
            match act(
                app,
                h,
                target,
                Some(&who),
                step,
                "New account, removed by a moderator",
            )
            .await?
            {
                Ok(_) => update(
                    json!({"content": format!("<@{target}>: {action} by <@{by}>."), "components": []}),
                ),
                Err(why) => private(why),
            }
        }
        ("panel", kind) => {
            let mut inputs = vec![(
                "reason",
                if kind == "note" { "Note" } else { "Reason" },
                true,
                true,
            )];
            if matches!(kind, "mute" | "ban") {
                inputs.push((
                    "duration",
                    "Length (like 2h or 7d; empty for the longest)",
                    false,
                    false,
                ));
            }
            form(
                format!("mod:form:{kind}:{target}"),
                &format!("{kind} member"),
                &inputs,
            )
        }
        ("form", kind) => {
            let reason = field(i, "reason").unwrap_or("No reason given");
            let length = match field(i, "duration") {
                Some(text) => match minutes(text) {
                    Some(m) => Some(m),
                    None => return Ok(private("Lengths look like 30m, 2h, 7d or 2w.")),
                },
                None => None,
            };
            let step = match kind {
                "note" => {
                    let id = add_note(app, target, by, reason).await?;
                    return Ok(private(format!("Note #{id} added for <@{target}>.")));
                }
                "warn" => Act::Warn,
                "mute" => Act::Mute(length.unwrap_or(LONGEST_MUTE)),
                "kick" => Act::Kick,
                "ban" => Act::Ban(length),
                _ => return Ok(private("Unknown action.")),
            };
            private(match act(app, h, target, Some(&who), step, reason).await? {
                Ok(s) | Err(s) => s,
            })
        }
        _ => private("That button isn't in use anymore."),
    })
}

async fn add_note(app: &App, user: &str, by: &str, text: &str) -> Res<i64> {
    Ok(sqlx::query_scalar("INSERT INTO discord_cases(user_id,moderator_id,action,reason) VALUES($1,$2,'note',$3) RETURNING id")
        .bind(user).bind(by).bind(text).fetch_one(&app.db).await?)
}

type CaseRow = (
    i64,
    String,
    String,
    Option<String>,
    chrono::DateTime<Utc>,
    bool,
);
/// The cases a list shows: number, action, reason, who and when.
async fn cases(app: &App, user: &str, which: &str) -> Res<Vec<String>> {
    let rows: Vec<CaseRow> = sqlx::query_as(
        "SELECT id,action,reason,moderator_id,created_at,active FROM discord_cases WHERE user_id=$1 AND
            CASE $2 WHEN 'warnings' THEN action='warn' AND active WHEN 'notes' THEN action='note' ELSE true END
         ORDER BY id DESC LIMIT 15")
        .bind(user).bind(which).fetch_all(&app.db).await?;
    Ok(rows
        .into_iter()
        .map(|(id, action, reason, by, at, active)| {
            let by = by.map_or("Automod".into(), |b| format!("<@{b}>"));
            let cleared = if action == "warn" && !active {
                " (cleared)"
            } else {
                ""
            };
            format!(
                "**#{id}** {action}{cleared}: {} ({by}, {})",
                super::clip(&reason, 150),
                ago(at)
            )
        })
        .collect())
}

/// /modpanel: a member's account, record and action buttons.
pub(super) async fn panel(app: &App, h: &Home, i: &Interaction) -> Res<Value> {
    let Some(user) = i.option("user").and_then(Value::as_str) else {
        return Ok(private("Pick a member."));
    };
    let m = member(app, h, user).await.ok().flatten();
    let joined = m
        .as_ref()
        .and_then(|m| m["joined_at"].as_str())
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map_or("Not in the server".into(), |t| ago(t.to_utc()));
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT action,count(*) FROM discord_cases WHERE user_id=$1 GROUP BY action",
    )
    .bind(user)
    .fetch_all(&app.db)
    .await?;
    let count = |a: &str| counts.iter().find(|c| c.0 == a).map_or(0, |c| c.1);
    let fields = json!([
        {"name": "Account", "value": format!("**Created:** {}\n**Joined:** {joined}\n**Roles:** {}",
            created(user).map_or("unknown".into(), ago),
            m.as_ref().and_then(|m| m["roles"].as_array()).map_or(0, Vec::len))},
        {"name": "Record", "value": format!("**Strikes:** {} active of {}\n**Mutes:** {}\n**Kicks:** {}\n**Bans:** {}\n**Notes:** {}",
            strikes(app, user).await?, count("warn"), count("mute"), count("kick"), count("ban") + count("softban"), count("note")), "inline": true},
    ]);
    let actions = ["warn", "mute", "kick", "ban", "note"]
        .iter()
        .map(|a| {
            let label = format!("{}{}", a[..1].to_uppercase(), &a[1..]);
            button(
                &label,
                if *a == "ban" { 4 } else { 2 },
                format!("mod:panel:{a}:{user}"),
            )
        })
        .collect();
    Ok(
        json!({"type": 4, "data": {"flags": 64, "embeds": [{"title": "Mod panel", "color": 0x58_65F2,
        "description": format!("<@{user}>"), "fields": fields}], "components": [row(actions)], "allowed_mentions": {"parse": []}}}),
    )
}

/// /mod and its subcommands.
pub(super) async fn command(app: &App, h: &Home, i: &Interaction) -> Res<Value> {
    let by = i.user.as_str();
    let text = |key: &str| i.option(key).and_then(Value::as_str);
    let user = text("user");
    let reason = text("reason").unwrap_or("No reason given");
    let length = match text("duration") {
        Some(t) => match minutes(t) {
            Some(m) => Some(m),
            None => return Ok(private("Lengths look like 30m, 2h, 7d or 2w.")),
        },
        None => None,
    };
    let sub = i.sub().unwrap_or_default();
    let step = match sub {
        "warn" => Some(Act::Warn),
        "mute" => Some(Act::Mute(length.unwrap_or(LONGEST_MUTE))),
        "unmute" => Some(Act::Unmute),
        "kick" => Some(Act::Kick),
        "ban" => Some(Act::Ban(length)),
        "softban" => Some(Act::Softban),
        _ => None,
    };
    let who = By::of(app, h, i).await;
    if let (Some(step), Some(user)) = (step, user) {
        return Ok(private(
            match act(app, h, user, Some(&who), step, reason).await? {
                Ok(s) | Err(s) => s,
            },
        ));
    }
    // The rest check their permission here (a length change checks it once the case is known).
    let bit = match sub {
        "purge" => MANAGE_MESSAGES,
        "lockdown" | "unlock" => MANAGE_CHANNELS,
        "protect" | "unprotect" | "bantrap" => MANAGE_GUILD,
        _ => MODERATE,
    };
    if !who.can(bit) {
        return Ok(needs(bit));
    }
    let list = |title: &str, lines: Vec<String>| {
        let description = if lines.is_empty() {
            "Nothing yet.".into()
        } else {
            lines.join("\n")
        };
        json!({"type": 4, "data": {"flags": 64, "embeds": [embed(title, 0x58_65F2, description)], "allowed_mentions": {"parse": []}}})
    };
    Ok(match (sub, user) {
        ("warnings", Some(user)) => list("Active strikes", cases(app, user, "warnings").await?),
        ("notes", Some(user)) => list("Notes", cases(app, user, "notes").await?),
        ("history", Some(user)) => {
            let mut lines = vec![format!("**Active strikes:** {}", strikes(app, user).await?)];
            lines.extend(cases(app, user, "all").await?);
            list("Moderation history", lines)
        }
        ("note", Some(user)) => {
            let id = add_note(app, user, by, text("text").unwrap_or_default()).await?;
            note(
                app,
                h,
                "Note added",
                0x58_65F2,
                format!("<@{by}> added a note about <@{user}>."),
            )
            .await;
            private(format!("Note #{id} added for <@{user}>."))
        }
        ("clearwarn", Some(user)) => {
            let one = i.option("id").and_then(Value::as_i64);
            let cleared = sqlx::query("UPDATE discord_cases SET active=false WHERE user_id=$1 AND action='warn' AND active AND ($2::bigint IS NULL OR id=$2)")
                .bind(user).bind(one).execute(&app.db).await?.rows_affected();
            if cleared > 0 {
                note(
                    app,
                    h,
                    "Strikes cleared",
                    0x2E_CC71,
                    format!("<@{by}> cleared {cleared} strike(s) from <@{user}>."),
                )
                .await;
            }
            private(format!(
                "Cleared {cleared} strike(s). Active strikes: **{}**.",
                strikes(app, user).await?
            ))
        }
        ("duration", Some(user)) => {
            let Some(m) = length else {
                return Ok(private("Give a length, like 2h or 7d."));
            };
            let active: Option<(i64, String)> = sqlx::query_as("SELECT id,action FROM discord_cases WHERE user_id=$1 AND action IN ('ban','mute')
                AND lifted_at IS NULL AND (expires_at IS NULL OR expires_at>now()) ORDER BY id DESC LIMIT 1")
                .bind(user).fetch_optional(&app.db).await?;
            if active.as_ref().is_some_and(|a| a.1 == "ban") && !who.can(BAN) {
                return Ok(needs(BAN));
            }
            let changed: Option<(String, chrono::DateTime<Utc>)> = sqlx::query_as("UPDATE discord_cases SET expires_at=now()+make_interval(mins=>$2) WHERE id=$1 RETURNING action,expires_at")
                .bind(active.map(|a| a.0)).bind(m as i32).fetch_optional(&app.db).await?;
            match changed {
                Some((action, at)) => {
                    if action == "mute" {
                        timeout(app, h, user, Some(m)).await;
                    }
                    note(
                        app,
                        h,
                        "Length changed",
                        ACTION,
                        format!("<@{by}> changed <@{user}>'s {action}: it ends {}.", ago(at)),
                    )
                    .await;
                    private(format!("<@{user}>'s {action} now ends {}.", ago(at)))
                }
                None => private(format!("<@{user}> has no active mute or ban.")),
            }
        }
        ("purge", _) => purge(app, h, i).await?,
        ("protect" | "unprotect", _) => {
            let Some(role) = text("role") else {
                return Ok(private("Pick a role."));
            };
            let sql = if sub == "protect" {
                "UPDATE discord_home SET settings=CASE WHEN settings->'protected' ? $1 THEN settings ELSE jsonb_set(settings,'{protected}',coalesce(settings->'protected','[]')||to_jsonb($1::text)) END"
            } else {
                "UPDATE discord_home SET settings=jsonb_set(settings,'{protected}',coalesce(settings->'protected','[]')-$1)"
            };
            sqlx::query(sql).bind(role).execute(&app.db).await?;
            note(
                app,
                h,
                "Protected roles",
                0x58_65F2,
                format!("<@{by}>: {sub} <@&{role}>."),
            )
            .await;
            private(if sub == "protect" {
                format!("Members with <@&{role}> can't be moderated now.")
            } else {
                format!("<@&{role}> isn't protected anymore.")
            })
        }
        ("lockdown", _) => {
            let channel = text("channel");
            // A manual lockdown holds the Community Member role when there is one, else @everyone.
            let role = home::id(&h.s, "community").unwrap_or_else(|| h.guild.clone());
            match lock(app, h, channel, &role, None, Some(by), length).await? {
                Ok((_, n)) => {
                    let until = length.map_or(String::new(), |m| format!(" for {}", span(m)));
                    note(
                        app,
                        h,
                        "Lockdown",
                        ACTION,
                        format!("<@{by}> locked {n} channel(s){until}."),
                    )
                    .await;
                    if let Some(c) = channel {
                        post(
                            app,
                            c,
                            json!({"content": "🔒 This channel has been locked by a moderator."}),
                        )
                        .await;
                    }
                    private(format!("🔒 Locked {n} channel(s){until}."))
                }
                Err(why) => private(why),
            }
        }
        ("unlock", _) => {
            let id: Option<i64> = sqlx::query_scalar("SELECT id FROM discord_lockdowns WHERE unlocked_at IS NULL AND channel_id IS NOT DISTINCT FROM $1 ORDER BY id DESC LIMIT 1")
                .bind(text("channel")).fetch_optional(&app.db).await?;
            match id {
                Some(id) => {
                    let n = unlock(app, id, Some(by)).await?.unwrap_or(0);
                    note(
                        app,
                        h,
                        "Unlocked",
                        0x2E_CC71,
                        format!("<@{by}> unlocked {n} channel(s)."),
                    )
                    .await;
                    private(format!("🔓 Unlocked {n} channel(s)."))
                }
                None => private("Nothing is locked there."),
            }
        }
        ("bantrap", _) => bantrap(app, h, i).await?,
        _ => private("Pick a member."),
    })
}

/// /mod purge: up to 100 recent messages here, optionally one member's, bots', links or files.
async fn purge(app: &App, h: &Home, i: &Interaction) -> Res<Value> {
    let Some(channel) = i.channel.as_deref() else {
        return Ok(private("Use it in the channel to clean up."));
    };
    let count = i
        .option("count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .clamp(1, 100) as usize;
    let Some((200, list)) = call(
        app,
        Method::GET,
        &format!("/channels/{channel}/messages?limit=100"),
        None,
    )
    .await
    else {
        return Ok(private("Discord didn't answer. Try again in a minute."));
    };
    let who = i.option("user").and_then(Value::as_str);
    let only = i.option("only").and_then(Value::as_str);
    // Discord bulk-deletes only messages younger than 14 days.
    let fresh = Utc::now() - Duration::days(14);
    let ids: Vec<String> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| who.is_none_or(|w| m["author"]["id"] == w))
        .filter(|m| match only {
            Some("bots") => m["author"]["bot"] == true,
            Some("links") => m["content"]
                .as_str()
                .is_some_and(|c| c.contains("http://") || c.contains("https://")),
            Some("files") => m["attachments"].as_array().is_some_and(|a| !a.is_empty()),
            _ => true,
        })
        .filter_map(|m| m["id"].as_str())
        .filter(|id| created(id).is_some_and(|t| t > fresh))
        .take(count)
        .map(String::from)
        .collect();
    sqlx::query(
        "UPDATE discord_messages SET deleted_at=coalesce(deleted_at,now()) WHERE id=ANY($1)",
    )
    .bind(&ids)
    .execute(&app.db)
    .await?;
    let ok = match ids.as_slice() {
        [] => return Ok(private("No matching messages.")),
        [one] => {
            done(
                app,
                Method::DELETE,
                &format!("/channels/{channel}/messages/{one}"),
                None,
            )
            .await
        }
        _ => {
            done(
                app,
                Method::POST,
                &format!("/channels/{channel}/messages/bulk-delete"),
                Some(json!({"messages": ids})),
            )
            .await
        }
    };
    if !ok {
        return Ok(private(
            "Discord refused. Check the bot can manage messages here.",
        ));
    }
    note(
        app,
        h,
        "Messages purged",
        ACTION,
        format!(
            "<@{}> deleted {} message(s) in <#{channel}>.",
            i.user,
            ids.len()
        ),
    )
    .await;
    Ok(private(format!("Deleted {} message(s).", ids.len())))
}

/// /mod bantrap status|set|create|off.
async fn bantrap(app: &App, h: &Home, i: &Interaction) -> Res<Value> {
    let set = async |channel: Option<&str>| -> Res<()> {
        sqlx::query("UPDATE discord_home SET settings=CASE WHEN $1::text IS NULL THEN settings-'ban_trap' ELSE jsonb_set(settings,'{ban_trap}',to_jsonb($1::text)) END")
            .bind(channel).execute(&app.db).await?;
        Ok(())
    };
    Ok(match i.option("action").and_then(Value::as_str) {
        Some("set") => match i.option("channel").and_then(Value::as_str) {
            Some(c) => {
                set(Some(c)).await?;
                note(
                    app,
                    h,
                    "Ban trap",
                    ACTION,
                    format!("<@{}> armed the ban trap in <#{c}>.", i.user),
                )
                .await;
                private(format!(
                    "⚠️ The ban trap is armed in <#{c}>. It applies to staff too."
                ))
            }
            None => private("Pick the channel."),
        },
        Some("create") => {
            let made = call(app, Method::POST, &format!("/guilds/{}/channels", h.guild), Some(json!({
                "name": "do-not-type", "type": 0,
                "topic": "Anti-spam trap: posting here means an immediate ban. Do not type here."}))).await;
            let Some(c) = made
                .filter(|(status, _)| *status < 300)
                .and_then(|(_, channel)| channel["id"].as_str().map(String::from))
            else {
                return Ok(private(
                    "Discord refused. Check the bot can manage channels.",
                ));
            };
            post(app, &c, json!({"content": "⚠️ **Do not type in this channel.** Posting here means an immediate ban and removal of your recent messages."})).await;
            set(Some(&c)).await?;
            note(
                app,
                h,
                "Ban trap",
                ACTION,
                format!("<@{}> created the ban trap <#{c}>.", i.user),
            )
            .await;
            private(format!("Ban trap created and armed: <#{c}>."))
        }
        Some("off") => {
            set(None).await?;
            note(
                app,
                h,
                "Ban trap",
                ACTION,
                format!("<@{}> turned the ban trap off.", i.user),
            )
            .await;
            private("The ban trap is off.")
        }
        _ => private(match home::id(&h.s, "ban_trap") {
            Some(c) => format!(
                "⚠️ The ban trap is armed in <#{c}>. Anyone but the server owner who posts there is banned."
            ),
            None => "The ban trap is off.".into(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_lengths() {
        assert_eq!(span(30), "30 minute(s)");
        assert_eq!(span(120), "2 hour(s)");
        assert_eq!(span(2880), "2 day(s)");
        assert_eq!(span(20_160), "2 week(s)");
        assert_eq!(Act::Mute(99_999).minutes(), Some(LONGEST_MUTE));
        assert_eq!(Act::Ban(None).minutes(), None);
    }
}
