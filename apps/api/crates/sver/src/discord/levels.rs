//! Discord levels and ranks on the home server (docs/DISCORD_BOT.md "Levels and ranks"): XP for
//! messages, reactions, voice and streaming, multipliers, the daily cap, the 22-rank faction ladder
//! with its roles, and `/rank`, `/leaderboard`, `/profile`, `/xpmult` and `/ranks`.
use super::{
    call, card,
    commands::{Interaction, MANAGE_GUILD, defer, embed, finish, private},
    home, quests, role, send,
};
use crate::{App, profiles::Res};
use reqwest::Method;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const DAILY_CAP: i64 = 2_000;
pub const FACTIONS: [&str; 3] = ["myria", "aetheron", "glint"];

/// floor(0.1·√XP): level L needs 100·L² XP.
pub fn level(xp: i64) -> i64 {
    ((xp.max(0) as f64).sqrt() * 0.1).floor() as i64
}
pub fn emoji(faction: Option<&str>) -> &'static str {
    match faction {
        Some("myria") => "⚔️",
        Some("aetheron") => "🛡️",
        Some("glint") => "✨",
        _ => "⭐",
    }
}
pub fn color(faction: Option<&str>) -> u32 {
    match faction {
        Some("myria") => 0xe7_4c3c,
        Some("aetheron") => 0x44_88ff,
        Some("glint") => 0xff_d700,
        _ => 0x95_a5a6,
    }
}
/// A random whole number below `n` (a v4 UUID's random bits, as elsewhere in the codebase).
fn random(n: u64) -> i64 {
    ((uuid::Uuid::new_v4().as_u128() as u64) % n.max(1)) as i64
}

/// Where XP comes from, and which faction points it also earns.
#[derive(Clone, Copy)]
pub enum Source {
    Message,
    Reaction,
    Voice,
    Watching,
    Stream,
    Quest,
    Milestone,
    Anniversary,
}
impl Source {
    fn points(self) -> Option<&'static str> {
        match self {
            Self::Message | Self::Reaction => Some("chat"),
            Self::Voice => Some("voice"),
            Self::Watching | Self::Stream => Some("stream_support"),
            // Quest points are the quest's own reward (quests::complete).
            Self::Quest | Self::Milestone | Self::Anniversary => None,
        }
    }
}

/// The member earning XP: their Discord id, roles and the channel it happened in.
pub struct Member<'a> {
    pub user: &'a str,
    pub roles: &'a [String],
    pub channel: Option<&'a str>,
}

/// The member's Discord faction.
pub async fn faction(app: &App, user: &str) -> Res<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT faction FROM discord_members WHERE discord_user=$1")
            .bind(user)
            .fetch_optional(&app.db)
            .await?
            .flatten(),
    )
}

