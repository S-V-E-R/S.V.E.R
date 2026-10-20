//! Factions on the home server (docs/DISCORD_BOT.md "Factions in Discord"): `/faction join` (for a
//! linked account the S.V.E.R faction wins), faction points per season with `/faction stats` and
//! `/faction leaderboard`, seasons that end on their own, holidays, `/territories` and the "has
//! taken control" posts, and Discord chat and voice as war influence.
use super::{
    card,
    commands::{ANSWERED, Interaction, MANAGE_GUILD, defer, embed, finish, private, respond},
    home, levels, quests, role, send, sync,
};
use crate::{App, profiles::Res};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub fn title(faction: &str) -> &'static str {
    match faction {
        "myria" => "Myria",
        "aetheron" => "Aetheron",
        "glint" => "Glint",
        _ => "No faction",
    }
}

/// True once every `minutes` for each job, the first call included. In memory: only the process
/// holding the Gateway lock runs the ticker.
pub(super) fn due(job: &'static str, minutes: u64) -> bool {
    static LAST: Mutex<Option<HashMap<&'static str, Instant>>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let last = last.get_or_insert_with(HashMap::new);
    // A few seconds' slack, so a minute tick that lands early still counts.
    let every = Duration::from_secs((minutes * 60).saturating_sub(5));
    let ready = last.get(job).is_none_or(|t| t.elapsed() >= every);
    if ready {
        last.insert(job, Instant::now());
    }
    ready
}

/// Where faction news goes: the faction channel, or the announcements channel.
fn news(settings: &Map<String, Value>) -> Option<String> {
    home::id(settings, "factions_channel").or_else(|| home::id(settings, "announcements"))
}

/// Discord chat or a voice minute as war influence: only for a linked account the war counts, in a
/// channel staff mapped to a genre (the war checks eligibility, verified email and membership).
pub async fn influence(app: &App, user: &str, channel: &str, event: &str) -> Res<()> {
    let settings = home::settings(app).await;
    let Some(genre) = settings
        .get("channel_genres")
        .and_then(|m| m.get(channel))
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    if !home::on(&settings, "factions_on") {
        return Ok(());
    }
    let Some(account) = sync::linked(app, user).await? else {
        return Ok(());
    };
    let mut tx = app.db.begin().await?;
    crate::factions::discord(app, &mut tx, &account.id, genre, event).await?;
    tx.commit().await?;
    Ok(())
}
/// A message moderation kept: its id is the event.
pub async fn message(app: &App, d: &Value) -> Res<()> {
    if d["author"]["bot"] == true || d["webhook_id"].is_string() || d["guild_id"].is_null() {
        return Ok(());
    }
    let (Some(user), Some(channel), Some(id)) = (
        d["author"]["id"].as_str(),
        d["channel_id"].as_str(),
        d["id"].as_str(),
    ) else {
        return Ok(());
    };
    influence(app, user, channel, id).await
}

/// Sets a member's Discord faction: the other faction roles off, the new one on, the rank role
/// swapped to the new faction's version, and "Sworn In". False when it was already theirs.
pub async fn set(app: &App, user: &str, to: &str) -> Res<bool> {
    let from = levels::faction(app, user).await?;
    if from.as_deref() == Some(to) {
        return Ok(false);
    }
    sqlx::query("INSERT INTO discord_members(discord_user,faction) VALUES($1,$2) ON CONFLICT(discord_user) DO UPDATE SET faction=$2")
        .bind(user).bind(to).execute(&app.db).await?;
    if let Some(guild) = home::guild(app).await {
        let settings = home::settings(app).await;
        for f in levels::FACTIONS {
            if let Some(r) = home::id(&settings, f) {
                role(app, &guild, user, &r, f == to).await;
            }
        }
    }
    levels::swap_variant(app, user, from.as_deref(), Some(to)).await?;
    quests::unlock(app, user, "faction_joined").await?;
    Ok(true)
}

