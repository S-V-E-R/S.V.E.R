//! Chat commands and the channel bot (docs/COMMUNITY.md "Chat commands and the channel bot"), part 1:
//! custom `!commands`, timed messages, `/help`'s list, and the channel's bot (its faction's PYRE,
//! ECHO or FAVOR, or the neutral VOLK). Bot lines are stored like linked-chat messages (platform
//! `bot`), so they keep their place in history and count toward nothing.
use crate::{
    App, auth,
    profiles::{self, Fail, Res},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch, put},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};

pub const BOTS: [(&str, &str); 4] = [
    ("pyre", "PYRE"),
    ("echo", "ECHO"),
    ("favor", "FAVOR"),
    ("volk", "VOLK"),
];
const ACCESS: [&str; 4] = ["everyone", "followers", "subscribers", "moderators"];
/// Built-in `!` words a custom command can't take.
const BUILT_IN: [&str; 6] = ["marker", "rally", "help", "commands", "giveaway", "bot"];
const MAX_TIMERS: i64 = 5;

/// The channel's bot: the chosen one, else its owner's faction's, else VOLK.
pub async fn bot(app: &App, channel: &str) -> Res<(&'static str, &'static str)> {
    let key: String = sqlx::query_scalar("SELECT coalesce((SELECT bot FROM chat_settings WHERE channel_id=$1),
            CASE (SELECT faction FROM faction_members WHERE user_id=$1) WHEN 'myria' THEN 'pyre' WHEN 'aetheron' THEN 'echo' WHEN 'glint' THEN 'favor' ELSE 'volk' END)")
        .bind(channel)
        .fetch_one(&app.db)
        .await?;
    Ok(BOTS
        .into_iter()
        .find(|b| b.0 == key)
        .unwrap_or(("volk", "VOLK")))
}
/// The bot says `text` in the channel's chat, unless the channel's banned words or link rule
/// would refuse it.
pub async fn say(app: &App, channel: &str, text: &str) -> Res<()> {
    let rules: Option<(Vec<String>, bool)> =
        sqlx::query_as("SELECT banned_words,block_links FROM chat_settings WHERE channel_id=$1")
            .bind(channel)
            .fetch_optional(&app.db)
            .await?;
    if let Some((words, links)) = rules
        && crate::moderation::check_content(text, &words, links, false).is_err()
    {
        return Ok(());
    }
    let (key, name) = bot(app, channel).await?;
    crate::linked_chat::post_system(app, channel, "bot", key, name, text).await
}

