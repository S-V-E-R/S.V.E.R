//! Events, polls, giveaways and reminders on the home server (docs/DISCORD_BOT.md "Community").
//!
//! - **Events** (`/event`): RSVP buttons, reminders 60 and 15 minutes before, repeating daily,
//!   weekly, biweekly or monthly on the event's own wall clock (daylight saving included). An event
//!   without an end closes after 24 hours, so a repeating one always comes round again.
//! - **Polls** (`/poll`): 2–4 options, one vote each (a new vote replaces the old), closed at their
//!   time (the legacy bot never closed them).
//! - **Giveaways** (`/giveaway`): members submit for staff review, staff create, end, reroll or
//!   cancel; entry by button or 🎉; the draw is uniform over the entries and runs within seconds of
//!   the end (the legacy one could be up to a day late).
//! - **Reminders** (`/remind`), which the legacy bot never fired.
//!
//! Everything due is claimed with one UPDATE before it's posted, so it happens once even if two
//! processes look at the same moment.
use super::{button, field, form, row, text, update};
use crate::discord::{
    call,
    commands::{Interaction, embed, private},
    dm, factions, home, levels,
    moderation::minutes,
    quests, send,
    streamers::{self, answer},
};
use crate::{App, profiles::Res};
use chrono::{DateTime, Duration, Months, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;
use reqwest::Method;
use serde_json::{Value, json};

async fn edit(app: &App, channel: &str, message: &str, body: Value) {
    call(
        app,
        Method::PATCH,
        &format!("/channels/{channel}/messages/{message}"),
        Some(body),
    )
    .await;
}

// ---- Events ----

const KINDS: [(&str, &str); 7] = [
    ("general", "General"),
    ("game_night", "Game night"),
    ("watch_party", "Watch party"),
    ("faction_war", "Faction war"),
    ("territory_push", "Territory push"),
    ("tournament", "Tournament"),
    ("custom", "Custom"),
];
fn kind_label(kind: &str) -> &'static str {
    KINDS
        .iter()
        .find(|k| k.0 == kind)
        .map_or("General", |k| k.1)
}

#[derive(sqlx::FromRow)]
struct Event {
    id: i64,
    title: String,
    description: Option<String>,
    kind: String,
    faction: Option<String>,
    zone: String,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    repeat: Option<String>,
    status: String,
    channel_id: Option<String>,
    message_id: Option<String>,
    ping_role: bool,
    created_by: String,
    parent_id: Option<i64>,
}
const EVENT: &str = "SELECT id,title,description,kind,faction,zone,starts_at,ends_at,repeat,status,channel_id,message_id,ping_role,created_by,parent_id FROM discord_events";

async fn event(app: &App, id: i64) -> Res<Option<Event>> {
    Ok(
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{EVENT} WHERE id=$1")))
            .bind(id)
            .fetch_optional(&app.db)
            .await?,
    )
}
/// An event's post: its card with the RSVP counts, and buttons while it's open.
async fn event_post(app: &App, e: &Event) -> Res<Value> {
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT count(*) FILTER (WHERE status='going'),count(*) FILTER (WHERE status='maybe'),count(*) FILTER (WHERE status='not_going') FROM discord_event_rsvps WHERE event_id=$1")
        .bind(e.id).fetch_one(&app.db).await?;
    let prefix = match e.status.as_str() {
        "active" => "🟢 HAPPENING NOW —",
        "completed" => "✅ DONE —",
        "cancelled" => "❌ CANCELLED —",
        _ => "📅",
    };
    let start = e.starts_at.timestamp();
    let mut fields = vec![
        json!({"name": "📆 When", "value": format!("<t:{start}:f> (<t:{start}:R>)"), "inline": true}),
    ];
    if let Some(end) = e.ends_at {
        fields.push(json!({"name": "⏰ Length", "value": streamers::hours((end - e.starts_at).num_minutes()), "inline": true}));
    }
    fields.push(json!({"name": "🎮 Type", "value": kind_label(&e.kind), "inline": true}));
    if let Some(f) = &e.faction {
        fields.push(json!({"name": "⚔️ Faction", "value": format!("{} only", streamers::faction_line(f)), "inline": true}));
    }
    if let Some(r) = &e.repeat {
        fields.push(json!({"name": "🔁 Repeats", "value": r, "inline": true}));
    }
    fields.push(json!({"name": "RSVPs", "value": format!("✅ {} going · 🤔 {} maybe · ❌ {} can't", counts.0, counts.1, counts.2)}));
    let mut card = json!({"title": format!("{prefix} {}", e.title), "color": levels::color(e.faction.as_deref()),
        "fields": fields, "footer": {"text": format!("S.V.E.R bot • Event #{} • {}", e.id, e.zone)}});
    if let Some(d) = &e.description {
        card["description"] = json!(d);
    }
    let open = matches!(e.status.as_str(), "scheduled" | "active");
    let rsvp = |label: &str, style: u8, status: &str| json!({"type": 2, "style": style, "label": label, "custom_id": format!("event:rsvp:{}:{status}", e.id), "disabled": !open});
    Ok(
        json!({"embeds": [card], "allowed_mentions": {"parse": []}, "components": [row(vec![
        rsvp("Going", 3, "going"), rsvp("Maybe", 1, "maybe"), rsvp("Can't go", 2, "not_going")])]}),
    )
}
async fn refresh_event(app: &App, id: i64) -> Res<()> {
    if let Some(e) = event(app, id).await?
        && let (Some(c), Some(m)) = (&e.channel_id, &e.message_id)
    {
        edit(app, c, m, event_post(app, &e).await?).await;
    }
    Ok(())
}
/// A local wall-clock time in `zone` (a time the clock skips moves an hour later).
fn at(local: NaiveDateTime, zone: Tz) -> Option<DateTime<Utc>> {
    local
        .and_local_timezone(zone)
        .earliest()
        .or_else(|| {
            (local + Duration::hours(1))
                .and_local_timezone(zone)
                .earliest()
        })
        .map(|t| t.to_utc())
}
/// The next start of a repeating event after `after`, on its first start's wall clock in its zone.
fn next_start(
    first: DateTime<Utc>,
    repeat: &str,
    zone: Tz,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let first = first.with_timezone(&zone).naive_local();
    // Counted from the first start, so a month's day never drifts (Jan 31, Feb 28, Mar 31).
    // ponytail: walks from the first start; a daily series over five years old stops repeating.
    (1..=2000).find_map(|n: u32| {
        let local = match repeat {
            "daily" => first + Duration::days(n.into()),
            "weekly" => first + Duration::days(7 * i64::from(n)),
            "biweekly" => first + Duration::days(14 * i64::from(n)),
            _ => first.checked_add_months(Months::new(n))?,
        };
        at(local, zone).filter(|t| *t > after)
    })
}