pub fn definitions() -> Vec<Value> {
    let staff = MANAGE_GUILD.to_string();
    let faction = json!({"type": 3, "name": "faction", "description": "The faction", "required": true, "choices": [
        {"name": "Myria", "value": "myria"}, {"name": "Aetheron", "value": "aetheron"}, {"name": "Glint", "value": "glint"}]});
    vec![
        json!({"name": "faction", "description": "Join a faction, and see faction points.", "options": [
            {"type": 1, "name": "join", "description": "Join a faction", "options": [faction]},
            {"type": 1, "name": "stats", "description": "Faction points and members this season"},
            {"type": 1, "name": "leaderboard", "description": "Top contributors this season"}]}),
        json!({"name": "territories", "description": "Who holds each genre in the S.V.E.R war."}),
        json!({"name": "season", "description": "Faction-point seasons.", "default_member_permissions": staff, "options": [
            {"type": 1, "name": "start", "description": "Start a season", "options": [
                {"type": 3, "name": "name", "description": "Name", "required": true, "max_length": 60},
                {"type": 4, "name": "days", "description": "How long, in days", "required": true, "min_value": 1, "max_value": 365}]},
            {"type": 1, "name": "end", "description": "End the running season now"},
            {"type": 1, "name": "status", "description": "The running season"}]}),
        json!({"name": "holiday", "description": "Faction holidays.", "default_member_permissions": staff, "options": [
            {"type": 1, "name": "create", "description": "Add a holiday", "options": [
                {"type": 3, "name": "name", "description": "Name", "required": true, "max_length": 60},
                faction,
                {"type": 4, "name": "month", "description": "Month (1–12)", "required": true, "min_value": 1, "max_value": 12},
                {"type": 4, "name": "day", "description": "Day of the month", "required": true, "min_value": 1, "max_value": 31},
                {"type": 10, "name": "factor", "description": "XP multiplier for the day, e.g. 2", "required": true, "min_value": 0.1, "max_value": 10},
                {"type": 4, "name": "points", "description": "Faction points for every member", "required": true, "min_value": 0, "max_value": 10000}]},
            {"type": 1, "name": "list", "description": "Every holiday"},
            {"type": 1, "name": "remove", "description": "Remove a holiday", "options": [
                {"type": 4, "name": "id", "description": "Its number from /holiday list", "required": true}]}]}),
    ]
}

fn answer(result: Res<Value>) -> Value {
    result.unwrap_or_else(|_| private("That couldn't be done right now. Try again in a minute."))
}

/// Answers this module's commands; None when the command isn't one of them.
pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(match (i.name(), i.sub()) {
        // Up to five role calls: answered "thinking…" first, so a slow Discord never misses the
        // three-second window.
        ("faction", Some("join")) => {
            let (app, i) = (app.clone(), Interaction::parse(&i.raw)?);
            tokio::spawn(async move {
                respond(&app, &i, defer(true)).await;
                let mut reply = answer(join(&app, &i).await)["data"].take();
                // An edit can't carry the private flag; the deferral already set it.
                if let Some(d) = reply.as_object_mut() {
                    d.remove("flags");
                }
                finish(&app, &i, reply, None).await;
            });
            ANSWERED
        }
        ("faction", Some("stats")) => answer(stats(app).await),
        ("faction", Some("leaderboard")) => answer(leaderboard(app).await),
        ("territories", _) => answer(territories(app).await),
        ("season" | "holiday", _) if !i.staff() => private("Only staff can use that."),
        ("season", _) => answer(season(app, i).await),
        ("holiday", _) => answer(holiday(app, i).await),
        _ => return None,
    })
}

fn welcome(faction: &str) -> String {
    format!(
        "{} Welcome to **{}**!",
        levels::emoji(Some(faction)),
        title(faction)
    )
}

async fn join(app: &App, i: &Interaction) -> Res<Value> {
    let pick = i.option("faction").and_then(Value::as_str).unwrap_or("");
    choose(app, &i.user, pick).await
}
/// Joining a faction from `/faction join` or the role-select panel: a private answer, with a
/// confirmation when it's a switch.
pub(super) async fn choose(app: &App, user: &str, pick: &str) -> Res<Value> {
    if !home::on(&home::settings(app).await, "factions_on") {
        return Ok(private("Factions are turned off on this server."));
    }
    if !levels::FACTIONS.contains(&pick) {
        return Ok(private("Pick Myria, Aetheron or Glint."));
    }
    // A linked account's faction comes from S.V.E.R; Discord can't move it.
    if let Some(sver) = sync::linked(app, user).await?.and_then(|a| a.faction) {
        set(app, user, &sver).await?;
        sync::soon(app, user);
        return Ok(private(format!(
            "Your faction comes from your S.V.E.R account: {} **{}**. Faction changes happen on S.V.E.R.",
            levels::emoji(Some(&sver)),
            title(&sver)
        )));
    }
    Ok(match levels::faction(app, user).await? {
        Some(f) if f == pick => private(format!("You're already in **{}**.", title(pick))),
        Some(f) => json!({"type": 4, "data": {"flags": 64, "allowed_mentions": {"parse": []},
            "content": format!("Switch from **{}** to **{}**? Your rank role moves to {}'s version.", title(&f), title(pick), title(pick)),
            "components": [{"type": 1, "components": [
                {"type": 2, "style": 3, "label": "Switch", "custom_id": format!("fac:join:{pick}")},
                {"type": 2, "style": 2, "label": "Cancel", "custom_id": "fac:cancel"}]}]}}),
        None => {
            set(app, user, pick).await?;
            private(welcome(pick))
        }
    })
}