/// Gives XP after multipliers and the daily cap, with the faction points it earns, then promotes.
/// Returns the XP actually given.
pub async fn award(app: &App, m: Member<'_>, base: i64, source: Source) -> Res<i64> {
    if base <= 0 || !home::on(&home::settings(app).await, "leveling") {
        return Ok(0);
    }
    let faction = faction(app, m.user).await?;
    let mut tx = app.db.begin().await?;
    // Multipliers stack by multiplying; a faction multiplier is for that faction only.
    let factor: f64 = sqlx::query_scalar("SELECT coalesce(exp(sum(ln(factor::float8))),1) FROM discord_multipliers WHERE starts_at<=now() AND (ends_at IS NULL OR ends_at>now())
        AND (kind='event' OR (kind='role' AND target=ANY($1)) OR (kind='channel' AND target=$2) OR (kind='user' AND target=$3) OR (kind='faction' AND target=$4))")
        .bind(m.roles).bind(m.channel).bind(m.user).bind(&faction).fetch_one(&mut *tx).await?;
    let adjusted = (base as f64 * factor).round() as i64;
    sqlx::query("INSERT INTO discord_xp(discord_user) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(m.user)
        .execute(&mut *tx)
        .await?;
    let today: i64 = sqlx::query_scalar("SELECT CASE WHEN day=current_date THEN day_xp ELSE 0 END::bigint FROM discord_xp WHERE discord_user=$1 FOR UPDATE")
        .bind(m.user).fetch_one(&mut *tx).await?;
    let given = adjusted.min(DAILY_CAP - today).max(0);
    if given == 0 {
        return Ok(0);
    }
    sqlx::query("UPDATE discord_xp SET total=total+$2,day_xp=CASE WHEN day=current_date THEN day_xp ELSE 0 END+$2,day=current_date,updated_at=now() WHERE discord_user=$1")
        .bind(m.user).bind(given).execute(&mut *tx).await?;
    if let (Some(faction), Some(kind)) = (&faction, source.points()) {
        sqlx::query(
            "INSERT INTO discord_points(discord_user,faction,kind,points) VALUES($1,$2,$3,$4)",
        )
        .bind(m.user)
        .bind(faction)
        .bind(kind)
        .bind(given as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    promote(app, m.user).await?;
    quests::check(app, m.user).await?;
    Ok(given)
}

/// Counters shown on `/profile` and the leaderboards.
#[derive(Clone, Copy)]
pub enum Counter {
    Voice,
    Watch,
    Stream,
}
pub async fn count(app: &App, user: &str, counter: Counter, n: i64) -> Res<()> {
    let column = match counter {
        Counter::Voice => "voice_minutes",
        Counter::Watch => "watch_minutes",
        Counter::Stream => "stream_minutes",
    };
    // The column is one of three fixed names; the user and amount are bound.
    sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO discord_xp(discord_user,{column}) VALUES($1,$2) ON CONFLICT(discord_user) DO UPDATE SET {column}=discord_xp.{column}+$2")))
        .bind(user).bind(n as i32).execute(&app.db).await?;
    Ok(())
}

/// A tracked stream ended: its minutes, and 50 XP per full 30 minutes (docs "XP sources").
pub async fn stream_ended(app: &App, user: &str, minutes: i64) -> Res<()> {
    if minutes <= 0 {
        return Ok(());
    }
    count(app, user, Counter::Stream, minutes).await?;
    let m = Member {
        user,
        roles: &[],
        channel: None,
    };
    award(app, m, 50 * (minutes / 30), Source::Stream).await?;
    Ok(())
}

async fn on_home(app: &App, d: &Value) -> bool {
    d["guild_id"].as_str().is_some() && d["guild_id"].as_str() == home::guild(app).await.as_deref()
}
fn roles(member: &Value) -> Vec<String> {
    member["roles"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|r| r.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// MESSAGE_CREATE: 15–25 XP, at most once a minute, for a message of 5+ characters.
pub async fn message(app: &App, d: &Value) -> Res<()> {
    if d["author"]["bot"] == true || d["webhook_id"].is_string() || !on_home(app, d).await {
        return Ok(());
    }
    let settings = home::settings(app).await;
    let (Some(user), Some(channel)) = (d["author"]["id"].as_str(), d["channel_id"].as_str()) else {
        return Ok(());
    };
    if !home::on(&settings, "leveling")
        || d["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .chars()
            .count()
            < 5
        || home::ids(&settings, "xp_off").iter().any(|c| c == channel)
    {
        return Ok(());
    }
    // The cooldown and the count move together, so two messages at once earn once.
    let fresh: Option<i32> = sqlx::query_scalar("INSERT INTO discord_xp(discord_user,message_at,messages) VALUES($1,now(),1)
        ON CONFLICT(discord_user) DO UPDATE SET message_at=now(),messages=discord_xp.messages+1
        WHERE discord_xp.message_at IS NULL OR discord_xp.message_at<now()-interval '60 seconds' RETURNING 1")
        .bind(user).fetch_optional(&app.db).await?;
    if fresh.is_none() {
        return Ok(());
    }
    let roles = roles(&d["member"]);
    let m = Member {
        user,
        roles: &roles,
        channel: Some(channel),
    };
    award(app, m, 15 + random(11), Source::Message).await?;
    quests::bump(app, user, "chat_messages", 1).await
}

/// MESSAGE_REACTION_ADD: 5 XP, at most once every 30 seconds, not on your own message.
pub async fn reaction(app: &App, d: &Value) -> Res<()> {
    let Some(user) = d["user_id"].as_str() else {
        return Ok(());
    };
    if d["member"]["user"]["bot"] == true
        || d["message_author_id"].as_str() == Some(user)
        || !on_home(app, d).await
        || !home::on(&home::settings(app).await, "leveling")
    {
        return Ok(());
    }
    let fresh: Option<i32> = sqlx::query_scalar("INSERT INTO discord_xp(discord_user,reaction_at,reactions) VALUES($1,now(),1)
        ON CONFLICT(discord_user) DO UPDATE SET reaction_at=now(),reactions=discord_xp.reactions+1
        WHERE discord_xp.reaction_at IS NULL OR discord_xp.reaction_at<now()-interval '30 seconds' RETURNING 1")
        .bind(user).fetch_optional(&app.db).await?;
    if fresh.is_none() {
        return Ok(());
    }
    let roles = roles(&d["member"]);
    let m = Member {
        user,
        roles: &roles,
        channel: d["channel_id"].as_str(),
    };
    award(app, m, 5, Source::Reaction).await?;
    Ok(())
}

async fn in_voice(
    app: &App,
    user: &str,
    channel: &str,
    roles: &[String],
    state: &Value,
) -> Res<()> {
    let quiet = state["self_mute"] == true && state["self_deaf"] == true;
    sqlx::query("INSERT INTO discord_voice(discord_user,channel_id,roles,streaming,quiet_since) VALUES($1,$2,$3,$4,CASE WHEN $5 THEN now() END)
        ON CONFLICT(discord_user) DO UPDATE SET channel_id=$2,roles=$3,streaming=$4,quiet_since=CASE WHEN $5 THEN coalesce(discord_voice.quiet_since,now()) END")
        .bind(user).bind(channel).bind(roles).bind(state["self_stream"] == true).bind(quiet).execute(&app.db).await?;
    Ok(())
}

/// VOICE_STATE_UPDATE: who is in which voice channel, muted or streaming.
pub async fn voice_state(app: &App, d: &Value) -> Res<()> {
    let Some(user) = d["user_id"].as_str() else {
        return Ok(());
    };
    if d["member"]["user"]["bot"] == true || !on_home(app, d).await {
        return Ok(());
    }
    match d["channel_id"].as_str() {
        Some(channel) => in_voice(app, user, channel, &roles(&d["member"]), d).await,
        None => {
            sqlx::query("DELETE FROM discord_voice WHERE discord_user=$1")
                .bind(user)
                .execute(&app.db)
                .await?;
            Ok(())
        }
    }
}

/// GUILD_CREATE for the home server: its AFK channel, and everyone in voice right now (so a
/// restart never loses voice time).
pub async fn guild_create(app: &App, d: &Value) -> Res<()> {
    if d["id"].as_str() != home::guild(app).await.as_deref() {
        return Ok(());
    }
    sqlx::query("UPDATE discord_guilds SET afk_channel=$2 WHERE guild_id=$1")
        .bind(d["id"].as_str())
        .bind(d["afk_channel_id"].as_str())
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM discord_voice")
        .execute(&app.db)
        .await?;
    let members: HashMap<&str, &Value> = d["members"]
        .as_array()
        .map(|m| {
            m.iter()
                .filter_map(|m| Some((m["user"]["id"].as_str()?, m)))
                .collect()
        })
        .unwrap_or_default();
    for state in d["voice_states"].as_array().into_iter().flatten() {
        let (Some(user), Some(channel)) = (state["user_id"].as_str(), state["channel_id"].as_str())
        else {
            continue;
        };
        let member = members.get(user).copied().unwrap_or(&Value::Null);
        if member["user"]["bot"] == true {
            continue;
        }
        in_voice(app, user, channel, &roles(member), state).await?;
    }
    Ok(())
}

/// Each minute: 3 XP for every minute in voice with another person (not in the AFK channel, not
/// muted and deafened for over 15 minutes), plus 2 while someone else there is streaming.
pub async fn minute(app: &App) -> Res<()> {
    let Some(guild) = home::guild(app).await else {
        return Ok(());
    };
    if !home::on(&home::settings(app).await, "leveling") {
        return Ok(());
    }
    let afk: Option<String> =
        sqlx::query_scalar("SELECT afk_channel FROM discord_guilds WHERE guild_id=$1")
            .bind(&guild)
            .fetch_optional(&app.db)
            .await?
            .flatten();
    // Every due row is marked credited whether or not it earns, so skipped minutes never pay later.
    let due: Vec<(String, String, Vec<String>, bool)> = sqlx::query_as("UPDATE discord_voice SET credited_at=now() WHERE credited_at<=now()-interval '59 seconds'
        RETURNING discord_user,channel_id,roles,coalesce(quiet_since<now()-interval '15 minutes',false)")
        .fetch_all(&app.db).await?;
    if due.is_empty() {
        return Ok(());
    }
    let everyone: Vec<(String, String, bool)> =
        sqlx::query_as("SELECT discord_user,channel_id,streaming FROM discord_voice")
            .fetch_all(&app.db)
            .await?;
    for (user, channel, roles, quiet) in due {
        let company = everyone.iter().filter(|e| e.1 == channel).count();
        if company <= 1 || quiet || afk.as_deref() == Some(channel.as_str()) {
            continue;
        }
        let m = || Member {
            user: &user,
            roles: &roles,
            channel: Some(&channel),
        };
        count(app, &user, Counter::Voice, 1).await?;
        award(app, m(), 3, Source::Voice).await?;
        quests::bump(app, &user, "voice_minutes", 1).await?;
        if everyone
            .iter()
            .any(|e| e.1 == channel && e.0 != user && e.2)
        {
            count(app, &user, Counter::Watch, 1).await?;
            award(app, m(), 2, Source::Watching).await?;
            quests::bump(app, &user, "stream_watch", 1).await?;
        }
    }
    Ok(())
}

/// One rank of the ladder.
#[derive(sqlx::FromRow, Clone)]
pub struct Rank {
    pub rank: i16,
    pub xp: i64,
    pub base_name: String,
    pub myria: String,
    pub aetheron: String,
    pub glint: String,
    pub roles: Value,
}
impl Rank {
    pub fn name(&self, faction: Option<&str>) -> &str {
        match faction {
            Some("myria") => &self.myria,
            Some("aetheron") => &self.aetheron,
            Some("glint") => &self.glint,
            _ => &self.base_name,
        }
    }
    pub fn role(&self, faction: Option<&str>) -> Option<String> {
        self.roles[faction.unwrap_or("none")]
            .as_str()
            .map(String::from)
    }
}
pub async fn ladder(app: &App) -> Res<Vec<Rank>> {
    Ok(sqlx::query_as(
        "SELECT rank,xp,base_name,myria,aetheron,glint,roles FROM discord_ranks ORDER BY rank",
    )
    .fetch_all(&app.db)
    .await?)
}
/// The highest rank a total reaches.
pub fn rank_for(ladder: &[Rank], total: i64) -> Option<&Rank> {
    ladder.iter().rev().find(|r| r.xp <= total)
}

/// Moves a member up to the rank their XP reaches (never down), swapping the rank role for their
/// faction's version and posting the promotion.
pub async fn promote(app: &App, user: &str) -> Res<()> {
    let ladder = ladder(app).await?;
    let mut tx = app.db.begin().await?;
    let Some((total, held)): Option<(i64, i16)> =
        sqlx::query_as("SELECT total,rank FROM discord_xp WHERE discord_user=$1 FOR UPDATE")
            .bind(user)
            .fetch_optional(&mut *tx)
            .await?
    else {
        return Ok(());
    };
    let Some(target) = rank_for(&ladder, total).filter(|r| r.rank > held && total > 0) else {
        return Ok(());
    };
    sqlx::query("UPDATE discord_xp SET rank=$2 WHERE discord_user=$1")
        .bind(user)
        .bind(target.rank)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let faction = faction(app, user).await?;
    let Some(guild) = home::guild(app).await else {
        return Ok(());
    };
    let old = ladder
        .iter()
        .find(|r| r.rank == held)
        .and_then(|r| r.role(faction.as_deref()));
    let new = target.role(faction.as_deref());
    if old != new {
        if let Some(old) = old {
            role(app, &guild, user, &old, false).await;
        }
        if let Some(new) = &new {
            role(app, &guild, user, new, true).await;
        }
    }
    // The first rank comes with the first XP; promotions after it are announced.
    let settings = home::settings(app).await;
    if held > 0
        && let Some(channel) = home::id(&settings, "rank_up")
    {
        let line = format!(
            "{} <@{user}> has been promoted to **{}**!",
            emoji(faction.as_deref()),
            target.name(faction.as_deref())
        );
        send(
            app,
            &channel,
            json!({"content": line, "allowed_mentions": {"users": [user]}}),
        )
        .await;
    }
    Ok(())
}

/// A faction change swaps the member's rank role to the new faction's version.
pub async fn swap_variant(app: &App, user: &str, from: Option<&str>, to: Option<&str>) -> Res<()> {
    let held: Option<i16> = sqlx::query_scalar("SELECT rank FROM discord_xp WHERE discord_user=$1")
        .bind(user)
        .fetch_optional(&app.db)
        .await?;
    let (Some(held), Some(guild)) = (held, home::guild(app).await) else {
        return Ok(());
    };
    let ladder = ladder(app).await?;
    let Some(rank) = ladder.iter().find(|r| r.rank == held) else {
        return Ok(());
    };
    let (old, new) = (rank.role(from), rank.role(to));
    if old != new {
        if let Some(old) = old {
            role(app, &guild, user, &old, false).await;
        }
        if let Some(new) = new {
            role(app, &guild, user, &new, true).await;
        }
    }
    Ok(())
}

pub fn definitions() -> Vec<Value> {
    let user = |d: &str| json!({"type": 6, "name": "user", "description": d, "required": false});
    vec![
        json!({"name": "rank", "description": "Your rank card, or someone else's.", "options": [user("Whose rank")]}),
        json!({"name": "leaderboard", "description": "The server's top members.", "options": [
            {"type": 3, "name": "by", "description": "What to rank by", "required": false, "choices": [
                {"name": "XP", "value": "xp"}, {"name": "Voice", "value": "voice"},
                {"name": "Messages", "value": "messages"}, {"name": "Streams", "value": "streams"}]},
            {"type": 4, "name": "page", "description": "Page", "required": false, "min_value": 1}]}),
        json!({"name": "profile", "description": "A member's Discord and S.V.E.R profile.", "options": [user("Whose profile")]}),
        json!({"name": "xpmult", "description": "XP multipliers.", "default_member_permissions": MANAGE_GUILD.to_string(), "options": [
            {"type": 1, "name": "list", "description": "Active and upcoming multipliers"},
            {"type": 1, "name": "create", "description": "Start a multiplier", "options": [
                {"type": 3, "name": "name", "description": "Name", "required": true, "max_length": 60},
                {"type": 10, "name": "factor", "description": "Multiplier, e.g. 2 or 1.5", "required": true, "min_value": 0.1, "max_value": 10},
                {"type": 3, "name": "for", "description": "Who it applies to", "required": true, "choices": [
                    {"name": "Everyone", "value": "event"}, {"name": "A role", "value": "role"},
                    {"name": "A channel", "value": "channel"}, {"name": "A faction", "value": "faction"}]},
                {"type": 8, "name": "role", "description": "The role", "required": false},
                {"type": 7, "name": "channel", "description": "The channel", "required": false},
                {"type": 3, "name": "faction", "description": "The faction", "required": false, "choices": [
                    {"name": "Myria", "value": "myria"}, {"name": "Aetheron", "value": "aetheron"}, {"name": "Glint", "value": "glint"}]},
                {"type": 4, "name": "hours", "description": "How long (none: until ended)", "required": false, "min_value": 1, "max_value": 8760}]},
            {"type": 1, "name": "end", "description": "End a multiplier", "options": [
                {"type": 4, "name": "id", "description": "Its number from /xpmult list", "required": true}]}]}),
        json!({"name": "ranks", "description": "The rank ladder's roles.", "default_member_permissions": MANAGE_GUILD.to_string(), "options": [
            {"type": 1, "name": "check", "description": "Which rank roles are missing"},
            {"type": 1, "name": "setup", "description": "Create the missing rank roles"},
            {"type": 1, "name": "backfill", "description": "Give every member the rank role their XP reaches"}]}),
    ]
}

/// Answers this module's commands; None when the command isn't one of them.
pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(match i.name() {
        "rank" => {
            let app = app.clone();
            let i = Interaction::parse(&i.raw)?;
            tokio::spawn(async move { rank_card(&app, &i).await });
            defer(false)
        }
        "leaderboard" => leaderboard(app, i).await,
        "profile" => profile(app, i).await,
        "xpmult" | "ranks" if !i.staff() => private("Only staff can use that."),
        "xpmult" => xpmult(app, i).await,
        "ranks" => {
            let app = app.clone();
            let i = Interaction::parse(&i.raw)?;
            tokio::spawn(async move { ranks(&app, &i).await });
            defer(true)
        }
        _ => return None,
    })
}

fn failed() -> Value {
    private("That couldn't be done right now. Try again in a minute.")
}

/// Rendered cards for five minutes, by member and total.
type Cards = HashMap<String, (Instant, Vec<u8>)>;
static CARDS: Mutex<Option<Cards>> = Mutex::new(None);

async fn avatar(app: &App, user: &str, hash: Option<&str>) -> Option<image::RgbaImage> {
    // Only Discord's own image host (DISCORD_CDN in tests), at a fixed size.
    let cdn = std::env::var("DISCORD_CDN").unwrap_or_else(|_| "https://cdn.discordapp.com".into());
    let url = match hash {
        Some(h) if h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
            format!("{cdn}/avatars/{user}/{h}.png?size=128")
        }
        _ => format!(
            "{cdn}/embed/avatars/{}.png",
            user.parse::<u64>().map_or(0, |id| (id >> 22) % 6)
        ),
    };
    let response = app
        .http
        .get(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let bytes = response.bytes().await.ok()?;
    if bytes.len() > 2_000_000 {
        return None;
    }
    image::load_from_memory(&bytes).ok().map(|i| i.to_rgba8())
}

async fn rank_card(app: &App, i: &Interaction) {
    let user = i.user_or_me("user");
    let fail = || json!({"content": "That rank card couldn't be drawn right now."});
    let Ok(row) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT total,CASE WHEN day=current_date THEN day_xp ELSE 0 END::bigint FROM discord_xp WHERE discord_user=$1",
    )
    .bind(&user)
    .fetch_optional(&app.db)
    .await
    else {
        return finish(app, i, fail(), None).await;
    };
    let Some((total, today)) = row else {
        let none = json!({"content": "No XP yet. Chat or hang out in voice to start earning it."});
        return finish(app, i, none, None).await;
    };
    let key = format!("{user}:{total}:{today}");
    let cached = CARDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(&key)
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
        .map(|(_, png)| png.clone());
    let png = match cached {
        Some(png) => png,
        None => {
            let (Ok(ladder), Ok(faction), Ok((position, members))) = (
                ladder(app).await,
                faction(app, &user).await,
                sqlx::query_as::<_, (i64, i64)>("SELECT count(*) FILTER(WHERE total>$1)+1,count(*) FILTER(WHERE total>0) FROM discord_xp")
                    .bind(total).fetch_one(&app.db).await,
            ) else {
                return finish(app, i, fail(), None).await;
            };
            let here = rank_for(&ladder, total);
            let next = ladder.iter().find(|r| r.xp > total);
            let (name, hash) = i.profile(&user);
            let picture = avatar(app, &user, hash.as_deref()).await;
            let c = card::Card {
                name,
                rank: here.map_or("Unranked".into(), |r| {
                    r.name(faction.as_deref()).to_string()
                }),
                faction,
                level: level(total),
                total,
                from: here.map_or(0, |r| r.xp),
                to: next.map(|r| r.xp),
                position,
                members: members.max(position),
                today,
                cap: DAILY_CAP,
                avatar: picture,
            };
            let png = tokio::task::spawn_blocking(move || card::render(&c))
                .await
                .unwrap_or_default();
            let mut cards = CARDS.lock().unwrap_or_else(|e| e.into_inner());
            let cards = cards.get_or_insert_with(HashMap::new);
            cards.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(300));
            cards.insert(key, (Instant::now(), png.clone()));
            png
        }
    };
    let data = json!({"embeds": [{"image": {"url": "attachment://rank.png"}, "color": 0x8b_9bb4}]});
    finish(app, i, data, Some(("rank.png", png))).await;
}

async fn leaderboard(app: &App, i: &Interaction) -> Value {
    let by = i.option("by").and_then(Value::as_str).unwrap_or("xp");
    let (column, label) = match by {
        "voice" => ("voice_minutes", "voice minutes"),
        "messages" => ("messages", "messages"),
        "streams" => ("stream_minutes", "stream minutes"),
        _ => ("total", "XP"),
    };
    let page = i.option("page").and_then(Value::as_i64).unwrap_or(1).max(1);
    // The column is one of four fixed names; the page is bound.
    let rows: Result<Vec<(String, i64)>, _> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT discord_user,{column}::bigint FROM discord_xp WHERE total>0 AND {column}>0 ORDER BY {column} DESC,discord_user LIMIT 10 OFFSET $1"
    )))
    .bind((page - 1) * 10)
    .fetch_all(&app.db)
    .await;
    let Ok(rows) = rows else {
        return failed();
    };
    if rows.is_empty() {
        return private("Nobody on that page yet.");
    }
    let lines: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(n, (user, value))| {
            format!(
                "**#{}** <@{user}> · {} {label}",
                (page - 1) * 10 + n as i64 + 1,
                card::thousands(*value)
            )
        })
        .collect();
    embed(
        json!({"title": format!("Leaderboard · {}", label), "description": lines.join("\n"),
            "color": 0x8b_9bb4, "footer": {"text": format!("Page {page}")}}),
        false,
    )
}

async fn profile(app: &App, i: &Interaction) -> Value {
    let user = i.user_or_me("user");
    let row: Option<(i64, i32, i32, i32)> = sqlx::query_as(
        "SELECT total,messages,voice_minutes,stream_minutes FROM discord_xp WHERE discord_user=$1",
    )
    .bind(&user)
    .fetch_optional(&app.db)
    .await
    .ok()
    .flatten();
    let (total, messages, voice, stream) = row.unwrap_or_default();
    let (Ok(ladder), Ok(faction)) = (ladder(app).await, faction(app, &user).await) else {
        return failed();
    };
    let rank = rank_for(&ladder, total)
        .filter(|_| total > 0)
        .map_or("Unranked".to_string(), |r| {
            r.name(faction.as_deref()).to_string()
        });
    let hours = |m: i32| format!("{:.1} h", m as f64 / 60.0);
    let mut fields = vec![
        json!({"name": "Rank", "value": rank, "inline": true}),
        json!({"name": "Level", "value": level(total).to_string(), "inline": true}),
        json!({"name": "XP", "value": card::thousands(total), "inline": true}),
        json!({"name": "Messages", "value": card::thousands(messages as i64), "inline": true}),
        json!({"name": "Voice", "value": hours(voice), "inline": true}),
        json!({"name": "Streamed", "value": hours(stream), "inline": true}),
    ];
    // The linked S.V.E.R account: username, level and faction.
    let linked: Option<(String, String, Option<String>)> = sqlx::query_as("SELECT u.id,u.username,f.faction FROM identities i JOIN users u ON u.id=i.user_id AND u.deleted_at IS NULL
        LEFT JOIN faction_members f ON f.user_id=u.id WHERE i.provider='discord' AND i.subject=$1")
        .bind(&user).fetch_optional(&app.db).await.ok().flatten();
    if let Some((id, username, sver_faction)) = linked {
        let sver_level = match app.db.acquire().await {
            Ok(mut db) => crate::progression::total(&mut db, &id)
                .await
                .map(crate::progression::level)
                .unwrap_or(1),
            Err(_) => 1,
        };
        let faction_name = sver_faction
            .as_deref()
            .map_or("No faction".to_string(), |f| {
                let mut c = f.chars();
                c.next().map_or(String::new(), |first| {
                    first.to_uppercase().chain(c).collect()
                })
            });
        fields.push(json!({"name": "S.V.E.R", "value": format!("[{username}]({}/{username}) · Level {sver_level} · {faction_name}", app.config.origin)}));
    }
    let (name, _) = i.profile(&user);
    embed(
        json!({"title": name, "color": color(faction.as_deref()), "fields": fields}),
        false,
    )
}

async fn xpmult(app: &App, i: &Interaction) -> Value {
    match i.sub() {
        Some("create") => {
            let kind = i.option("for").and_then(Value::as_str).unwrap_or("event");
            let target = match kind {
                "role" => i.option("role").and_then(Value::as_str),
                "channel" => i.option("channel").and_then(Value::as_str),
                "faction" => i.option("faction").and_then(Value::as_str),
                _ => None,
            };
            if kind != "event" && target.is_none() {
                return private(format!("Pick the {kind} it applies to."));
            }
            let name = i
                .option("name")
                .and_then(Value::as_str)
                .unwrap_or("Multiplier")
                .trim();
            let factor = i.option("factor").and_then(Value::as_f64).unwrap_or(1.0);
            let hours = i.option("hours").and_then(Value::as_i64);
            let saved: Result<i64, _> = sqlx::query_scalar("INSERT INTO discord_multipliers(name,factor,kind,target,ends_at) VALUES($1,round($2::numeric,2),$3,$4,CASE WHEN $5::int IS NULL THEN NULL ELSE now()+make_interval(hours=>$5) END) RETURNING id")
                .bind(name).bind(factor).bind(kind).bind(target).bind(hours.map(|h| h as i32)).fetch_one(&app.db).await;
            match saved {
                Ok(id) => private(format!("Multiplier #{id} is on: **{name}**, ×{factor}.")),
                Err(_) => private("That multiplier couldn't be saved (a factor from 0.1 to 10)."),
            }
        }
        Some("end") => {
            let id = i.option("id").and_then(Value::as_i64).unwrap_or(0);
            let ended = sqlx::query("UPDATE discord_multipliers SET ends_at=now() WHERE id=$1 AND (ends_at IS NULL OR ends_at>now())")
                .bind(id).execute(&app.db).await.map(|r| r.rows_affected()).unwrap_or(0);
            private(if ended > 0 {
                format!("Multiplier #{id} has ended.")
            } else {
                format!("Multiplier #{id} isn't running.")
            })
        }
        _ => {
            type Row = (i64, String, f64, String, Option<String>, Option<String>);
            let rows: Vec<Row> = sqlx::query_as("SELECT id,name,factor::float8,kind,target,to_char(ends_at,'YYYY-MM-DD HH24:MI') FROM discord_multipliers WHERE ends_at IS NULL OR ends_at>now() ORDER BY id")
                .fetch_all(&app.db).await.unwrap_or_default();
            if rows.is_empty() {
                return private("No multipliers are running.");
            }
            let lines: Vec<String> = rows
                .iter()
                .map(|(id, name, factor, kind, target, ends)| {
                    let who = match (kind.as_str(), target) {
                        ("role", Some(t)) => format!("<@&{t}>"),
                        ("channel", Some(t)) => format!("<#{t}>"),
                        ("user", Some(t)) => format!("<@{t}>"),
                        ("faction", Some(t)) => t.clone(),
                        _ => "everyone".into(),
                    };
                    format!(
                        "**#{id}** {name} · ×{factor} for {who}{}",
                        ends.as_ref()
                            .map(|e| format!(" · until {e} UTC"))
                            .unwrap_or_default()
                    )
                })
                .collect();
            embed(
                json!({"title": "XP multipliers", "description": lines.join("\n"), "color": 0x8b_9bb4}),
                true,
            )
        }
    }
}

/// Role colours for the ladder: the faction colour, darker at the bottom.
fn shade(faction: Option<&str>, rank: i16) -> u32 {
    let base = color(faction);
    let t = 0.4 + 0.6 * (rank as f64 - 1.0) / 21.0;
    let c = |shift: u32| (((base >> shift) & 0xff) as f64 * t).round() as u32;
    (c(16) << 16) | (c(8) << 8) | c(0)
}

async fn ranks(app: &App, i: &Interaction) {
    let Some(guild) = home::guild(app).await else {
        return finish(
            app,
            i,
            json!({"content": "Pick the home server first."}),
            None,
        )
        .await;
    };
    let Ok(ladder) = ladder(app).await else {
        return finish(
            app,
            i,
            json!({"content": "The ladder couldn't be read."}),
            None,
        )
        .await;
    };
    let existing: Vec<String> = match super::lists(app, &guild).await {
        Ok(l) => l
            .roles
            .iter()
            .filter_map(|r| r["id"].as_str().map(String::from))
            .collect(),
        Err(e) => return finish(app, i, json!({"content": e.message}), None).await,
    };
    let variants = [None, Some("myria"), Some("aetheron"), Some("glint")];
    let missing: Vec<(&Rank, Option<&str>)> = ladder
        .iter()
        .flat_map(|r| variants.iter().map(move |v| (r, *v)))
        .filter(|(r, v)| r.role(*v).is_none_or(|id| !existing.contains(&id)))
        .collect();
    let content = match i.sub() {
        Some("setup") => {
            // Highest first, so each new role lands below the one above it.
            let mut made = 0;
            for (rank, variant) in missing.iter().rev() {
                let name = match variant {
                    Some(f) => {
                        let mut c = f.chars();
                        let label: String = c
                            .next()
                            .map(|x| x.to_uppercase().chain(c).collect())
                            .unwrap_or_default();
                        format!("{} [{label}]", rank.name(*variant))
                    }
                    None => rank.base_name.clone(),
                };
                let body = json!({"name": name, "color": shade(*variant, rank.rank), "hoist": false, "mentionable": false});
                let Some((status, created)) = call(
                    app,
                    Method::POST,
                    &format!("/guilds/{guild}/roles"),
                    Some(body),
                )
                .await
                else {
                    break;
                };
                let Some(id) = created["id"].as_str().filter(|_| status < 300) else {
                    break;
                };
                let key = variant.unwrap_or("none");
                if sqlx::query("UPDATE discord_ranks SET roles=jsonb_set(roles,ARRAY[$2],to_jsonb($3::text)) WHERE rank=$1")
                    .bind(rank.rank).bind(key).bind(id).execute(&app.db).await.is_err() {
                    break;
                }
                made += 1;
            }
            format!("Created {made} of {} missing rank roles.", missing.len())
        }
        Some("backfill") => {
            let members: Vec<(String, i64, Option<String>)> = sqlx::query_as("SELECT x.discord_user,x.total,m.faction FROM discord_xp x LEFT JOIN discord_members m ON m.discord_user=x.discord_user WHERE x.total>0")
                .fetch_all(&app.db).await.unwrap_or_default();
            let mut given = 0;
            for (user, total, faction) in &members {
                let Some(rank) = rank_for(&ladder, *total) else {
                    continue;
                };
                let _ = sqlx::query(
                    "UPDATE discord_xp SET rank=greatest(rank,$2) WHERE discord_user=$1",
                )
                .bind(user)
                .bind(rank.rank)
                .execute(&app.db)
                .await;
                if let Some(id) = rank.role(faction.as_deref())
                    && role(app, &guild, user, &id, true).await
                {
                    given += 1;
                }
            }
            format!("Gave {given} of {} members their rank role.", members.len())
        }
        _ if missing.is_empty() => "Every rank role is in place.".into(),
        _ => format!(
            "{} rank roles are missing. `/ranks setup` creates them.",
            missing.len()
        ),
    };
    finish(app, i, json!({"content": content}), None).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels_follow_the_square_root_curve() {
        assert_eq!(level(0), 0);
        assert_eq!(level(99), 0);
        assert_eq!(level(100), 1);
        assert_eq!(level(10_000), 10);
        assert_eq!(level(48_065), 21);
        assert_eq!(level(1_000_000), 100);
    }
    #[test]
    fn rank_roles_darken_down_the_ladder() {
        assert_eq!(shade(Some("myria"), 22), 0xe7_4c3c);
        assert!(shade(Some("myria"), 1) < shade(Some("myria"), 22));
    }
}
