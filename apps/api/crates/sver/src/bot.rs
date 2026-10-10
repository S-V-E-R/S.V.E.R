//! The channel bot, part 2 (docs/COMMUNITY.md "Chat commands and the channel bot"): event lines in
//! the chosen personality (docs/LORE.md "The faction bots"), AutoMod with its warn-and-timeout
//! ladder, giveaways, and the Nightbot/Fossabot command import (docs/DEVELOPER_PLATFORM.md §7).
use crate::{
    App, auth,
    commands::{bot, say},
    moderation::{self, Role},
    profiles::{self, Fail, Res},
};
use axum::{Json, Router, extract::State, routing::put};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Event {
    Follow,
    Sub,
    Raid,
    Timeout,
}
/// Lines per bot (PYRE, ECHO, FAVOR, VOLK), personality (chill, battle, event) and event (follow,
/// sub, raid, timeout). `{user}` is the person, `{count}` a month or raid size.
const LINES: [[[&str; 4]; 3]; 4] = [
    [
        [
            "Welcome, {user}.",
            "Thanks for subscribing, {user}.",
            "{user} is raiding with {count}. Welcome, everyone.",
            "{user}, take a short break.",
        ],
        [
            "Another one walks into the fire. Welcome, {user}.",
            "Oath sworn, {user}. The forge remembers. Month {count}.",
            "Warband at the gate: {user} with {count}. Stand and greet them.",
            "Cool off, {user}. Come back sharper.",
        ],
        [
            "{user} STEPS INTO THE FIRE!",
            "{user} SWEARS THE OATH! Month {count}! The forge roars!",
            "WARBAND OF {count} AT THE GATE, LED BY {user}!",
            "{user}, cool off. The fire waits.",
        ],
    ],
    [
        [
            "Welcome, {user}. Glad you found us.",
            "Thanks for subscribing, {user}. Month {count}.",
            "{user} brought {count} friends. Welcome.",
            "{user}, paused for a bit.",
        ],
        [
            "New signal detected: {user}. Logging it.",
            "A pattern begins: {user}, month {count}.",
            "Arrivals inbound: {user} with {count}. Adjusting the model.",
            "Pattern flagged, {user}. Take a pause.",
        ],
        [
            "SIGNAL LOCKED: {user}!",
            "{user}: month {count} added to the archive!",
            "{count} ARRIVALS FROM {user}! THE PATTERN GROWS!",
            "{user}, pattern flagged. Back soon.",
        ],
    ],
    [
        [
            "Welcome in, {user}!",
            "{user} subscribed. Thank you!",
            "{user} and {count} friends just arrived. Say hi!",
            "{user}, a quick breather, okay?",
        ],
        [
            "There's a seat for you, {user}. There always was.",
            "{user}, your fortune's in the hall now. Month {count}.",
            "Open the doors, {user} brought {count}!",
            "Easy, {user}. Take a breather.",
        ],
        [
            "{user} TAKES A SEAT AT THE TABLE!",
            "{user} rolls the dice: month {count}! Fortune smiles!",
            "{user} BRINGS {count} TO THE PARTY!",
            "Easy, {user}! Catch your breath.",
        ],
    ],
    [
        [
            "Welcome, {user}.",
            "Thanks for the support, {user}.",
            "Welcome, {user} and company ({count}).",
            "{user}, a short pause.",
        ],
        [
            "Noted. Welcome under the Accord, {user}.",
            "The Accord thanks you, {user}. Month {count}.",
            "{user} and a company of {count} cross under the Accord.",
            "The Accord holds, {user}. Take a breath.",
        ],
        [
            "{user} JOINS THE WATCH!",
            "{user} pledges to the Accord! Month {count}!",
            "{user} CROSSES WITH {count}! THE ACCORD HOLDS!",
            "{user}, the Accord pauses you.",
        ],
    ],
];
pub fn line(bot: &str, personality: &str, event: Event, user: &str, count: i64) -> String {
    let b = ["pyre", "echo", "favor", "volk"]
        .iter()
        .position(|k| *k == bot)
        .unwrap_or(3);
    let p = ["chill", "battle", "event"]
        .iter()
        .position(|k| *k == personality)
        .unwrap_or(0);
    LINES[b][p][event as usize]
        .replace("{user}", user)
        .replace("{count}", &count.to_string())
}
async fn personality(app: &App, channel: &str) -> Res<String> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT bot_personality FROM chat_settings WHERE channel_id=$1),'chill')",
    )
    .bind(channel)
    .fetch_one(&app.db)
    .await?)
}
async fn announce(app: &App, channel: &str, event: Event, user: &str, count: i64) -> Res<()> {
    let (key, _) = bot(app, channel).await?;
    let text = line(key, &personality(app, channel).await?, event, user, count);
    say(app, channel, &text).await
}

