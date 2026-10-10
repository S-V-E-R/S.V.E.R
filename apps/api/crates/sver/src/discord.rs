//! The S.V.E.R Discord bot (docs/COMMUNITY.md "Discord bot"): a streamer adds it to their Discord
//! server, it posts when they go live and keeps Discord roles in step with S.V.E.R: subscribers to
//! the channel, each faction, and members of the streamer's guild. REST only, with the bot token;
//! no gateway connection. `DISCORD_BOT_TOKEN` (plus the Discord sign-in app's client ID and secret)
//! turns it on.
//!
//! Roles are the streamer's own, picked in Studio. The bot only removes a role it gave (recorded in
//! `discord_grants`), so roles a server's moderators hand out by hand are never touched.
use crate::{
    App,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    response::Redirect,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// View Channels, Send Messages, Embed Links and Manage Roles: nothing more.
const PERMISSIONS: u64 = 1024 | 2048 | 16384 | 268_435_456;
/// A full role sweep per server this often (plus right after Studio saves).
const SWEEP_MINUTES: i32 = 15;
const FACTIONS: [&str; 3] = ["myria", "aetheron", "glint"];

fn api() -> String {
    std::env::var("DISCORD_API").unwrap_or_else(|_| "https://discord.com/api/v10".into())
}
fn bot_token() -> Option<String> {
    std::env::var("DISCORD_BOT_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
}
fn client(app: &App) -> Option<&crate::oauth::Provider> {
    app.config.providers.iter().find(|p| p.name == "discord")
}
fn available(app: &App) -> bool {
    bot_token().is_some() && client(app).is_some()
}
fn callback(app: &App) -> String {
    format!("{}/api/me/discord/callback", app.config.origin)
}

/// One Discord API call: the status and JSON body, or None when Discord couldn't be reached.
async fn call(app: &App, method: Method, path: &str, body: Option<Value>) -> Option<(u16, Value)> {
    let mut request = app
        .http
        .request(method, format!("{}{path}", api()))
        .header("Authorization", format!("Bot {}", bot_token()?));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.ok()?;
    let status = response.status().as_u16();
    Some((status, response.json().await.unwrap_or(Value::Null)))
}

/// What a failed call means for the streamer, shown in Studio; None for a passing problem.
fn problem((status, body): &(u16, Value)) -> Option<&'static str> {
    match (status, body["code"].as_i64()) {
        (403, Some(50013)) => Some(
            "The bot can't give one of the roles. In Discord, open Server Settings → Roles and drag the S.V.E.R role above every role you picked.",
        ),
        (403, Some(50001)) | (404, Some(10004)) => {
            Some("The bot isn't in your Discord server anymore. Add it again.")
        }
        (404, Some(10003)) => Some("The go-live channel was deleted in Discord. Pick another one."),
        (404, Some(10011)) => Some("One of the roles was deleted in Discord. Pick it again."),
        _ => None,
    }
}
async fn set_problem(app: &App, channel: &str, problem: Option<&str>) -> Res<()> {
    sqlx::query("UPDATE discord_servers SET problem=$2 WHERE channel_id=$1")
        .bind(channel)
        .bind(problem)
        .execute(&app.db)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Server {
    guild_id: String,
    guild_name: String,
    post_channel: Option<String>,
    sub_role: Option<String>,
    guild_role: Option<String>,
    faction_roles: Value,
    problem: Option<String>,
    synced_at: Option<chrono::DateTime<chrono::Utc>>,
}
async fn server(app: &App, channel: &str) -> Res<Option<Server>> {
    Ok(sqlx::query_as("SELECT guild_id,guild_name,post_channel,sub_role,guild_role,faction_roles,problem,synced_at FROM discord_servers WHERE channel_id=$1")
        .bind(channel).fetch_optional(&app.db).await?)
}

/// The server's text channels and the roles the bot may hand out, read live from Discord.
async fn choices(app: &App, guild: &str) -> Res<(Vec<Value>, Vec<Value>)> {
    let fail = || Fail::unavailable("Discord didn't answer. Try again in a minute.");
    let channels = call(app, Method::GET, &format!("/guilds/{guild}/channels"), None)
        .await
        .ok_or_else(fail)?;
    let roles = call(app, Method::GET, &format!("/guilds/{guild}/roles"), None)
        .await
        .ok_or_else(fail)?;
    let me = call(app, Method::GET, "/users/@me", None)
        .await
        .ok_or_else(fail)?;
    let bot = match me.1["id"].as_str() {
        Some(id) => call(
            app,
            Method::GET,
            &format!("/guilds/{guild}/members/{id}"),
            None,
        )
        .await
        .ok_or_else(fail)?,
        None => (0, Value::Null),
    };
    if let Some(why) = [&channels, &roles, &bot].into_iter().find_map(problem) {
        return Err(Fail::conflict(why));
    }
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let position = |id: &str| {
        list(&roles.1)
            .iter()
            .find(|r| r["id"] == id)
            .and_then(|r| r["position"].as_i64())
            .unwrap_or(0)
    };
    // The bot can give a role only below its own highest role.
    let top = list(&bot.1["roles"])
        .iter()
        .filter_map(|r| r.as_str().map(position))
        .max()
        .unwrap_or(0);
    let text = list(&channels.1)
        .into_iter()
        .filter(|c| matches!(c["type"].as_i64(), Some(0 | 5)))
        .map(|c| json!({"id": c["id"], "name": c["name"]}))
        .collect();
    let givable = list(&roles.1)
        .into_iter()
        .filter(|r| r["id"] != guild && r["managed"] != true)
        .map(|r| json!({"id": r["id"], "name": r["name"], "givable": r["position"].as_i64().unwrap_or(0) < top}))
        .collect();
    Ok((text, givable))
}

/// GET /api/me/discord: whether the bot is available, and the linked server with its choices.
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let Some(s) = server(&app, &me.id).await? else {
        return Ok(Json(json!({"available": available(&app), "server": null})));
    };
    let (channels, roles, error) = match choices(&app, &s.guild_id).await {
        Ok((c, r)) => (c, r, None),
        Err(e) => (vec![], vec![], Some(e.message)),
    };
    Ok(Json(json!({"available": available(&app), "server": {
        "guild_name": s.guild_name, "post_channel": s.post_channel, "sub_role": s.sub_role,
        "guild_role": s.guild_role, "faction_roles": s.faction_roles,
        "problem": error.or(s.problem.map(Into::into)), "synced_at": s.synced_at,
        "channels": channels, "roles": roles,
    }})))
}

/// POST /api/me/discord/install: Discord's "add to server" page for this channel.
async fn install(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let Some(p) = client(&app).filter(|_| available(&app)) else {
        return Err(Fail::unavailable("The Discord bot isn't available yet."));
    };
    // The state names this account and expires in 10 minutes; sealed, so it can't be forged.
    let expires = chrono::Utc::now().timestamp() + 600;
    let state = sec::seal(&app, "discord-install", &format!("{}:{expires}", me.id))?;
    let mut url = url::Url::parse("https://discord.com/oauth2/authorize")
        .map_err(|_| Fail::bad("Invalid URL."))?;
    url.query_pairs_mut()
        .append_pair("client_id", &p.client_id)
        .append_pair("scope", "bot")
        .append_pair("permissions", &PERMISSIONS.to_string())
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &callback(&app))
        .append_pair("integration_type", "0")
        .append_pair("state", &state);
    Ok(Json(json!({"url": url.as_str()})))
}

