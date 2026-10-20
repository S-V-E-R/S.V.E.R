//! S.V.E.R sync on the home server (docs/DISCORD_BOT.md "Account link and sync"). A Discord account
//! is linked when its owner signs in with Discord or links it in settings (`identities`). Every 15
//! minutes, and right after `/link` and `/faction`, each linked member in good standing gets their
//! S.V.E.R faction, supporter, Founder, Streamer and level-band roles, an S.V.E.R level-up post and,
//! if they asked, their S.V.E.R username as nickname. Only roles the sync gave (in
//! `discord_home_grants`) are ever taken back. The owner can show their Discord rank on S.V.E.R.
use super::{
    call,
    commands::{Interaction, MANAGE_GUILD, embed, private},
    factions, home, levels, lists, quests, send,
};
use crate::{
    App,
    profiles::{self, Fail, Res},
};
use axum::{Json, Router, extract::State, routing::get};
use axum_extra::extract::cookie::CookieJar;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::PgConnection;

/// The S.V.E.R account linked to a Discord account, when it's in good standing.
#[derive(sqlx::FromRow)]
pub(super) struct Account {
    pub id: String,
    pub username: String,
    pub faction: Option<String>,
}
pub(super) async fn linked(app: &App, discord: &str) -> Res<Option<Account>> {
    Ok(sqlx::query_as("SELECT u.id,u.username,f.faction FROM identities i JOIN channel_users u ON u.id=i.user_id AND u.eligible AND u.deleted_at IS NULL
        LEFT JOIN faction_members f ON f.user_id=u.id WHERE i.provider='discord' AND i.subject=$1")
        .bind(discord).fetch_optional(&app.db).await?)
}

/// The role of the highest level band at or below `level`.
pub fn band(bands: &[(i32, String)], level: i64) -> Option<String> {
    bands
        .iter()
        .filter(|b| i64::from(b.0) <= level)
        .max_by_key(|b| b.0)
        .map(|b| b.1.clone())
}

#[derive(sqlx::FromRow)]
struct Linked {
    discord: String,
    id: String,
    username: String,
    faction: Option<String>,
    tier: Option<i16>,
    founder: bool,
    streamer: bool,
    nickname: bool,
    sver_level: Option<i32>,
}

/// Every linked member (or just `only`).
pub async fn all(app: &App) -> Res<()> {
    run(app, None).await
}
pub async fn one(app: &App, discord: &str) -> Res<()> {
    run(app, Some(discord)).await
}
/// Syncs one member in the background (a command's answer can't wait for it).
pub(super) fn soon(app: &App, discord: &str) {
    let (app, discord) = (app.clone(), discord.to_string());
    tokio::spawn(async move {
        if one(&app, &discord).await.is_err() {
            eprintln!("discord_event=sync outcome=retry");
        }
    });
}

async fn run(app: &App, only: Option<&str>) -> Res<()> {
    let Some(guild) = home::guild(app).await else {
        return Ok(());
    };
    let settings = home::settings(app).await;
    let bands: Vec<(i32, String)> =
        sqlx::query_as("SELECT level,role FROM discord_level_roles ORDER BY level")
            .fetch_all(&app.db)
            .await?;
    let members: Vec<Linked> = sqlx::query_as("SELECT i.subject AS discord,u.id,u.username,f.faction,
            (SELECT p.tier FROM platform_subscriptions p WHERE p.user_id=u.id AND p.paid_through>now()) AS tier,
            EXISTS(SELECT 1 FROM founders x WHERE x.user_id=u.id) AS founder,
            EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=u.id) AS streamer,
            coalesce(m.nickname,false) AS nickname,m.sver_level
        FROM identities i JOIN channel_users u ON u.id=i.user_id AND u.eligible AND u.deleted_at IS NULL
        LEFT JOIN faction_members f ON f.user_id=u.id LEFT JOIN discord_members m ON m.discord_user=i.subject
        WHERE i.provider='discord' AND ($1::text IS NULL OR i.subject=$1)")
        .bind(only).fetch_all(&app.db).await?;
    // ponytail: one member read (plus changes) per linked member each run; fine for a community
    // server, list members in pages past a few thousand links.
    for m in &members {
        // One member's failure never stops the others.
        if member(app, &guild, &settings, &bands, m).await.is_err() {
            eprintln!("discord_event=sync_member outcome=retry");
        }
    }
    // Unlinked (or no longer in good standing): take back what the sync gave.
    let gone: Vec<String> = sqlx::query_scalar("SELECT DISTINCT g.discord_user FROM discord_home_grants g WHERE ($1::text IS NULL OR g.discord_user=$1)
        AND NOT EXISTS(SELECT 1 FROM identities i JOIN channel_users u ON u.id=i.user_id AND u.eligible AND u.deleted_at IS NULL WHERE i.provider='discord' AND i.subject=g.discord_user)")
        .bind(only).fetch_all(&app.db).await?;
    for user in gone {
        reconcile(app, &guild, &user, &[], &[]).await?;
    }
    Ok(())
}

async fn member(
    app: &App,
    guild: &str,
    settings: &Map<String, Value>,
    bands: &[(i32, String)],
    m: &Linked,
) -> Res<()> {
    // Not in the server (10007), or Discord didn't answer: nothing to do until the next run.
    let path = format!("/guilds/{guild}/members/{}", m.discord);
    let Some((200, current)) = call(app, Method::GET, &path, None).await else {
        return Ok(());
    };
    if let Some(f) = &m.faction {
        factions::set(app, &m.discord, f).await?;
    }
    let level = crate::progression::level(
        crate::progression::total(&mut *app.db.acquire().await?, &m.id).await?,
    );
    let mut wanted: Vec<String> = [
        (m.tier == Some(1), "sver_wolfsworn"),
        (m.tier == Some(2), "sver_greycloak"),
        (m.founder, "sver_founder"),
        (m.streamer, "sver_streamer"),
    ]
    .into_iter()
    .filter(|w| w.0)
    .filter_map(|w| home::id(settings, w.1))
    .collect();
    wanted.extend(band(bands, level));
    let have: Vec<String> = current["roles"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|r| r.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    reconcile(app, guild, &m.discord, &wanted, &have).await?;
    if m.tier.is_some() {
        quests::unlock(app, &m.discord, "sver_subscriber").await?;
    }
    if m.nickname && current["nick"].as_str() != Some(m.username.as_str()) {
        // Discord refuses the server owner's nickname; nothing more to do about it.
        call(app, Method::PATCH, &path, Some(json!({"nick": m.username}))).await;
    }
    if m.sver_level != Some(level as i32) {
        // Only the sync that actually moves the level posts, when two overlap.
        let moved = sqlx::query("INSERT INTO discord_members(discord_user,sver_level) VALUES($1,$2) ON CONFLICT(discord_user) DO UPDATE SET sver_level=$2 WHERE discord_members.sver_level IS DISTINCT FROM $2")
            .bind(&m.discord).bind(level as i32).execute(&app.db).await?.rows_affected() == 1;
        if let (true, Some(before), Some(channel)) =
            (moved, m.sver_level, home::id(settings, "rank_up"))
            && level > i64::from(before)
        {
            let line = format!("<@{}> reached **Level {level}** on S.V.E.R!", m.discord);
            send(
                app,
                &channel,
                json!({"content": line, "allowed_mentions": {"users": [m.discord]}}),
            )
            .await;
        }
    }
    Ok(())
}

/// Gives the wanted roles the member lacks (recording each grant) and takes back the roles the
/// sync gave that they no longer earn. A role they already had is theirs, never recorded.
async fn reconcile(
    app: &App,
    guild: &str,
    user: &str,
    wanted: &[String],
    have: &[String],
) -> Res<()> {
    let given: Vec<String> =
        sqlx::query_scalar("SELECT role FROM discord_home_grants WHERE discord_user=$1")
            .bind(user)
            .fetch_all(&app.db)
            .await?;
    for role in wanted
        .iter()
        .filter(|r| !given.contains(r) && !have.contains(r))
    {
        if super::role(app, guild, user, role, true).await {
            sqlx::query(
                "INSERT INTO discord_home_grants(discord_user,role) VALUES($1,$2) ON CONFLICT DO NOTHING",
            )
            .bind(user)
            .bind(role)
            .execute(&app.db)
            .await?;
        }
    }
    for role in given.iter().filter(|r| !wanted.contains(r)) {
        let path = format!("/guilds/{guild}/members/{user}/roles/{role}");
        // Gone from the server or the role deleted: the grant is over either way.
        if let Some((status, body)) = call(app, Method::DELETE, &path, None).await
            && (status < 300
                || (status == 404 && matches!(body["code"].as_i64(), Some(10007 | 10011))))
        {
            sqlx::query("DELETE FROM discord_home_grants WHERE discord_user=$1 AND role=$2")
                .bind(user)
                .bind(role)
                .execute(&app.db)
                .await?;
        }
    }
    Ok(())
}

/// GUILD_MEMBER_ADD / GUILD_MEMBER_REMOVE on the home server. Leaving strips every role, so the
/// sync's grants end with it; rejoining brings back the faction and rank roles and syncs the rest.
pub async fn membership(app: &App, kind: &str, d: &Value) -> Res<()> {
    let Some(user) = d["user"]["id"].as_str() else {
        return Ok(());
    };
    if d["user"]["bot"] == true || d["guild_id"].as_str() != home::guild(app).await.as_deref() {
        return Ok(());
    }
    if kind == "GUILD_MEMBER_REMOVE" {
        sqlx::query("DELETE FROM discord_home_grants WHERE discord_user=$1")
            .bind(user)
            .execute(&app.db)
            .await?;
        return Ok(());
    }
    factions::rejoined(app, user).await?;
    soon(app, user);
    Ok(())
}

pub fn definitions() -> Vec<Value> {
    vec![
        json!({"name": "link", "description": "Your S.V.E.R account link.", "options": [
            {"type": 1, "name": "sver", "description": "How to link your S.V.E.R account"},
            {"type": 1, "name": "status", "description": "Whether this Discord account is linked"},
            {"type": 1, "name": "nickname", "description": "Use your S.V.E.R username as your nickname here", "options": [
                {"type": 3, "name": "state", "description": "On or off", "required": true, "choices": [
                    {"name": "On", "value": "on"}, {"name": "Off", "value": "off"}]}]}]}),
        json!({"name": "levelroles", "description": "Roles for S.V.E.R level bands.", "default_member_permissions": MANAGE_GUILD.to_string(), "options": [
            {"type": 1, "name": "add", "description": "Give a role from a S.V.E.R level up", "options": [
                {"type": 4, "name": "level", "description": "From this S.V.E.R level", "required": true, "min_value": 1, "max_value": 100},
                {"type": 8, "name": "role", "description": "The role", "required": true}]},
            {"type": 1, "name": "remove", "description": "Remove a level band", "options": [
                {"type": 4, "name": "level", "description": "The band's level", "required": true, "min_value": 1, "max_value": 100}]},
            {"type": 1, "name": "list", "description": "Every level band"}]}),
    ]
}

/// Answers this module's commands; None when the command isn't one of them.
pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    let reply = match i.name() {
        "link" => link(app, i).await,
        "levelroles" if !i.staff() => Ok(private("Only staff can use that.")),
        "levelroles" => levelroles(app, i).await,
        _ => return None,
    };
    Some(
        reply
            .unwrap_or_else(|_| private("That couldn't be done right now. Try again in a minute.")),
    )
}

async fn link(app: &App, i: &Interaction) -> Res<Value> {
    let account = linked(app, &i.user).await?;
    let status = match &account {
        Some(a) => format!(
            "This Discord account is linked to **{}** on S.V.E.R.",
            a.username
        ),
        None => "This Discord account isn't linked to S.V.E.R yet.".into(),
    };
    let reply = match i.sub() {
        Some("nickname") if account.is_none() => {
            private(format!("{status} `/link sver` shows how to link it."))
        }
        Some("nickname") => {
            let on = i.option("state").and_then(Value::as_str) == Some("on");
            sqlx::query("INSERT INTO discord_members(discord_user,nickname) VALUES($1,$2) ON CONFLICT(discord_user) DO UPDATE SET nickname=$2")
                .bind(&i.user).bind(on).execute(&app.db).await?;
            private(if on {
                "Your nickname here now follows your S.V.E.R username."
            } else {
                "The bot won't change your nickname anymore."
            })
        }
        Some("status") => match &account {
            Some(a) => {
                let total = crate::progression::total(&mut *app.db.acquire().await?, &a.id).await?;
                let nickname: bool = sqlx::query_scalar(
                    "SELECT coalesce((SELECT nickname FROM discord_members WHERE discord_user=$1),false)",
                )
                .bind(&i.user)
                .fetch_one(&app.db)
                .await?;
                private(format!(
                    "{status} Level {} · {} · nickname sync {}.",
                    crate::progression::level(total),
                    a.faction.as_deref().map_or("No faction", factions::title),
                    if nickname { "on" } else { "off" }
                ))
            }
            None => private(format!("{status} `/link sver` shows how to link it.")),
        },
        _ => private(format!(
            "Sign in to S.V.E.R with Discord, or link Discord on your account page, and your accounts link automatically: {}/account\n{status}",
            app.config.origin
        )),
    };
    if account.is_some() {
        soon(app, &i.user);
    }
    Ok(reply)
}

async fn levelroles(app: &App, i: &Interaction) -> Res<Value> {
    let level = i.option("level").and_then(Value::as_i64).unwrap_or(0);
    match i.sub() {
        Some("add") => {
            let role = i.option("role").and_then(Value::as_str).unwrap_or_default();
            let Some(guild) = home::guild(app).await else {
                return Ok(private("Pick the home server first."));
            };
            let givable = lists(app, &guild)
                .await?
                .roles
                .iter()
                .any(|r| r["id"] == role && r["givable"] == true);
            if !givable || !(1..=100).contains(&level) {
                return Ok(private(
                    "Pick a role below the bot's own role (Server Settings → Roles), and a level from 1 to 100.",
                ));
            }
            let saved = sqlx::query("INSERT INTO discord_level_roles(level,role) VALUES($1,$2) ON CONFLICT(level) DO UPDATE SET role=$2")
                .bind(level as i32).bind(role).execute(&app.db).await;
            Ok(private(match saved {
                Ok(_) => format!("From S.V.E.R level {level}, linked members get <@&{role}>."),
                Err(_) => "That role already belongs to another level band.".into(),
            }))
        }
        Some("remove") => {
            let gone = sqlx::query("DELETE FROM discord_level_roles WHERE level=$1")
                .bind(level as i32)
                .execute(&app.db)
                .await?
                .rows_affected();
            Ok(private(if gone > 0 {
                format!("The level {level} band is removed; the next sync takes its role back.")
            } else {
                format!("There's no level {level} band.")
            }))
        }
        _ => {
            let bands: Vec<(i32, String)> =
                sqlx::query_as("SELECT level,role FROM discord_level_roles ORDER BY level")
                    .fetch_all(&app.db)
                    .await?;
            if bands.is_empty() {
                return Ok(private(
                    "No level bands yet. `/levelroles add` creates one.",
                ));
            }
            let lines: Vec<String> = bands
                .iter()
                .map(|(level, role)| format!("Level {level}+ · <@&{role}>"))
                .collect();
            Ok(embed(
                json!({"title": "S.V.E.R level bands", "description": lines.join("\n"), "color": 0x8b_9bb4}),
                true,
            ))
        }
    }
}

/// A linked account's Discord rank: its name for their Discord faction, level and faction.
pub async fn rank(db: &mut PgConnection, user: &str) -> Res<Option<Value>> {
    let row: Option<(String, i64, Option<String>)> = sqlx::query_as("SELECT CASE m.faction WHEN 'myria' THEN r.myria WHEN 'aetheron' THEN r.aetheron WHEN 'glint' THEN r.glint ELSE r.base_name END,x.total,m.faction
        FROM identities i JOIN discord_xp x ON x.discord_user=i.subject AND x.total>0 LEFT JOIN discord_members m ON m.discord_user=i.subject
        JOIN LATERAL (SELECT * FROM discord_ranks WHERE xp<=x.total ORDER BY xp DESC LIMIT 1) r ON true
        WHERE i.provider='discord' AND i.user_id=$1")
        .bind(user).fetch_optional(db).await?;
    Ok(row.map(|(name, total, faction)| {
        json!({"name": name, "level": levels::level(total), "faction": faction})
    }))
}
/// For the profile payload: the rank, only when its owner chose to show it.
pub async fn shown_rank(db: &mut PgConnection, user: &str) -> Res<Value> {
    let shown: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_rank_shown WHERE user_id=$1)")
            .bind(user)
            .fetch_one(&mut *db)
            .await?;
    Ok(match shown {
        true => rank(db, user).await?.unwrap_or(Value::Null),
        false => Value::Null,
    })
}

async fn view(app: &App, user: &str) -> Res<Value> {
    let mut db = app.db.acquire().await?;
    let (linked, shown): (bool, bool) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM identities WHERE provider='discord' AND user_id=$1),EXISTS(SELECT 1 FROM discord_rank_shown WHERE user_id=$1)")
        .bind(user).fetch_one(&mut *db).await?;
    Ok(json!({"linked": linked, "shown": shown, "rank": rank(&mut db, user).await?}))
}
/// GET /api/me/discord-rank: whether Discord is linked, whether the rank shows, and the rank.
async fn get_rank(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    Ok(Json(view(&app, &me.id).await?))
}
#[derive(Deserialize)]
pub struct Shown {
    shown: bool,
}
/// PUT /api/me/discord-rank: show or hide the Discord rank on the profile (linked accounts only).
async fn put_rank(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Shown>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    if input.shown {
        let saved = sqlx::query("INSERT INTO discord_rank_shown(user_id) SELECT $1 WHERE EXISTS(SELECT 1 FROM identities WHERE provider='discord' AND user_id=$1) ON CONFLICT DO NOTHING")
            .bind(&me.id).execute(&app.db).await?;
        let view = view(&app, &me.id).await?;
        if saved.rows_affected() == 0 && view["linked"] != true {
            return Err(Fail::conflict("Link your Discord account first."));
        }
        return Ok(Json(view));
    }
    sqlx::query("DELETE FROM discord_rank_shown WHERE user_id=$1")
        .bind(&me.id)
        .execute(&app.db)
        .await?;
    Ok(Json(view(&app, &me.id).await?))
}

pub fn routes() -> Router<App> {
    Router::new().route("/api/me/discord-rank", get(get_rank).put(put_rank))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_highest_band_reached() {
        let bands = vec![(5, "a".to_string()), (10, "b".into()), (25, "c".into())];
        assert_eq!(band(&bands, 4), None);
        assert_eq!(band(&bands, 5), Some("a".into()));
        assert_eq!(band(&bands, 24), Some("b".into()));
        assert_eq!(band(&bands, 100), Some("c".into()));
    }
}
