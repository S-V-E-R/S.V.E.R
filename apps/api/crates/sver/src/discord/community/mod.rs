//! Community features on the home server (docs/DISCORD_BOT.md "Community"): the public welcome,
//! DM onboarding (rules, faction, platform roles, S.V.E.R link, birthday), the goodbye post and the
//! role-select panel live here. Birthdays, anniversaries, seasonal events and spotlights are in
//! `rituals`; events, polls, giveaways and reminders in `events`; mentors, threads and forums, temp
//! voice rooms, custom commands and embeds in `tools`; free games, the GitHub changelog and
//! WoW/Chromely in `feeds`.
//!
//! Onboarding is a row (`discord_members.onboarding`, the step a member is on), so it never times
//! out and survives restarts; each button moves it one step, once.
pub mod events;
pub mod feeds;
pub mod rituals;
pub mod tools;

use super::{
    commands::{ANSWERED, Interaction, MANAGE_GUILD, finish, private, respond},
    dm, factions, home, levels, quests, role, send,
    streamers::{answer, later, platform_name},
    sync,
};
use crate::{App, profiles::Res};
use axum::{Router, routing::post};
use serde_json::{Value, json};
use std::future::Future;

/// Acknowledges a button or form at once and edits its message when `work` finishes, for anything
/// that gives roles or calls Discord. `work` returns the new message (an update, type 7).
pub(super) fn updating<F>(app: &App, i: &Interaction, work: fn(App, Interaction) -> F)
where
    F: Future<Output = Res<Value>> + Send + 'static,
{
    let (Some(first), Some(second)) = (Interaction::parse(&i.raw), Interaction::parse(&i.raw))
    else {
        return;
    };
    let app = app.clone();
    tokio::spawn(async move {
        respond(&app, &first, json!({"type": 6})).await;
        match work(app.clone(), second).await {
            Ok(mut reply) => finish(&app, &first, reply["data"].take(), None).await,
            // The message stays as it was, so its buttons can be pressed again.
            Err(_) => eprintln!("discord_event=community_update outcome=retry"),
        }
    });
}
/// A message update, as a button's answer.
pub(super) fn update(data: Value) -> Value {
    json!({"type": 7, "data": data})
}
/// A form (modal) with text inputs: (id, label, long, required, max length, prefilled).
pub(super) fn form(id: &str, title: &str, inputs: &[(&str, &str, bool, bool, u16, &str)]) -> Value {
    let rows: Vec<Value> = inputs
        .iter()
        .map(|(id, label, long, required, max, value)| {
            let mut input = json!({"type": 4, "custom_id": id, "label": label, "style": if *long { 2 } else { 1 },
                "required": required, "max_length": max});
            // Discord refuses a form whose prefilled text is over the input's limit.
            if !value.is_empty() {
                input["value"] = json!(value.chars().take(*max as usize).collect::<String>());
            }
            json!({"type": 1, "components": [input]})
        })
        .collect();
    json!({"type": 9, "data": {"custom_id": id, "title": title, "components": rows}})
}
/// A form's answer by input id, trimmed; None when empty.
pub(super) fn field(i: &Interaction, id: &str) -> Option<String> {
    i.data["components"]
        .as_array()?
        .iter()
        .flat_map(|row| row["components"].as_array().cloned().unwrap_or_default())
        .find(|c| c["custom_id"] == id)?["value"]
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(String::from)
}
/// A command's text option, trimmed and cut to `max` characters; None when empty.
pub(super) fn text(i: &Interaction, name: &str, max: usize) -> Option<String> {
    i.option(name)
        .and_then(Value::as_str)
        .map(|v| v.trim().chars().take(max).collect::<String>())
        .filter(|v| !v.is_empty())
}
pub(super) fn button(label: &str, style: u8, id: String) -> Value {
    json!({"type": 2, "style": style, "label": label, "custom_id": id})
}
pub(super) fn row(buttons: Vec<Value>) -> Value {
    json!({"type": 1, "components": buttons})
}
/// The member's display name in an interaction (in a DM there's no member, only the user).
pub(super) fn name_of(i: &Interaction) -> String {
    let u = if i.raw["member"].is_object() {
        &i.raw["member"]["user"]
    } else {
        &i.raw["user"]
    };
    u["global_name"]
        .as_str()
        .or(u["username"].as_str())
        .unwrap_or("Member")
        .to_string()
}