// ---- Event lines (a transactional outbox) ----

/// Records a follow or a sub month for the bot to greet (in the caller's transaction, so nothing
/// is announced for a change that rolls back). `user` is the person's account.
pub async fn record(
    db: &mut PgConnection,
    channel: &str,
    kind: &str,
    user: &str,
    count: i64,
) -> Res<()> {
    sqlx::query("INSERT INTO bot_events(channel_id,kind,name,count) SELECT $1,$2,display_name,$4 FROM channel_users WHERE id=$3")
        .bind(channel).bind(kind).bind(user).bind(count as i32)
        .execute(db).await?;
    Ok(())
}
/// A counted raid (the raider's name and how many arrived).
pub async fn record_raid(app: &App, channel: &str, raider: &str, arrivals: i64) -> Res<()> {
    sqlx::query("INSERT INTO bot_events(channel_id,kind,name,count) VALUES($1,'raid',$2,$3)")
        .bind(channel)
        .bind(raider)
        .bind(arrivals as i32)
        .execute(&app.db)
        .await?;
    Ok(())
}
/// Every few seconds: greets recent events on live channels; anything else is dropped.
pub async fn drain(app: &App) -> Res<()> {
    let events: Vec<(String, String, String, i32, bool)> = sqlx::query_as("DELETE FROM bot_events e RETURNING e.channel_id,e.kind,e.name,e.count,
        e.created_at>now()-interval '2 minutes' AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=e.channel_id AND b.state='LIVE')")
        .fetch_all(&app.db)
        .await?;
    for (channel, kind, name, count, live) in events {
        if !live {
            continue;
        }
        let event = match kind.as_str() {
            "follow" => Event::Follow,
            "sub" => Event::Sub,
            _ => Event::Raid,
        };
        announce(app, &channel, event, &name, count.into()).await?;
    }
    Ok(())
}

// ---- AutoMod ----

