//! Community tools on the home server (docs/DISCORD_BOT.md "Community"): mentors (`/mentor`),
//! threads in managed channels (`/thread create`) with forum setup and starter posts from the staff
//! page, temporary voice rooms (deleted once empty: the legacy bot never deleted them), custom
//! commands (`/command`, `/c`) and embeds (`/embed`).
use super::{button, field, form, name_of, row, text, update};
use crate::discord::{
    call,
    commands::{ANSWERED, Interaction, MANAGE_GUILD, embed, private, say},
    dm, home, role, send,
    streamers::{answer, later},
};
use crate::{
    App,
    profiles::{Fail, Res},
    safety,
};
use axum::{Json, extract::State};
use axum_extra::extract::cookie::CookieJar;
use reqwest::Method;
use serde_json::{Value, json};

/// The id from a Discord answer that succeeded.
fn created(r: Option<(u16, Value)>) -> Option<String> {
    r.filter(|r| r.0 < 300)
        .and_then(|r| r.1["id"].as_str().map(String::from))
}
fn list(value: &str, max: usize) -> Vec<String> {
    value
        .split(',')
        .map(|v| v.trim().chars().take(40).collect::<String>())
        .filter(|v| !v.is_empty())
        .take(max)
        .collect()
}
/// Posts in a text channel, or as a new post when the channel is a forum.
async fn post_in(app: &App, channel: &str, title: &str, body: Value) -> Option<String> {
    match call(app, Method::GET, &format!("/channels/{channel}"), None).await {
        Some((200, c)) if c["type"] == 15 => {
            let name: String = title.chars().take(100).collect();
            created(
                call(
                    app,
                    Method::POST,
                    &format!("/channels/{channel}/threads"),
                    Some(json!({"name": name, "message": body})),
                )
                .await,
            )
        }
        Some((200, _)) => send(app, channel, body).await,
        _ => None,
    }
}

// ---- Mentors ----

#[derive(sqlx::FromRow)]
struct Mentor {
    discord_user: String,
    platforms: Vec<String>,
    specialties: Vec<String>,
    games: Vec<String>,
    availability: Option<String>,
    max_mentees: i16,
    load: i64,
}
const MENTORS: &str = "SELECT m.discord_user,m.platforms,m.specialties,m.games,m.availability,m.max_mentees,
    (SELECT count(*) FROM discord_mentorships s WHERE s.mentor=m.discord_user AND s.status IN ('pending','active')) load FROM discord_mentors m WHERE m.active";

/// The best free mentor for a request: a platform match counts 5, each word of the request found
/// in their specialties 3, and every mentee they already have takes one off. Mentors who passed
/// on this member in the last week, and the member themself, are left out.
async fn best_mentor(
    app: &App,
    mentee: &str,
    help_with: &str,
    platform: Option<&str>,
) -> Res<Option<Mentor>> {
    let all: Vec<Mentor> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{MENTORS} AND m.discord_user<>$1
        AND NOT EXISTS(SELECT 1 FROM discord_mentorships p WHERE p.mentor=m.discord_user AND p.mentee=$1 AND p.status='passed' AND p.created_at>now()-interval '7 days')")))
        .bind(mentee).fetch_all(&app.db).await?;
    let words: Vec<String> = help_with
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let platform = platform.map(str::to_lowercase);
    let score = |m: &Mentor| {
        let lower = |v: &[String]| v.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
        let (platforms, specialties) = (lower(&m.platforms), lower(&m.specialties));
        let p = match &platform {
            Some(p) if platforms.iter().any(|x| x.contains(p.as_str())) => 5,
            _ => 0,
        };
        let w = words
            .iter()
            .filter(|w| specialties.iter().any(|s| s.contains(w.as_str())))
            .count() as i64
            * 3;
        p + w - m.load
    };
    Ok(all
        .into_iter()
        .filter(|m| m.load < i64::from(m.max_mentees))
        .max_by_key(score))
}
/// Suggests a mentor (and tells them by DM): the match post with accept/pass buttons.
async fn suggest(
    app: &App,
    mentee: &str,
    help_with: &str,
    platform: Option<&str>,
) -> Res<Option<Value>> {
    let Some(m) = best_mentor(app, mentee, help_with, platform).await? else {
        return Ok(None);
    };
    // The unique index keeps it to one open mentorship per member, even with two requests at once.
    let id: Option<i64> = sqlx::query_scalar("INSERT INTO discord_mentorships(mentor,mentee,help_with,platform) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING id")
        .bind(&m.discord_user).bind(mentee).bind(help_with).bind(platform).fetch_optional(&app.db).await?;
    let Some(id) = id else {
        return Ok(None);
    };
    dm(app, &m.discord_user, json!({"content": format!("🎓 <@{mentee}> could use your help with **{help_with}**. Accept or pass on the match in the server's stream-help channel."),
        "allowed_mentions": {"parse": []}})).await;
    Ok(Some(
        json!({"embeds": [{"title": "🎓 Mentor match!", "color": 0x34_98db,
            "description": format!("<@{mentee}> is looking for help with: **{help_with}**\n\n**Suggested mentor:** <@{}>\n**Specialties:** {}\n**Platforms:** {}",
                m.discord_user, m.specialties.join(", "), m.platforms.join(", ")),
            "footer": {"text": format!("S.V.E.R bot • Match #{id} • only the mentor can answer")}}],
        "components": [row(vec![button("Accept", 3, format!("mentor:accept:{id}")), button("Pass", 2, format!("mentor:pass:{id}"))])],
        "allowed_mentions": {"users": [m.discord_user]}}),
    ))
}

