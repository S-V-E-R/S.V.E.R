//! The S.V.E.R home server's settings (docs/DISCORD_BOT.md "How it runs"): which Discord server
//! the community bot serves, and its channels, roles and switches, set by staff at /admin/discord.
//! Each feature reads its settings by key from `FIELDS`.
use super::{Lists, lists};
use crate::{
    App,
    profiles::{Fail, Res},
    safety,
};
use axum::{Json, Router, extract::State, routing::get};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// A text or announcement channel.
    Text,
    /// A forum channel, or a text channel used for threads.
    Forum,
    Voice,
    Category,
    /// Several text channels.
    Texts,
    /// A role the bot gives and takes, so it must sit below the bot's own role.
    Given,
    /// A role the bot only reads or mentions.
    Role,
    Toggle,
    /// An IANA time zone.
    Zone,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Forum => "forum",
            Self::Voice => "voice",
            Self::Category => "category",
            Self::Texts => "texts",
            Self::Given => "given",
            Self::Role => "role",
            Self::Toggle => "toggle",
            Self::Zone => "zone",
        }
    }
    /// Discord channel types each kind accepts.
    fn channel_types(self) -> &'static [i64] {
        match self {
            Self::Text | Self::Texts => &[0, 5],
            Self::Forum => &[0, 5, 15],
            Self::Voice => &[2, 13],
            Self::Category => &[4],
            _ => &[],
        }
    }
}

/// Every setting: key, kind, group and label. Toggles are on unless staff turn them off.
pub const FIELDS: &[(&str, Kind, &str, &str)] = &[
    ("mod_log", Kind::Text, "Moderation", "Mod log"),
    ("ban_trap", Kind::Text, "Moderation", "Ban trap channel"),
    ("mod_team", Kind::Role, "Moderation", "Mod team role"),
    ("restricted", Kind::Given, "Moderation", "Restricted role"),
    ("automod", Kind::Toggle, "Moderation", "Automod"),
    ("support", Kind::Texts, "Moderation", "Support channels"),
    ("welcome", Kind::Text, "Welcome", "Welcome channel"),
    ("goodbye", Kind::Text, "Welcome", "Goodbye channel"),
    ("community", Kind::Given, "Welcome", "Community Member role"),
    (
        "welcome_post",
        Kind::Toggle,
        "Welcome",
        "Public welcome post",
    ),
    ("welcome_dm", Kind::Toggle, "Welcome", "Onboarding by DM"),
    ("goodbye_post", Kind::Toggle, "Welcome", "Goodbye post"),
    ("announcements", Kind::Text, "Community", "Announcements"),
    ("events", Kind::Text, "Community", "Events channel"),
    ("giveaways", Kind::Text, "Community", "Giveaways channel"),
    ("birthdays", Kind::Text, "Community", "Birthday channel"),
    ("birthday_role", Kind::Given, "Community", "Birthday role"),
    ("free_games", Kind::Text, "Community", "Free games channel"),
    ("free_games_on", Kind::Toggle, "Community", "Free games"),
    ("game_chat", Kind::Forum, "Community", "Game chat"),
    ("stream_help", Kind::Forum, "Community", "Stream help"),
    ("clips", Kind::Forum, "Community", "Clips and highlights"),
    ("lfg", Kind::Forum, "Community", "Looking for group"),
    ("mentor_role", Kind::Given, "Community", "Mentor role"),
    (
        "wow_leaderboard",
        Kind::Forum,
        "Community",
        "WoW speedrun leaderboard",
    ),
    ("zone", Kind::Zone, "Community", "Default time zone"),
    ("go_live", Kind::Text, "Streamers", "Go-live channel"),
    ("stream_promo", Kind::Text, "Streamers", "Stream promotion"),
    ("now_live", Kind::Given, "Streamers", "Now Live role"),
    ("alerts_on", Kind::Toggle, "Streamers", "Go-live alerts"),
    ("rank_up", Kind::Text, "Levels", "Rank-up channel"),
    ("achievements", Kind::Text, "Levels", "Achievements channel"),
    ("leveling", Kind::Toggle, "Levels", "Levels"),
    ("xp_off", Kind::Texts, "Levels", "Channels with no XP"),
    (
        "factions_channel",
        Kind::Text,
        "Factions",
        "Faction channel",
    ),
    ("myria", Kind::Given, "Factions", "Myria role"),
    ("aetheron", Kind::Given, "Factions", "Aetheron role"),
    ("glint", Kind::Given, "Factions", "Glint role"),
    ("factions_on", Kind::Toggle, "Factions", "Faction system"),
];