/// The switch confirmation's buttons.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let update = |content: String| json!({"type": 7, "data": {"content": content, "components": [], "allowed_mentions": {"parse": []}}});
    let id = i.custom_id();
    if id == "fac:cancel" {
        return Some(update("Nothing changed.".into()));
    }
    let pick = id
        .strip_prefix("fac:join:")
        .filter(|f| levels::FACTIONS.contains(f))?;
    let (app, i, pick) = (app.clone(), Interaction::parse(&i.raw)?, pick.to_string());
    // The role calls happen after a deferred update, as for /faction join.
    tokio::spawn(async move {
        respond(&app, &i, json!({"type": 6})).await;
        let done: Res<String> = async {
            // Linked in the meantime: S.V.E.R still wins.
            if let Some(sver) = sync::linked(&app, &i.user).await?.and_then(|a| a.faction) {
                set(&app, &i.user, &sver).await?;
                return Ok(format!(
                    "Your faction comes from your S.V.E.R account: **{}**.",
                    title(&sver)
                ));
            }
            set(&app, &i.user, &pick).await?;
            Ok(welcome(&pick))
        }
        .await;
        let content = done
            .unwrap_or_else(|_| "That couldn't be done right now. Try again in a minute.".into());
        finish(&app, &i, update(content)["data"].take(), None).await;
    });
    Some(ANSWERED)
}

/// A member rejoined the home server: Discord took their roles when they left, so their faction
/// and rank roles come back (the sync restores the rest).
pub async fn rejoined(app: &App, user: &str) -> Res<()> {
    let Some(guild) = home::guild(app).await else {
        return Ok(());
    };
    let faction = levels::faction(app, user).await?;
    let settings = home::settings(app).await;
    if let Some(r) = faction.as_deref().and_then(|f| home::id(&settings, f)) {
        role(app, &guild, user, &r, true).await;
    }
    let held: Option<i16> = sqlx::query_scalar("SELECT rank FROM discord_xp WHERE discord_user=$1")
        .bind(user)
        .fetch_optional(&app.db)
        .await?;
    if let Some(r) = levels::ladder(app)
        .await?
        .iter()
        .find(|r| Some(r.rank) == held)
        .and_then(|r| r.role(faction.as_deref()))
    {
        role(app, &guild, user, &r, true).await;
    }
    Ok(())
}

/// The running season (number, name, start, end), if any.
async fn running(app: &App) -> Res<Option<(i32, String, DateTime<Utc>, DateTime<Utc>)>> {
    Ok(sqlx::query_as(
        "SELECT number,name,starts_at,ends_at FROM discord_seasons WHERE ended_at IS NULL",
    )
    .fetch_optional(&app.db)
    .await?)
}