#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
}
/// GET /api/me/discord/callback: Discord sends the streamer back after adding the bot. The code
/// exchange proves which server it joined.
async fn callback_route(
    State(app): State<App>,
    jar: CookieJar,
    Query(input): Query<Callback>,
) -> Redirect {
    let target = |outcome: &str| Redirect::to(&format!("/studio/discord?{outcome}"));
    match link(&app, &jar, input).await {
        Ok(()) => target("linked=1"),
        Err(e) => target(&format!(
            "error={}",
            url::form_urlencoded::byte_serialize(e.message.as_bytes()).collect::<String>()
        )),
    }
}
async fn link(app: &App, jar: &CookieJar, input: Callback) -> Res<()> {
    let me = profiles::signed_in(app, jar).await?;
    let refused = || Fail::bad("Adding the bot didn't finish. Try again from Studio.");
    let (Some(code), Some(state)) = (input.code, input.state) else {
        return Err(refused());
    };
    let opened = sec::unseal(app, "discord-install", &state).map_err(|_| refused())?;
    let (who, expires) = opened.split_once(':').ok_or_else(refused)?;
    if who != me.id || expires.parse::<i64>().unwrap_or(0) < chrono::Utc::now().timestamp() {
        return Err(refused());
    }
    let p = client(app).ok_or_else(refused)?;
    let response = app
        .http
        .post(format!("{}/oauth2/token", api()))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", &callback(app)),
            ("client_id", &p.client_id),
            ("client_secret", &p.client_secret),
        ])
        .send()
        .await
        .map_err(|_| refused())?;
    let body: Value = response.json().await.map_err(|_| refused())?;
    let (Some(guild), name) = (body["guild"]["id"].as_str(), body["guild"]["name"].as_str()) else {
        return Err(refused());
    };
    if !guild.chars().all(|c| c.is_ascii_digit()) {
        return Err(refused());
    }
    let mut tx = app.db.begin().await?;
    // A different server replaces the old link; the old server's grants no longer apply.
    sqlx::query("DELETE FROM discord_grants WHERE channel_id=$1 AND guild_id<>$2")
        .bind(&me.id)
        .bind(guild)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO discord_servers(channel_id,guild_id,guild_name) VALUES($1,$2,$3) ON CONFLICT(channel_id) DO UPDATE SET guild_name=EXCLUDED.guild_name,problem=NULL,synced_at=NULL,
        post_channel=CASE WHEN discord_servers.guild_id=EXCLUDED.guild_id THEN discord_servers.post_channel END,
        sub_role=CASE WHEN discord_servers.guild_id=EXCLUDED.guild_id THEN discord_servers.sub_role END,
        guild_role=CASE WHEN discord_servers.guild_id=EXCLUDED.guild_id THEN discord_servers.guild_role END,
        faction_roles=CASE WHEN discord_servers.guild_id=EXCLUDED.guild_id THEN discord_servers.faction_roles ELSE '{}' END,
        guild_id=EXCLUDED.guild_id")
        .bind(&me.id).bind(guild).bind(name.unwrap_or("")).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct Settings {
    post_channel: Option<String>,
    sub_role: Option<String>,
    guild_role: Option<String>,
    #[serde(default)]
    faction_roles: HashMap<String, String>,
}
/// PUT /api/me/discord: where go-live posts go and which roles to keep in sync. Every ID must be
/// one of the server's own channels or roles the bot can give.
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let s = server(&app, &me.id).await?.ok_or_else(Fail::missing)?;
    let (channels, roles) = choices(&app, &s.guild_id).await?;
    let has = |list: &[Value], id: &str, givable: bool| {
        list.iter()
            .any(|x| x["id"] == id && (!givable || x["givable"] == true))
    };
    if input
        .post_channel
        .as_deref()
        .is_some_and(|c| !has(&channels, c, false))
    {
        return Err(Fail::field(
            "post_channel",
            "Pick one of the server's text channels.",
        ));
    }
    if input
        .faction_roles
        .keys()
        .any(|f| !FACTIONS.contains(&f.as_str()))
    {
        return Err(Fail::field("faction_roles", "Unknown faction."));
    }
    let picked = [&input.sub_role, &input.guild_role]
        .into_iter()
        .flatten()
        .chain(input.faction_roles.values());
    for role in picked {
        if !has(&roles, role, true) {
            return Err(Fail::field(
                "roles",
                "Pick roles below the S.V.E.R role (Server Settings → Roles in Discord).",
            ));
        }
    }
    sqlx::query("UPDATE discord_servers SET post_channel=$2,sub_role=$3,guild_role=$4,faction_roles=$5,problem=NULL,synced_at=NULL WHERE channel_id=$1")
        .bind(&me.id).bind(&input.post_channel).bind(&input.sub_role).bind(&input.guild_role)
        .bind(json!(input.faction_roles)).execute(&app.db).await?;
    mine(State(app), jar).await
}

