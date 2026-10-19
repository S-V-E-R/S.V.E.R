//! Discord quests and achievements on the home server (docs/DISCORD_BOT.md "Quests and
//! achievements"): daily, weekly, monthly and one-time quests with XP, faction-point and role
//! rewards and completion milestones; the 16 achievements, posted to the achievements channel.
use super::{
    commands::{Interaction, MANAGE_GUILD, embed, private},
    home,
    levels::{self, Member, Source},
    role, send,
};
use crate::{App, profiles::Res};
use chrono::{Datelike, Utc};
use serde_json::{Map, Value, json};

/// Key, title, description, rarity, emoji.
pub const ACHIEVEMENTS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "sver_linked",
        "SVER Pioneer",
        "Linked your S.V.E.R account",
        "common",
        "🔗",
    ),
    (
        "sver_subscriber",
        "SVER Supporter",
        "Supports S.V.E.R",
        "rare",
        "⭐",
    ),
    (
        "sver_streamer",
        "On the Air",
        "Streams on S.V.E.R",
        "rare",
        "💎",
    ),
    (
        "first_stream",
        "Going Live",
        "Streamed for the first time",
        "common",
        "📡",
    ),
    (
        "streamer_10h",
        "Grinding",
        "Logged 10+ hours of streaming",
        "rare",
        "🎮",
    ),
    (
        "collab_posted",
        "Open for Business",
        "Posted a collab request",
        "common",
        "💼",
    ),
    (
        "birthday_set",
        "Known Birthday",
        "Set your birthday",
        "common",
        "🎂",
    ),
    (
        "faction_joined",
        "Sworn In",
        "Joined a faction",
        "common",
        "⚔️",
    ),
    (
        "hero_nominated",
        "Sees Greatness",
        "Nominated a community hero",
        "rare",
        "🦸",
    ),
    (
        "level_10",
        "Rising Star",
        "Reached level 10",
        "common",
        "🌟",
    ),
    ("level_25", "Veteran", "Reached level 25", "rare", "🏅"),
    ("level_50", "Elite", "Reached level 50", "epic", "🏆"),
    (
        "level_100",
        "Legend",
        "Reached level 100",
        "legendary",
        "👑",
    ),
    (
        "voice_10h",
        "Voice Regular",
        "Spent 10+ hours in voice channels",
        "common",
        "🎤",
    ),
    (
        "voice_50h",
        "Voice Veteran",
        "Spent 50+ hours in voice channels",
        "rare",
        "🔊",
    ),
    (
        "voice_100h",
        "Voice Legend",
        "Spent 100+ hours in voice channels",
        "epic",
        "📻",
    ),
];
fn rarity_color(rarity: &str) -> u32 {
    match rarity {
        "rare" => 0x34_98db,
        "epic" => 0x9b_59b6,
        "legendary" => 0xf3_9c12,
        _ => 0xaa_aaaa,
    }
}

/// Unlocks one achievement (from a later feature: collab posted, birthday set, faction joined…).
pub async fn unlock(app: &App, user: &str, key: &str) -> Res<()> {
    if ACHIEVEMENTS.iter().any(|a| a.0 == key) {
        sqlx::query("INSERT INTO discord_achievements(discord_user,key) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(user).bind(key).execute(&app.db).await?;
    }
    Ok(())
}

/// Unlocks every achievement a member's numbers have reached.
pub async fn check(app: &App, user: &str) -> Res<()> {
    let Some((total, voice, stream)): Option<(i64, i32, i32)> = sqlx::query_as(
        "SELECT total,voice_minutes,stream_minutes FROM discord_xp WHERE discord_user=$1",
    )
    .bind(user)
    .fetch_optional(&app.db)
    .await?
    else {
        return Ok(());
    };
    let level = levels::level(total);
    let linked: Option<(bool,)> = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=i.user_id) FROM identities i JOIN users u ON u.id=i.user_id AND u.deleted_at IS NULL WHERE i.provider='discord' AND i.subject=$1")
        .bind(user).fetch_optional(&app.db).await?;
    let earned = [
        ("level_10", level >= 10),
        ("level_25", level >= 25),
        ("level_50", level >= 50),
        ("level_100", level >= 100),
        ("voice_10h", voice >= 600),
        ("voice_50h", voice >= 3_000),
        ("voice_100h", voice >= 6_000),
        ("first_stream", stream >= 1),
        ("streamer_10h", stream >= 600),
        ("sver_linked", linked.is_some()),
        ("sver_streamer", linked.is_some_and(|l| l.0)),
    ];
    let keys: Vec<&str> = earned.iter().filter(|e| e.1).map(|e| e.0).collect();
    if !keys.is_empty() {
        sqlx::query("INSERT INTO discord_achievements(discord_user,key) SELECT $1,unnest($2::text[]) ON CONFLICT DO NOTHING")
            .bind(user).bind(&keys).execute(&app.db).await?;
    }
    Ok(())
}