fn field(key: &str) -> Option<(&'static str, Kind)> {
    FIELDS.iter().find(|f| f.0 == key).map(|f| (f.0, f.1))
}

/// The home server's id, once staff have chosen it.
pub async fn guild(app: &App) -> Option<String> {
    sqlx::query_scalar("SELECT guild_id FROM discord_home")
        .fetch_optional(&app.db)
        .await
        .ok()
        .flatten()
}
/// The home server's settings, keyed as in `FIELDS`.
pub async fn settings(app: &App) -> Map<String, Value> {
    sqlx::query_scalar::<_, Value>("SELECT settings FROM discord_home")
        .fetch_optional(&app.db)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}
/// A channel or role setting.
pub fn id(settings: &Map<String, Value>, key: &str) -> Option<String> {
    settings.get(key)?.as_str().map(String::from)
}
/// A switch; on unless staff turned it off.
pub fn on(settings: &Map<String, Value>, key: &str) -> bool {
    settings.get(key).and_then(Value::as_bool).unwrap_or(true)
}
/// A list of channels.
pub fn ids(settings: &Map<String, Value>, key: &str) -> Vec<String> {
    settings
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Remembers the name of every server the bot is in, for the staff page.
pub async fn guild_seen(app: &App, data: &Value) {
    let (Some(id), Some(name)) = (data["id"].as_str(), data["name"].as_str()) else {
        return;
    };
    let _ = sqlx::query("INSERT INTO discord_guilds(guild_id,name) VALUES($1,$2) ON CONFLICT(guild_id) DO UPDATE SET name=$2,seen_at=now()")
        .bind(id).bind(name).execute(&app.db).await;
}

/// What the bot needs on the home server, with Discord's permission bits.
const NEEDED: &[(u64, &str)] = &[
    (1 << 1, "Kick Members"),
    (1 << 2, "Ban Members"),
    (1 << 4, "Manage Channels"),
    (1 << 6, "Add Reactions"),
    (1 << 10, "View Channels"),
    (1 << 11, "Send Messages"),
    (1 << 13, "Manage Messages"),
    (1 << 14, "Embed Links"),
    (1 << 15, "Attach Files"),
    (1 << 16, "Read Message History"),
    (1 << 24, "Move Members"),
    (1 << 27, "Manage Nicknames"),
    (1 << 28, "Manage Roles"),
    (1 << 34, "Manage Threads"),
    (1 << 40, "Timeout Members"),
];
const ADMINISTRATOR: u64 = 1 << 3;
pub fn missing(permissions: u64) -> Vec<&'static str> {
    if permissions & ADMINISTRATOR != 0 {
        return vec![];
    }
    NEEDED
        .iter()
        .filter(|(bit, _)| permissions & bit == 0)
        .map(|(_, name)| *name)
        .collect()
}

/// The bot's health: the Gateway connection, and on the home server the permissions it lacks and
/// any role it must give that sits above its own.
pub async fn health(app: &App) -> Res<Value> {
    let gateway: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('connected',connected,'connected_at',connected_at,'last_event_at',last_event_at,'problem',problem,'bot_name',bot_name) FROM discord_gateway")
        .fetch_optional(&app.db).await?;
    let mut report = json!({"enabled": super::gateway::enabled(), "gateway": gateway});
    if let Some(guild) = guild(app).await {
        report["home"] = match lists(app, &guild).await {
            Ok(l) => {
                let settings = settings(app).await;
                let above: Vec<Value> = FIELDS
                    .iter()
                    .filter(|f| f.1 == Kind::Given)
                    .filter_map(|f| {
                        let role = id(&settings, f.0)?;
                        let r = l.roles.iter().find(|r| r["id"] == role.as_str())?;
                        (r["givable"] != true).then(|| json!(f.3))
                    })
                    .collect();
                json!({"guild_id": guild, "missing": missing(l.permissions), "roles_above_bot": above})
            }
            Err(e) => json!({"guild_id": guild, "error": e.message}),
        };
    }
    Ok(report)
}