/// POST /api/me/discord/test: a sample go-live post, to check the channel and permissions.
async fn test(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    profiles::rate(&app, format!("discord-test:{}", me.id), 5, 600).await?;
    let s = server(&app, &me.id).await?.ok_or_else(Fail::missing)?;
    let to = s
        .post_channel
        .ok_or_else(|| Fail::field("post_channel", "Pick a go-live channel first."))?;
    let content = format!(
        "This is a test. When you go live, a post like this appears here:\n**{}** is live on S.V.E.R\n{}/{}",
        me.username, app.config.origin, me.username
    );
    let result = send_post(&app, &to, &content)
        .await
        .ok_or_else(|| Fail::unavailable("Discord didn't answer. Try again in a minute."))?;
    if result.0 >= 300 {
        return Err(Fail::conflict(problem(&result).unwrap_or(
            "Discord refused the post. Check the bot can send messages in that channel.",
        )));
    }
    Ok(Json(json!({"posted": true})))
}

/// DELETE /api/me/discord: takes back the roles the bot gave, leaves the server and forgets it.
async fn unlink(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let s = server(&app, &me.id).await?.ok_or_else(Fail::missing)?;
    let grants: Vec<(String, String)> =
        sqlx::query_as("SELECT discord_user,role_id FROM discord_grants WHERE channel_id=$1")
            .bind(&me.id)
            .fetch_all(&app.db)
            .await?;
    // ponytail: one request per grant, best effort; a big server's cleanup would want batching.
    for (user, role) in grants {
        call(
            &app,
            Method::DELETE,
            &format!("/guilds/{}/members/{user}/roles/{role}", s.guild_id),
            None,
        )
        .await;
    }
    call(
        &app,
        Method::DELETE,
        &format!("/users/@me/guilds/{}", s.guild_id),
        None,
    )
    .await;
    sqlx::query("DELETE FROM discord_servers WHERE channel_id=$1")
        .bind(&me.id)
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM discord_grants WHERE channel_id=$1")
        .bind(&me.id)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"available": available(&app), "server": null})))
}