/// `{user}`, `{channel}`, `{uptime}`, `{game}` and `{followers}`, filled in.
async fn fill(app: &App, channel: &str, user: Option<&str>, text: &str) -> Res<String> {
    let (name, started, game, followers): (String, Option<chrono::DateTime<chrono::Utc>>, Option<String>, i64) = sqlx::query_as("SELECT c.display_name,
            (SELECT started_at FROM broadcasts WHERE owner_id=c.id AND state IN ('LIVE','RECONNECTING') ORDER BY started_at DESC LIMIT 1),
            (SELECT k.name FROM stream_settings s JOIN stream_categories k ON k.id=s.category_id WHERE s.owner_id=c.id),
            (SELECT count(*) FROM follows WHERE following_id=c.id)
        FROM channel_users c WHERE c.id=$1")
        .bind(channel)
        .fetch_one(&app.db)
        .await?;
    let uptime = match started {
        Some(at) => {
            let minutes = (chrono::Utc::now() - at).num_minutes().max(0);
            if minutes >= 60 {
                format!("{}h {}m", minutes / 60, minutes % 60)
            } else {
                format!("{minutes}m")
            }
        }
        None => "offline".into(),
    };
    Ok(text
        .replace("{user}", user.unwrap_or("everyone"))
        .replace("{channel}", &name)
        .replace("{uptime}", &uptime)
        .replace("{game}", game.as_deref().unwrap_or("no category"))
        .replace("{followers}", &followers.to_string()))
}

/// After a chat message is published: a `!name` matching a custom command gets the bot's reply,
/// within the command's permission and cooldown. Never fails the sender's message.
pub async fn respond(app: &App, channel: &str, user: &auth::User, body: &str) {
    if let Err(error) = try_respond(app, channel, user, body).await {
        eprintln!(
            "commands_event=respond outcome=error status={}",
            error.status.as_u16()
        );
    }
}
async fn try_respond(app: &App, channel: &str, user: &auth::User, body: &str) -> Res<()> {
    let Some(name) = body
        .trim()
        .strip_prefix('!')
        .and_then(|r| r.split_whitespace().next())
        .map(str::to_ascii_lowercase)
    else {
        return Ok(());
    };
    let command: Option<(String, String, i32)> = sqlx::query_as(
        "SELECT reply,access,cooldown_seconds FROM chat_commands WHERE channel_id=$1 AND name=$2",
    )
    .bind(channel)
    .bind(&name)
    .fetch_optional(&app.db)
    .await?;
    let Some((reply, access, cooldown)) = command else {
        return Ok(());
    };
    let staff = crate::moderation::role_of(app, channel, user)
        .await?
        .is_some();
    let allowed =
        staff
            || match access.as_str() {
                "everyone" => true,
                "followers" => sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2)",
                )
                .bind(&user.id)
                .bind(channel)
                .fetch_one(&app.db)
                .await?,
                "subscribers" => crate::subs::active_tier(app, channel, &user.id)
                    .await?
                    .is_some(),
                _ => false,
            };
    if !allowed {
        return Ok(());
    }
    if cooldown > 0
        && crate::security::reserve(
            app,
            vec![format!("command:{channel}:{name}")],
            1,
            cooldown.into(),
        )
        .await
        .is_err()
    {
        return Ok(());
    }
    sqlx::query("UPDATE chat_commands SET uses=uses+1 WHERE channel_id=$1 AND name=$2")
        .bind(channel)
        .bind(&name)
        .execute(&app.db)
        .await?;
    let display: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM channel_users WHERE id=$1")
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    let text = fill(
        app,
        channel,
        Some(display.as_deref().unwrap_or(&user.username)),
        &reply,
    )
    .await?;
    say(app, channel, &text).await
}

