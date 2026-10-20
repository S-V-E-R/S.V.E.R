//! Collab requests and LFG on the home server (docs/DISCORD_BOT.md "Go-live alerts and streamers"):
//! `/collab post|browse|myrequest|history` with an "I'm interested" button that tells the poster,
//! and `/lfg` posts in the LFG forum (or a thread in the LFG channel) with a join button.
use super::{
    call,
    commands::{ANSWERED, Interaction, embed, private},
    dm, home, quests, send,
    streamers::{PLATFORMS, answer, later, platform_name},
};
use crate::{App, profiles::Res};
use reqwest::Method;
use serde_json::{Value, json};

const PAGE: i64 = 3;

#[derive(sqlx::FromRow)]
struct Request {
    id: i64,
    discord_user: String,
    platform: String,
    game: Option<String>,
    schedule: Option<String>,
    description: String,
    max_responses: i16,
    status: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    responses: i64,
}
const SELECT: &str = "SELECT c.id,c.discord_user,c.platform,c.game,c.schedule,c.description,c.max_responses,c.status,c.expires_at,(SELECT count(*) FROM discord_collab_responses r WHERE r.collab_id=c.id) responses FROM discord_collabs c";

fn card(r: &Request) -> Value {
    let mut text = format!(
        "**Posted by:** <@{}>\n**Platform:** {}\n",
        r.discord_user,
        platform_name(&r.platform)
    );
    if let Some(g) = &r.game {
        text.push_str(&format!("**Game or content:** {g}\n"));
    }
    if let Some(s) = &r.schedule {
        text.push_str(&format!("**When:** {s}\n"));
    }
    text.push_str(&format!(
        "\n{}\n\n**Responses:** {}/{} • Ends <t:{}:R>",
        r.description,
        r.responses,
        r.max_responses,
        r.expires_at.timestamp()
    ));
    json!({"title": format!("💼 Collab request #{}", r.id), "description": text, "color": 0x58_65f2,
        "footer": {"text": format!("S.V.E.R bot • Collab board • {}", r.status)}})
}
fn respond_button(r: &Request) -> Value {
    let full = r.responses >= r.max_responses as i64 || r.status != "open";
    json!({"type": 2, "style": if full { 2 } else { 1 }, "disabled": full, "custom_id": format!("collab:respond:{}", r.id),
        "label": if full { "Full".to_string() } else { format!("I'm interested ({})", r.responses) }})
}

/// Each minute: requests past their week close.
pub async fn expire(app: &App) -> Res<()> {
    sqlx::query(
        "UPDATE discord_collabs SET status='expired' WHERE status='open' AND expires_at<now()",
    )
    .execute(&app.db)
    .await?;
    Ok(())
}

async fn browse(app: &App, page: i64, platform: Option<&str>) -> Res<Value> {
    expire(app).await?;
    // A platform filter also shows requests open to any platform.
    let filter =
        " WHERE c.status='open' AND ($1::text IS NULL OR c.platform=$1 OR c.platform='any')";
    let total: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM discord_collabs c{filter}"
    )))
    .bind(platform)
    .fetch_one(&app.db)
    .await?;
    let pages = ((total + PAGE - 1) / PAGE).max(1);
    let page = page.clamp(0, pages - 1);
    let rows: Vec<Request> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT}{filter} ORDER BY c.created_at DESC LIMIT {PAGE} OFFSET $2"
    )))
    .bind(platform)
    .bind(page * PAGE)
    .fetch_all(&app.db)
    .await?;
    if rows.is_empty() {
        return Ok(
            json!({"embeds": [{"title": "No open collab requests", "description": "`/collab post` starts one.", "color": 0x8b_9bb4}], "components": []}),
        );
    }
    let mut components: Vec<Value> = rows
        .iter()
        .map(|r| json!({"type": 1, "components": [respond_button(r)]}))
        .collect();
    let tag = platform.unwrap_or("all");
    components.push(json!({"type": 1, "components": [
        {"type": 2, "style": 2, "label": "◀ Previous", "custom_id": format!("collab:browse:{}:{tag}", page - 1), "disabled": page == 0},
        {"type": 2, "style": 2, "label": format!("Page {}/{pages}", page + 1), "custom_id": "collab:page", "disabled": true},
        {"type": 2, "style": 2, "label": "Next ▶", "custom_id": format!("collab:browse:{}:{tag}", page + 1), "disabled": page + 1 >= pages}]}));
    let embeds: Vec<Value> = rows.iter().map(card).collect();
    Ok(json!({"embeds": embeds, "components": components, "allowed_mentions": {"parse": []}}))
}

