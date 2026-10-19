//! Slash commands, buttons and forms on the home server (docs/DISCORD_BOT.md "How it runs"). Every
//! command is registered on the home server only; admin commands need Manage Server and are hidden
//! from everyone else by default.
use super::{call, home};
use crate::App;
use reqwest::Method;
use serde_json::{Value, json};

pub const MANAGE_GUILD: u64 = 1 << 5;
const ADMINISTRATOR: u64 = 1 << 3;
/// Discord's flag for a reply only the person who asked can see.
const EPHEMERAL: u64 = 1 << 6;

/// One interaction, as Discord sends it.
pub struct Interaction {
    pub id: String,
    pub token: String,
    /// 2 command, 3 button or menu, 4 autocomplete, 5 form.
    pub kind: u64,
    pub guild: Option<String>,
    pub channel: Option<String>,
    pub user: String,
    pub roles: Vec<String>,
    pub permissions: u64,
    pub data: Value,
    /// The whole interaction (the member's name and avatar, resolved users).
    pub raw: Value,
}
impl Interaction {
    pub fn parse(raw: &Value) -> Option<Self> {
        let user = raw["member"]["user"]["id"]
            .as_str()
            .or(raw["user"]["id"].as_str())?;
        Some(Self {
            id: raw["id"].as_str()?.into(),
            token: raw["token"].as_str()?.into(),
            kind: raw["type"].as_u64()?,
            guild: raw["guild_id"].as_str().map(String::from),
            channel: raw["channel_id"].as_str().map(String::from),
            user: user.into(),
            roles: raw["member"]["roles"]
                .as_array()
                .map(|r| {
                    r.iter()
                        .filter_map(|r| r.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            permissions: raw["member"]["permissions"]
                .as_str()
                .and_then(|p| p.parse().ok())
                .unwrap_or(0),
            data: raw["data"].clone(),
            raw: raw.clone(),
        })
    }
    /// The user an option names (its id), or the asker.
    pub fn user_or_me(&self, option: &str) -> String {
        self.option(option)
            .and_then(Value::as_str)
            .unwrap_or(&self.user)
            .to_string()
    }
    /// A user's display name and avatar hash, from the asker or the resolved option.
    pub fn profile(&self, user: &str) -> (String, Option<String>) {
        let u = if user == self.user {
            self.raw["member"]["user"].clone()
        } else {
            self.data["resolved"]["users"][user].clone()
        };
        let name = u["global_name"]
            .as_str()
            .or(u["username"].as_str())
            .unwrap_or("Member")
            .to_string();
        (name, u["avatar"].as_str().map(String::from))
    }
    /// Manage Server or Administrator.
    pub fn staff(&self) -> bool {
        self.permissions & (MANAGE_GUILD | ADMINISTRATOR) != 0
    }
    pub fn name(&self) -> &str {
        self.data["name"].as_str().unwrap_or_default()
    }
    /// The chosen subcommand, when the command has them.
    pub fn sub(&self) -> Option<&str> {
        let first = &self.data["options"][0];
        matches!(first["type"].as_u64(), Some(1 | 2))
            .then(|| first["name"].as_str())
            .flatten()
    }
    /// An option's value, looking inside a subcommand.
    pub fn option(&self, name: &str) -> Option<&Value> {
        let mut options = &self.data["options"];
        if self.sub().is_some() {
            options = &options[0]["options"];
        }
        options
            .as_array()?
            .iter()
            .find(|o| o["name"] == name)
            .map(|o| &o["value"])
    }
    /// A button's or menu's id.
    pub fn custom_id(&self) -> &str {
        self.data["custom_id"].as_str().unwrap_or_default()
    }
}

/// A reply in the channel.
pub fn say(content: impl Into<String>) -> Value {
    json!({"type": 4, "data": {"content": content.into(), "allowed_mentions": {"parse": []}}})
}
/// A reply only the asker sees.
pub fn private(content: impl Into<String>) -> Value {
    json!({"type": 4, "data": {"content": content.into(), "flags": EPHEMERAL, "allowed_mentions": {"parse": []}}})
}
/// An embed reply, private or not.
pub fn embed(embed: Value, only_you: bool) -> Value {
    json!({"type": 4, "data": {"embeds": [embed], "flags": if only_you { EPHEMERAL } else { 0 }, "allowed_mentions": {"parse": []}}})
}

/// Every command's definition, registered on the home server.
pub fn definitions() -> Vec<Value> {
    let mut all = vec![
        json!({"name": "help", "description": "What the S.V.E.R bot can do."}),
        json!({"name": "health", "description": "How the S.V.E.R bot is doing.",
            "default_member_permissions": MANAGE_GUILD.to_string()}),
    ];
    all.extend(super::levels::definitions());
    all.extend(super::quests::definitions());
    all.extend(super::factions::definitions());
    all.extend(super::sync::definitions());
    all.extend(super::moderation::definitions());
    all
}

/// Puts `definitions` on the home server (Discord replaces the whole set). False when Discord
/// refused or couldn't be reached; true when done, or when there's no home server or connection yet.
pub async fn register(app: &App) -> bool {
    let application: Option<String> =
        sqlx::query_scalar("SELECT application_id FROM discord_gateway")
            .fetch_optional(&app.db)
            .await
            .ok()
            .flatten()
            .flatten();
    let (Some(application), Some(guild)) = (application, home::guild(app).await) else {
        return true;
    };
    matches!(
        call(
            app,
            Method::PUT,
            &format!("/applications/{application}/guilds/{guild}/commands"),
            Some(json!(definitions())),
        )
        .await,
        Some((status, _)) if status < 300
    )
}

/// Answers one interaction.
pub async fn interaction(app: &App, raw: Value) {
    let Some(i) = Interaction::parse(&raw) else {
        return;
    };
    if i.guild.is_some() && i.guild != home::guild(app).await {
        respond(
            app,
            &i,
            private("The S.V.E.R bot's commands work on the S.V.E.R server."),
        )
        .await;
        return;
    }
    let reply = match i.kind {
        2 => command(app, &i).await,
        3 | 5 if i.custom_id().starts_with("mod:") => super::moderation::component(app, &i).await,
        3 if i.custom_id().starts_with("fac:") => super::factions::component(app, &i).await,
        _ => None,
    };
    if let Some(reply) = reply.filter(|r| *r != ANSWERED) {
        respond(app, &i, reply).await;
    }
}
/// What a command returns when it has answered on its own (a spawned slow answer).
pub const ANSWERED: Value = Value::Null;

async fn command(app: &App, i: &Interaction) -> Option<Value> {
    if let Some(reply) = super::levels::command(app, i).await {
        return Some(reply);
    }
    if let Some(reply) = super::quests::command(app, i).await {
        return Some(reply);
    }
    if let Some(reply) = super::factions::command(app, i).await {
        return Some(reply);
    }
    if let Some(reply) = super::sync::command(app, i).await {
        return Some(reply);
    }
    Some(match i.name() {
        "help" => help(i),
        "health" if i.staff() => health(app).await,
        "health" => private("Only staff can use that."),
        "mod" | "modpanel" | "ticket" => return super::moderation::command(app, i).await,
        _ => private("That command isn't available right now."),
    })
}

pub async fn respond(app: &App, i: &Interaction, reply: Value) {
    let path = format!("/interactions/{}/{}/callback", i.id, i.token);
    if call(app, Method::POST, &path, Some(reply)).await.is_none() {
        eprintln!("discord_event=interaction outcome=failed");
    }
}

/// "Thinking…": buys the 15 minutes Discord allows for a slow answer, sent with `finish`.
pub fn defer(only_you: bool) -> Value {
    json!({"type": 5, "data": {"flags": if only_you { EPHEMERAL } else { 0 }}})
}
/// Replaces a deferred reply, optionally with one attached file.
pub async fn finish(app: &App, i: &Interaction, data: Value, file: Option<(&str, Vec<u8>)>) {
    let Some(token) = super::bot_token() else {
        return;
    };
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
    let url = format!(
        "{}/webhooks/{application}/{}/messages/@original",
        super::api(),
        i.token
    );
    let request = app
        .http
        .patch(url)
        .header("Authorization", format!("Bot {token}"));
    let request = match file {
        Some((name, bytes)) => {
            let mut data = data;
            data["attachments"] = json!([{"id": 0, "filename": name}]);
            let part = reqwest::multipart::Part::bytes(bytes)
                .file_name(name.to_string())
                .mime_str("image/png")
                .expect("a fixed media type");
            request.multipart(
                reqwest::multipart::Form::new()
                    .text("payload_json", data.to_string())
                    .part("files[0]", part),
            )
        }
        None => request.json(&data),
    };
    if !request.send().await.is_ok_and(|r| r.status().is_success()) {
        eprintln!("discord_event=interaction_finish outcome=failed");
    }
}

fn help(i: &Interaction) -> Value {
    let lines: Vec<String> = definitions()
        .iter()
        .filter(|d| i.staff() || d["default_member_permissions"].is_null())
        .map(|d| {
            format!(
                "`/{}` {}",
                d["name"].as_str().unwrap_or_default(),
                d["description"].as_str().unwrap_or_default()
            )
        })
        .collect();
    embed(
        json!({"title": "S.V.E.R bot", "description": lines.join("\n"), "color": 0x8b_9bb4}),
        true,
    )
}

async fn health(app: &App) -> Value {
    let Ok(h) = home::health(app).await else {
        return private("The bot's status couldn't be read. Try again in a minute.");
    };
    let when = |v: &Value| {
        v.as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map_or("never".into(), |t| format!("<t:{}:R>", t.timestamp()))
    };
    let g = &h["gateway"];
    let connection = if g["connected"] == true {
        format!("Connected {}", when(&g["connected_at"]))
    } else {
        format!(
            "Not connected{}",
            g["problem"]
                .as_str()
                .map(|p| format!(": {p}"))
                .unwrap_or_default()
        )
    };
    let list = |v: &Value| {
        let items: Vec<&str> = v
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if items.is_empty() {
            "None".to_string()
        } else {
            items.join(", ")
        }
    };
    let healthy = g["connected"] == true
        && h["home"]["missing"].as_array().is_some_and(Vec::is_empty)
        && h["home"]["roles_above_bot"]
            .as_array()
            .is_some_and(Vec::is_empty);
    embed(
        json!({"title": "S.V.E.R bot health", "color": if healthy { 0x2e_cc71 } else { 0xe6_7e22 },
        "fields": [
            {"name": "Gateway", "value": connection},
            {"name": "Last event", "value": when(&g["last_event_at"]), "inline": true},
            {"name": "Missing permissions", "value": list(&h["home"]["missing"])},
            {"name": "Roles above the bot", "value": list(&h["home"]["roles_above_bot"])},
        ]}),
        true,
    )
}
