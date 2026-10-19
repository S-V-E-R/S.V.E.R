//! Moderation and safety on the home server (docs/DISCORD_BOT.md "Moderation and safety"):
//! automod and the safety layer (`automod`); strikes, escalation, manual actions, lockdowns,
//! anti-raid, the new-account filter and the ban trap (`actions`); tickets and modmail
//! (`tickets`); and the mod log.
//!
//! Everything that has to outlive a restart is a row: strikes and cases, open escalations and their
//! re-ping, timed bans, lockdowns and their auto-unlock, tickets, and recent messages kept as
//! evidence. Thresholds and word lists are the staff's private settings (`home::FIELDS`); unset,
//! the check they drive is off.
mod actions;
mod automod;
mod tickets;

use super::{
    api, bot_token, call,
    commands::{self, Interaction, private},
    home,
};
use crate::App;
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde_json::{Map, Value, json};

/// Discord's permission bits that moderation checks.
const KICK: u64 = 1 << 1;
const BAN: u64 = 1 << 2;
const ADMINISTRATOR: u64 = 1 << 3;
const MANAGE_CHANNELS: u64 = 1 << 4;
const MANAGE_GUILD: u64 = 1 << 5;
const MANAGE_MESSAGES: u64 = 1 << 13;
/// Moderate Members (timeouts): who sees /mod and /modpanel by default.
const MODERATE: u64 = 1 << 40;
/// Permissions that make a member staff, whom moderation never acts on.
const STAFF: u64 = KICK | BAN | ADMINISTRATOR | MANAGE_GUILD | MODERATE;

/// The moderator acting: their id, roles and server permissions.
#[derive(Clone)]
struct By {
    id: String,
    roles: Vec<String>,
    permissions: u64,
}
impl By {
    fn of(i: &Interaction) -> Self {
        Self {
            id: i.user.clone(),
            roles: i.roles.clone(),
            permissions: i.permissions,
        }
    }
    /// Has this permission (Administrator has them all).
    fn can(&self, bit: u64) -> bool {
        self.permissions & (bit | ADMINISTRATOR) != 0
    }
}
/// The answer when a moderator lacks the permission an action needs.
fn needs(bit: u64) -> Value {
    private(missing(bit))
}
fn missing(bit: u64) -> String {
    let name = match bit {
        KICK => "Kick Members",
        BAN => "Ban Members",
        MANAGE_CHANNELS => "Manage Channels",
        MANAGE_GUILD => "Manage Server",
        MANAGE_MESSAGES => "Manage Messages",
        _ => "Timeout Members",
    };
    format!("That needs the {name} permission.")
}

/// The home server and its settings.
#[derive(Clone)]
struct Home {
    guild: String,
    s: Map<String, Value>,
}
async fn load(app: &App) -> Option<Home> {
    let guild = home::guild(app).await?;
    Some(Home {
        guild,
        s: home::settings(app).await,
    })
}

/// The Gateway events moderation handles, processed in order off the Gateway's own task.
pub const QUEUED: &[&str] = &[
    "MESSAGE_CREATE",
    "MESSAGE_UPDATE",
    "MESSAGE_DELETE",
    "MESSAGE_DELETE_BULK",
    "GUILD_MEMBER_ADD",
    "GUILD_MEMBER_REMOVE",
];

/// One Gateway event. True when moderation removed the message or is holding the new member, so
/// other features (XP, welcome) leave it alone.
pub async fn event(app: &App, kind: &str, data: &Value) -> bool {
    let Some(h) = load(app).await else {
        return false;
    };
    let home = data["guild_id"].as_str() == Some(h.guild.as_str());
    let result = match kind {
        "MESSAGE_CREATE" if data["guild_id"].is_null() => {
            tickets::direct(app, &h, data).await.map(|()| false)
        }
        _ if !home => Ok(false),
        "MESSAGE_CREATE" => automod::message(app, &h, data).await,
        "MESSAGE_UPDATE" => automod::edited(app, &h, data).await.map(|()| false),
        "MESSAGE_DELETE" => automod::deleted(app, &h, std::slice::from_ref(&data["id"]), false)
            .await
            .map(|()| false),
        "MESSAGE_DELETE_BULK" => {
            let ids = data["ids"].as_array().cloned().unwrap_or_default();
            automod::deleted(app, &h, &ids, true).await.map(|()| false)
        }
        "GUILD_MEMBER_ADD" => actions::joined(app, &h, data).await,
        "GUILD_MEMBER_REMOVE" => actions::left(app, &h, data).await.map(|()| false),
        _ => Ok(false),
    };
    result.unwrap_or_else(|_| {
        eprintln!("discord_event=moderation kind={kind} outcome=error");
        false
    })
}