async fn create_event(app: &App, i: &Interaction) -> Res<Value> {
    let settings = home::settings(app).await;
    let zone = text(i, "zone", 40)
        .or_else(|| {
            settings
                .get("zone")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_else(|| "UTC".into());
    let Some(zone) = streamers::zone(&zone) else {
        return Ok(private(
            "That time zone isn't known. Use a name like America/New_York or Europe/London.",
        ));
    };
    let date = text(i, "date", 10).and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
    let time = text(i, "time", 10).as_deref().and_then(streamers::clock);
    let (Some(date), Some(time)) = (date, time) else {
        return Ok(private(
            "Give the date as 2026-10-31 and the time like 20:00 or 8:00 PM.",
        ));
    };
    let Some(starts) = at(date.and_time(time), zone).filter(|t| *t > Utc::now()) else {
        return Ok(private("That time has already passed."));
    };
    let ends = match text(i, "duration", 10) {
        None => None,
        Some(d) => match minutes(&d).filter(|m| *m <= 7 * 1440) {
            Some(m) => Some(starts + Duration::minutes(m)),
            None => {
                return Ok(private(
                    "A length looks like 90m, 2h or 1d (at most a week).",
                ));
            }
        },
    };
    let kind = i
        .option("type")
        .and_then(Value::as_str)
        .filter(|k| KINDS.iter().any(|x| x.0 == *k))
        .unwrap_or("general");
    let faction = i
        .option("faction")
        .and_then(Value::as_str)
        .filter(|f| levels::FACTIONS.contains(f));
    let repeat = i
        .option("repeat")
        .and_then(Value::as_str)
        .filter(|r| ["daily", "weekly", "biweekly", "monthly"].contains(r));
    let Some(channel) = home::id(&settings, "events").or(i.channel.clone()) else {
        return Ok(private("No events channel is set."));
    };
    // Only staff events ping a whole faction's role.
    let id: i64 = sqlx::query_scalar("INSERT INTO discord_events(title,description,kind,faction,zone,starts_at,ends_at,repeat,channel_id,ping_role,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id")
        .bind(text(i, "title", 100).unwrap_or_else(|| "Event".into())).bind(text(i, "description", 1000)).bind(kind).bind(faction)
        .bind(zone.name()).bind(starts).bind(ends).bind(repeat).bind(&channel).bind(i.staff()).bind(&i.user)
        .fetch_one(&app.db).await?;
    post_event(app, id).await?;
    Ok(private(format!(
        "📅 Event **#{id}** is up in <#{channel}>: <t:{}:f>.",
        starts.timestamp()
    )))
}
async fn post_event(app: &App, id: i64) -> Res<()> {
    let Some(e) = event(app, id).await? else {
        return Ok(());
    };
    let Some(channel) = &e.channel_id else {
        return Ok(());
    };
    if let Some(m) = send(app, channel, event_post(app, &e).await?).await {
        sqlx::query("UPDATE discord_events SET message_id=$2 WHERE id=$1")
            .bind(id)
            .bind(m)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}
async fn rsvp(app: &App, i: &Interaction, id: i64, status: &str) -> Res<Value> {
    let Some(e) = event(app, id)
        .await?
        .filter(|e| matches!(e.status.as_str(), "scheduled" | "active"))
    else {
        return Ok(private("This event is over."));
    };
    if let Some(f) = &e.faction
        && status != "not_going"
        && levels::faction(app, &i.user).await?.as_ref() != Some(f)
    {
        return Ok(private(format!(
            "This event is for {} only.",
            factions::title(f)
        )));
    }
    sqlx::query("INSERT INTO discord_event_rsvps(event_id,discord_user,status) VALUES($1,$2,$3) ON CONFLICT(event_id,discord_user) DO UPDATE SET status=$3,at=now()")
        .bind(id).bind(&i.user).bind(status).execute(&app.db).await?;
    // Going counts once per event, however often someone switches (the legacy bot paid every time),
    // and never for the event's own host, or anyone could pay themselves by making events.
    let first = status == "going"
        && e.created_by != i.user
        && sqlx::query("UPDATE discord_event_rsvps SET counted=true WHERE event_id=$1 AND discord_user=$2 AND NOT counted")
            .bind(id).bind(&i.user).execute(&app.db).await?.rows_affected() == 1;
    if first {
        if let Some(f) = levels::faction(app, &i.user).await? {
            sqlx::query("INSERT INTO discord_points(discord_user,faction,kind,points) VALUES($1,$2,'event',10)")
                .bind(&i.user).bind(f).execute(&app.db).await?;
        }
        quests::bump(app, &i.user, "event_rsvp", 1).await?;
    }
    Ok(update(event_post(app, &e).await?))
}
async fn events_command(app: &App, i: &Interaction) -> Res<Value> {
    match i.sub() {
        Some("create") => create_event(app, i).await,
        Some("cancel") => {
            let id = i.option("id").and_then(Value::as_i64).unwrap_or(0);
            let Some(e) = event(app, id).await? else {
                return Ok(private("No event has that number."));
            };
            if e.created_by != i.user && !i.staff() {
                return Ok(private("Only the event's creator or staff can cancel it."));
            }
            let done = sqlx::query("UPDATE discord_events SET status='cancelled' WHERE id=$1 AND status IN ('scheduled','active')")
                .bind(id).execute(&app.db).await?.rows_affected();
            if done == 0 {
                return Ok(private("That event is already over."));
            }
            refresh_event(app, id).await?;
            Ok(private(format!("Event #{id} is cancelled.")))
        }
        _ => {
            let page = i.option("page").and_then(Value::as_i64).unwrap_or(1).max(1);
            let rows: Vec<Event> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "{EVENT} WHERE status IN ('scheduled','active') ORDER BY starts_at LIMIT 5 OFFSET $1"
            )))
            .bind((page - 1) * 5)
            .fetch_all(&app.db)
            .await?;
            if rows.is_empty() {
                return Ok(private("No upcoming events. `/event create` adds one."));
            }
            let lines: Vec<String> = rows
                .iter()
                .map(|e| {
                    let live = if e.status == "active" { " 🟢" } else { "" };
                    format!(
                        "**#{}** {}{live} · <t:{}:f> · {}",
                        e.id,
                        e.title,
                        e.starts_at.timestamp(),
                        kind_label(&e.kind)
                    )
                })
                .collect();
            Ok(embed(
                json!({"title": "📅 Upcoming events", "description": lines.join("\n"), "color": 0x34_98db,
                "footer": {"text": format!("Page {page}")}}),
                false,
            ))
        }
    }
}