/// Every jobs tick: each live channel's due timed message is posted when chat has been active
/// since its last one (and at least `every_minutes` after the stream started).
pub async fn tick(app: &App) -> Res<()> {
    let due: Vec<(String, String)> = sqlx::query_as("UPDATE chat_timers t SET last_sent_at=now() FROM broadcasts b
        WHERE b.owner_id=t.channel_id AND b.state='LIVE' AND t.enabled
          AND coalesce(t.last_sent_at, b.started_at) <= now()-make_interval(mins=>t.every_minutes)
          AND EXISTS(SELECT 1 FROM chat_messages m WHERE m.channel_id=t.channel_id AND m.squad_id IS NULL AND m.created_at>coalesce(t.last_sent_at, b.started_at))
          AND t.id=(SELECT t2.id FROM chat_timers t2 WHERE t2.channel_id=t.channel_id AND t2.enabled AND coalesce(t2.last_sent_at, b.started_at) <= now()-make_interval(mins=>t2.every_minutes) ORDER BY t2.last_sent_at NULLS FIRST, t2.created_at LIMIT 1)
        RETURNING t.channel_id,t.body")
        .fetch_all(&app.db)
        .await?;
    for (channel, body) in due {
        let text = fill(app, &channel, None, &body).await?;
        say(app, &channel, &text).await?;
    }
    Ok(())
}

// ---- Public list (/help) ----

/// GET /api/channels/{username}/commands: the channel's custom commands for `/help`.
async fn list(State(app): State<App>, Path(name): Path<String>) -> Res<Json<Value>> {
    let channel = crate::moderation::channel(&app, &name).await?;
    let commands: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('name',name,'access',access) FROM chat_commands WHERE channel_id=$1 ORDER BY name")
        .bind(&channel).fetch_all(&app.db).await?;
    let (_, bot) = bot(&app, &channel).await?;
    Ok(Json(json!({"commands": commands, "bot": bot})))
}

// ---- Creator Studio ----

async fn view(app: &App, owner: &str) -> Res<Json<Value>> {
    let commands: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('name',name,'reply',reply,'access',access,'cooldown_seconds',cooldown_seconds,'uses',uses) FROM chat_commands WHERE channel_id=$1 ORDER BY name")
        .bind(owner).fetch_all(&app.db).await?;
    let timers: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'body',body,'every_minutes',every_minutes,'enabled',enabled,'last_sent_at',last_sent_at) FROM chat_timers WHERE channel_id=$1 ORDER BY created_at")
        .bind(owner).fetch_all(&app.db).await?;
    let chosen: Option<(Option<String>, String)> =
        sqlx::query_as("SELECT bot,bot_personality FROM chat_settings WHERE channel_id=$1")
            .bind(owner)
            .fetch_optional(&app.db)
            .await?;
    let (chosen, personality) = chosen.unwrap_or((None, "chill".into()));
    let (key, name) = bot(app, owner).await?;
    Ok(Json(
        json!({"commands": commands, "timers": timers, "bot": {"key": key, "name": name, "chosen": chosen, "personality": personality}, "max_timers": MAX_TIMERS}),
    ))
}
/// GET /api/me/commands
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    view(&app, &user.id).await
}
#[derive(Deserialize)]
pub struct BotSettings {
    /// None: the faction's bot.
    bot: Option<String>,
    personality: String,
}
/// PUT /api/me/commands/bot: which bot speaks, and its personality.
async fn set_bot(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<BotSettings>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if input
        .bot
        .as_deref()
        .is_some_and(|b| !BOTS.iter().any(|x| x.0 == b))
        || !["chill", "battle", "event"].contains(&input.personality.as_str())
    {
        return Err(Fail::bad("Choose a bot and a personality."));
    }
    sqlx::query("INSERT INTO chat_settings(channel_id,bot,bot_personality) VALUES($1,$2,$3) ON CONFLICT(channel_id) DO UPDATE SET bot=EXCLUDED.bot,bot_personality=EXCLUDED.bot_personality")
        .bind(&user.id).bind(&input.bot).bind(&input.personality).execute(&app.db).await?;
    view(&app, &user.id).await
}
#[derive(Deserialize)]
pub struct Command {
    reply: String,
    access: String,
    cooldown_seconds: i32,
}
/// PUT /api/me/commands/{name}: creates or changes `!name`.
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Command>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let name = name.trim().trim_start_matches('!').to_ascii_lowercase();
    if !(1..=25).contains(&name.len())
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Fail::field(
            "name",
            "Use 1–25 letters, numbers or underscores.",
        ));
    }
    let counter: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM counters WHERE channel_id=$1 AND id=$2)")
            .bind(&user.id)
            .bind(&name)
            .fetch_one(&app.db)
            .await?;
    if BUILT_IN.contains(&name.as_str()) || counter {
        return Err(Fail::field(
            "name",
            "That name is already a built-in command or one of your counters.",
        ));
    }
    let reply = input.reply.trim();
    if !(1..=300).contains(&reply.chars().count()) {
        return Err(Fail::field("reply", "Write a reply of 1–300 characters."));
    }
    if !ACCESS.contains(&input.access.as_str()) || !(0..=3600).contains(&input.cooldown_seconds) {
        return Err(Fail::bad(
            "Choose who can use it and a cooldown of 0–3600 seconds.",
        ));
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_commands WHERE channel_id=$1 AND name<>$2")
            .bind(&user.id)
            .bind(&name)
            .fetch_one(&app.db)
            .await?;
    if count >= 100 {
        return Err(Fail::bad("A channel can have up to 100 commands."));
    }
    sqlx::query("INSERT INTO chat_commands(channel_id,name,reply,access,cooldown_seconds) VALUES($1,$2,$3,$4,$5) ON CONFLICT(channel_id,name) DO UPDATE SET reply=EXCLUDED.reply,access=EXCLUDED.access,cooldown_seconds=EXCLUDED.cooldown_seconds")
        .bind(&user.id).bind(&name).bind(reply).bind(&input.access).bind(input.cooldown_seconds).execute(&app.db).await?;
    view(&app, &user.id).await
}
/// DELETE /api/me/commands/{name}
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query("DELETE FROM chat_commands WHERE channel_id=$1 AND name=$2")
        .bind(&user.id)
        .bind(name.to_ascii_lowercase())
        .execute(&app.db)
        .await?;
    view(&app, &user.id).await
}
#[derive(Deserialize)]
pub struct Timer {
    body: Option<String>,
    every_minutes: Option<i32>,
    enabled: Option<bool>,
}
fn check_timer(input: &Timer) -> Res<()> {
    if input
        .body
        .as_deref()
        .is_some_and(|b| !(1..=300).contains(&b.trim().chars().count()))
    {
        return Err(Fail::field("body", "Write a message of 1–300 characters."));
    }
    if input
        .every_minutes
        .is_some_and(|m| !(10..=1440).contains(&m))
    {
        return Err(Fail::field(
            "every_minutes",
            "Post it every 10–1440 minutes.",
        ));
    }
    Ok(())
}
/// POST /api/me/timers: adds a timed message (up to 5).
async fn add_timer(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Timer>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    check_timer(&input)?;
    let (Some(body), Some(every)) = (input.body.as_deref(), input.every_minutes) else {
        return Err(Fail::bad("Write a message and how often to post it."));
    };
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_timers WHERE channel_id=$1")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    if count >= MAX_TIMERS {
        return Err(Fail::bad("A channel can have up to 5 timed messages."));
    }
    sqlx::query("INSERT INTO chat_timers(id,channel_id,body,every_minutes) VALUES($1,$2,$3,$4)")
        .bind(profiles::new_id())
        .bind(&user.id)
        .bind(body.trim())
        .bind(every)
        .execute(&app.db)
        .await?;
    view(&app, &user.id).await
}
/// PATCH /api/me/timers/{id}
async fn change_timer(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Timer>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    check_timer(&input)?;
    sqlx::query("UPDATE chat_timers SET body=coalesce($3,body),every_minutes=coalesce($4,every_minutes),enabled=coalesce($5,enabled) WHERE id=$1 AND channel_id=$2")
        .bind(&id).bind(&user.id).bind(input.body.as_deref().map(str::trim)).bind(input.every_minutes).bind(input.enabled)
        .execute(&app.db).await?;
    view(&app, &user.id).await
}
/// DELETE /api/me/timers/{id}
async fn remove_timer(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query("DELETE FROM chat_timers WHERE id=$1 AND channel_id=$2")
        .bind(&id)
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    view(&app, &user.id).await
}
/// POST /api/me/timers/starter: adds the starter timers (chat rules, and social links when set).
async fn starter(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let links: Vec<String> = sqlx::query_scalar(
        "SELECT url FROM social_links WHERE user_id=$1 ORDER BY position LIMIT 3",
    )
    .bind(&user.id)
    .fetch_all(&app.db)
    .await
    .unwrap_or_default();
    let mut starters = vec![(
        "Welcome to {channel}'s chat! Be kind, keep it on topic, and have fun. Follow so you know when we're live.".to_string(),
        30,
    )];
    if !links.is_empty() {
        starters.push((
            format!("Find {{channel}} elsewhere: {}", links.join(" · ")),
            45,
        ));
    }
    for (body, every) in starters {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_timers WHERE channel_id=$1")
            .bind(&user.id)
            .fetch_one(&app.db)
            .await?;
        if count >= MAX_TIMERS {
            break;
        }
        sqlx::query(
            "INSERT INTO chat_timers(id,channel_id,body,every_minutes) VALUES($1,$2,$3,$4)",
        )
        .bind(profiles::new_id())
        .bind(&user.id)
        .bind(&body)
        .bind(every)
        .execute(&app.db)
        .await?;
    }
    view(&app, &user.id).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/commands", get(list))
        .route("/api/me/commands", get(mine))
        .route("/api/me/commands/bot", put(set_bot))
        .route("/api/me/commands/{name}", put(save).delete(remove))
        .route("/api/me/timers", axum::routing::post(add_timer))
        .route("/api/me/timers/starter", axum::routing::post(starter))
        .route(
            "/api/me/timers/{id}",
            patch(change_timer).delete(remove_timer),
        )
}