async fn mentor(app: App, i: Interaction) -> Res<Value> {
    let settings = home::settings(&app).await;
    let help = home::id(&settings, "stream_help");
    match i.sub() {
        Some("signup") => {
            let platforms = list(&text(&i, "platforms", 300).unwrap_or_default(), 10);
            let specialties = list(&text(&i, "specialties", 300).unwrap_or_default(), 10);
            if platforms.is_empty() || specialties.is_empty() {
                return Ok(private(
                    "List your platforms and specialties, separated by commas.",
                ));
            }
            let games = list(&text(&i, "games", 300).unwrap_or_default(), 20);
            let availability = text(&i, "availability", 200);
            sqlx::query("INSERT INTO discord_mentors(discord_user,platforms,specialties,games,availability) VALUES($1,$2,$3,$4,$5)
                ON CONFLICT(discord_user) DO UPDATE SET platforms=$2,specialties=$3,games=$4,availability=$5,active=true")
                .bind(&i.user).bind(&platforms).bind(&specialties).bind(&games).bind(&availability).execute(&app.db).await?;
            if let (Some(g), Some(r)) =
                (home::guild(&app).await, home::id(&settings, "mentor_role"))
            {
                role(&app, &g, &i.user, &r, true).await;
            }
            if let Some(help) = &help {
                let mut text = format!(
                    "<@{}> is now available as a mentor!\n\n**Specialties:** {}\n**Platforms:** {}",
                    i.user,
                    specialties.join(", "),
                    platforms.join(", ")
                );
                if !games.is_empty() {
                    text.push_str(&format!("\n**Games:** {}", games.join(", ")));
                }
                if let Some(a) = &availability {
                    text.push_str(&format!("\n**Available:** {a}"));
                }
                post_in(&app, help, &format!("🎓 New mentor: {}", name_of(&i)), json!({"embeds": [{"title": "🎓 New mentor available!", "description": text, "color": 0x2e_cc71}],
                    "allowed_mentions": {"parse": []}})).await;
            }
            Ok(private(format!(
                "🎓 You're a mentor now. Specialties: **{}**",
                specialties.join(", ")
            )))
        }
        Some("request") => {
            let Some(help_with) = text(&i, "help_with", 200) else {
                return Ok(private("Say what you'd like help with."));
            };
            let platform = text(&i, "platform", 40);
            let open: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_mentorships WHERE mentee=$1 AND status IN ('pending','active'))")
                .bind(&i.user).fetch_one(&app.db).await?;
            if open {
                return Ok(private(
                    "You already have a mentorship going. Finish it before asking for another.",
                ));
            }
            let Some(help) = help else {
                return Ok(private(
                    "Mentoring isn't set up yet: staff need to pick the stream-help channel.",
                ));
            };
            let Some(body) = suggest(&app, &i.user, &help_with, platform.as_deref()).await? else {
                return Ok(private(
                    "No mentor matching that is free right now. Try wider words, or check back later.",
                ));
            };
            post_in(&app, &help, "🎓 Mentor match", body).await;
            Ok(private(
                "🎓 We found a mentor and let them know. You'll hear back here or by DM.",
            ))
        }
        Some("complete") => {
            let mentee = i.option("mentee").and_then(Value::as_str).unwrap_or("");
            let done = sqlx::query("UPDATE discord_mentorships SET status='completed',completed_at=now() WHERE mentor=$1 AND mentee=$2 AND status='active'")
                .bind(&i.user).bind(mentee).execute(&app.db).await?.rows_affected();
            Ok(if done > 0 {
                say(format!(
                    "🎉 <@{}>'s mentorship of <@{mentee}> is complete. Great work!",
                    i.user
                ))
            } else {
                private(format!("You have no running mentorship with <@{mentee}>."))
            })
        }
        _ => {
            let all: Vec<Mentor> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "{MENTORS} ORDER BY 7,1 LIMIT 25"
            )))
            .fetch_all(&app.db)
            .await?;
            if all.is_empty() {
                return Ok(private(
                    "No mentors yet. Be the first with `/mentor signup`!",
                ));
            }
            let lines: Vec<String> = all
                .iter()
                .map(|m| {
                    let free = if m.load < i64::from(m.max_mentees) {
                        "🟢"
                    } else {
                        "🔴"
                    };
                    let games = if m.games.is_empty() {
                        String::new()
                    } else {
                        format!(" · 🎮 {}", m.games.join(", "))
                    };
                    let when = m
                        .availability
                        .as_deref()
                        .map(|a| format!(" · {a}"))
                        .unwrap_or_default();
                    format!(
                        "{free} <@{}>: {} ({}){games}{when}",
                        m.discord_user,
                        m.specialties.join(", "),
                        m.platforms.join(", ")
                    )
                })
                .collect();
            Ok(embed(
                json!({"title": "🎓 Mentors", "description": lines.join("\n"), "color": 0x34_98db}),
                false,
            ))
        }
    }
}
/// The suggested mentor accepted (a private thread for the two of them) or passed (the next
/// mentor is suggested in the same post).
async fn mentor_answer(app: App, i: Interaction) -> Res<Value> {
    let id = i.custom_id().to_string();
    let accept = id.starts_with("mentor:accept:");
    let match_id = id
        .rsplit(':')
        .next()
        .and_then(|n| n.parse::<i64>().ok())
        .unwrap_or(0);
    let to = if accept { "active" } else { "passed" };
    let pair: Option<(String, String, String, Option<String>)> = sqlx::query_as("UPDATE discord_mentorships SET status=$3 WHERE id=$1 AND mentor=$2 AND status='pending' RETURNING mentor,mentee,help_with,platform")
        .bind(match_id).bind(&i.user).bind(to).fetch_optional(&app.db).await?;
    let Some((mentor, mentee, help_with, platform)) = pair else {
        return Ok(update(
            json!({"content": "This match was already answered.", "embeds": [], "components": []}),
        ));
    };
    if !accept {
        if let Some(body) = suggest(&app, &mentee, &help_with, platform.as_deref()).await? {
            return Ok(update(body));
        }
        dm(&app, &mentee, json!({"content": format!("🎓 No mentor is free for **{help_with}** right now. Try `/mentor request` again later."),
            "allowed_mentions": {"parse": []}})).await;
        return Ok(update(
            json!({"content": format!("<@{mentor}> passed; no other mentor is free for <@{mentee}> right now."),
            "embeds": [], "components": [], "allowed_mentions": {"parse": []}}),
        ));
    }
    // A private thread for the two of them, or DMs when the channel can't hold one.
    let help = home::id(&home::settings(&app).await, "stream_help");
    let thread = match &help {
        Some(h) => created(call(&app, Method::POST, &format!("/channels/{h}/threads"),
            Some(json!({"name": format!("Mentorship #{match_id}"), "type": 12, "invitable": false, "auto_archive_duration": 10080}))).await),
        None => None,
    };
    let intro = format!(
        "Welcome! <@{mentor}> is mentoring <@{mentee}> on **{help_with}**.\n\nWhen you're done, the mentor runs `/mentor complete`."
    );
    match &thread {
        Some(t) => {
            for who in [&mentor, &mentee] {
                call(
                    &app,
                    Method::PUT,
                    &format!("/channels/{t}/thread-members/{who}"),
                    None,
                )
                .await;
            }
            send(
                &app,
                t,
                json!({"content": intro, "allowed_mentions": {"users": [mentor, mentee]}}),
            )
            .await;
            sqlx::query("UPDATE discord_mentorships SET thread_id=$2 WHERE id=$1")
                .bind(match_id)
                .bind(t)
                .execute(&app.db)
                .await?;
        }
        None => {
            for (who, other) in [(&mentor, &mentee), (&mentee, &mentor)] {
                dm(&app, who, json!({"content": format!("🎓 You're matched with <@{other}> for **{help_with}**. Send them a message to get started!"),
                    "allowed_mentions": {"parse": []}})).await;
            }
        }
    }
    let place = thread
        .map(|t| format!(" in <#{t}>"))
        .unwrap_or_else(|| " (by DM)".into());
    Ok(update(
        json!({"content": format!("✅ <@{mentor}> is mentoring <@{mentee}>{place}."), "embeds": [], "components": [],
        "allowed_mentions": {"parse": []}}),
    ))
}