/// Each minute: posts unlocked achievements to the achievements channel. Without a channel they
/// wait (the legacy bot marked them posted and dropped them).
pub async fn announce(app: &App) -> Res<()> {
    let Some(channel) = home::id(&home::settings(app).await, "achievements") else {
        return Ok(());
    };
    let due: Vec<(String, String)> = sqlx::query_as("SELECT discord_user,key FROM discord_achievements WHERE announced_at IS NULL ORDER BY unlocked_at LIMIT 10")
        .fetch_all(&app.db).await?;
    for (user, key) in due {
        let Some((_, title, description, rarity, emoji)) = ACHIEVEMENTS.iter().find(|a| a.0 == key)
        else {
            continue;
        };
        let message = json!({"content": format!("<@{user}>"), "allowed_mentions": {"users": [user]},
            "embeds": [{"title": format!("{emoji} {title}"), "description": description, "color": rarity_color(rarity),
                "footer": {"text": format!("{} achievement", rarity[..1].to_uppercase() + &rarity[1..])}}]});
        if send(app, &channel, message).await.is_none() {
            // The channel is gone or closed to the bot; try again next minute.
            return Ok(());
        }
        sqlx::query(
            "UPDATE discord_achievements SET announced_at=now() WHERE discord_user=$1 AND key=$2",
        )
        .bind(&user)
        .bind(&key)
        .execute(&app.db)
        .await?;
    }
    Ok(())
}

/// The progress key for a quest's period: the UTC day, its week's Monday, its month, or once.
pub fn period_key(period: &str) -> String {
    let today = Utc::now().date_naive();
    match period {
        "daily" => today.to_string(),
        "weekly" => {
            let monday =
                today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
            format!("week-{monday}")
        }
        "monthly" => format!("{}-{:02}", today.year(), today.month()),
        _ => "once".into(),
    }
}
/// Every requirement met.
fn met(needs: &Map<String, Value>, progress: &Map<String, Value>) -> bool {
    needs.iter().all(|(k, target)| {
        progress.get(k).and_then(Value::as_i64).unwrap_or(0) >= target.as_i64().unwrap_or(i64::MAX)
    })
}

#[derive(sqlx::FromRow)]
struct Quest {
    id: i64,
    name: String,
    period: String,
    needs: Value,
    xp: i32,
    points: i32,
    role: Option<String>,
    max_completions: Option<i32>,
}