/// Each minute: reminders, starts, ends (after 24 hours when there's no end), and the next
/// occurrence of a repeating event.
async fn event_minute(app: &App) -> Res<()> {
    let settings = home::settings(app).await;
    for (column, from, to, words) in [
        ("reminded_60", 15, 60, "in 1 hour"),
        ("reminded_15", 0, 15, "in 15 minutes"),
    ] {
        // The column is one of two fixed names.
        let due: Vec<Event> = sqlx::query_as(sqlx::AssertSqlSafe(format!("WITH due AS (UPDATE discord_events SET {column}=true WHERE status='scheduled' AND NOT {column}
                AND starts_at>now()+make_interval(mins=>$1) AND starts_at<=now()+make_interval(mins=>$2) RETURNING id)
            {EVENT} WHERE id IN (SELECT id FROM due)")))
            .bind(from).bind(to).fetch_all(&app.db).await?;
        for e in due {
            let Some(channel) = &e.channel_id else {
                continue;
            };
            let going: Vec<String> = sqlx::query_scalar("SELECT discord_user FROM discord_event_rsvps WHERE event_id=$1 AND status='going' LIMIT 90")
                .bind(e.id).fetch_all(&app.db).await?;
            let role = e
                .faction
                .as_deref()
                .filter(|_| e.ping_role)
                .and_then(|f| home::id(&settings, f));
            let mentions: Vec<String> = going.iter().map(|u| format!("<@{u}>")).collect();
            let ping = role
                .as_ref()
                .map(|r| format!("<@&{r}> "))
                .unwrap_or_default();
            send(app, channel, json!({"content": format!("⏰ {ping}**{}** starts {words}! {}", e.title, mentions.join(" ")),
                "allowed_mentions": {"users": going, "roles": role.into_iter().collect::<Vec<_>>()}})).await;
        }
    }
    let started: Vec<i64> = sqlx::query_scalar("UPDATE discord_events SET status='active' WHERE status='scheduled' AND starts_at<=now() RETURNING id")
        .fetch_all(&app.db).await?;
    for id in started {
        refresh_event(app, id).await?;
    }
    let ended: Vec<i64> = sqlx::query_scalar("UPDATE discord_events SET status='completed' WHERE status='active' AND coalesce(ends_at,starts_at+interval '24 hours')<=now() RETURNING id")
        .fetch_all(&app.db).await?;
    for id in ended {
        refresh_event(app, id).await?;
        let Some(e) = event(app, id).await? else {
            continue;
        };
        let (Some(repeat), Some(zone)) = (&e.repeat, streamers::zone(&e.zone)) else {
            continue;
        };
        // The series counts from its first event's start.
        let first = match e.parent_id {
            Some(p) => event(app, p).await?.map_or(e.starts_at, |r| r.starts_at),
            None => e.starts_at,
        };
        let Some(next) = next_start(first, repeat, zone, Utc::now()) else {
            continue;
        };
        let ends = e.ends_at.map(|end| next + (end - e.starts_at));
        let new: i64 = sqlx::query_scalar("INSERT INTO discord_events(title,description,kind,faction,zone,starts_at,ends_at,repeat,channel_id,ping_role,created_by,parent_id)
            SELECT title,description,kind,faction,zone,$2,$3,repeat,channel_id,ping_role,created_by,coalesce(parent_id,id) FROM discord_events WHERE id=$1 RETURNING id")
            .bind(id).bind(next).bind(ends).fetch_one(&app.db).await?;
        post_event(app, new).await?;
    }
    Ok(())
}

// ---- Polls ----

const LETTERS: [&str; 4] = ["A", "B", "C", "D"];
type PollPost = (String, Option<String>, Value);
/// A poll's channel, message and post (results so far, buttons while it's open).
async fn poll_post(app: &App, id: i64) -> Res<Option<PollPost>> {
    type Poll = (
        String,
        Vec<String>,
        String,
        Option<String>,
        Option<DateTime<Utc>>,
        bool,
    );
    let poll: Option<Poll> = sqlx::query_as("SELECT question,options,channel_id,message_id,ends_at,closed_at IS NOT NULL FROM discord_polls WHERE id=$1")
        .bind(id).fetch_optional(&app.db).await?;
    let Some((question, options, channel, message, ends, closed)) = poll else {
        return Ok(None);
    };
    let votes: Vec<(i16, i64)> = sqlx::query_as(
        "SELECT option,count(*) FROM discord_poll_votes WHERE poll_id=$1 GROUP BY option",
    )
    .bind(id)
    .fetch_all(&app.db)
    .await?;
    let count = |n: usize| votes.iter().find(|v| v.0 as usize == n).map_or(0, |v| v.1);
    let total: i64 = votes.iter().map(|v| v.1).sum();
    let chart: Vec<String> = options
        .iter()
        .enumerate()
        .map(|(n, o)| {
            let c = count(n);
            let (filled, percent) = if total > 0 {
                (((c * 10 + total / 2) / total) as usize, c * 100 / total)
            } else {
                (0, 0)
            };
            format!(
                "**{}.** {o}\n{}{}  {c} vote{} ({percent}%)",
                LETTERS[n],
                "▓".repeat(filled),
                "░".repeat(10 - filled),
                if c == 1 { "" } else { "s" }
            )
        })
        .collect();
    let state = match (closed, ends) {
        (true, _) => "**Closed**".to_string(),
        (false, Some(t)) => format!("Ends <t:{}:R>", t.timestamp()),
        (false, None) => "Open, with no end".into(),
    };
    let buttons: Vec<Value> = options
        .iter()
        .enumerate()
        .map(|(n, o)| {
            let label = if o.chars().count() <= 25 {
                o.clone()
            } else {
                LETTERS[n].to_string()
            };
            json!({"type": 2, "style": 1, "label": label, "custom_id": format!("poll:vote:{id}:{n}"), "disabled": closed})
        })
        .collect();
    let body = json!({"embeds": [{"title": format!("📊 {question}"), "color": 0x9b_59b6,
            "description": format!("{}\n\nTotal votes: {total}\n{state}", chart.join("\n\n")),
            "footer": {"text": format!("S.V.E.R bot • Poll #{id} • one vote each")}}],
        "components": [row(buttons)], "allowed_mentions": {"parse": []}});
    Ok(Some((channel, message, body)))
}
async fn create_poll(app: &App, i: &Interaction) -> Res<Value> {
    let Some(question) = text(i, "question", 200) else {
        return Ok(private("Ask a question."));
    };
    let options: Vec<String> = ["option1", "option2", "option3", "option4"]
        .iter()
        .filter_map(|k| text(i, k, 80))
        .collect();
    if options.len() < 2 {
        return Ok(private("A poll needs at least two options."));
    }
    let ends = match text(i, "duration", 10) {
        None => None,
        Some(d) => match minutes(&d).filter(|m| *m <= 30 * 1440) {
            Some(m) => Some(Utc::now() + Duration::minutes(m)),
            None => {
                return Ok(private(
                    "A length looks like 30m, 24h or 3d (at most 30 days).",
                ));
            }
        },
    };
    let Some(channel) = i.channel.clone() else {
        return Ok(private("Use this in a channel."));
    };
    let id: i64 = sqlx::query_scalar("INSERT INTO discord_polls(question,options,channel_id,created_by,ends_at) VALUES($1,$2,$3,$4,$5) RETURNING id")
        .bind(&question).bind(&options).bind(&channel).bind(&i.user).bind(ends).fetch_one(&app.db).await?;
    let Some((_, _, body)) = poll_post(app, id).await? else {
        return Ok(private("The poll couldn't be made."));
    };
    let Some(m) = send(app, &channel, body).await else {
        sqlx::query("DELETE FROM discord_polls WHERE id=$1")
            .bind(id)
            .execute(&app.db)
            .await?;
        return Ok(private(
            "The poll couldn't be posted here. Check the bot can send messages in this channel.",
        ));
    };
    sqlx::query("UPDATE discord_polls SET message_id=$2 WHERE id=$1")
        .bind(id)
        .bind(m)
        .execute(&app.db)
        .await?;
    Ok(private(format!("📊 Poll #{id} is up.")))
}
async fn vote(app: &App, i: &Interaction, id: i64, option: i64) -> Res<Value> {
    let open: Option<i32> = sqlx::query_scalar("SELECT cardinality(options) FROM discord_polls WHERE id=$1 AND closed_at IS NULL AND (ends_at IS NULL OR ends_at>now())")
        .bind(id).fetch_optional(&app.db).await?;
    if !open.is_some_and(|n| (0..i64::from(n)).contains(&option)) {
        return Ok(private("This poll is closed."));
    }
    sqlx::query("INSERT INTO discord_poll_votes(poll_id,discord_user,option) VALUES($1,$2,$3) ON CONFLICT(poll_id,discord_user) DO UPDATE SET option=$3")
        .bind(id).bind(&i.user).bind(option as i16).execute(&app.db).await?;
    Ok(match poll_post(app, id).await? {
        Some((_, _, body)) => update(body),
        None => private("This poll is gone."),
    })
}
/// Polls past their time close: the results stay, the buttons go grey and the result is posted.
async fn close_polls(app: &App) -> Res<()> {
    let closed: Vec<(i64, String, Vec<String>)> = sqlx::query_as("UPDATE discord_polls SET closed_at=now() WHERE closed_at IS NULL AND ends_at<=now() RETURNING id,question,options")
        .fetch_all(&app.db).await?;
    for (id, question, options) in closed {
        let Some((channel, message, body)) = poll_post(app, id).await? else {
            continue;
        };
        if let Some(m) = &message {
            edit(app, &channel, m, body).await;
        }
        let top: Vec<(i16, i64)> = sqlx::query_as("SELECT option,count(*) c FROM discord_poll_votes WHERE poll_id=$1 GROUP BY option ORDER BY c DESC LIMIT 2")
            .bind(id).fetch_all(&app.db).await?;
        let result = match top.as_slice() {
            [] => "Nobody voted.".to_string(),
            [a, b] if a.1 == b.1 => format!("It's a tie at {} votes each.", a.1),
            [a, ..] => format!(
                "**{}** wins with {} vote{}.",
                options.get(a.0 as usize).map_or("?", String::as_str),
                a.1,
                if a.1 == 1 { "" } else { "s" }
            ),
        };
        let mut post = json!({"content": format!("📊 The poll **{question}** is closed. {result}"), "allowed_mentions": {"parse": []}});
        if let Some(m) = message {
            post["message_reference"] = json!({"message_id": m, "fail_if_not_exists": false});
        }
        send(app, &channel, post).await;
    }
    Ok(())
}

// ---- Giveaways ----

#[derive(sqlx::FromRow)]
struct Giveaway {
    id: i64,
    prize: String,
    description: Option<String>,
    winners: i16,
    minutes: i32,
    status: String,
    channel_id: Option<String>,
    message_id: Option<String>,
    created_by: String,
    ends_at: Option<DateTime<Utc>>,
    winner_ids: Vec<String>,
}
const GIVEAWAY: &str = "SELECT id,prize,description,winners,minutes,status,channel_id,message_id,created_by,ends_at,winner_ids FROM discord_giveaways";
async fn giveaway(app: &App, id: i64) -> Res<Option<Giveaway>> {
    Ok(
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{GIVEAWAY} WHERE id=$1")))
            .bind(id)
            .fetch_optional(&app.db)
            .await?,
    )
}
async fn giveaway_post(app: &App, g: &Giveaway) -> Res<Value> {
    let entries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM discord_giveaway_entries WHERE giveaway_id=$1")
            .bind(g.id)
            .fetch_one(&app.db)
            .await?;
    let open = g.status == "active";
    let mut lines = vec![format!("**Hosted by:** <@{}>", g.created_by)];
    if let Some(d) = &g.description {
        lines.push(d.clone());
    }
    match (open, g.ends_at) {
        (true, Some(t)) => lines.push(format!(
            "**Ends:** <t:{}:R>\n**Winners:** {}\n\nPress the button or react with 🎉 to enter!",
            t.timestamp(),
            g.winners
        )),
        _ if g.status == "cancelled" => lines.push("**Cancelled.**".into()),
        _ if g.winner_ids.is_empty() => lines.push("**Ended.** Nobody entered.".into()),
        _ => {
            let names: Vec<String> = g.winner_ids.iter().map(|w| format!("<@{w}>")).collect();
            lines.push(format!("**Ended.** Winner(s): {}", names.join(", ")));
        }
    }
    Ok(
        json!({"embeds": [{"title": format!("🎉 GIVEAWAY: {}", g.prize), "description": lines.join("\n"),
            "color": if open { 0xf1_c40f } else { 0x95_a5a6 }, "footer": {"text": format!("S.V.E.R bot • Giveaway #{} • {entries} entered", g.id)}}],
        "components": [row(vec![json!({"type": 2, "style": 1, "label": if open { "Enter" } else { "Closed" }, "emoji": {"name": "🎉"},
            "custom_id": format!("give:enter:{}", g.id), "disabled": !open})])],
        "allowed_mentions": {"parse": []}}),
    )
}
async fn refresh_giveaway(app: &App, id: i64) -> Res<()> {
    if let Some(g) = giveaway(app, id).await?
        && let (Some(c), Some(m)) = (&g.channel_id, &g.message_id)
    {
        edit(app, c, m, giveaway_post(app, &g).await?).await;
    }
    Ok(())
}
/// Starts a pending giveaway: its end is set from now, and it's posted with the entry button and a
/// 🎉. Its channel, when it started.
async fn start_giveaway(app: &App, id: i64, by: &str) -> Res<Option<String>> {
    let started: Option<String> = sqlx::query_scalar("UPDATE discord_giveaways SET status='active',reviewed_by=$2,starts_at=now(),ends_at=now()+make_interval(mins=>minutes) WHERE id=$1 AND status='pending' RETURNING channel_id")
        .bind(id).bind(by).fetch_optional(&app.db).await?.flatten();
    let Some(channel) = started else {
        return Ok(None);
    };
    let Some(g) = giveaway(app, id).await? else {
        return Ok(None);
    };
    if let Some(m) = send(app, &channel, giveaway_post(app, &g).await?).await {
        sqlx::query("UPDATE discord_giveaways SET message_id=$2 WHERE id=$1")
            .bind(id)
            .bind(&m)
            .execute(&app.db)
            .await?;
        call(
            app,
            Method::PUT,
            &format!("/channels/{channel}/messages/{m}/reactions/%F0%9F%8E%89/@me"),
            None,
        )
        .await;
    }
    Ok(Some(channel))
}
/// A fair draw: winners picked uniformly from the entries, without repeats, leaving out anyone in
/// `skip` (earlier winners on a reroll).
async fn draw(app: &App, id: i64, count: i16, skip: &[String]) -> Res<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT discord_user FROM discord_giveaway_entries WHERE giveaway_id=$1 AND NOT discord_user=ANY($2) ORDER BY random() LIMIT $3")
        .bind(id).bind(skip).bind(i64::from(count)).fetch_all(&app.db).await?)
}
async fn announce_winners(app: &App, g: &Giveaway, winners: &[String], reroll: bool) {
    let Some(channel) = &g.channel_id else {
        return;
    };
    let content = if winners.is_empty() {
        format!("Nobody entered the giveaway for **{}**.", g.prize)
    } else {
        let names: Vec<String> = winners.iter().map(|w| format!("<@{w}>")).collect();
        format!(
            "🎉 {}Congratulations {}! You won **{}**!",
            if reroll { "New draw! " } else { "" },
            names.join(", "),
            g.prize
        )
    };
    send(
        app,
        channel,
        json!({"content": content, "allowed_mentions": {"users": winners}}),
    )
    .await;
}
/// Ends one running giveaway now (on time, or by staff): the draw, the post and its winners.
async fn end_giveaway(app: &App, id: i64) -> Res<bool> {
    let claimed: Option<i16> = sqlx::query_scalar("UPDATE discord_giveaways SET status='ended',ended_at=now() WHERE id=$1 AND status='active' RETURNING winners")
        .bind(id).fetch_optional(&app.db).await?;
    let Some(count) = claimed else {
        return Ok(false);
    };
    let winners = draw(app, id, count, &[]).await?;
    sqlx::query("UPDATE discord_giveaways SET winner_ids=$2 WHERE id=$1")
        .bind(id)
        .bind(&winners)
        .execute(&app.db)
        .await?;
    refresh_giveaway(app, id).await?;
    if let Some(g) = giveaway(app, id).await? {
        announce_winners(app, &g, &winners, false).await;
    }
    Ok(true)
}
/// Enters a member: Some(true) when they're newly in, None when it's closed. The host can't enter
/// their own giveaway.
async fn enter(app: &App, g: &Giveaway, user: &str) -> Res<Option<bool>> {
    if g.status != "active" || g.ends_at.is_some_and(|t| t <= Utc::now()) {
        return Ok(None);
    }
    if user == g.created_by {
        return Ok(Some(false));
    }
    let new = sqlx::query("INSERT INTO discord_giveaway_entries(giveaway_id,discord_user) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(g.id).bind(user).execute(&app.db).await?.rows_affected() == 1;
    if new {
        if let Some(f) = levels::faction(app, user).await? {
            sqlx::query("INSERT INTO discord_points(discord_user,faction,kind,points) VALUES($1,$2,'giveaway',5)")
                .bind(user).bind(f).execute(&app.db).await?;
        }
        quests::bump(app, user, "giveaway_enter", 1).await?;
        // The count on the post follows, off the answer's clock.
        let (app, id) = (app.clone(), g.id);
        tokio::spawn(async move {
            let _ = refresh_giveaway(&app, id).await;
        });
    }
    Ok(Some(new))
}
/// MESSAGE_REACTION_ADD: a 🎉 on a giveaway's post is an entry. True when the reaction was on a
/// giveaway post (it earns no XP then).
pub async fn reaction(app: &App, d: &Value) -> Res<bool> {
    let (Some(user), Some(message)) = (d["user_id"].as_str(), d["message_id"].as_str()) else {
        return Ok(false);
    };
    if d["emoji"]["name"] != "🎉" || d["member"]["user"]["bot"] == true {
        return Ok(false);
    }
    let g: Option<Giveaway> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{GIVEAWAY} WHERE message_id=$1"
    )))
    .bind(message)
    .fetch_optional(&app.db)
    .await?;
    let Some(g) = g else {
        return Ok(false);
    };
    enter(app, &g, user).await?;
    Ok(true)
}
fn review_post(g: &Giveaway) -> Value {
    json!({"embeds": [{"title": "🎁 Giveaway for review", "color": 0xf1_c40f,
            "description": format!("**Prize:** {}\n**From:** <@{}>\n**Winners:** {}\n**Runs for:** {}{}", g.prize, g.created_by, g.winners,
                streamers::hours(i64::from(g.minutes)), g.description.as_deref().map(|d| format!("\n\n{d}")).unwrap_or_default())}],
        "components": [row(vec![button("Approve", 3, format!("give:approve:{}", g.id)), button("Deny", 4, format!("give:deny:{}", g.id))])],
        "allowed_mentions": {"parse": []}})
}
async fn giveaway_command(app: &App, i: &Interaction) -> Res<Value> {
    let sub = i.sub().unwrap_or_default();
    if sub != "submit" && !i.staff() {
        return Ok(private("Only staff can do that."));
    }
    let settings = home::settings(app).await;
    let id = i.option("id").and_then(Value::as_i64).unwrap_or(0);
    match sub {
        "submit" | "create" => {
            let Some(prize) = text(i, "prize", 200) else {
                return Ok(private("Name the prize."));
            };
            let Some(length) = text(i, "duration", 10)
                .as_deref()
                .and_then(minutes)
                .filter(|m| *m <= 30 * 1440)
            else {
                return Ok(private(
                    "A length looks like 1h, 24h, 3d or 1w (at most 30 days).",
                ));
            };
            let winners = i
                .option("winners")
                .and_then(Value::as_i64)
                .unwrap_or(1)
                .clamp(1, 20);
            let channel = i
                .option("channel")
                .and_then(Value::as_str)
                .map(String::from)
                .or_else(|| home::id(&settings, "giveaways"))
                .or(i.channel.clone());
            let review = home::id(&settings, "mod_log");
            if sub == "submit" && review.is_none() {
                return Ok(private(
                    "Giveaway review isn't set up yet. Ask staff to set the mod log.",
                ));
            }
            let id: i64 = sqlx::query_scalar("INSERT INTO discord_giveaways(prize,description,winners,minutes,channel_id,created_by) VALUES($1,$2,$3,$4,$5,$6) RETURNING id")
                .bind(&prize).bind(text(i, "description", 1000)).bind(winners as i16).bind(length as i32).bind(&channel).bind(&i.user)
                .fetch_one(&app.db).await?;
            if sub == "create" {
                return Ok(private(match start_giveaway(app, id, &i.user).await? {
                    Some(c) => format!("🎉 Giveaway #{id} is running in <#{c}>."),
                    None => "The giveaway couldn't be started.".into(),
                }));
            }
            if let (Some(review), Some(g)) = (review, giveaway(app, id).await?) {
                send(app, &review, review_post(&g)).await;
            }
            Ok(private(format!(
                "🎁 Your giveaway for **{prize}** is with staff for review. You'll get a DM when they decide."
            )))
        }
        "end" => Ok(private(if end_giveaway(app, id).await? {
            format!("Giveaway #{id} is drawn.")
        } else {
            "No running giveaway has that number.".into()
        })),
        "reroll" => {
            let Some(g) = giveaway(app, id).await?.filter(|g| g.status == "ended") else {
                return Ok(private("Only an ended giveaway can be rerolled."));
            };
            let count = i
                .option("winners")
                .and_then(Value::as_i64)
                .unwrap_or(g.winners.into())
                .clamp(1, 20) as i16;
            let winners = draw(app, id, count, &g.winner_ids).await?;
            if winners.is_empty() {
                return Ok(private("Nobody else entered."));
            }
            sqlx::query(
                "UPDATE discord_giveaways SET winner_ids=winner_ids||$2::text[] WHERE id=$1",
            )
            .bind(id)
            .bind(&winners)
            .execute(&app.db)
            .await?;
            announce_winners(app, &g, &winners, true).await;
            Ok(private(format!("Giveaway #{id} is rerolled.")))
        }
        _ => {
            let done = sqlx::query("UPDATE discord_giveaways SET status='cancelled',ended_at=now() WHERE id=$1 AND status IN ('pending','active')")
                .bind(id).execute(&app.db).await?.rows_affected();
            refresh_giveaway(app, id).await?;
            Ok(private(if done > 0 {
                format!("Giveaway #{id} is cancelled.")
            } else {
                "No pending or running giveaway has that number.".into()
            }))
        }
    }
}
async fn review(app: &App, i: &Interaction, id: i64, approve: bool) -> Res<Value> {
    if !i.staff() {
        return Ok(private("Only staff can review giveaways."));
    }
    let Some(g) = giveaway(app, id).await?.filter(|g| g.status == "pending") else {
        return Ok(update(
            json!({"content": "This giveaway was already reviewed.", "components": []}),
        ));
    };
    if !approve {
        return Ok(form(
            &format!("give:denied:{id}"),
            "Deny the giveaway",
            &[("reason", "Why (the member sees this)", true, true, 500, "")],
        ));
    }
    let Some(channel) = start_giveaway(app, id, &i.user).await? else {
        return Ok(private("This giveaway was already reviewed."));
    };
    dm(app, &g.created_by, json!({"content": format!("🎉 Your giveaway for **{}** was approved and is running in the server.", g.prize),
        "allowed_mentions": {"parse": []}})).await;
    Ok(update(
        json!({"content": format!("✅ Approved by <@{}>, running in <#{channel}>.", i.user), "components": [], "allowed_mentions": {"parse": []}}),
    ))
}
async fn denied(app: &App, i: &Interaction, id: i64) -> Res<Value> {
    if !i.staff() {
        return Ok(private("Only staff can review giveaways."));
    }
    let reason = field(i, "reason").unwrap_or_default();
    let who: Option<(String, String)> = sqlx::query_as("UPDATE discord_giveaways SET status='cancelled',reviewed_by=$2,deny_reason=$3 WHERE id=$1 AND status='pending' RETURNING created_by,prize")
        .bind(id).bind(&i.user).bind(&reason).fetch_optional(&app.db).await?;
    let Some((member, prize)) = who else {
        return Ok(update(
            json!({"content": "This giveaway was already reviewed.", "components": []}),
        ));
    };
    dm(app, &member, json!({"content": format!("Your giveaway for **{prize}** wasn't approved.\n**Why:** {reason}"),
        "allowed_mentions": {"parse": []}})).await;
    Ok(update(
        json!({"content": format!("❌ Denied by <@{}>: {reason}", i.user), "components": [], "allowed_mentions": {"parse": []}}),
    ))
}