// ---- Welcome and onboarding ----

const STEPS: usize = 5;
fn step_embed(n: usize, title: &str, description: String, color: u32) -> Value {
    json!({"title": title, "description": description, "color": color,
        "footer": {"text": format!("S.V.E.R bot • Step {n} of {STEPS}")}})
}
fn rules() -> Value {
    json!({"embeds": [step_embed(1, "Welcome to S.V.E.R!", "Before you get the whole server, please read and accept the community rules:\n\n\
        1. **Be respectful.** No harassment, hate speech or personal attacks.\n\
        2. **No spam.** Keep channels on topic and self-promotion in its channels.\n\
        3. **Follow Discord's Terms of Service.**\n\
        4. **Support each other.** We're here to grow together.\n\
        5. **Have fun.** This is a community for people who play, build and make.\n\nPress **Accept** to go on.".into(), 0x58_65f2)],
        "components": [row(vec![button("Accept the rules", 3, "onb:rules".into())])]})
}
fn faction_step() -> Value {
    let pick = |f: &str, style: u8| json!({"type": 2, "style": style, "label": factions::title(f), "emoji": {"name": levels::emoji(Some(f))}, "custom_id": format!("onb:faction:{f}")});
    json!({"embeds": [step_embed(2, "Choose your faction", format!(
        "Three factions fight a seasonal war on S.V.E.R. Yours sets your rank names, your team and the territory you fight for.\n\n\
        {} **Myria**: bold, fiery, always on the attack\n{} **Aetheron**: disciplined guardians, calculated and steady\n{} **Glint**: creative, unconventional, brilliant",
        levels::emoji(Some("myria")), levels::emoji(Some("aetheron")), levels::emoji(Some("glint"))), 0x58_65f2)],
        "components": [row(vec![pick("myria", 4), pick("aetheron", 1), pick("glint", 2)])]})
}
fn platforms_step(faction: Option<&str>) -> Value {
    let options: Vec<Value> = ["twitch", "youtube", "kick"]
        .iter()
        .map(|p| json!({"label": platform_name(p), "value": p}))
        .collect();
    json!({"embeds": [step_embed(3, "Where do you stream?", "Pick every platform you stream on, so we can connect you with the right people.".into(), levels::color(faction))],
        "components": [
            {"type": 1, "components": [{"type": 3, "custom_id": "onb:platforms", "placeholder": "Your platforms", "min_values": 1, "max_values": 3, "options": options}]},
            row(vec![button("I don't stream", 2, "onb:platforms:none".into())])]})
}
async fn link_step(app: &App, user: &str, faction: Option<&str>) -> Res<Value> {
    let (description, mut buttons) = match sync::linked(app, user).await? {
        Some(a) => (
            format!(
                "✅ This Discord account is linked to **{}** on S.V.E.R: your level, faction and supporter roles follow it here.",
                a.username
            ),
            vec![],
        ),
        None => (
            "Have a S.V.E.R account? Sign in to S.V.E.R with Discord, or link Discord on your account page, and your faction, level and supporter roles follow you here.\n\nYou can always do this later: `/link sver`.".to_string(),
            vec![
                json!({"type": 2, "style": 5, "label": "Link on S.V.E.R", "url": format!("{}/account", app.config.origin)}),
            ],
        ),
    };
    buttons.push(button("Continue", 1, "onb:link".into()));
    Ok(
        json!({"embeds": [step_embed(4, "Your S.V.E.R account", description, levels::color(faction))],
        "components": [row(buttons)]}),
    )
}
fn birthday_step() -> Value {
    json!({"embeds": [step_embed(5, "🎂 When's your birthday?", "Set it for a shout-out and **2× XP** on your day, in your own time zone.\n\nYou can always set it later with `/birthday set` or on the role-select panel.".into(), 0xff_69b4)],
        "components": [row(vec![button("Set my birthday", 1, "bday:open:onb".into()), button("Skip", 2, "onb:skip".into())])]})
}