// ---- Threads and forums ----

const MANAGED: [&str; 4] = ["game_chat", "stream_help", "clips", "lfg"];
async fn thread(app: App, i: Interaction) -> Res<Value> {
    let settings = home::settings(&app).await;
    let managed: Vec<String> = MANAGED
        .iter()
        .filter_map(|k| home::id(&settings, k))
        .collect();
    let Some(name) = text(&i, "name", 100) else {
        return Ok(private("Name the thread."));
    };
    let channel = i
        .option("channel")
        .and_then(Value::as_str)
        .map(String::from)
        .or(i.channel.clone())
        .unwrap_or_default();
    if !managed.contains(&channel) {
        let names: Vec<String> = managed.iter().map(|c| format!("<#{c}>")).collect();
        return Ok(private(if names.is_empty() {
            "No thread channels are set up yet. Staff pick them on the staff page.".to_string()
        } else {
            format!("Threads can be made in {}.", names.join(", "))
        }));
    }
    let hello = format!("Started by <@{}>. Welcome! 👋", i.user);
    let forum = matches!(call(&app, Method::GET, &format!("/channels/{channel}"), None).await, Some((200, c)) if c["type"] == 15);
    let made = if forum {
        created(call(&app, Method::POST, &format!("/channels/{channel}/threads"),
            Some(json!({"name": name, "message": {"content": hello, "allowed_mentions": {"parse": []}}}))).await)
    } else {
        match created(
            call(
                &app,
                Method::POST,
                &format!("/channels/{channel}/threads"),
                Some(json!({"name": name, "type": 11, "auto_archive_duration": 10080})),
            )
            .await,
        ) {
            Some(t) => {
                send(
                    &app,
                    &t,
                    json!({"content": hello, "allowed_mentions": {"parse": []}}),
                )
                .await;
                Some(t)
            }
            None => None,
        }
    };
    let Some(t) = made else {
        return Ok(private(
            "The thread couldn't be made. Ask staff to check the bot can make threads there.",
        ));
    };
    sqlx::query("INSERT INTO discord_threads(thread_id,channel_id,name,category,created_by) VALUES($1,$2,$3,'member',$4) ON CONFLICT DO NOTHING")
        .bind(&t).bind(&channel).bind(&name).bind(&i.user).execute(&app.db).await?;
    Ok(private(format!("Your thread is up: <#{t}>")))
}