// ---- Reminders ----

async fn remind(app: &App, i: &Interaction) -> Res<Value> {
    match i.sub() {
        Some("set") => {
            let Some(length) = text(i, "time", 10).as_deref().and_then(minutes) else {
                return Ok(private("A time looks like 30m, 2h or 1d."));
            };
            let Some(message) = text(i, "message", 500) else {
                return Ok(private("Say what to remind you about."));
            };
            let open: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM discord_reminders WHERE discord_user=$1 AND fired_at IS NULL",
            )
            .bind(&i.user)
            .fetch_one(&app.db)
            .await?;
            if open >= 25 {
                return Ok(private("You have 25 reminders waiting. Cancel one first."));
            }
            let channel = i.channel.clone().unwrap_or_default();
            let when = Utc::now() + Duration::minutes(length);
            let id: i64 = sqlx::query_scalar("INSERT INTO discord_reminders(discord_user,channel_id,message,remind_at) VALUES($1,$2,$3,$4) RETURNING id")
                .bind(&i.user).bind(channel).bind(&message).bind(when).fetch_one(&app.db).await?;
            Ok(private(format!(
                "⏰ Reminder #{id} set for <t:{}:R>:\n> {message}",
                when.timestamp()
            )))
        }
        Some("cancel") => {
            let id = i.option("id").and_then(Value::as_i64).unwrap_or(0);
            let gone = sqlx::query("DELETE FROM discord_reminders WHERE id=$1 AND discord_user=$2 AND fired_at IS NULL")
                .bind(id).bind(&i.user).execute(&app.db).await?.rows_affected();
            Ok(private(if gone > 0 {
                format!("Reminder #{id} is cancelled.")
            } else {
                format!("You have no waiting reminder #{id}.")
            }))
        }
        _ => {
            let rows: Vec<(i64, String, DateTime<Utc>)> = sqlx::query_as("SELECT id,message,remind_at FROM discord_reminders WHERE discord_user=$1 AND fired_at IS NULL ORDER BY remind_at")
                .bind(&i.user).fetch_all(&app.db).await?;
            if rows.is_empty() {
                return Ok(private("You have no reminders waiting."));
            }
            let lines: Vec<String> = rows
                .iter()
                .map(|(id, m, t)| format!("**#{id}** <t:{}:R>\n> {m}", t.timestamp()))
                .collect();
            Ok(embed(
                json!({"title": "⏰ Your reminders", "description": lines.join("\n\n"), "color": 0x34_98db}),
                true,
            ))
        }
    }
}
async fn fire_reminders(app: &App) -> Res<()> {
    let due: Vec<(String, String, String)> = sqlx::query_as("UPDATE discord_reminders SET fired_at=now() WHERE fired_at IS NULL AND remind_at<=now() RETURNING discord_user,channel_id,message")
        .fetch_all(&app.db).await?;
    for (user, channel, message) in due {
        let body = json!({"content": format!("⏰ <@{user}>, reminder: **{message}**"), "allowed_mentions": {"users": [user]}});
        // The channel may be gone or closed to the bot: a DM instead.
        if send(app, &channel, body.clone()).await.is_none() {
            dm(app, &user, body).await;
        }
    }
    Ok(())
}