/// Starts onboarding for a member who just joined (or was approved after being held): the Pending
/// role, the public welcome and the first DM step, or a note in the welcome channel when their DMs
/// are closed. A member who finished onboarding before only gets Community Member back. `user` is
/// Discord's user object.
pub(crate) async fn welcome(app: &App, user: &Value) -> Res<()> {
    let Some(id) = user["id"].as_str() else {
        return Ok(());
    };
    let Some(guild) = home::guild(app).await else {
        return Ok(());
    };
    let settings = home::settings(app).await;
    let returning: bool = sqlx::query_scalar(
        "INSERT INTO discord_members(discord_user,joined_at) VALUES($1,now())
        ON CONFLICT(discord_user) DO UPDATE SET joined_at=now() RETURNING onboarded_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&app.db)
    .await?;
    if returning {
        // Discord took their roles when they left; the sync restores the rest.
        if let Some(r) = home::id(&settings, "community") {
            role(app, &guild, id, &r, true).await;
        }
        return Ok(());
    }
    if let (true, Some(channel)) = (
        home::on(&settings, "welcome_post"),
        home::id(&settings, "welcome"),
    ) {
        let start = home::id(&settings, "factions_channel").unwrap_or_else(|| channel.clone());
        let mut e = json!({"title": "Welcome to S.V.E.R! ⚔️", "color": 0x43_b581,
            "description": format!("Hey <@{id}>, welcome to the community! 🎉\n\nHead to <#{start}> to pick your faction, link your accounts and get started.")});
        if let Some(a) = user["avatar"].as_str() {
            e["thumbnail"] =
                json!({"url": format!("https://cdn.discordapp.com/avatars/{id}/{a}.png")});
        }
        send(
            app,
            &channel,
            json!({"embeds": [e], "allowed_mentions": {"parse": []}}),
        )
        .await;
    }
    if !home::on(&settings, "welcome_dm") {
        return Ok(());
    }
    sqlx::query("UPDATE discord_members SET onboarding='rules' WHERE discord_user=$1")
        .bind(id)
        .execute(&app.db)
        .await?;
    if let Some(r) = home::id(&settings, "pending") {
        role(app, &guild, id, &r, true).await;
    }
    if !dm(app, id, rules()).await
        && let Some(channel) = home::id(&settings, "welcome")
    {
        send(app, &channel, json!({"content": format!(
            "<@{id}> Welcome! I couldn't send you a DM. Turn on DMs from server members, or use the role-select panel to set yourself up."),
            "allowed_mentions": {"users": [id]}})).await;
    }
    Ok(())
}

/// GUILD_MEMBER_ADD that moderation didn't hold.
pub async fn joined(app: &App, d: &Value) -> Res<()> {
    if d["user"]["bot"] == true || d["guild_id"].as_str() != home::guild(app).await.as_deref() {
        return Ok(());
    }
    welcome(app, &d["user"]).await
}

/// GUILD_MEMBER_REMOVE: one goodbye post (the legacy bot posted two).
pub async fn left(app: &App, d: &Value) -> Res<()> {
    let Some(user) = d["user"]["id"].as_str() else {
        return Ok(());
    };
    if d["user"]["bot"] == true || d["guild_id"].as_str() != home::guild(app).await.as_deref() {
        return Ok(());
    }
    let settings = home::settings(app).await;
    let Some(channel) = home::id(&settings, "goodbye").or_else(|| home::id(&settings, "welcome"))
    else {
        return Ok(());
    };
    if !home::on(&settings, "goodbye_post") {
        return Ok(());
    }
    type Was = (Option<chrono::DateTime<chrono::Utc>>, Option<String>);
    let member: Option<Was> =
        sqlx::query_as("SELECT joined_at,faction FROM discord_members WHERE discord_user=$1")
            .bind(user)
            .fetch_optional(&app.db)
            .await?;
    let (joined, faction) = member.unwrap_or_default();
    let name = d["user"]["global_name"]
        .as_str()
        .or(d["user"]["username"].as_str())
        .unwrap_or("A member");
    let mut text = format!("**{name}** has left the server.");
    if let Some(t) = joined {
        let minutes = (chrono::Utc::now() - t).num_minutes().max(0);
        let (n, unit) = match minutes {
            m if m < 60 => (m, "minute"),
            m if m < 1440 => (m / 60, "hour"),
            m => (m / 1440, "day"),
        };
        text.push_str(&format!(
            " We had them for {n} {unit}{}.",
            if n == 1 { "" } else { "s" }
        ));
    }
    if let Some(f) = faction.as_deref() {
        text.push_str(&format!(
            " They were part of {} **{}**.",
            levels::emoji(Some(f)),
            factions::title(f)
        ));
    }
    send(
        app,
        &channel,
        json!({"embeds": [{"title": "👋 Member left", "description": text, "color": 0xed_4245}],
        "allowed_mentions": {"parse": []}}),
    )
    .await;
    Ok(())
}