/// A starter post: name, tags, text, pinned.
type Seed = (&'static str, &'static [&'static str], &'static str, bool);
/// Each managed forum's guidelines, tags and starter posts, as the legacy forum setup made them.
const FORUMS: &[(&str, &str, &[&str], &[Seed])] = &[
    (
        "game_chat",
        "Start a new post for any game. Pick the tag that matches your game or genre.",
        &[
            "FPS",
            "RPG",
            "MMO",
            "Battle Royale",
            "Horror",
            "Indie",
            "Strategy",
            "Sports",
            "Sandbox",
            "WoW",
            "Valorant",
            "Minecraft",
            "Other",
        ],
        &[
            (
                "WoW",
                &["MMO", "WoW"],
                "World of Warcraft talk: speedruns, raids, M+, leveling. Chromely records post here too.",
                true,
            ),
            (
                "Valorant",
                &["FPS", "Valorant"],
                "Valorant talk: ranked tips, agent picks, comps and finding teammates.",
                false,
            ),
            (
                "Minecraft",
                &["Sandbox", "Minecraft"],
                "Minecraft talk: survival, creative, mods and servers. Share your builds!",
                false,
            ),
            (
                "Fortnite",
                &["Battle Royale"],
                "Fortnite talk: seasons, skins, competitive, and squad up.",
                false,
            ),
            (
                "Apex Legends",
                &["FPS", "Battle Royale"],
                "Apex Legends talk: legend metas, ranked, and finding a squad.",
                false,
            ),
            (
                "League of Legends",
                &["Strategy"],
                "League talk: champions, the ranked climb and team comps.",
                false,
            ),
            (
                "Horror Games",
                &["Horror"],
                "Horror games: Phasmophobia, Dead by Daylight, Lethal Company and more.",
                false,
            ),
            (
                "Indie Games",
                &["Indie"],
                "Indie gems! Share what you're playing and discover new titles.",
                false,
            ),
            (
                "Call of Duty",
                &["FPS"],
                "CoD talk: Warzone, multiplayer, zombies.",
                false,
            ),
        ],
    ),
    (
        "lfg",
        "Looking for teammates? Make a post with the game and how many players you need, or use /lfg.",
        &[
            "Valorant",
            "Fortnite",
            "Minecraft",
            "Apex",
            "WoW",
            "CoD",
            "League",
            "Rocket League",
            "Other",
            "Any Game",
            "Need Tank",
            "Need Healer",
            "Need DPS",
            "Need Support",
            "Any Role",
        ],
        &[(
            "Example: Valorant ranked duo/trio",
            &["Valorant"],
            "**Game:** Valorant\n**Players needed:** 2\n**Rank:** Gold+\n**Time:** Evenings\n**Notes:** Looking for steady ranked partners. Comms required.\n\n*This is an example: make your own with `/lfg` or a new post!*",
            false,
        )],
    ),
    (
        "stream_help",
        "Need streaming help? Start a new post describing your question.",
        &[
            "OBS",
            "Audio",
            "Camera & Lighting",
            "Overlays & Alerts",
            "Growth",
            "Content Ideas",
            "Networking",
            "Hardware",
            "General",
        ],
        &[
            (
                "OBS setup guide",
                &["OBS"],
                "New to streaming? Start here! Ask about OBS Studio setup, scenes, sources, encoding and troubleshooting.",
                false,
            ),
            (
                "Audio setup",
                &["Audio"],
                "Microphones, mixing, filters and noise suppression: getting your audio sounding great.",
                false,
            ),
            (
                "Camera & lighting",
                &["Camera & Lighting"],
                "Webcams, lighting, green screens and looking your best on stream.",
                false,
            ),
            (
                "Channel growth",
                &["Growth"],
                "Growing your channel: networking, content plans, schedules and building an audience.",
                false,
            ),
            (
                "Overlays & alerts",
                &["Overlays & Alerts"],
                "Overlays, alerts and widgets: making your stream look polished.",
                false,
            ),
            (
                "Content ideas",
                &["Content Ideas"],
                "Brainstorm stream ideas, formats and creative ways to involve your audience.",
                false,
            ),
        ],
    ),
    (
        "clips",
        "Share your best clips! Add a title and pick the right tag.",
        &[
            "Funny",
            "Clutch",
            "Fail",
            "Highlight",
            "Tutorial",
            "S.V.E.R Clip",
            "Twitch Clip",
            "YouTube",
            "Other",
        ],
        &[(
            "Welcome to clips!",
            &["Highlight"],
            "Share your best moments: upload clips or link your S.V.E.R and Twitch clips. The best S.V.E.R clip each week is the **Clip of the Week**!",
            true,
        )],
    ),
];
fn named<'a>(tags: &'a [Value], name: &str) -> Option<&'a Value> {
    tags.iter().find(|h| {
        h["name"]
            .as_str()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}
/// Sets up one forum (guidelines and missing tags, up to Discord's 20) and makes its missing
/// starter posts; in a text channel they become threads. (made, skipped, problem).
async fn set_up(app: &App, key: &str, channel: &str) -> Res<(usize, usize, Option<String>)> {
    let Some((_, guidelines, tags, seeds)) = FORUMS.iter().find(|f| f.0 == key) else {
        return Ok((0, 0, None));
    };
    let Some((200, c)) = call(app, Method::GET, &format!("/channels/{channel}"), None).await else {
        return Ok((0, 0, Some("Discord didn't answer.".into())));
    };
    let forum = c["type"] == 15;
    let mut have: Vec<Value> = c["available_tags"].as_array().cloned().unwrap_or_default();
    if forum {
        for t in tags.iter() {
            if named(&have, t).is_none() && have.len() < 20 {
                have.push(json!({"name": t}));
            }
        }
        match call(
            app,
            Method::PATCH,
            &format!("/channels/{channel}"),
            Some(json!({"topic": guidelines, "available_tags": have})),
        )
        .await
        {
            Some((200, updated)) => {
                have = updated["available_tags"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            }
            _ => {
                return Ok((
                    0,
                    0,
                    Some("The bot couldn't edit the forum (it needs Manage Channels).".into()),
                ));
            }
        }
    }
    let (mut made, mut skipped) = (0, 0);
    for (name, seed_tags, content, pinned) in seeds.iter() {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_threads WHERE channel_id=$1 AND lower(name)=lower($2))")
            .bind(channel).bind(name).fetch_one(&app.db).await?;
        if exists {
            skipped += 1;
            continue;
        }
        let applied: Vec<&Value> = seed_tags
            .iter()
            .filter_map(|t| named(&have, t))
            .map(|h| &h["id"])
            .collect();
        let thread = if forum {
            created(call(app, Method::POST, &format!("/channels/{channel}/threads"),
                Some(json!({"name": name, "message": {"content": content}, "applied_tags": applied}))).await)
        } else {
            match created(
                call(
                    app,
                    Method::POST,
                    &format!("/channels/{channel}/threads"),
                    Some(json!({"name": name, "type": 11, "auto_archive_duration": 10080})),
                )
                .await,
            ) {
                Some(t) => {
                    send(app, &t, json!({"content": content})).await;
                    Some(t)
                }
                None => None,
            }
        };
        let Some(t) = thread else {
            return Ok((
                made,
                skipped,
                Some(format!("\"{name}\" couldn't be posted.")),
            ));
        };
        // A pinned forum post stays at the top (flag 2).
        if forum && *pinned {
            call(
                app,
                Method::PATCH,
                &format!("/channels/{t}"),
                Some(json!({"flags": 2})),
            )
            .await;
        }
        sqlx::query("INSERT INTO discord_threads(thread_id,channel_id,name,category) VALUES($1,$2,$3,'starter') ON CONFLICT DO NOTHING")
            .bind(&t).bind(channel).bind(name).execute(&app.db).await?;
        made += 1;
    }
    Ok((made, skipped, None))
}
/// POST /api/admin/discord/forums: sets up the managed channels picked on the staff page (game
/// chat, LFG, stream help, clips): guidelines, tags and starter posts, skipping posts already made.
pub async fn forums(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let settings = home::settings(&app).await;
    let mut report = Vec::new();
    for key in MANAGED {
        let Some(channel) = home::id(&settings, key) else {
            continue;
        };
        let (made, skipped, problem) = set_up(&app, key, &channel).await?;
        report.push(json!({"key": key, "channel": channel, "made": made, "skipped": skipped, "problem": problem}));
    }
    if report.is_empty() {
        return Err(Fail::conflict(
            "Pick the game chat, LFG, stream help or clips channel first.",
        ));
    }
    let mut tx = app.db.begin().await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "discord_forums",
        "discord",
        "forums",
        &[],
        "",
        json!({"report": report}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"forums": report})))
}