/// Which AutoMod rule a message breaks, if any (caps, repeats, spam), before the database check
/// for a repeated message.
pub fn rule(body: &str, caps: bool, repeats: bool, spam: bool) -> Option<&'static str> {
    let letters: Vec<char> = body.chars().filter(|c| c.is_alphabetic()).collect();
    if caps
        && letters.len() >= 10
        && letters.iter().filter(|c| c.is_uppercase()).count() * 10 > letters.len() * 7
    {
        return Some("caps");
    }
    if repeats {
        let chars: Vec<char> = body.chars().collect();
        if chars
            .windows(10)
            .any(|w| w.iter().all(|c| *c == w[0] && !c.is_whitespace()))
        {
            return Some("repeats");
        }
    }
    if spam {
        let symbols = body
            .chars()
            .filter(|c| {
                !c.is_alphanumeric() && !c.is_whitespace() && !".,!?'\"()-:;@#".contains(*c)
            })
            .count();
        if symbols > 15 {
            return Some("spam");
        }
    }
    None
}
const WARNINGS: [(&str, &str); 3] = [
    ("caps", "Please ease off the caps."),
    ("repeats", "Please don't repeat yourself."),
    ("spam", "Please go easy on symbols and emojis."),
];
/// Runs before a message is stored (moderation::check_send); channel roles are exempt. A first
/// strike in an hour warns (the message is refused); later ones time out per the channel's ladder,
/// announced by the bot and logged for moderators, who can lift it.
pub async fn automod(app: &App, channel: &str, user: &auth::User, body: &str) -> Res<()> {
    let settings: Option<(bool, bool, bool, Vec<i32>)> = sqlx::query_as("SELECT automod_caps,automod_repeats,automod_spam,automod_ladder FROM chat_settings WHERE channel_id=$1")
        .bind(channel).fetch_optional(&app.db).await?;
    let Some((caps, repeats, spam, ladder)) = settings.filter(|s| s.0 || s.1 || s.2) else {
        return Ok(());
    };
    let mut broken = rule(body, caps, repeats, spam);
    if broken.is_none() && repeats {
        let same: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_messages WHERE channel_id=$1 AND author_id=$2 AND squad_id IS NULL AND created_at>now()-interval '30 seconds' AND lower(body)=lower($3))")
            .bind(channel).bind(&user.id).bind(body.trim()).fetch_one(&app.db).await?;
        broken = same.then_some("repeats");
    }
    let Some(broken) = broken else {
        return Ok(());
    };
    if moderation::role_of(app, channel, user).await?.is_some() {
        return Ok(());
    }
    let mut tx = app.db.begin().await?;
    let strikes: i64 = sqlx::query_scalar("SELECT count(*) FROM automod_strikes WHERE channel_id=$1 AND user_id=$2 AND at>now()-interval '1 hour'")
        .bind(channel).bind(&user.id).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO automod_strikes(channel_id,user_id,rule) VALUES($1,$2,$3)")
        .bind(channel)
        .bind(&user.id)
        .bind(broken)
        .execute(&mut *tx)
        .await?;
    let seconds = ladder
        .get(strikes as usize)
        .or(ladder.last())
        .copied()
        .unwrap_or(0);
    let warning = WARNINGS.iter().find(|w| w.0 == broken).map_or("", |w| w.1);
    if seconds <= 0 {
        tx.commit().await?;
        return Err(Fail::bad(format!(
            "The channel bot held your message. {warning}"
        )));
    }
    sqlx::query("INSERT INTO channel_restrictions(channel_id,user_id,kind,until) VALUES($1,$2,'timeout',now()+make_interval(secs=>$3)) ON CONFLICT(channel_id,user_id,kind) DO UPDATE SET until=greatest(channel_restrictions.until,EXCLUDED.until)")
        .bind(channel).bind(&user.id).bind(f64::from(seconds)).execute(&mut *tx).await?;
    let detail = json!({"rule": broken, "seconds": seconds, "by": "bot"});
    let reason = format!("AutoMod: {broken}");
    moderation::log(
        &mut tx,
        channel,
        &user.id,
        Role::Owner,
        "automod_timeout",
        Some(&user.id),
        None,
        detail,
        &reason,
    )
    .await?;
    tx.commit().await?;
    let name: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM channel_users WHERE id=$1")
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    announce(
        app,
        channel,
        Event::Timeout,
        name.as_deref().unwrap_or(&user.username),
        0,
    )
    .await?;
    let length = if seconds >= 120 {
        format!("{} minutes", seconds / 60)
    } else {
        format!("{seconds} seconds")
    };
    Err(Fail::denied(format!(
        "The channel bot timed you out for {length}. {warning}"
    )))
}
#[derive(Deserialize)]
pub struct AutoMod {
    caps: bool,
    repeats: bool,
    spam: bool,
    ladder: Vec<i32>,
}
/// PUT /api/me/automod
async fn set_automod(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<AutoMod>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if input.ladder.is_empty()
        || input.ladder.len() > 5
        || input
            .ladder
            .iter()
            .any(|s| *s != 0 && !(10..=1_209_600).contains(s))
    {
        return Err(Fail::field(
            "ladder",
            "Use 1–5 steps: a warning (0) or 10 seconds to 14 days.",
        ));
    }
    sqlx::query("INSERT INTO chat_settings(channel_id,automod_caps,automod_repeats,automod_spam,automod_ladder) VALUES($1,$2,$3,$4,$5) ON CONFLICT(channel_id) DO UPDATE SET automod_caps=$2,automod_repeats=$3,automod_spam=$4,automod_ladder=$5")
        .bind(&user.id).bind(input.caps).bind(input.repeats).bind(input.spam).bind(&input.ladder).execute(&app.db).await?;
    Ok(Json(json!({"automod": settings(&app, &user.id).await?})))
}
/// The AutoMod settings for Creator Studio.
pub async fn settings(app: &App, owner: &str) -> Res<Value> {
    let row: Option<(bool, bool, bool, Vec<i32>)> = sqlx::query_as("SELECT automod_caps,automod_repeats,automod_spam,automod_ladder FROM chat_settings WHERE channel_id=$1")
        .bind(owner).fetch_optional(&app.db).await?;
    let (caps, repeats, spam, ladder) = row.unwrap_or((false, false, false, vec![0, 60, 600]));
    Ok(json!({"caps": caps, "repeats": repeats, "spam": spam, "ladder": ladder}))
}