// ---- Commands, buttons and timers ----

pub fn definitions() -> Vec<Value> {
    let factions = json!([{"name": "Myria", "value": "myria"}, {"name": "Aetheron", "value": "aetheron"}, {"name": "Glint", "value": "glint"}]);
    let kinds: Vec<Value> = KINDS
        .iter()
        .map(|(v, n)| json!({"name": n, "value": v}))
        .collect();
    let id = json!({"type": 4, "name": "id", "description": "Its number", "required": true, "min_value": 1});
    let string = |name: &str, description: &str, required: bool, max: u16| json!({"type": 3, "name": name, "description": description, "required": required, "max_length": max});
    let giveaway = |name: &str, description: &str, channel: bool| {
        let mut options = vec![
            string("prize", "What's given away", true, 200),
            string("duration", "How long, like 24h, 3d or 1w", true, 10),
            json!({"type": 4, "name": "winners", "description": "How many winners (1 by default)", "min_value": 1, "max_value": 20}),
            string("description", "Anything else", false, 1000),
        ];
        if channel {
            options.push(json!({"type": 7, "name": "channel", "description": "Where (the giveaways channel by default)", "channel_types": [0, 5]}));
        }
        json!({"type": 1, "name": name, "description": description, "options": options})
    };
    vec![
        json!({"name": "event", "description": "Community events with RSVPs.", "options": [
            {"type": 1, "name": "create", "description": "Plan an event", "options": [
                string("title", "What it is", true, 100),
                string("date", "The day, like 2026-10-31", true, 10),
                string("time", "Start time, like 20:00 or 8:00 PM", true, 10),
                string("zone", "Time zone, like America/New_York (the server's by default)", false, 40),
                string("description", "Details", false, 1000),
                {"type": 3, "name": "type", "description": "Kind of event", "choices": kinds},
                {"type": 3, "name": "faction", "description": "Only for one faction", "choices": factions},
                string("duration", "How long, like 2h or 90m", false, 10),
                {"type": 3, "name": "repeat", "description": "Repeat it", "choices": [
                    {"name": "Daily", "value": "daily"}, {"name": "Weekly", "value": "weekly"},
                    {"name": "Every two weeks", "value": "biweekly"}, {"name": "Monthly", "value": "monthly"}]}]},
            {"type": 1, "name": "list", "description": "Upcoming events", "options": [
                {"type": 4, "name": "page", "description": "Page", "min_value": 1}]},
            {"type": 1, "name": "cancel", "description": "Cancel an event you made", "options": [id.clone()]}]}),
        json!({"name": "poll", "description": "Ask the server a question.", "options": [
            string("question", "The question", true, 200),
            string("option1", "First option", true, 80),
            string("option2", "Second option", true, 80),
            string("option3", "Third option", false, 80),
            string("option4", "Fourth option", false, 80),
            string("duration", "How long, like 1h, 24h or 3d (open-ended by default)", false, 10)]}),
        // Members submit giveaways; the staff parts are checked when they're used.
        json!({"name": "giveaway", "description": "Giveaways (submit one for staff review).", "options": [
            giveaway("submit", "Submit a giveaway for staff review", false),
            giveaway("create", "Start a giveaway now (staff)", true),
            {"type": 1, "name": "end", "description": "Draw a giveaway now (staff)", "options": [id.clone()]},
            {"type": 1, "name": "reroll", "description": "Draw new winners (staff)", "options": [id.clone(),
                {"type": 4, "name": "winners", "description": "How many (as before by default)", "min_value": 1, "max_value": 20}]},
            {"type": 1, "name": "cancel", "description": "Cancel a giveaway (staff)", "options": [id.clone()]}]}),
        json!({"name": "remind", "description": "Reminders.", "options": [
            {"type": 1, "name": "set", "description": "Set a reminder", "options": [
                string("time", "In how long, like 30m, 2h or 1d", true, 10),
                string("message", "What to remind you about", true, 500)]},
            {"type": 1, "name": "list", "description": "Your reminders"},
            {"type": 1, "name": "cancel", "description": "Cancel a reminder", "options": [id]}]}),
    ]
}

pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(answer(match i.name() {
        "event" => events_command(app, i).await,
        "poll" => create_poll(app, i).await,
        "giveaway" => giveaway_command(app, i).await,
        "remind" => remind(app, i).await,
        _ => return None,
    }))
}

/// RSVP, vote, entry and review buttons, and the deny form.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let id = i.custom_id().to_string();
    let parts: Vec<&str> = id.split(':').collect();
    let n = |k: usize| parts.get(k).and_then(|p| p.parse::<i64>().ok());
    Some(answer(match parts.as_slice() {
        ["event", "rsvp", _, status] if ["going", "maybe", "not_going"].contains(status) => {
            rsvp(app, i, n(2)?, status).await
        }
        ["poll", "vote", _, _] => vote(app, i, n(2)?, n(3)?).await,
        ["give", "enter", _] => match giveaway(app, n(2)?).await {
            Ok(Some(g)) => enter(app, &g, &i.user).await.map(|r| {
                private(match r {
                    None => "This giveaway is closed.",
                    Some(true) => "🎉 You're in! Good luck!",
                    Some(false) if i.user == g.created_by => "You can't enter your own giveaway.",
                    Some(false) => "You're already in this giveaway.",
                })
            }),
            Ok(None) => Ok(private("This giveaway is gone.")),
            Err(e) => Err(e),
        },
        ["give", "approve", _] => review(app, i, n(2)?, true).await,
        ["give", "deny", _] => review(app, i, n(2)?, false).await,
        ["give", "denied", _] => denied(app, i, n(2)?).await,
        _ => return None,
    }))
}