/// Every few seconds: timed bans, escalation re-pings, auto-unlocks, ticket inactivity,
/// unanswered support questions and evidence retention.
pub async fn tick(app: &App) -> crate::profiles::Res<()> {
    let Some(h) = load(app).await else {
        return Ok(());
    };
    actions::tick(app, &h).await?;
    tickets::tick(app, &h).await?;
    automod::tick(app, &h).await
}

/// The commands this feature adds.
pub fn definitions() -> Vec<Value> {
    let user = json!({"type": 6, "name": "user", "description": "The member", "required": true});
    let reason = |required: bool| json!({"type": 3, "name": "reason", "description": "Why", "required": required, "max_length": 500});
    let length = json!({"type": 3, "name": "duration", "description": "How long, like 30m, 2h, 7d or 2w", "max_length": 10});
    let sub = |name: &str, description: &str, options: Vec<Value>| json!({"type": 1, "name": name, "description": description, "options": options});
    let role = json!({"type": 8, "name": "role", "description": "The role", "required": true});
    let channel = json!({"type": 7, "name": "channel", "description": "One channel instead of the whole server", "channel_types": [0, 5]});
    let mods = MODERATE.to_string();
    vec![
        json!({"name": "mod", "description": "Moderation tools.", "default_member_permissions": mods, "options": [
            sub("warn", "Warn a member (a strike).", vec![user.clone(), reason(true)]),
            sub("warnings", "A member's active strikes.", vec![user.clone()]),
            sub("clearwarn", "Clear one strike, or all of a member's strikes.", vec![user.clone(),
                json!({"type": 4, "name": "id", "description": "The strike's number (all when left out)", "min_value": 1})]),
            sub("mute", "Time a member out.", vec![user.clone(), length.clone(), reason(false)]),
            sub("unmute", "End a member's timeout.", vec![user.clone()]),
            sub("kick", "Kick a member.", vec![user.clone(), reason(false)]),
            sub("ban", "Ban someone, for a while or for good.", vec![user.clone(), length.clone(), reason(false)]),
            sub("softban", "Ban and unban at once, deleting a week of messages.", vec![user.clone(), reason(false)]),
            sub("duration", "Change how long an active mute or ban lasts.", vec![user.clone(),
                json!({"type": 3, "name": "duration", "description": "New length from now, like 2h or 7d", "required": true, "max_length": 10})]),
            sub("purge", "Delete recent messages here.", vec![
                json!({"type": 4, "name": "count", "description": "How many (1-100)", "required": true, "min_value": 1, "max_value": 100}),
                json!({"type": 6, "name": "user", "description": "Only this member's"}),
                json!({"type": 3, "name": "only", "description": "Only these", "choices": [
                    {"name": "bots", "value": "bots"}, {"name": "links", "value": "links"}, {"name": "files", "value": "files"}]})]),
            sub("note", "Add a private note about a member.", vec![user.clone(),
                json!({"type": 3, "name": "text", "description": "The note", "required": true, "max_length": 1000})]),
            sub("notes", "A member's notes.", vec![user.clone()]),
            sub("history", "A member's moderation history.", vec![user.clone()]),
            sub("protect", "Members with this role can't be moderated.", vec![role.clone()]),
            sub("unprotect", "Stop protecting a role.", vec![role]),
            sub("lockdown", "Stop members sending messages.", vec![channel.clone(), length]),
            sub("unlock", "Lift a lockdown.", vec![channel.clone()]),
            sub("bantrap", "The channel where posting means a ban.", vec![
                json!({"type": 3, "name": "action", "description": "What to do", "required": true, "choices": [
                    {"name": "status", "value": "status"}, {"name": "set", "value": "set"},
                    {"name": "create", "value": "create"}, {"name": "off", "value": "off"}]}),
                json!({"type": 7, "name": "channel", "description": "The channel (for set)", "channel_types": [0, 5]})]),
        ]}),
        json!({"name": "modpanel", "description": "A member's moderation panel.", "default_member_permissions": MODERATE.to_string(),
            "options": [user]}),
        json!({"name": "ticket", "description": "Open a private ticket with the moderators.", "options": [
            {"type": 3, "name": "subject", "description": "What it's about", "required": true, "max_length": 1000},
            {"type": 3, "name": "category", "description": "What kind of ticket", "choices": tickets::CATEGORIES.iter()
                .map(|c| json!({"name": c, "value": c})).collect::<Vec<_>>()}]}),
    ]
}