// ---- Giveaways ----

/// `!giveaway start KEYWORD`, `!giveaway end` and `!giveaway cancel` (owner and moderators), and
/// entries: a viewer who types the keyword while watching (a Counted session) enters once.
/// Returns true when the message was a giveaway command.
pub async fn giveaway(app: &App, channel: &str, user: &auth::User, body: &str) -> Res<bool> {
    let words: Vec<&str> = body.split_whitespace().collect();
    if words
        .first()
        .is_some_and(|w| w.eq_ignore_ascii_case("!giveaway"))
    {
        if moderation::role_of(app, channel, user).await?.is_none() {
            return Ok(true);
        }
        let text = match words.get(1).map(|w| w.to_ascii_lowercase()).as_deref() {
            Some("start") => match words
                .get(2)
                .filter(|k| (1..=25).contains(&k.chars().count()))
            {
                None => "Start a giveaway with !giveaway start KEYWORD.".to_string(),
                Some(keyword) => {
                    let started = sqlx::query("INSERT INTO giveaways(id,channel_id,keyword,started_by) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                        .bind(profiles::new_id()).bind(channel).bind(keyword).bind(&user.id).execute(&app.db).await?;
                    if started.rows_affected() == 1 {
                        format!(
                            "Giveaway! Type {keyword} in chat while you're watching to enter. The prize is the streamer's; good luck!"
                        )
                    } else {
                        "A giveaway is already running. End it with !giveaway end.".into()
                    }
                }
            },
            Some("end") => {
                let ended: Option<(Option<String>, i64)> = sqlx::query_as("WITH g AS (SELECT id FROM giveaways WHERE channel_id=$1 AND ended_at IS NULL),
                    w AS (SELECT e.user_id FROM giveaway_entries e JOIN g ON g.id=e.giveaway_id
                          WHERE NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=$1 AND r.user_id=e.user_id AND r.kind='ban')
                          ORDER BY random() LIMIT 1),
                    u AS (UPDATE giveaways SET ended_at=now(),winner_id=(SELECT user_id FROM w),entries=(SELECT count(*) FROM giveaway_entries x WHERE x.giveaway_id=giveaways.id)
                          WHERE id=(SELECT id FROM g) RETURNING winner_id,entries)
                    SELECT (SELECT display_name FROM channel_users c WHERE c.id=u.winner_id),u.entries::bigint FROM u")
                    .bind(channel).fetch_optional(&app.db).await?;
                match ended {
                    Some((Some(winner), entries)) => format!(
                        "The giveaway winner is {winner}! ({entries} entered.) Congratulations!"
                    ),
                    Some(_) => "The giveaway ended with no entries.".into(),
                    None => "There's no giveaway running.".into(),
                }
            }
            Some("cancel") => {
                sqlx::query(
                    "UPDATE giveaways SET ended_at=now() WHERE channel_id=$1 AND ended_at IS NULL",
                )
                .bind(channel)
                .execute(&app.db)
                .await?;
                "The giveaway was cancelled.".into()
            }
            _ => "Giveaways: !giveaway start KEYWORD, !giveaway end, !giveaway cancel.".into(),
        };
        say(app, channel, &text).await?;
        return Ok(true);
    }
    // An entry: the exact keyword, from someone with a Counted session on this live channel.
    if words.len() == 1 && user.id != channel {
        sqlx::query("INSERT INTO giveaway_entries(giveaway_id,user_id)
            SELECT g.id,$2 FROM giveaways g WHERE g.channel_id=$1 AND g.ended_at IS NULL AND lower(g.keyword)=lower($3)
              AND EXISTS(SELECT 1 FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id
                         WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') AND l.viewer_key='u:'||$2 AND l.expires_at>now() AND l.level IN ('counted','trusted'))
            ON CONFLICT DO NOTHING")
            .bind(channel).bind(&user.id).bind(words[0]).execute(&app.db).await?;
    }
    Ok(false)
}

// ---- Command import (Nightbot, Fossabot) ----

/// Common variables, translated; anything else in `$(…)` or `${…}` is flagged.
const VARIABLES: [(&str, &str); 10] = [
    ("$(user)", "{user}"),
    ("$(touser)", "{user}"),
    ("$(channel)", "{channel}"),
    ("$(uptime)", "{uptime}"),
    ("$(game)", "{game}"),
    ("${user}", "{user}"),
    ("${touser}", "{user}"),
    ("${channel}", "{channel}"),
    ("${uptime}", "{uptime}"),
    ("${game}", "{game}"),
];
/// One pasted line, `!name response` or `!name: response`: (name, reply, flagged variables).
pub fn parse_import(line: &str) -> Option<(String, String, Vec<String>)> {
    let rest = line.trim().strip_prefix('!')?;
    let (name, reply) = rest.split_once(|c: char| c.is_whitespace() || c == ':')?;
    let name = name.to_ascii_lowercase();
    let mut reply = reply
        .trim_start_matches([':', ' ', '-', '\t'])
        .trim()
        .to_string();
    for (from, to) in VARIABLES {
        reply = reply.replace(from, to);
    }
    let mut flagged = Vec::new();
    for (open, close) in [("$(", ')'), ("${", '}')] {
        let mut from = 0;
        while let Some(at) = reply[from..].find(open) {
            let start = from + at;
            let end = reply[start..]
                .find(close)
                .map_or(reply.len(), |e| start + e + 1);
            flagged.push(reply[start..end].to_string());
            from = end;
        }
    }
    (!name.is_empty() && !reply.is_empty()).then_some((name, reply, flagged))
}
#[derive(Deserialize)]
pub struct Import {
    text: String,
}
/// POST /api/me/commands-import: a pasted Nightbot or Fossabot command list becomes commands.
async fn import(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Import>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if input.text.len() > 50_000 {
        return Err(Fail::bad("Paste up to 50,000 characters."));
    }
    let (mut imported, mut skipped, mut flagged) = (Vec::new(), Vec::new(), Vec::new());
    for line in input
        .text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(200)
    {
        let short: String = line.chars().take(80).collect();
        let Some((name, reply, vars)) = parse_import(line) else {
            skipped.push(json!({"line": short, "why": "Not a !command and reply."}));
            continue;
        };
        if let Err(why) =
            crate::commands::store(&app, &user.id, &name, &reply, "everyone", 10).await
        {
            skipped.push(json!({"line": short, "why": why.message}));
            continue;
        }
        if !vars.is_empty() {
            flagged.push(json!({"name": name, "variables": vars}));
        }
        imported.push(name);
    }
    Ok(Json(
        json!({"imported": imported, "skipped": skipped, "flagged": flagged}),
    ))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/automod", put(set_automod))
        .route("/api/me/commands-import", axum::routing::post(import))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_and_lines() {
        assert_eq!(
            rule("THIS IS ALL CAPS YELLING", true, false, false),
            Some("caps")
        );
        assert_eq!(rule("Hello THERE friend of mine", true, false, false), None);
        assert_eq!(rule("noooooooooooooo", false, true, false), Some("repeats"));
        assert_eq!(rule("$$$%%%^^^&&&***~~~", false, false, true), Some("spam"));
        assert_eq!(rule("normal message", true, true, true), None);
        assert_eq!(
            line("volk", "battle", Event::Raid, "Ana", 12),
            "Ana and a company of 12 cross under the Accord."
        );
        assert_eq!(
            line("nope", "nope", Event::Follow, "Ana", 0),
            "Welcome, Ana."
        );
    }
    #[test]
    fn imports_translate_and_flag() {
        assert_eq!(
            parse_import("!discord Join us: https://discord.gg/x $(user)"),
            Some((
                "discord".into(),
                "Join us: https://discord.gg/x {user}".into(),
                vec![]
            ))
        );
        let (name, reply, flagged) =
            parse_import("!Hug: $(user) hugs ${random.chatter} $(count)").unwrap();
        assert_eq!(
            (name.as_str(), reply.as_str()),
            ("hug", "{user} hugs ${random.chatter} $(count)")
        );
        assert_eq!(flagged, vec!["$(count)", "${random.chatter}"]);
        assert_eq!(parse_import("not a command"), None);
    }
}