/// Moves onboarding from one step to the next (None: done); false when the member isn't on `from`
/// (a button pressed twice, or an old message).
async fn advance(app: &App, user: &str, from: &str, to: Option<&str>) -> Res<bool> {
    Ok(sqlx::query("UPDATE discord_members SET onboarding=$3,onboarded_at=CASE WHEN $3::text IS NULL THEN now() ELSE onboarded_at END WHERE discord_user=$1 AND onboarding=$2")
        .bind(user).bind(from).bind(to).execute(&app.db).await?.rows_affected() == 1)
}
fn stale() -> Value {
    update(json!({"content": "That step is already done.", "embeds": [], "components": []}))
}

async fn rules_accepted(app: App, i: Interaction) -> Res<Value> {
    if !advance(&app, &i.user, "rules", Some("faction")).await? {
        return Ok(stale());
    }
    // A linked account's faction comes from S.V.E.R: that step is done for them.
    if let Some(f) = sync::linked(&app, &i.user).await?.and_then(|a| a.faction) {
        factions::set(&app, &i.user, &f).await?;
        advance(&app, &i.user, "faction", Some("platforms")).await?;
        let mut next = platforms_step(Some(&f));
        next["content"] = json!(format!(
            "Your faction comes from your S.V.E.R account: {} **{}**.",
            levels::emoji(Some(&f)),
            factions::title(&f)
        ));
        return Ok(update(next));
    }
    Ok(update(faction_step()))
}
async fn faction_chosen(app: App, i: Interaction) -> Res<Value> {
    let Some(pick) = i
        .custom_id()
        .strip_prefix("onb:faction:")
        .filter(|f| levels::FACTIONS.contains(f))
        .map(String::from)
    else {
        return Ok(stale());
    };
    if !advance(&app, &i.user, "faction", Some("platforms")).await? {
        return Ok(stale());
    }
    // Linked in the meantime: S.V.E.R still wins.
    let faction = sync::linked(&app, &i.user)
        .await?
        .and_then(|a| a.faction)
        .unwrap_or(pick);
    factions::set(&app, &i.user, &faction).await?;
    Ok(update(platforms_step(Some(&faction))))
}
async fn platforms_chosen(app: App, i: Interaction) -> Res<Value> {
    if !advance(&app, &i.user, "platforms", Some("link")).await? {
        return Ok(stale());
    }
    let settings = home::settings(&app).await;
    if let Some(guild) = home::guild(&app).await {
        for p in i.data["values"].as_array().into_iter().flatten() {
            let Some(p) = p
                .as_str()
                .filter(|p| ["twitch", "youtube", "kick"].contains(p))
            else {
                continue;
            };
            if let Some(r) = home::id(&settings, &format!("role_{p}")) {
                role(&app, &guild, &i.user, &r, true).await;
            }
        }
    }
    let faction = levels::faction(&app, &i.user).await?;
    Ok(update(link_step(&app, &i.user, faction.as_deref()).await?))
}
async fn link_done(app: App, i: Interaction) -> Res<Value> {
    if !advance(&app, &i.user, "link", Some("birthday")).await? {
        return Ok(stale());
    }
    if sync::linked(&app, &i.user).await?.is_some() {
        quests::check(&app, &i.user).await?;
        sync::soon(&app, &i.user);
    }
    Ok(update(birthday_step()))
}
/// The last step: Community Member on, Pending off, and the completion post.
pub(super) async fn complete(app: &App, user: &str, name: &str) -> Res<Value> {
    if !advance(app, user, "birthday", None).await? {
        return Ok(stale());
    }
    let settings = home::settings(app).await;
    if let Some(guild) = home::guild(app).await {
        if let Some(r) = home::id(&settings, "community") {
            role(app, &guild, user, &r, true).await;
        }
        if let Some(r) = home::id(&settings, "pending") {
            role(app, &guild, user, &r, false).await;
        }
    }
    let faction = levels::faction(app, user).await?;
    if let Some(channel) = home::id(&settings, "welcome") {
        let title = match faction.as_deref() {
            Some(f) => format!(
                "{} {name} has joined {}!",
                levels::emoji(Some(f)),
                factions::title(f)
            ),
            None => format!("⭐ {name} has joined the community!"),
        };
        send(app, &channel, json!({"embeds": [{"title": title, "description": format!("Welcome to the community, <@{user}>!"),
            "color": levels::color(faction.as_deref())}], "allowed_mentions": {"parse": []}})).await;
    }
    Ok(update(
        json!({"content": "🎉 You're all set! Welcome to S.V.E.R.", "embeds": [], "components": []}),
    ))
}
async fn skipped(app: App, i: Interaction) -> Res<Value> {
    let name = name_of(&i);
    complete(&app, &i.user, &name).await
}