/// Counts something a member did (`chat_messages`, `voice_minutes`, `event_rsvp`, `giveaway_enter`,
/// `stream_watch`) toward every active quest that needs it, completing the ones now met.
pub async fn bump(app: &App, user: &str, kind: &str, n: i64) -> Res<()> {
    let faction = levels::faction(app, user).await?;
    let quests: Vec<Quest> = sqlx::query_as("SELECT id,name,period,needs,xp,points,role,max_completions FROM discord_quests
        WHERE active AND (ends_at IS NULL OR ends_at>now()) AND needs ? $1 AND (faction IS NULL OR faction=$2)")
        .bind(kind).bind(&faction).fetch_all(&app.db).await?;
    for q in quests {
        let key = period_key(&q.period);
        let mut tx = app.db.begin().await?;
        let progress: Value = sqlx::query_scalar("INSERT INTO discord_quest_progress(quest_id,discord_user,period_key,progress) VALUES($1,$2,$3,jsonb_build_object($4::text,$5::bigint))
            ON CONFLICT(quest_id,discord_user,period_key) DO UPDATE SET progress=discord_quest_progress.progress||jsonb_build_object($4::text,coalesce((discord_quest_progress.progress->>$4)::bigint,0)+$5)
            WHERE discord_quest_progress.completed_at IS NULL RETURNING progress")
            .bind(q.id).bind(user).bind(&key).bind(kind).bind(n).fetch_optional(&mut *tx).await?.unwrap_or(Value::Null);
        let (Some(needs), Some(have)) = (q.needs.as_object(), progress.as_object()) else {
            tx.commit().await?;
            continue;
        };
        if !met(needs, have) {
            tx.commit().await?;
            continue;
        }
        // A completion limit counts every period's completions of this quest.
        let done: i64 = sqlx::query_scalar("SELECT count(*) FROM discord_quest_progress WHERE quest_id=$1 AND discord_user=$2 AND completed_at IS NOT NULL")
            .bind(q.id).bind(user).fetch_one(&mut *tx).await?;
        if q.max_completions.is_some_and(|m| done >= m as i64) {
            tx.commit().await?;
            continue;
        }
        let claimed = sqlx::query("UPDATE discord_quest_progress SET completed_at=now() WHERE quest_id=$1 AND discord_user=$2 AND period_key=$3 AND completed_at IS NULL")
            .bind(q.id).bind(user).bind(&key).execute(&mut *tx).await?.rows_affected();
        if let (1, Some(f), true) = (claimed, &faction, q.points > 0) {
            sqlx::query("INSERT INTO discord_points(discord_user,faction,kind,points) VALUES($1,$2,'quest',$3)")
                .bind(user).bind(f).bind(q.points).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        if claimed == 1 {
            complete(app, user, &q).await?;
        }
    }
    Ok(())
}

async fn complete(app: &App, user: &str, q: &Quest) -> Res<()> {
    let m = || Member {
        user,
        roles: &[],
        channel: None,
    };
    levels::award(app, m(), q.xp as i64, Source::Quest).await?;
    if let (Some(r), Some(guild)) = (&q.role, home::guild(app).await) {
        role(app, &guild, user, r, true).await;
    }
    // Milestones at 5, 10, 25, 50 and 100 completions: 10 XP per completion, and a post.
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM discord_quest_progress WHERE discord_user=$1 AND completed_at IS NOT NULL")
        .bind(user).fetch_one(&app.db).await?;
    if [5, 10, 25, 50, 100].contains(&total) {
        let first = sqlx::query("INSERT INTO discord_quest_milestones(discord_user,completions) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(user).bind(total as i32).execute(&app.db).await?.rows_affected() == 1;
        if first {
            levels::award(app, m(), 10 * total, Source::Milestone).await?;
            if let Some(channel) = home::id(&home::settings(app).await, "factions_channel") {
                let line = format!("🏆 <@{user}> has completed **{total} quests**!");
                send(
                    app,
                    &channel,
                    json!({"content": line, "allowed_mentions": {"users": [user]}}),
                )
                .await;
            }
        }
    }
    let note = format!(
        "Quest complete: **{}**{}.",
        q.name,
        if q.xp > 0 {
            format!(" (+{} XP)", q.xp)
        } else {
            String::new()
        }
    );
    super::dm(app, user, json!({"content": note})).await;
    Ok(())
}

const KINDS: &[(&str, &str)] = &[
    ("chat_messages", "Messages"),
    ("voice_minutes", "Minutes in voice"),
    ("event_rsvp", "Event RSVPs"),
    ("giveaway_enter", "Giveaway entries"),
    ("stream_watch", "Minutes watching a stream"),
];

pub fn definitions() -> Vec<Value> {
    let kinds: Vec<Value> = KINDS
        .iter()
        .map(|(v, n)| json!({"name": n, "value": v}))
        .collect();
    vec![
        json!({"name": "achievements", "description": "A member's achievements.", "options": [
            {"type": 6, "name": "user", "description": "Whose achievements", "required": false}]}),
        json!({"name": "quests", "description": "Quests: rewards for taking part.", "options": [
            {"type": 1, "name": "list", "description": "The quests running now"},
            {"type": 1, "name": "progress", "description": "Your progress this period"},
            {"type": 1, "name": "board", "description": "Most quests completed this month"}]}),
        json!({"name": "questadmin", "description": "Manage quests.", "default_member_permissions": MANAGE_GUILD.to_string(), "options": [
            {"type": 1, "name": "create", "description": "Add a quest", "options": [
                {"type": 3, "name": "name", "description": "Name", "required": true, "max_length": 60},
                {"type": 3, "name": "period", "description": "How often it resets", "required": true, "choices": [
                    {"name": "Daily", "value": "daily"}, {"name": "Weekly", "value": "weekly"},
                    {"name": "Monthly", "value": "monthly"}, {"name": "Once", "value": "once"}]},
                {"type": 3, "name": "needs", "description": "What it counts", "required": true, "choices": kinds},
                {"type": 4, "name": "target", "description": "How many", "required": true, "min_value": 1, "max_value": 100000},
                {"type": 4, "name": "xp", "description": "XP reward", "required": false, "min_value": 0, "max_value": 10000},
                {"type": 4, "name": "points", "description": "Faction points", "required": false, "min_value": 0, "max_value": 10000},
                {"type": 8, "name": "role", "description": "Role reward", "required": false},
                {"type": 3, "name": "faction", "description": "Only for one faction", "required": false, "choices": [
                    {"name": "Myria", "value": "myria"}, {"name": "Aetheron", "value": "aetheron"}, {"name": "Glint", "value": "glint"}]},
                {"type": 4, "name": "limit", "description": "Most completions per member", "required": false, "min_value": 1},
                {"type": 3, "name": "description", "description": "Description", "required": false, "max_length": 200}]},
            {"type": 1, "name": "remove", "description": "Stop a quest", "options": [
                {"type": 4, "name": "id", "description": "Its number from /quests list", "required": true}]},
            {"type": 1, "name": "stats", "description": "Completions per quest"}]}),
    ]
}

pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(match i.name() {
        "achievements" => achievements(app, i).await,
        "quests" => quests(app, i).await,
        "questadmin" if !i.staff() => private("Only staff can use that."),
        "questadmin" => admin(app, i).await,
        _ => return None,
    })
}

async fn achievements(app: &App, i: &Interaction) -> Value {
    let user = i.user_or_me("user");
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM discord_achievements WHERE discord_user=$1 ORDER BY unlocked_at",
    )
    .bind(&user)
    .fetch_all(&app.db)
    .await
    .unwrap_or_default();
    let (name, _) = i.profile(&user);
    let lines: Vec<String> = ACHIEVEMENTS
        .iter()
        .map(|(key, title, description, _, emoji)| {
            if keys.iter().any(|k| k == key) {
                format!("{emoji} **{title}** · {description}")
            } else {
                format!("🔒 {title}")
            }
        })
        .collect();
    embed(
        json!({"title": format!("{name}'s achievements ({}/{})", keys.len(), ACHIEVEMENTS.len()),
            "description": lines.join("\n"), "color": 0x8b_9bb4}),
        false,
    )
}

fn label(kind: &str) -> &str {
    KINDS.iter().find(|k| k.0 == kind).map_or(kind, |k| k.1)
}

async fn quests(app: &App, i: &Interaction) -> Value {
    let all: Vec<(i64, String, String, String, Value, i32, i32)> = sqlx::query_as("SELECT id,name,description,period,needs,xp,points FROM discord_quests WHERE active AND (ends_at IS NULL OR ends_at>now()) ORDER BY id")
        .fetch_all(&app.db).await.unwrap_or_default();
    match i.sub() {
        Some("progress") => {
            let mut lines = Vec::new();
            for (id, name, _, period, needs, ..) in &all {
                let progress: Option<(Value, bool)> = sqlx::query_as("SELECT progress,completed_at IS NOT NULL FROM discord_quest_progress WHERE quest_id=$1 AND discord_user=$2 AND period_key=$3")
                    .bind(id).bind(&i.user).bind(period_key(period)).fetch_optional(&app.db).await.ok().flatten();
                let (have, done) = progress.unwrap_or((json!({}), false));
                let parts: Vec<String> = needs
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, t)| format!("{} {}/{}", label(k), have[k].as_i64().unwrap_or(0), t))
                    .collect();
                lines.push(format!(
                    "{} **{name}** · {}",
                    if done { "✅" } else { "▫️" },
                    parts.join(", ")
                ));
            }
            if lines.is_empty() {
                return private("No quests are running.");
            }
            embed(
                json!({"title": "Your quests", "description": lines.join("\n"), "color": 0x8b_9bb4}),
                true,
            )
        }
        Some("board") => {
            let rows: Vec<(String, i64)> = sqlx::query_as("SELECT discord_user,count(*) FROM discord_quest_progress WHERE completed_at>=date_trunc('month',now()) GROUP BY 1 ORDER BY 2 DESC,1 LIMIT 10")
                .fetch_all(&app.db).await.unwrap_or_default();
            if rows.is_empty() {
                return private("No quests completed this month yet.");
            }
            let lines: Vec<String> = rows
                .iter()
                .enumerate()
                .map(|(n, (user, count))| format!("**#{}** <@{user}> · {count}", n + 1))
                .collect();
            embed(
                json!({"title": "Quest board · this month", "description": lines.join("\n"), "color": 0x8b_9bb4}),
                false,
            )
        }
        _ => {
            if all.is_empty() {
                return private("No quests are running.");
            }
            let lines: Vec<String> = all
                .iter()
                .map(|(id, name, description, period, needs, xp, points)| {
                    let what: Vec<String> = needs
                        .as_object()
                        .into_iter()
                        .flatten()
                        .map(|(k, t)| format!("{t} {}", label(k).to_lowercase()))
                        .collect();
                    let mut reward = vec![];
                    if *xp > 0 {
                        reward.push(format!("{xp} XP"));
                    }
                    if *points > 0 {
                        reward.push(format!("{points} faction points"));
                    }
                    format!(
                        "**#{id} {name}** ({period}) · {}{}{}",
                        what.join(" and "),
                        if reward.is_empty() {
                            String::new()
                        } else {
                            format!(" → {}", reward.join(", "))
                        },
                        if description.is_empty() {
                            String::new()
                        } else {
                            format!("\n{description}")
                        }
                    )
                })
                .collect();
            embed(
                json!({"title": "Quests", "description": lines.join("\n"), "color": 0x8b_9bb4}),
                false,
            )
        }
    }
}