async fn view(app: &App) -> Res<Value> {
    let home: Option<(String, Value)> =
        sqlx::query_as("SELECT guild_id,settings FROM discord_home")
            .fetch_optional(&app.db)
            .await?;
    let guilds: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',g.guild_id,'name',g.name) FROM discord_guilds g ORDER BY g.name")
        .fetch_all(&app.db).await?;
    let fields: Vec<Value> = FIELDS
        .iter()
        .map(|(key, kind, group, label)| json!({"key": key, "kind": kind.name(), "group": group, "label": label}))
        .collect();
    let mut out =
        json!({"health": health(app).await?, "guilds": guilds, "fields": fields, "home": null});
    if let Some((guild, settings)) = home {
        let (channels, roles) = match lists(app, &guild).await {
            Ok(Lists {
                channels, roles, ..
            }) => (channels, roles),
            Err(_) => (vec![], vec![]),
        };
        out["home"] =
            json!({"guild_id": guild, "settings": settings, "channels": channels, "roles": roles});
    }
    Ok(out)
}

/// GET /api/admin/discord: the bot's health, the servers it's in and the home server's settings.
async fn get_home(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    Ok(Json(view(&app).await?))
}

#[derive(Deserialize)]
pub struct Save {
    guild_id: String,
    #[serde(default)]
    settings: Map<String, Value>,
}
/// PUT /api/admin/discord: choose the home server and its settings. Every channel and role must be
/// one of that server's own, of the right kind; roles the bot gives must sit below its own role.
async fn save(State(app): State<App>, jar: CookieJar, Json(input): Json<Save>) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_guilds WHERE guild_id=$1)")
            .bind(&input.guild_id)
            .fetch_one(&app.db)
            .await?;
    if !known {
        return Err(Fail::field("guild_id", "Pick a server the bot is in."));
    }
    let l = lists(&app, &input.guild_id).await?;
    let mut clean = Map::new();
    for (key, value) in input.settings {
        let (name, kind) =
            field(&key).ok_or_else(|| Fail::field("settings", "Unknown setting."))?;
        if value.is_null() {
            continue;
        }
        let channel_ok = |id: &Value| {
            l.channels.iter().any(|c| {
                c["id"] == *id
                    && c["type"]
                        .as_i64()
                        .is_some_and(|t| kind.channel_types().contains(&t))
            })
        };
        let ok = match kind {
            Kind::Toggle => value.is_boolean(),
            Kind::Zone => value
                .as_str()
                .is_some_and(|z| z.parse::<chrono_tz::Tz>().is_ok()),
            Kind::Texts => value
                .as_array()
                .is_some_and(|a| a.len() <= 50 && a.iter().all(channel_ok)),
            Kind::Given | Kind::Role => l
                .roles
                .iter()
                .any(|r| r["id"] == value && (kind == Kind::Role || r["givable"] == true)),
            _ => channel_ok(&value),
        };
        if !ok {
            let why = match kind {
                Kind::Given => "Pick a role below the bot's own role (Server Settings → Roles).",
                Kind::Zone => "Pick a time zone.",
                Kind::Toggle => "Turn it on or off.",
                _ => "Pick one of the server's channels of that kind.",
            };
            return Err(Fail::field(name, why));
        }
        clean.insert(key, value);
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO discord_home(singleton,guild_id,settings,updated_by) VALUES(true,$1,$2,$3)
        ON CONFLICT(singleton) DO UPDATE SET guild_id=$1,settings=$2,updated_by=$3,updated_at=now()")
        .bind(&input.guild_id).bind(Value::Object(clean.clone())).bind(&staff.id).execute(&mut *tx).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "discord_home",
        "discord",
        &input.guild_id,
        &[],
        "",
        json!({"settings": clean}),
        false,
    )
    .await?;
    tx.commit().await?;
    // The home server's slash commands follow it.
    if !super::commands::register(&app).await {
        eprintln!("discord_event=commands outcome=retry");
    }
    Ok(Json(view(&app).await?))
}

pub fn routes() -> Router<App> {
    Router::new().route("/api/admin/discord", get(get_home).put(save))
}