// ---- The role-select panel ----

async fn panel(app: App, i: Interaction) -> Res<Value> {
    let Some(to) = i
        .option("channel")
        .and_then(Value::as_str)
        .map(String::from)
        .or(i.channel.clone())
    else {
        return Ok(private("Pick a channel."));
    };
    let f = |x: &str| levels::emoji(Some(x));
    // The link buttons are the streamer panel's own (`str:link:`).
    let link = |p: &str, emoji: &str| json!({"type": 2, "style": 2, "label": format!("Link {}", platform_name(p)), "emoji": {"name": emoji}, "custom_id": format!("str:link:{p}")});
    let faction = |x: &str, style: u8| json!({"type": 2, "style": style, "label": factions::title(x), "emoji": {"name": f(x)}, "custom_id": format!("panel:faction:{x}")});
    let body = json!({"embeds": [
            {"color": 0x9b_59b6, "title": "📺 Link your streaming accounts", "description": "Get go-live posts and streaming XP. Press a button and enter your name there."},
            {"color": 0xe7_4c3c, "title": "Choose your faction", "description": format!("Join the war! Earn points for your team and unlock your faction's ranks.\n\n{} **Myria**: strength and conquest\n{} **Aetheron**: knowledge and strategy\n{} **Glint**: creativity and energy", f("myria"), f("aetheron"), f("glint"))},
            {"color": 0x34_98db, "title": "🔔 Notifications", "description": "🎁 **Free Games**: get pinged when free games are posted\n📺 **Stream alerts**: turn go-live posts for your linked accounts on or off"},
            {"color": 0xff_69b4, "title": "🎂 Set your birthday", "description": "A shout-out and **2× XP** all day on your birthday."}],
        "components": [
            row(vec![link("twitch", "💜"), link("youtube", "▶️"), link("kick", "💚")]),
            row(vec![faction("myria", 4), faction("aetheron", 1), faction("glint", 2)]),
            row(vec![json!({"type": 2, "style": 2, "label": "Free Games", "emoji": {"name": "🎁"}, "custom_id": "panel:freegames"}),
                json!({"type": 2, "style": 2, "label": "Stream alerts on/off", "emoji": {"name": "📺"}, "custom_id": "panel:alerts"})]),
            row(vec![json!({"type": 2, "style": 2, "label": "Set my birthday", "emoji": {"name": "🎂"}, "custom_id": "bday:open:panel"})])],
        "allowed_mentions": {"parse": []}});
    Ok(private(match send(&app, &to, body).await {
        Some(_) => format!("Role-select panel posted in <#{to}>."),
        None => {
            "The panel couldn't be posted there. Check the bot can send messages in that channel."
                .into()
        }
    }))
}

async fn free_games_role(app: App, i: Interaction) -> Res<Value> {
    let (Some(guild), Some(r)) = (
        home::guild(&app).await,
        home::id(&home::settings(&app).await, "free_games_role"),
    ) else {
        return Ok(private("The Free Games role isn't set up yet."));
    };
    let has = i.roles.contains(&r);
    if !role(&app, &guild, &i.user, &r, !has).await {
        return Ok(private(
            "The role couldn't be changed. Try again in a minute.",
        ));
    }
    Ok(private(if has {
        "🎁 You won't be pinged for free games anymore."
    } else {
        "🎁 You'll be pinged when free games are posted."
    }))
}
async fn alerts_toggle(app: &App, i: &Interaction) -> Res<Value> {
    // Posts on for any account turns them all off; otherwise all on.
    let on: Option<bool> = sqlx::query_scalar("UPDATE discord_stream_accounts SET alerts=NOT (SELECT bool_or(alerts) FROM discord_stream_accounts WHERE discord_user=$1) WHERE discord_user=$1 RETURNING alerts")
        .bind(&i.user).fetch_optional(&app.db).await?;
    Ok(private(match on {
        None => "You have no streaming accounts linked yet. Use the buttons above to link one.",
        Some(true) => "📺 Go-live posts for your streams are on.",
        Some(false) => "📺 Go-live posts for your streams are off.",
    }))
}
async fn panel_faction(app: App, i: Interaction) -> Res<Value> {
    let pick = i.custom_id().rsplit(':').next().unwrap_or_default();
    factions::choose(&app, &i.user, pick).await
}