/// Moderate Members, Manage Server, Administrator, or the mod team role.
fn moderator(i: &Interaction, h: &Home) -> bool {
    i.staff()
        || i.permissions & MODERATE != 0
        || home::id(&h.s, "mod_team").is_some_and(|r| i.roles.contains(&r))
}

/// /mod, /modpanel and /ticket. They can take a few Discord calls, so the answer follows a
/// "thinking…" reply.
pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    let Some(h) = load(app).await else {
        return Some(private("The S.V.E.R server isn't set up yet."));
    };
    if i.name() != "ticket" && !moderator(i, &h) {
        return Some(private("Only moderators can use that."));
    }
    commands::respond(app, i, json!({"type": 5, "data": {"flags": 64}})).await;
    let reply = match i.name() {
        "ticket" => tickets::command(app, &h, i).await,
        "modpanel" => actions::panel(app, &h, i).await,
        _ => actions::command(app, &h, i).await,
    }
    .unwrap_or_else(|_| private("That didn't work. Try again in a minute."));
    follow(app, i, &reply, true).await;
    None
}

/// The answer to an interaction that was deferred: it edits the deferred message (`edit`), or
/// arrives as a new private message.
async fn follow(app: &App, i: &Interaction, reply: &Value, edit: bool) {
    let application: Option<String> =
        sqlx::query_scalar("SELECT application_id FROM discord_gateway")
            .fetch_optional(&app.db)
            .await
            .ok()
            .flatten()
            .flatten();
    let Some(application) = application else {
        return;
    };
    let hook = format!("/webhooks/{application}/{}", i.token);
    if edit {
        let path = format!("{hook}/messages/@original");
        call(app, Method::PATCH, &path, Some(reply["data"].clone())).await;
    } else {
        call(app, Method::POST, &hook, Some(reply["data"].clone())).await;
    }
}

/// Buttons, menus and forms (custom ids `mod:<area>:<action>:<id>`); moderators only.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let h = load(app).await?;
    if !moderator(i, &h) {
        return Some(private("Only moderators can use this."));
    }
    let parts: Vec<&str> = i.custom_id().split(':').collect();
    let part = |n: usize| parts.get(n).copied().unwrap_or_default();
    // A form opens at once; everything else may take several Discord calls, so it's acknowledged
    // first and answered after (editing the message, or privately).
    let opens_form = i.kind == 3
        && matches!(
            (part(1), part(2)),
            ("esc", "custom") | ("panel", _) | ("ticket", "close" | "note")
        );
    if !opens_form {
        commands::respond(app, i, json!({"type": 6})).await;
    }
    let reply = match part(1) {
        "ticket" => tickets::component(app, &h, i, part(2), part(3)).await,
        area => actions::component(app, &h, i, area, part(2), part(3)).await,
    }
    .unwrap_or_else(|_| private("That didn't work. Try again in a minute."));
    if opens_form {
        return Some(reply);
    }
    follow(app, i, &reply, reply["type"] == 7).await;
    None
}

/// A Discord call that should succeed.
async fn done(app: &App, method: Method, path: &str, body: Option<Value>) -> bool {
    matches!(call(app, method, path, body).await, Some((status, _)) if status < 300)
}
/// Posts a message (pinging no one unless the body says so); its id.
async fn post(app: &App, channel: &str, mut body: Value) -> Option<String> {
    if body.get("allowed_mentions").is_none() {
        body["allowed_mentions"] = json!({"parse": []});
    }
    match call(
        app,
        Method::POST,
        &format!("/channels/{channel}/messages"),
        Some(body),
    )
    .await
    {
        Some((status, v)) if status < 300 => v["id"].as_str().map(String::from),
        _ => None,
    }
}
/// Posts a message with a text file attached (transcripts, bulk-delete evidence).
async fn upload(app: &App, channel: &str, mut body: Value, name: &str, text: &str) -> bool {
    let Some(token) = bot_token() else {
        return false;
    };
    body["allowed_mentions"] = json!({"parse": []});
    body["attachments"] = json!([{"id": 0, "filename": name}]);
    let boundary = format!("sver{}", uuid::Uuid::new_v4().simple());
    let form = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{body}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"{name}\"\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n--{boundary}--\r\n"
    );
    let sent = app
        .http
        .post(format!("{}/channels/{channel}/messages", api()))
        .header("Authorization", format!("Bot {token}"))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(form)
        .send()
        .await;
    sent.is_ok_and(|r| r.status().is_success())
}
/// A direct message; false when the member's DMs are closed or Discord refused.
async fn dm(app: &App, user: &str, body: Value) -> bool {
    let Some((200, channel)) = call(
        app,
        Method::POST,
        "/users/@me/channels",
        Some(json!({"recipient_id": user})),
    )
    .await
    else {
        return false;
    };
    match channel["id"].as_str() {
        Some(id) => post(app, id, body).await.is_some(),
        None => false,
    }
}