async fn collab(app: &App, i: &Interaction) -> Res<Value> {
    match i.sub() {
        Some("post") => {
            let platform = i
                .option("platform")
                .and_then(Value::as_str)
                .unwrap_or("any");
            let open: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_collabs WHERE discord_user=$1 AND status='open' AND expires_at>now())")
                .bind(&i.user).fetch_one(&app.db).await?;
            if open {
                return Ok(private(
                    "You already have an open collab request. `/collab myrequest` shows it.",
                ));
            }
            let input = |id: &str, label: &str, style: u8, required: bool, max: u16| {
                json!({"type": 1, "components": [
                {"type": 4, "custom_id": id, "label": label, "style": style, "required": required, "max_length": max}]})
            };
            Ok(
                json!({"type": 9, "data": {"custom_id": format!("collab:modal:{platform}"), "title": "Post a collab request",
                "components": [
                    input("game", "Game or content (optional)", 1, false, 100),
                    input("schedule", "When you're free (optional)", 1, false, 100),
                    input("description", "What are you looking for?", 2, true, 500)]}}),
            )
        }
        Some("browse") => {
            let platform = i.option("platform").and_then(Value::as_str);
            Ok(json!({"type": 4, "data": browse(app, 0, platform).await?}))
        }
        Some("myrequest") => {
            let r: Option<Request> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "{SELECT} WHERE c.discord_user=$1 AND c.status='open' AND c.expires_at>now()"
            )))
            .bind(&i.user)
            .fetch_optional(&app.db)
            .await?;
            let Some(r) = r else {
                return Ok(private(
                    "You have no open collab request. `/collab post` starts one.",
                ));
            };
            Ok(
                json!({"type": 4, "data": {"embeds": [card(&r)], "flags": 64, "allowed_mentions": {"parse": []},
                "components": [{"type": 1, "components": [{"type": 2, "style": 4, "label": "Close request", "custom_id": format!("collab:close:{}", r.id)}]}]}}),
            )
        }
        _ => {
            let mine: Vec<(i64, String, Option<String>, String)> = sqlx::query_as("SELECT id,platform,game,status FROM discord_collabs WHERE discord_user=$1 ORDER BY created_at DESC LIMIT 5")
                .bind(&i.user).fetch_all(&app.db).await?;
            let answered: Vec<(i64, String, String, Option<String>, String)> = sqlx::query_as("SELECT c.id,c.discord_user,c.platform,c.game,c.status FROM discord_collab_responses r JOIN discord_collabs c ON c.id=r.collab_id WHERE r.discord_user=$1 ORDER BY r.created_at DESC LIMIT 5")
                .bind(&i.user).fetch_all(&app.db).await?;
            let game =
                |g: &Option<String>| g.as_deref().map(|g| format!(" • {g}")).unwrap_or_default();
            let mut text = "**Your requests**\n".to_string();
            if mine.is_empty() {
                text.push_str("_None_\n");
            }
            for (id, p, g, s) in &mine {
                text.push_str(&format!(
                    "#{id} — **{}**{} — `{s}`\n",
                    platform_name(p),
                    game(g)
                ));
            }
            text.push_str("\n**Requests you answered**\n");
            if answered.is_empty() {
                text.push_str("_None_\n");
            }
            for (id, by, p, g, s) in &answered {
                text.push_str(&format!(
                    "#{id} — <@{by}> • **{}**{} — `{s}`\n",
                    platform_name(p),
                    game(g)
                ));
            }
            Ok(embed(
                json!({"title": "Collab history", "description": text, "color": 0x8b_9bb4}),
                true,
            ))
        }
    }
}

async fn posted(app: &App, i: &Interaction) -> Res<Value> {
    let platform = i.custom_id().rsplit(':').next().unwrap_or("any");
    let platform = if PLATFORMS.contains(&platform) {
        platform
    } else {
        "any"
    };
    let field = |n: usize| {
        i.data["components"][n]["components"][0]["value"]
            .as_str()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.chars().take(500).collect::<String>())
    };
    let Some(description) = field(2) else {
        return Ok(private("Say what you're looking for."));
    };
    expire(app).await?;
    // The unique index keeps it to one open request, even with two forms sent at once.
    let id: Option<i64> = sqlx::query_scalar("INSERT INTO discord_collabs(discord_user,platform,game,schedule,description) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING RETURNING id")
        .bind(&i.user).bind(platform).bind(field(0)).bind(field(1)).bind(&description)
        .fetch_optional(&app.db).await?;
    let Some(id) = id else {
        return Ok(private("You already have an open collab request."));
    };
    quests::unlock(app, &i.user, "collab_posted").await?;
    let r: Request = sqlx::query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE c.id=$1")))
        .bind(id)
        .fetch_one(&app.db)
        .await?;
    Ok(
        json!({"type": 4, "data": {"content": "A new collab request is up!", "embeds": [card(&r)], "allowed_mentions": {"parse": []},
        "components": [{"type": 1, "components": [respond_button(&r)]}]}}),
    )
}