/// Every few seconds: giveaways that ended are drawn, polls close and reminders fire.
pub async fn due(app: &App) -> Res<()> {
    let ended: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM discord_giveaways WHERE status='active' AND ends_at<=now() LIMIT 10",
    )
    .fetch_all(&app.db)
    .await?;
    for id in ended {
        end_giveaway(app, id).await?;
    }
    close_polls(app).await?;
    fire_reminders(app).await
}

/// Each minute: event reminders, starts, ends and repeats.
pub async fn minute(app: &App) -> Res<()> {
    event_minute(app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeats_keep_the_wall_clock() {
        let ny: Tz = "America/New_York".parse().unwrap();
        let t = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        // Weekly at 20:00 New York: EDT before the change, EST after.
        let first = t("2026-10-27T00:00:00Z");
        assert_eq!(
            next_start(first, "weekly", ny, first).unwrap().to_rfc3339(),
            "2026-11-03T01:00:00+00:00"
        );
        // Monthly from Jan 31 lands on Feb 28, then Mar 31 again.
        let jan = t("2027-01-31T17:00:00Z");
        assert_eq!(
            next_start(jan, "monthly", ny, jan).unwrap().to_rfc3339(),
            "2027-02-28T17:00:00+00:00"
        );
        assert_eq!(
            next_start(jan, "monthly", ny, t("2027-03-01T00:00:00Z"))
                .unwrap()
                .to_rfc3339(),
            "2027-03-31T16:00:00+00:00"
        );
        // Long past: the first one after now, on the 07:00 New York clock (now EDT).
        let old = t("2026-01-01T12:00:00Z");
        assert_eq!(
            next_start(old, "daily", ny, t("2026-10-20T00:00:00Z"))
                .unwrap()
                .to_rfc3339(),
            "2026-10-20T11:00:00+00:00"
        );
    }
}