fn embed(title: &str, colour: u32, description: impl Into<String>) -> Value {
    json!({"title": title, "color": colour, "description": description.into()})
}
fn button(label: &str, style: u8, id: String) -> Value {
    json!({"type": 2, "style": style, "label": label, "custom_id": id})
}
fn row(components: Vec<Value>) -> Value {
    json!({"type": 1, "components": components})
}
/// Edits the message a button sits on; fields left out stay as they were.
fn update(data: Value) -> Value {
    json!({"type": 7, "data": data})
}
/// A form with text inputs: (id, label, required, long).
fn form(id: String, title: &str, inputs: &[(&str, &str, bool, bool)]) -> Value {
    let rows: Vec<Value> = inputs
        .iter()
        .map(|(key, label, required, long)| {
            row(vec![
                json!({"type": 4, "custom_id": key, "label": label, "required": required,
                "style": if *long { 2 } else { 1 }, "max_length": if *long { 1000 } else { 10 }}),
            ])
        })
        .collect();
    json!({"type": 9, "data": {"custom_id": id, "title": title, "components": rows}})
}
/// A value typed into a form.
fn field<'a>(i: &'a Interaction, key: &str) -> Option<&'a str> {
    i.data["components"]
        .as_array()?
        .iter()
        .filter_map(|r| r["components"].as_array())
        .flatten()
        .find(|c| c["custom_id"] == key)?["value"]
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// Posts in the mod log; the message id.
async fn log(app: &App, h: &Home, body: Value) -> Option<String> {
    post(app, &home::id(&h.s, "mod_log")?, body).await
}
async fn note(app: &App, h: &Home, title: &str, colour: u32, description: impl Into<String>) {
    log(
        app,
        h,
        json!({"embeds": [embed(title, colour, description)]}),
    )
    .await;
}
/// A message to the mod team in the mod log, pinging their role.
async fn alert_team(app: &App, h: &Home, text: &str) {
    let body = match home::id(&h.s, "mod_team") {
        Some(role) => {
            json!({"content": format!("<@&{role}> {text}"), "allowed_mentions": {"roles": [role]}})
        }
        None => json!({"content": text}),
    };
    log(app, h, body).await;
}

/// When a Discord account (or message) was created, from its id.
fn created(id: &str) -> Option<DateTime<Utc>> {
    let n: u64 = id.parse().ok()?;
    DateTime::from_timestamp_millis(((n >> 22) + 1_420_070_400_000) as i64)
}
/// Discord's relative-time markup.
fn ago(t: DateTime<Utc>) -> String {
    format!("<t:{}:R>", t.timestamp())
}
/// A length like 30m, 2h, 7d or 2w, in minutes (at most ten years).
fn minutes(text: &str) -> Option<i64> {
    let t = text.trim().to_lowercase();
    let split = t.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = t.split_at(split);
    let n: i64 = n.parse().ok()?;
    let per = match unit.trim() {
        "m" | "min" | "mins" => 1,
        "h" | "hr" | "hrs" => 60,
        "d" | "day" | "days" => 1440,
        "w" | "wk" | "wks" => 10_080,
        _ => return None,
    };
    let total = n.checked_mul(per)?;
    (1..=5_256_000).contains(&total).then_some(total)
}
/// Text for a code block in an embed: no fences, at most `max` characters.
fn clip(text: &str, max: usize) -> String {
    let text = if text.trim().is_empty() {
        "[no text]"
    } else {
        text
    };
    let mut out: String = text.replace('`', "'").chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}
/// Attachment URLs of a message.
fn files(d: &Value) -> Vec<String> {
    d["attachments"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| f["url"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_lengths_ids_and_text() {
        assert_eq!(minutes("30m"), Some(30));
        assert_eq!(minutes("2h"), Some(120));
        assert_eq!(minutes("7d"), Some(10_080));
        assert_eq!(minutes(" 2W "), Some(20_160));
        for bad in ["", "m", "10", "5x", "0m", "-3h", "99999999999w"] {
            assert_eq!(minutes(bad), None, "{bad}");
        }
        // Discord's own example snowflake.
        assert_eq!(
            created("175928847299117063").unwrap().to_rfc3339(),
            "2016-04-30T11:18:25.796+00:00"
        );
        assert_eq!(clip("a`b", 10), "a'b");
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(clip(" ", 20), "[no text]");
    }
}