async fn admin(app: &App, i: &Interaction) -> Value {
    match i.sub() {
        Some("create") => {
            let s = |k: &str| i.option(k).and_then(Value::as_str).map(str::trim);
            let n = |k: &str| i.option(k).and_then(Value::as_i64);
            let (Some(name), Some(period), Some(kind), Some(target)) =
                (s("name"), s("period"), s("needs"), n("target"))
            else {
                return private("Name, period, what it counts and how many are needed.");
            };
            let needs = json!({kind: target});
            let saved: Result<i64, _> = sqlx::query_scalar("INSERT INTO discord_quests(name,description,period,needs,xp,points,role,faction,max_completions) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id")
                .bind(name).bind(s("description").unwrap_or("")).bind(period).bind(needs)
                .bind(n("xp").unwrap_or(0) as i32).bind(n("points").unwrap_or(0) as i32)
                .bind(s("role")).bind(s("faction")).bind(n("limit").map(|l| l as i32))
                .fetch_one(&app.db).await;
            match saved {
                Ok(id) => private(format!("Quest #{id} **{name}** is live.")),
                Err(_) => private("That quest couldn't be saved."),
            }
        }
        Some("remove") => {
            let id = i.option("id").and_then(Value::as_i64).unwrap_or(0);
            let stopped =
                sqlx::query("UPDATE discord_quests SET active=false WHERE id=$1 AND active")
                    .bind(id)
                    .execute(&app.db)
                    .await
                    .map(|r| r.rows_affected())
                    .unwrap_or(0);
            private(if stopped > 0 {
                format!("Quest #{id} has stopped.")
            } else {
                format!("Quest #{id} isn't running.")
            })
        }
        _ => {
            let rows: Vec<(i64, String, bool, i64)> = sqlx::query_as("SELECT q.id,q.name,q.active,count(p.completed_at) FROM discord_quests q LEFT JOIN discord_quest_progress p ON p.quest_id=q.id GROUP BY q.id ORDER BY q.id")
                .fetch_all(&app.db).await.unwrap_or_default();
            let lines: Vec<String> = rows
                .iter()
                .map(|(id, name, active, done)| {
                    format!(
                        "**#{id} {name}**{} · {done} completions",
                        if *active { "" } else { " (stopped)" }
                    )
                })
                .collect();
            embed(
                json!({"title": "Quest stats", "description": if lines.is_empty() { "No quests yet.".to_string() } else { lines.join("\n") }, "color": 0x8b_9bb4}),
                true,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quests_need_every_requirement() {
        let needs = json!({"chat_messages": 10, "voice_minutes": 5});
        let needs = needs.as_object().unwrap();
        assert!(!met(
            needs,
            json!({"chat_messages": 10}).as_object().unwrap()
        ));
        assert!(met(
            needs,
            json!({"chat_messages": 12, "voice_minutes": 5})
                .as_object()
                .unwrap()
        ));
        assert_eq!(period_key("once"), "once");
        assert!(period_key("weekly").starts_with("week-"));
        assert_eq!(period_key("monthly").len(), 7);
    }
}