async fn send_post(app: &App, to: &str, content: &str) -> Option<(u16, Value)> {
    // No pings: a title can't mention @everyone or a role.
    call(
        app,
        Method::POST,
        &format!("/channels/{to}/messages"),
        Some(json!({"content": content, "allowed_mentions": {"parse": []}})),
    )
    .await
}

/// Every few seconds: due go-live posts (queued by `alerts::fan_out`), then servers due a sweep.
pub async fn tick(app: &App) -> Res<()> {
    if bot_token().is_none() {
        return Ok(());
    }
    deliver(app).await?;
    let due: Vec<String> = sqlx::query_scalar("SELECT channel_id FROM discord_servers WHERE problem IS NULL AND (synced_at IS NULL OR synced_at<now()-make_interval(mins=>$1)) ORDER BY synced_at NULLS FIRST LIMIT 5")
        .bind(SWEEP_MINUTES).fetch_all(&app.db).await?;
    for channel in due {
        sweep(app, &channel).await?;
    }
    Ok(())
}

type Due = (
    i64,
    String,
    i32,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    bool,
);
async fn deliver(app: &App) -> Res<()> {
    // A go-live post older than two hours, or for a stream that already ended, is dropped.
    sqlx::query("UPDATE discord_posts p SET available_at='infinity',error='The stream ended before the post went out.' FROM broadcasts b WHERE b.id=p.broadcast_id AND p.delivered_at IS NULL AND p.available_at<>'infinity' AND (b.state='ENDED' OR p.created_at<now()-interval '2 hours')")
        .execute(&app.db).await?;
    let due: Vec<Due> = sqlx::query_as("SELECT p.id,p.channel_id,p.attempts,s.post_channel,u.username,u.display_name,coalesce(st.title,''),c.name,s.problem IS NOT NULL
        FROM discord_posts p JOIN discord_servers s ON s.channel_id=p.channel_id JOIN channel_users u ON u.id=p.channel_id
        LEFT JOIN stream_settings st ON st.owner_id=p.channel_id LEFT JOIN stream_categories c ON c.id=st.category_id
        WHERE p.delivered_at IS NULL AND p.available_at<=now() ORDER BY p.id LIMIT 10")
        .fetch_all(&app.db).await?;
    for (id, channel, attempts, to, username, display, title, category, broken) in due {
        let Some(to) = to.filter(|_| !broken) else {
            give_up(
                app,
                id,
                "No go-live channel is set, or the bot needs attention in Studio.",
            )
            .await?;
            continue;
        };
        let mut content = format!("**{display}** is live on S.V.E.R");
        if !title.is_empty() {
            content.push_str(&format!(": {title}"));
        }
        if let Some(category) = category {
            content.push_str(&format!(" ({category})"));
        }
        content.push_str(&format!("\n{}/{username}", app.config.origin));
        match send_post(app, &to, &content).await {
            Some((status, _)) if status < 300 => {
                sqlx::query("UPDATE discord_posts SET delivered_at=now(),attempts=attempts+1,error=NULL WHERE id=$1")
                    .bind(id).execute(&app.db).await?;
            }
            Some(result) if problem(&result).is_some() => {
                set_problem(app, &channel, problem(&result)).await?;
                give_up(app, id, problem(&result).unwrap_or_default()).await?;
            }
            other => {
                let error = match other {
                    Some((status, _)) => format!("Discord answered {status}."),
                    None => "Discord didn't answer.".into(),
                };
                sqlx::query("UPDATE discord_posts SET attempts=$2,error=$3,available_at=CASE WHEN $2>=6 THEN 'infinity' ELSE now()+make_interval(secs=>10*power(2,$2-1)) END WHERE id=$1")
                    .bind(id).bind(attempts + 1).bind(error).execute(&app.db).await?;
            }
        }
    }
    Ok(())
}
async fn give_up(app: &App, id: i64, error: &str) -> Res<()> {
    sqlx::query("UPDATE discord_posts SET available_at='infinity',error=$2 WHERE id=$1")
        .bind(id)
        .bind(error)
        .execute(&app.db)
        .await?;
    Ok(())
}

/// Brings one server's mapped roles in line with S.V.E.R for everyone with a linked Discord
/// account: gives what they've earned, takes back what the bot gave and they no longer have.
async fn sweep(app: &App, channel: &str) -> Res<()> {
    sqlx::query("UPDATE discord_servers SET synced_at=now() WHERE channel_id=$1")
        .bind(channel)
        .execute(&app.db)
        .await?;
    let Some(s) = server(app, channel).await? else {
        return Ok(());
    };
    // Linked, eligible accounts and the roles each should have here.
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT i.subject,r.role FROM identities i JOIN channel_users c ON c.id=i.user_id AND c.eligible
        CROSS JOIN LATERAL (VALUES
            (CASE WHEN EXISTS(SELECT 1 FROM channel_subs x WHERE x.channel_id=$1 AND x.user_id=i.user_id AND x.paid_through>now()) THEN $2 END),
            ($3::jsonb->>(SELECT faction FROM faction_members WHERE user_id=i.user_id)),
            (CASE WHEN EXISTS(SELECT 1 FROM guild_members a JOIN guild_members b ON b.guild_id=a.guild_id WHERE a.user_id=$1 AND b.user_id=i.user_id) THEN $4 END)
        ) r(role) WHERE i.provider='discord' AND r.role IS NOT NULL")
        .bind(channel).bind(&s.sub_role).bind(&s.faction_roles).bind(&s.guild_role)
        .fetch_all(&app.db).await?;
    let wanted: HashSet<(String, String)> = rows.into_iter().collect();
    let given: HashSet<(String, String)> =
        sqlx::query_as("SELECT discord_user,role_id FROM discord_grants WHERE channel_id=$1")
            .bind(channel)
            .fetch_all(&app.db)
            .await?
            .into_iter()
            .collect();
    // ponytail: one request per change, plus one per entitled person not in the server each sweep;
    // fine for small communities, list members (needs the members intent) past a few thousand.
    for (user, role) in wanted.difference(&given) {
        let path = format!("/guilds/{}/members/{user}/roles/{role}", s.guild_id);
        match call(app, Method::PUT, &path, None).await {
            Some((status, _)) if status < 300 => {
                sqlx::query("INSERT INTO discord_grants(channel_id,discord_user,role_id,guild_id) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                    .bind(channel).bind(user).bind(role).bind(&s.guild_id).execute(&app.db).await?;
            }
            // Not in this server (yet): nothing to do.
            Some((404, body)) if body["code"] == 10007 => {}
            Some(result) => {
                if let Some(why) = problem(&result) {
                    return set_problem(app, channel, Some(why)).await;
                }
                // Rate limited or a passing error: the next sweep picks it up.
                return Ok(());
            }
            None => return Ok(()),
        }
    }
    for (user, role) in given.difference(&wanted) {
        let path = format!("/guilds/{}/members/{user}/roles/{role}", s.guild_id);
        match call(app, Method::DELETE, &path, None).await {
            // Gone from the server or the role deleted: the grant is over either way.
            Some((status, body))
                if status < 300
                    || (status == 404 && matches!(body["code"].as_i64(), Some(10007 | 10011))) =>
            {
                sqlx::query("DELETE FROM discord_grants WHERE channel_id=$1 AND discord_user=$2 AND role_id=$3")
                    .bind(channel).bind(user).bind(role).execute(&app.db).await?;
            }
            Some(result) => {
                if let Some(why) = problem(&result) {
                    return set_problem(app, channel, Some(why)).await;
                }
                return Ok(());
            }
            None => return Ok(()),
        }
    }
    Ok(())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/discord", get(mine).put(save).delete(unlink))
        .route("/api/me/discord/install", post(install))
        .route("/api/me/discord/callback", get(callback_route))
        .route("/api/me/discord/test", post(test))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explains_discord_errors() {
        assert!(
            problem(&(403, json!({"code": 50013})))
                .unwrap()
                .contains("drag the S.V.E.R role")
        );
        assert!(
            problem(&(404, json!({"code": 10004})))
                .unwrap()
                .contains("isn't in your Discord server")
        );
        assert_eq!(problem(&(429, json!({"retry_after": 1.5}))), None);
        assert_eq!(problem(&(500, Value::Null)), None);
    }
}