// ---- Temporary voice rooms ----

const VIEW: u64 = 1 << 10;
const CONNECT: u64 = 1 << 20;
/// Connect, mute, deafen, move, manage the channel and stream: the owner runs their room.
const OWNER: u64 = CONNECT | (1 << 22) | (1 << 23) | (1 << 24) | (1 << 4) | (1 << 9);
/// VOICE_STATE_UPDATE: joining the create-a-room channel makes a private room and moves the member
/// into it (their own, if they already have one); then empty rooms are deleted.
pub async fn voice(app: &App, d: &Value) -> Res<()> {
    let guild = home::guild(app).await;
    if d["guild_id"].as_str() != guild.as_deref() || d["member"]["user"]["bot"] == true {
        return Ok(());
    }
    let (Some(guild), Some(user)) = (guild, d["user_id"].as_str()) else {
        return Ok(());
    };
    if let Some(trigger) = home::id(&home::settings(app).await, "temp_room")
        && d["channel_id"].as_str() == Some(trigger.as_str())
    {
        let mine: Option<String> =
            sqlx::query_scalar("SELECT channel_id FROM discord_temp_rooms WHERE owner=$1")
                .bind(user)
                .fetch_optional(&app.db)
                .await?;
        let room = match mine {
            Some(r) => Some(r),
            None => {
                let parent =
                    match call(app, Method::GET, &format!("/channels/{trigger}"), None).await {
                        Some((200, c)) => c["parent_id"].clone(),
                        _ => Value::Null,
                    };
                let m = &d["member"];
                let name: String = m["nick"]
                    .as_str()
                    .or(m["user"]["global_name"].as_str())
                    .or(m["user"]["username"].as_str())
                    .unwrap_or("Member")
                    .chars()
                    .take(80)
                    .collect();
                let room = created(call(app, Method::POST, &format!("/guilds/{guild}/channels"), Some(json!({
                    "name": format!("🔒 {name}'s Room"), "type": 2, "parent_id": parent,
                    "permission_overwrites": [
                        {"id": guild, "type": 0, "allow": VIEW.to_string(), "deny": CONNECT.to_string()},
                        {"id": user, "type": 1, "allow": OWNER.to_string(), "deny": "0"}]}))).await);
                if let Some(r) = &room {
                    sqlx::query("INSERT INTO discord_temp_rooms(channel_id,owner) VALUES($1,$2)")
                        .bind(r)
                        .bind(user)
                        .execute(&app.db)
                        .await?;
                }
                room
            }
        };
        if let Some(r) = room {
            call(
                app,
                Method::PATCH,
                &format!("/guilds/{guild}/members/{user}"),
                Some(json!({"channel_id": r})),
            )
            .await;
        }
    }
    sweep(app).await
}
/// Deletes temp rooms nobody is in (a new one gets 30 seconds for its owner to arrive).
async fn sweep(app: &App) -> Res<()> {
    let empty: Vec<String> = sqlx::query_scalar("SELECT r.channel_id FROM discord_temp_rooms r WHERE r.created_at<now()-interval '30 seconds'
        AND NOT EXISTS(SELECT 1 FROM discord_voice v WHERE v.channel_id=r.channel_id)")
        .fetch_all(&app.db).await?;
    for room in empty {
        // Already gone in Discord (10003) counts as deleted.
        let gone = matches!(call(app, Method::DELETE, &format!("/channels/{room}"), None).await,
            Some((s, b)) if s < 300 || b["code"] == 10003);
        if gone {
            sqlx::query("DELETE FROM discord_temp_rooms WHERE channel_id=$1")
                .bind(&room)
                .execute(&app.db)
                .await?;
        }
    }
    Ok(())
}

// ---- Custom commands ----

fn command_name(i: &Interaction) -> String {
    text(i, "name", 32)
        .unwrap_or_default()
        .to_lowercase()
        .trim_start_matches(['!', '/'])
        .to_string()
}
async fn custom(app: &App, i: &Interaction) -> Res<Value> {
    if !i.staff() {
        return Ok(private("Only staff can use that."));
    }
    let name = command_name(i);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    match i.sub() {
        Some("add") => {
            let Some(response) = text(i, "response", 2000) else {
                return Ok(private("Give the reply."));
            };
            if !valid {
                return Ok(private(
                    "A name is up to 32 letters, numbers, - or _ (no spaces).",
                ));
            }
            let added = sqlx::query("INSERT INTO discord_custom_commands(name,response,created_by) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                .bind(&name).bind(response).bind(&i.user).execute(&app.db).await?.rows_affected();
            Ok(private(if added > 0 {
                format!("`/c {name}` is ready.")
            } else {
                format!("`{name}` already exists. Remove it first to change it.")
            }))
        }
        Some("remove") => {
            let gone = sqlx::query("DELETE FROM discord_custom_commands WHERE name=$1")
                .bind(&name)
                .execute(&app.db)
                .await?
                .rows_affected();
            Ok(private(if gone > 0 {
                format!("`{name}` is removed.")
            } else {
                format!("There's no `{name}`.")
            }))
        }
        _ => {
            let all: Vec<(String, i32)> = sqlx::query_as(
                "SELECT name,uses FROM discord_custom_commands ORDER BY name LIMIT 100",
            )
            .fetch_all(&app.db)
            .await?;
            if all.is_empty() {
                return Ok(private("No custom commands yet. `/command add` makes one."));
            }
            let lines: Vec<String> = all
                .iter()
                .map(|(n, u)| format!("`{n}` · used {u} times"))
                .collect();
            Ok(embed(
                json!({"title": "Custom commands", "description": lines.join("\n"), "color": 0x8b_9bb4}),
                true,
            ))
        }
    }
}
async fn use_custom(app: &App, i: &Interaction) -> Res<Value> {
    let name = command_name(i);
    let reply: Option<String> = sqlx::query_scalar(
        "UPDATE discord_custom_commands SET uses=uses+1 WHERE name=$1 RETURNING response",
    )
    .bind(&name)
    .fetch_optional(&app.db)
    .await?;
    Ok(match reply {
        Some(r) => say(r),
        None => private(format!(
            "There's no `{name}`. Staff list them with `/command list`."
        )),
    })
}

// ---- Embeds ----

fn embed_form(id: String, title: &str, e: &Value) -> Value {
    let color = e["color"]
        .as_u64()
        .map(|c| format!("#{c:06X}"))
        .unwrap_or_default();
    form(
        &id,
        title,
        &[
            (
                "title",
                "Title (optional)",
                false,
                false,
                256,
                e["title"].as_str().unwrap_or_default(),
            ),
            (
                "description",
                "Text",
                true,
                true,
                4000,
                e["description"].as_str().unwrap_or_default(),
            ),
            (
                "color",
                "Colour, like #FF0000 (optional)",
                false,
                false,
                7,
                &color,
            ),
            (
                "image",
                "Image link, https:// (optional)",
                false,
                false,
                500,
                e["image"]["url"].as_str().unwrap_or_default(),
            ),
        ],
    )
}
async fn embeds(app: &App, i: &Interaction) -> Res<Value> {
    if !i.staff() {
        return Ok(private("Only staff can use that."));
    }
    let channel = i
        .option("channel")
        .and_then(Value::as_str)
        .map(String::from)
        .or(i.channel.clone())
        .unwrap_or_default();
    match i.sub() {
        Some("json") => Ok(form(
            &format!("embed:json:{channel}"),
            "Embed from JSON",
            &[(
                "json",
                "Embed JSON, like {\"title\":\"Hi\"}",
                true,
                true,
                4000,
                "",
            )],
        )),
        Some("edit") => {
            let message = text(i, "message", 30).unwrap_or_default();
            let bot: Option<String> = sqlx::query_scalar("SELECT bot_id FROM discord_gateway")
                .fetch_optional(&app.db)
                .await?
                .flatten();
            // A form must be the first answer, so this read can't be deferred.
            match call(
                app,
                Method::GET,
                &format!("/channels/{channel}/messages/{message}"),
                None,
            )
            .await
            {
                Some((200, m)) if m["author"]["id"].as_str() == bot.as_deref() => Ok(embed_form(
                    format!("embed:edit:{channel}:{message}"),
                    "Edit the embed",
                    &m["embeds"][0],
                )),
                Some((200, _)) => Ok(private("The bot can only edit its own posts.")),
                _ => Ok(private(
                    "That message wasn't found in that channel. Give its ID and the channel it's in.",
                )),
            }
        }
        _ => Ok(embed_form(
            format!("embed:create:{channel}"),
            "New embed",
            &Value::Null,
        )),
    }
}
/// The embed a form describes, or why it can't be one.
fn embed_from(i: &Interaction) -> Result<Value, &'static str> {
    let mut e = json!({"description": field(i, "description").ok_or("Write the embed's text.")?});
    if let Some(t) = field(i, "title") {
        e["title"] = json!(t);
    }
    if let Some(c) = field(i, "color") {
        let n = u32::from_str_radix(c.trim_start_matches('#'), 16)
            .ok()
            .filter(|n| *n <= 0xff_ffff)
            .ok_or("A colour looks like #FF0000.")?;
        e["color"] = json!(n);
    }
    if let Some(url) = field(i, "image") {
        if !url.starts_with("https://") {
            return Err("An image link starts with https://.");
        }
        e["image"] = json!({"url": url});
    }
    Ok(e)
}
async fn embed_saved(app: App, i: Interaction) -> Res<Value> {
    if !i.staff() {
        return Ok(private("Only staff can use that."));
    }
    let id = i.custom_id().to_string();
    let parts: Vec<&str> = id.split(':').collect();
    let e = match parts.as_slice() {
        ["embed", "json", _] => {
            match field(&i, "json").and_then(|j| serde_json::from_str::<Value>(&j).ok()) {
                Some(v) if v.is_object() => v,
                _ => {
                    return Ok(private(
                        "That isn't a JSON object. Embeds look like {\"title\":\"Hi\",\"description\":\"Text\"}.",
                    ));
                }
            }
        }
        _ => match embed_from(&i) {
            Ok(e) => e,
            Err(why) => return Ok(private(why)),
        },
    };
    let body = json!({"embeds": [e], "allowed_mentions": {"parse": []}});
    let done = match parts.as_slice() {
        ["embed", "edit", channel, message] => matches!(
            call(&app, Method::PATCH, &format!("/channels/{channel}/messages/{message}"), Some(body)).await,
            Some((s, _)) if s < 300
        ),
        ["embed", _, channel] => send(&app, channel, body).await.is_some(),
        _ => false,
    };
    Ok(private(if done {
        "✅ Done."
    } else {
        "Discord refused that embed. Check the channel, and that every part is a valid embed field."
    }))
}

// ---- Commands, buttons and timers ----

pub fn definitions() -> Vec<Value> {
    let staff = MANAGE_GUILD.to_string();
    let string = |name: &str, description: &str, required: bool, max: u16| json!({"type": 3, "name": name, "description": description, "required": required, "max_length": max});
    let channel = |required: bool| json!({"type": 7, "name": "channel", "description": "Where", "required": required, "channel_types": [0, 5]});
    vec![
        json!({"name": "mentor", "description": "Mentoring.", "options": [
            {"type": 1, "name": "signup", "description": "Become a mentor", "options": [
                string("platforms", "Platforms you know, separated by commas", true, 300),
                string("specialties", "What you can help with, separated by commas", true, 300),
                string("games", "Games, separated by commas", false, 300),
                string("availability", "When you're around", false, 200)]},
            {"type": 1, "name": "request", "description": "Ask for a mentor", "options": [
                string("help_with", "What you'd like help with", true, 200),
                string("platform", "Your platform", false, 40)]},
            {"type": 1, "name": "list", "description": "Mentors"},
            {"type": 1, "name": "complete", "description": "Finish a mentorship", "options": [
                {"type": 6, "name": "mentee", "description": "Your mentee", "required": true}]}]}),
        json!({"name": "thread", "description": "Threads in the managed channels.", "options": [
            {"type": 1, "name": "create", "description": "Start a thread or forum post", "options": [
                string("name", "Its name", true, 100),
                {"type": 7, "name": "channel", "description": "Where (here by default)", "channel_types": [0, 15]}]}]}),
        json!({"name": "command", "description": "Custom commands.", "default_member_permissions": staff, "options": [
            {"type": 1, "name": "add", "description": "Add a custom command", "options": [
                string("name", "Its name (used as /c name)", true, 32),
                string("response", "What the bot replies", true, 2000)]},
            {"type": 1, "name": "remove", "description": "Remove a custom command", "options": [string("name", "Its name", true, 32)]},
            {"type": 1, "name": "list", "description": "Every custom command"}]}),
        json!({"name": "c", "description": "Run a custom command.", "options": [string("name", "The command", true, 32)]}),
        json!({"name": "embed", "description": "Post or edit an embed.", "default_member_permissions": staff, "options": [
            {"type": 1, "name": "create", "description": "Post a new embed", "options": [channel(true)]},
            {"type": 1, "name": "edit", "description": "Edit one of the bot's embeds", "options": [
                string("message", "The message ID", true, 30), channel(false)]},
            {"type": 1, "name": "json", "description": "Post an embed from JSON", "options": [channel(true)]}]}),
    ]
}

pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(match i.name() {
        "mentor" => {
            later(app, i, true, mentor);
            ANSWERED
        }
        "thread" => {
            later(app, i, true, thread);
            ANSWERED
        }
        "command" => answer(custom(app, i).await),
        "c" => answer(use_custom(app, i).await),
        "embed" => answer(embeds(app, i).await),
        _ => return None,
    })
}

/// Mentor match buttons and the embed forms.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let id = i.custom_id();
    if id.starts_with("mentor:") {
        // Anyone but the suggested mentor gets a private answer; the match post stays as it is.
        let n = id.rsplit(':').next()?.parse::<i64>().ok()?;
        let mentor: Option<String> =
            sqlx::query_scalar("SELECT mentor FROM discord_mentorships WHERE id=$1")
                .bind(n)
                .fetch_optional(&app.db)
                .await
                .ok()?;
        if mentor.as_deref() != Some(i.user.as_str()) {
            return Some(private("Only the suggested mentor can answer this."));
        }
        super::updating(app, i, mentor_answer);
        return Some(ANSWERED);
    }
    if id.starts_with("embed:") {
        later(app, i, true, embed_saved);
        return Some(ANSWERED);
    }
    None
}

/// Each minute: temp rooms left empty (after a restart, say) are deleted.
pub async fn minute(app: &App) -> Res<()> {
    sweep(app).await
}