async fn stats(app: &App) -> Res<Value> {
    let season = running(app).await?;
    let since = season.as_ref().map(|s| s.2);
    let rows: Vec<(String, i64, i64)> = sqlx::query_as("SELECT f,(SELECT coalesce(sum(points),0) FROM discord_points WHERE faction=f AND at>=coalesce($1,'-infinity'))::bigint,
        (SELECT count(*) FROM discord_members WHERE faction=f) FROM unnest($2::text[]) f")
        .bind(since).bind(&levels::FACTIONS[..]).fetch_all(&app.db).await?;
    let fields: Vec<Value> = rows
        .iter()
        .map(|(f, points, members)| {
            json!({"name": format!("{} {}", levels::emoji(Some(f)), title(f)), "inline": true,
                "value": format!("{} points\n{} members", card::thousands(*points), card::thousands(*members))})
        })
        .collect();
    let heading = match &season {
        Some((number, name, _, ends)) => {
            format!("Season {number}: {name} · ends <t:{}:R>", ends.timestamp())
        }
        None => "All time (no season is running)".into(),
    };
    Ok(embed(
        json!({"title": "Faction standings", "description": heading, "fields": fields, "color": 0x8b_9bb4}),
        false,
    ))
}

async fn leaderboard(app: &App) -> Res<Value> {
    let season = running(app).await?;
    let rows: Vec<(String, Option<String>, i64)> = sqlx::query_as("SELECT p.discord_user,m.faction,sum(p.points)::bigint FROM discord_points p LEFT JOIN discord_members m ON m.discord_user=p.discord_user
        WHERE p.at>=coalesce($1,'-infinity') GROUP BY 1,2 HAVING sum(p.points)>0 ORDER BY 3 DESC,1 LIMIT 10")
        .bind(season.as_ref().map(|s| s.2)).fetch_all(&app.db).await?;
    if rows.is_empty() {
        return Ok(private("No faction points yet."));
    }
    let lines: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(n, (user, faction, points))| {
            format!(
                "**#{}** {} <@{user}> · {} points",
                n + 1,
                levels::emoji(faction.as_deref()),
                card::thousands(*points)
            )
        })
        .collect();
    let heading = season.map_or("Top contributors · all time".into(), |s| {
        format!("Top contributors · Season {}: {}", s.0, s.1)
    });
    Ok(embed(
        json!({"title": heading, "description": lines.join("\n"), "color": 0x8b_9bb4}),
        false,
    ))
}

async fn territories(app: &App) -> Res<Value> {
    let map = crate::factions::territories(&mut *app.db.acquire().await?).await?;
    let Some((_, held)) = map.filter(|m| !m.1.is_empty()) else {
        return Ok(private("The war hasn't started yet."));
    };
    let lines: Vec<String> = held
        .iter()
        .map(|(_, genre, holder)| {
            format!(
                "{} **{genre}** · {}",
                levels::emoji(holder.as_deref()),
                holder.as_deref().map_or("Unclaimed", title)
            )
        })
        .collect();
    Ok(embed(
        json!({"title": "Territories", "description": lines.join("\n"), "color": 0x8b_9bb4}),
        false,
    ))
}

/// The faction with the most points, unless the top is tied or nobody scored.
pub fn winner(totals: &[(String, i64)]) -> Option<String> {
    let top = totals.iter().map(|t| t.1).max().filter(|t| *t > 0)?;
    let mut leaders = totals.iter().filter(|t| t.1 == top);
    let first = leaders.next()?;
    leaders.next().is_none().then(|| first.0.clone())
}

/// Ends the running season (only once it's due, when `due_only`): the faction with the most points
/// in its window wins a 1.5× faction multiplier for 14 days, and the end is announced.
pub async fn end(app: &App, due_only: bool) -> Res<Option<(i32, String, Option<String>)>> {
    let mut tx = app.db.begin().await?;
    let Some((number, name, starts, ends)): Option<(i32, String, DateTime<Utc>, DateTime<Utc>)> =
        sqlx::query_as("SELECT number,name,starts_at,ends_at FROM discord_seasons WHERE ended_at IS NULL AND (NOT $1 OR ends_at<=now()) FOR UPDATE")
            .bind(due_only).fetch_optional(&mut *tx).await?
    else {
        return Ok(None);
    };
    let totals: Vec<(String, i64)> = sqlx::query_as("SELECT faction,sum(points)::bigint FROM discord_points WHERE at>=$1 AND at<least($2,now()) GROUP BY faction")
        .bind(starts).bind(ends).fetch_all(&mut *tx).await?;
    let won = winner(&totals);
    sqlx::query("UPDATE discord_seasons SET ended_at=now(),winner=$2 WHERE number=$1")
        .bind(number)
        .bind(&won)
        .execute(&mut *tx)
        .await?;
    if let Some(f) = &won {
        sqlx::query("INSERT INTO discord_multipliers(name,factor,kind,target,ends_at) VALUES($1,1.5,'faction',$2,now()+interval '14 days')")
            .bind(format!("{name} champions")).bind(f).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    if let Some(channel) = news(&home::settings(app).await) {
        let points = |f: &str| totals.iter().find(|t| t.0 == f).map_or(0, |t| t.1);
        let line = match &won {
            Some(f) => format!(
                "🏆 **{name}** (Season {number}) has ended. {} **{}** wins with {} points, and earns ×1.5 XP for 14 days!",
                levels::emoji(Some(f)),
                title(f),
                card::thousands(points(f))
            ),
            None => format!("🏆 **{name}** (Season {number}) has ended with no single winner."),
        };
        send(
            app,
            &channel,
            json!({"content": line, "allowed_mentions": {"parse": []}}),
        )
        .await;
    }
    Ok(Some((number, name, won)))
}

async fn season(app: &App, i: &Interaction) -> Res<Value> {
    match i.sub() {
        Some("start") => {
            if let Some((number, name, ..)) = running(app).await? {
                return Ok(private(format!(
                    "Season {number} ({name}) is still running. `/season end` ends it."
                )));
            }
            let name = i
                .option("name")
                .and_then(Value::as_str)
                .unwrap_or("Season")
                .trim();
            let days = i
                .option("days")
                .and_then(Value::as_i64)
                .unwrap_or(30)
                .clamp(1, 365);
            let number: i32 = sqlx::query_scalar("INSERT INTO discord_seasons(number,name,ends_at) SELECT coalesce(max(number),0)+1,$1,now()+make_interval(days=>$2) FROM discord_seasons RETURNING number")
                .bind(name).bind(days as i32).fetch_one(&app.db).await?;
            Ok(private(format!(
                "Season {number}, **{name}**, has started and runs {days} days."
            )))
        }
        Some("end") => Ok(private(match end(app, false).await? {
            Some((number, name, Some(f))) => {
                format!("Season {number} ({name}) has ended. {} wins.", title(&f))
            }
            Some((number, name, None)) => {
                format!("Season {number} ({name}) has ended with no single winner.")
            }
            None => "No season is running.".into(),
        })),
        _ => stats(app).await,
    }
}

async fn holiday(app: &App, i: &Interaction) -> Res<Value> {
    match i.sub() {
        Some("create") => {
            let text = |k: &str| i.option(k).and_then(Value::as_str).unwrap_or_default();
            let number = |k: &str| i.option(k).and_then(Value::as_i64).unwrap_or(0);
            let (name, faction) = (text("name").trim(), text("faction"));
            let (month, day) = (number("month"), number("day"));
            // Any year's date, February 29 included (it then comes in leap years).
            if !levels::FACTIONS.contains(&faction)
                || name.is_empty()
                || chrono::NaiveDate::from_ymd_opt(2024, month as u32, day as u32).is_none()
            {
                return Ok(private("Pick a faction, a name and a real date."));
            }
            let factor = i.option("factor").and_then(Value::as_f64).unwrap_or(1.0);
            let id: Option<i64> = sqlx::query_scalar("INSERT INTO discord_holidays(faction,name,month,day,factor,points) VALUES($1,$2,$3,$4,round($5::numeric,2),$6) ON CONFLICT(name) DO NOTHING RETURNING id")
                .bind(faction).bind(name).bind(month as i16).bind(day as i16).bind(factor).bind(number("points").clamp(0, 10_000) as i32)
                .fetch_optional(&app.db).await?;
            Ok(private(match id {
                Some(id) => format!("Holiday #{id}, **{name}**, is set for {month}/{day} (UTC)."),
                None => "A holiday with that name already exists.".into(),
            }))
        }
        Some("remove") => {
            let id = i.option("id").and_then(Value::as_i64).unwrap_or(0);
            let gone = sqlx::query("DELETE FROM discord_holidays WHERE id=$1")
                .bind(id)
                .execute(&app.db)
                .await?
                .rows_affected();
            Ok(private(if gone > 0 {
                format!("Holiday #{id} is removed.")
            } else {
                format!("There's no holiday #{id}.")
            }))
        }
        _ => {
            type Row = (i64, String, String, i16, i16, f64, i32, bool);
            let rows: Vec<Row> = sqlx::query_as("SELECT id,faction,name,month,day,factor::float8,points,active FROM discord_holidays ORDER BY month,day,id")
                .fetch_all(&app.db).await?;
            if rows.is_empty() {
                return Ok(private("No holidays are set."));
            }
            let lines: Vec<String> = rows
                .iter()
                .map(|(id, faction, name, month, day, factor, points, active)| {
                    format!(
                        "**#{id}** {} {name} · {month}/{day} · ×{factor} · {points} points{}",
                        levels::emoji(Some(faction)),
                        if *active { "" } else { " · off" }
                    )
                })
                .collect();
            Ok(embed(
                json!({"title": "Faction holidays", "description": lines.join("\n"), "color": 0x8b_9bb4}),
                true,
            ))
        }
    }
}

/// Each minute: a season that reached its end is closed and announced.
pub async fn seasons(app: &App) -> Res<()> {
    end(app, true).await.map(drop)
}

/// Each minute: a holiday falling today (UTC) runs once: its faction's multiplier until the next
/// UTC midnight, its points for every member of the faction, and a post.
pub async fn holidays(app: &App) -> Res<()> {
    let settings = home::settings(app).await;
    if home::guild(app).await.is_none() || !home::on(&settings, "factions_on") {
        return Ok(());
    }
    // One statement, so a holiday is marked run only with its multiplier and points.
    let fired: Vec<(String, String, f64, i32)> = sqlx::query_as("WITH today AS (SELECT (now() AT TIME ZONE 'UTC')::date AS d),
        fired AS (UPDATE discord_holidays h SET last_run=today.d FROM today WHERE h.active AND h.month=extract(month FROM today.d) AND h.day=extract(day FROM today.d)
            AND h.last_run IS DISTINCT FROM today.d RETURNING h.faction,h.name,h.factor,h.points,today.d),
        m AS (INSERT INTO discord_multipliers(name,factor,kind,target,ends_at) SELECT name,factor,'faction',faction,(d+1)::timestamp AT TIME ZONE 'UTC' FROM fired),
        p AS (INSERT INTO discord_points(discord_user,faction,kind,points) SELECT x.discord_user,f.faction,'holiday',f.points FROM fired f JOIN discord_members x ON x.faction=f.faction WHERE f.points>0)
        SELECT faction,name,factor::float8,points FROM fired")
        .fetch_all(&app.db).await?;
    let Some(channel) = news(&settings) else {
        return Ok(());
    };
    for (faction, name, factor, points) in fired {
        let mut line = format!(
            "{} **{name}**: a {} holiday! ×{factor} XP for {} today",
            levels::emoji(Some(&faction)),
            title(&faction),
            title(&faction)
        );
        if points > 0 {
            line.push_str(&format!(", and {points} faction points for every member"));
        }
        line.push('.');
        send(
            app,
            &channel,
            json!({"content": line, "allowed_mentions": {"parse": []}}),
        )
        .await;
    }
    Ok(())
}

/// Every five minutes: "has taken control" for each genre that changed hands in the S.V.E.R war
/// since the last look. The first look, a new genre and a new season only record.
pub async fn territory_posts(app: &App) -> Res<()> {
    let Some((season, held)) = crate::factions::territories(&mut *app.db.acquire().await?).await?
    else {
        return Ok(());
    };
    let seen: Vec<(String, i64, Option<String>)> =
        sqlx::query_as("SELECT genre,season_id,holder FROM discord_territory_seen")
            .fetch_all(&app.db)
            .await?;
    let settings = home::settings(app).await;
    let channel = news(&settings).filter(|_| home::on(&settings, "factions_on"));
    for (genre, name, holder) in held {
        let before = seen.iter().find(|s| s.0 == genre);
        if before.is_some_and(|b| b.1 == season && b.2 == holder) {
            continue;
        }
        let taken = before.is_some_and(|b| b.1 == season);
        if let (true, Some(f), Some(channel)) = (taken, &holder, &channel) {
            let line = format!(
                "{} **{}** has taken control of **{name}**!",
                levels::emoji(Some(f)),
                title(f)
            );
            let message = json!({"content": line, "allowed_mentions": {"parse": []}});
            // Not recorded unless posted, so the next look tries again.
            if send(app, channel, message).await.is_none() {
                continue;
            }
        }
        sqlx::query("INSERT INTO discord_territory_seen(genre,season_id,holder) VALUES($1,$2,$3) ON CONFLICT(genre) DO UPDATE SET season_id=$2,holder=$3")
            .bind(&genre).bind(season).bind(&holder).execute(&app.db).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_season_needs_one_clear_winner() {
        let t = |v: &[(&str, i64)]| {
            v.iter()
                .map(|(f, p)| (f.to_string(), *p))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            winner(&t(&[("myria", 5), ("glint", 9)])),
            Some("glint".into())
        );
        assert_eq!(winner(&t(&[("myria", 9), ("glint", 9)])), None);
        assert_eq!(winner(&t(&[("myria", 0)])), None);
        assert_eq!(winner(&[]), None);
    }
}