// ---- Commands and buttons ----

/// Every command this feature adds.
pub fn definitions() -> Vec<Value> {
    let mut all = vec![
        json!({"name": "rolepanel", "description": "Post the role-select panel.", "default_member_permissions": MANAGE_GUILD.to_string(), "options": [
            {"type": 7, "name": "channel", "description": "Where (here by default)", "channel_types": [0, 5]}]}),
    ];
    all.extend(rituals::definitions());
    all.extend(events::definitions());
    all.extend(tools::definitions());
    all.extend(feeds::definitions());
    all
}

/// Answers this feature's commands; None when the command isn't one of them.
pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    if i.name() == "rolepanel" {
        if !i.staff() {
            return Some(private("Only staff can use that."));
        }
        later(app, i, true, panel);
        return Some(ANSWERED);
    }
    if let Some(reply) = rituals::command(app, i).await {
        return Some(reply);
    }
    if let Some(reply) = events::command(app, i).await {
        return Some(reply);
    }
    if let Some(reply) = tools::command(app, i).await {
        return Some(reply);
    }
    feeds::command(app, i).await
}

/// Button, menu and form ids this feature answers.
pub fn owns(id: &str) -> bool {
    [
        "onb:", "panel:", "bday:", "event:", "poll:", "give:", "mentor:", "ritual:", "embed:",
        "wow:",
    ]
    .iter()
    .any(|p| id.starts_with(p))
}
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let id = i.custom_id();
    Some(match id {
        "onb:rules" => {
            updating(app, i, rules_accepted);
            ANSWERED
        }
        _ if id.starts_with("onb:faction:") => {
            updating(app, i, faction_chosen);
            ANSWERED
        }
        "onb:platforms" | "onb:platforms:none" => {
            updating(app, i, platforms_chosen);
            ANSWERED
        }
        "onb:link" => {
            updating(app, i, link_done);
            ANSWERED
        }
        "onb:skip" => {
            updating(app, i, skipped);
            ANSWERED
        }
        _ if id.starts_with("panel:faction:") => {
            later(app, i, true, panel_faction);
            ANSWERED
        }
        "panel:freegames" => {
            later(app, i, true, free_games_role);
            ANSWERED
        }
        "panel:alerts" => answer(alerts_toggle(app, i).await),
        _ if id.starts_with("bday:") || id.starts_with("ritual:") => {
            return rituals::component(app, i).await;
        }
        _ if id.starts_with("event:") || id.starts_with("poll:") || id.starts_with("give:") => {
            return events::component(app, i).await;
        }
        _ if id.starts_with("mentor:") || id.starts_with("embed:") => {
            return tools::component(app, i).await;
        }
        _ if id.starts_with("wow:") => return feeds::component(app, i).await,
        _ => return None,
    })
}

/// The staff page's forum setup and the GitHub and Chromely webhooks.
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/admin/discord/forums", post(tools::forums))
        .route("/api/integrations/github", post(feeds::github))
        .route("/api/integrations/chromely", post(feeds::chromely_hook))
}

/// Every few seconds: giveaways, polls and reminders that are due, so each lands on time. Only in
/// the process running the bot.
pub async fn tick(app: &App) -> Res<()> {
    if !super::gateway::leading() {
        return Ok(());
    }
    events::due(app).await
}

/// Each minute on the Gateway's ticker.
pub async fn minute(app: &App) -> Res<()> {
    rituals::minute(app).await?;
    events::minute(app).await?;
    tools::minute(app).await?;
    if factions::due("free_games", 30) {
        feeds::free_games(app).await?;
    }
    Ok(())
}