async fn interested(app: &App, i: &Interaction, id: i64) -> Res<Value> {
    let mut tx = app.db.begin().await?;
    // The request row is locked, so the last place can't be taken twice.
    let r: Option<(String, i16)> = sqlx::query_as("SELECT discord_user,max_responses FROM discord_collabs WHERE id=$1 AND status='open' AND expires_at>now() FOR UPDATE")
        .bind(id).fetch_optional(&mut *tx).await?;
    let Some((poster, max)) = r else {
        return Ok(private("This collab request is closed."));
    };
    if poster == i.user {
        return Ok(private("That's your own request."));
    }
    let added = sqlx::query("INSERT INTO discord_collab_responses(collab_id,discord_user) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(id).bind(&i.user).execute(&mut *tx).await?.rows_affected();
    if added == 0 {
        return Ok(private("You've already said you're interested."));
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM discord_collab_responses WHERE collab_id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let full = count >= max as i64;
    if full {
        sqlx::query("UPDATE discord_collabs SET status='filled',filled_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let (name, _) = i.profile(&i.user);
    let note = if full {
        "\n\nYour request is now full and closed."
    } else {
        ""
    };
    dm(app, &poster, json!({"content": format!("💼 **{name}** (<@{}>) is interested in your collab request #{id}.{note}", i.user),
        "allowed_mentions": {"parse": []}})).await;
    Ok(private(format!(
        "<@{poster}> has been told you're interested.{}",
        if full {
            " Their request is now full."
        } else {
            ""
        }
    )))
}

// ---- LFG ----

async fn lfg(app: App, i: Interaction) -> Res<Value> {
    let Some(to) = home::id(&home::settings(&app).await, "lfg") else {
        return Ok(private(
            "No LFG channel is set yet. Staff set one on the staff page.",
        ));
    };
    let text = |k: &str| {
        i.option(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    let game = text("game").unwrap_or_else(|| "A game".into());
    let players = i.option("players").and_then(Value::as_i64).unwrap_or(1);
    let rank = text("rank");
    let mut body = format!("**Host:** <@{}>\n", i.user);
    if let Some(r) = &rank {
        body.push_str(&format!("**Rank:** {r}\n"));
    }
    if let Some(d) = text("description") {
        body.push_str(&format!("\n{d}"));
    }
    let post = json!({"embeds": [{"title": format!("🎮 {game} — looking for {players}"), "description": body, "color": 0x9b_59b6,
        "footer": {"text": "S.V.E.R bot • LFG"}}], "allowed_mentions": {"parse": []}});
    let name: String = format!(
        "🎮 {game} — need {players}{}",
        rank.map(|r| format!(" ({r})")).unwrap_or_default()
    )
    .chars()
    .take(100)
    .collect();
    let id_of = |r: Option<(u16, Value)>| {
        r.filter(|r| r.0 < 300)
            .and_then(|r| r.1["id"].as_str().map(String::from))
    };
    let thread = match call(&app, Method::GET, &format!("/channels/{to}"), None).await {
        // A forum: a post of its own, tagged with the game when the forum has that tag.
        Some((200, c)) if c["type"] == 15 => {
            let tags: Vec<&Value> = c["available_tags"]
                .as_array()
                .into_iter()
                .flatten()
                .collect();
            let tag = |n: &str| {
                tags.iter()
                    .find(|t| {
                        t["name"]
                            .as_str()
                            .is_some_and(|x| x.eq_ignore_ascii_case(n))
                    })
                    .and_then(|t| t["id"].as_str())
            };
            let applied: Vec<&str> = tag(&game).or_else(|| tag("Any Game")).into_iter().collect();
            let thread = json!({"name": name, "message": post, "applied_tags": applied});
            id_of(
                call(
                    &app,
                    Method::POST,
                    &format!("/channels/{to}/threads"),
                    Some(thread),
                )
                .await,
            )
        }
        // A text channel: a public thread, then the post inside it.
        Some((200, _)) => {
            let thread = json!({"name": name, "type": 11, "auto_archive_duration": 1440});
            match id_of(
                call(
                    &app,
                    Method::POST,
                    &format!("/channels/{to}/threads"),
                    Some(thread),
                )
                .await,
            ) {
                Some(t) => send(&app, &t, post).await.map(|_| t),
                None => None,
            }
        }
        _ => None,
    };
    let Some(thread) = thread else {
        return Ok(private(
            "The LFG post couldn't be made. Ask staff to check the bot can post in the LFG channel.",
        ));
    };
    send(&app, &thread, json!({"components": [{"type": 1, "components": [
        {"type": 2, "style": 3, "label": "I'm in!", "emoji": {"name": "✋"}, "custom_id": format!("lfg:join:{thread}")}]}]})).await;
    Ok(private(format!("Your LFG post is up: <#{thread}>")))
}

async fn join(app: App, i: Interaction) -> Res<Value> {
    let thread = i.custom_id().rsplit(':').next().unwrap_or_default();
    let path = format!("/channels/{thread}/thread-members/{}", i.user);
    if !matches!(call(&app, Method::PUT, &path, None).await, Some((s, _)) if s < 300) {
        return Ok(private("That LFG thread is gone or closed."));
    }
    let (name, _) = i.profile(&i.user);
    send(
        &app,
        thread,
        json!({"content": format!("✋ **{name}** joined the group!"), "allowed_mentions": {"parse": []}}),
    )
    .await;
    Ok(private(format!("You're in: <#{thread}>")))
}

// ---- Commands and buttons ----

pub fn definitions() -> Vec<Value> {
    let mut platforms: Vec<Value> = PLATFORMS
        .iter()
        .map(|p| json!({"name": platform_name(p), "value": p}))
        .collect();
    platforms.push(json!({"name": "Any platform", "value": "any"}));
    vec![
        json!({"name": "collab", "description": "The streamer collab board.", "options": [
            {"type": 1, "name": "post", "description": "Post a collab request", "options": [
                {"type": 3, "name": "platform", "description": "Where you stream", "required": true, "choices": platforms}]},
            {"type": 1, "name": "browse", "description": "Open collab requests", "options": [
                {"type": 3, "name": "platform", "description": "Only this platform", "choices": platforms}]},
            {"type": 1, "name": "myrequest", "description": "Your open request"},
            {"type": 1, "name": "history", "description": "Your last requests and answers"}]}),
        json!({"name": "lfg", "description": "Find people to play with.", "options": [
            {"type": 3, "name": "game", "description": "The game", "required": true, "max_length": 80},
            {"type": 4, "name": "players", "description": "Players needed", "required": true, "min_value": 1, "max_value": 50},
            {"type": 3, "name": "rank", "description": "Minimum rank or skill", "max_length": 40},
            {"type": 3, "name": "description", "description": "Anything else", "max_length": 500}]}),
    ]
}

pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(match i.name() {
        "collab" => answer(collab(app, i).await),
        "lfg" => {
            later(app, i, true, lfg);
            ANSWERED
        }
        _ => return None,
    })
}

/// Buttons and forms whose id starts with `collab:` or `lfg:`.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let id = i.custom_id().to_string();
    let parts: Vec<&str> = id.split(':').collect();
    Some(match parts.as_slice() {
        ["collab", "modal", _] => answer(posted(app, i).await),
        ["collab", "respond", n] => answer(interested(app, i, n.parse().ok()?).await),
        ["collab", "close", n] => {
            let closed = sqlx::query("UPDATE discord_collabs SET status='cancelled' WHERE id=$1 AND discord_user=$2 AND status='open'")
                .bind(n.parse::<i64>().ok()?).bind(&i.user).execute(&app.db).await;
            match closed.map(|r| r.rows_affected()) {
                Ok(1) => {
                    json!({"type": 7, "data": {"content": "✅ Your collab request is closed.", "embeds": [], "components": []}})
                }
                Ok(_) => private("That request is already closed."),
                Err(_) => private("That couldn't be done right now. Try again in a minute."),
            }
        }
        ["collab", "browse", page, platform] => {
            let platform = Some(*platform).filter(|p| *p != "all");
            match browse(app, page.parse().unwrap_or(0), platform).await {
                Ok(data) => json!({"type": 7, "data": data}),
                Err(e) => answer(Err(e)),
            }
        }
        ["lfg", "join", _] => {
            later(app, i, true, join);
            ANSWERED
        }
        _ => return None,
    })
}
