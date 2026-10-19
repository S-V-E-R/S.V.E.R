//! Tickets and modmail: a member DMs the bot (or uses /ticket) and gets a private thread in the
//! tickets channel. Their DMs land in the thread; staff replies in the thread go back by DM. Staff
//! claim, add notes, set priority and category, and close with a reason; a transcript goes to the
//! transcripts channel. A ticket nobody wrote in for 5 days gets a warning, and closes at 7.
use super::{
    Home, actions, button, dm, done, embed, field, files, form, note, post, row, update, upload,
};
use crate::{
    App,
    discord::{
        call,
        commands::{Interaction, private},
        home,
    },
    profiles::Res,
};
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde_json::{Value, json};

pub(super) const CATEGORIES: [&str; 4] = ["support", "appeal", "report", "other"];
const PRIORITIES: [&str; 4] = ["low", "normal", "high", "urgent"];
const ONE_AN_HOUR: &str =
    "You can open one ticket an hour. Please wait a little before opening another.";

#[derive(sqlx::FromRow)]
struct Ticket {
    id: i64,
    user_id: String,
    thread_id: Option<String>,
    category: String,
    priority: String,
    status: String,
    claimed_by: Option<String>,
    created_at: DateTime<Utc>,
}
const COLUMNS: &str = "id,user_id,thread_id,category,priority,status,claimed_by,created_at";
async fn ticket(app: &App, id: i64) -> Res<Option<Ticket>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM discord_tickets WHERE id=$1"
    )))
    .bind(id)
    .fetch_optional(&app.db)
    .await?)
}
async fn open_ticket(app: &App, user: &str) -> Res<Option<Ticket>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM discord_tickets WHERE user_id=$1 AND status<>'closed'"
    )))
    .bind(user)
    .fetch_optional(&app.db)
    .await?)
}
/// One new ticket an hour per member.
async fn too_soon(app: &App, user: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_tickets WHERE user_id=$1 AND created_at>now()-interval '1 hour')")
        .bind(user).fetch_one(&app.db).await?)
}
async fn record(
    app: &App,
    id: i64,
    author: &str,
    kind: &str,
    content: &str,
    attached: &[String],
) -> Res<()> {
    sqlx::query("INSERT INTO discord_ticket_messages(ticket_id,author_id,kind,content,attachments) VALUES($1,$2,$3,$4,$5)")
        .bind(id).bind(author).bind(kind).bind(content).bind(json!(attached)).execute(&app.db).await?;
    if kind != "note" {
        sqlx::query("UPDATE discord_tickets SET active_at=now(),warned_at=NULL WHERE id=$1")
            .bind(id)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}
fn with_files(text: &str, attached: &[String]) -> String {
    let text = if text.is_empty() { "(no text)" } else { text };
    if attached.is_empty() {
        text.into()
    } else {
        format!("{text}\n{}", attached.join("\n"))
    }
}
fn title(t: &Ticket) -> String {
    match t.status.as_str() {
        "closed" => format!("[CLOSED] Ticket #{}", t.id),
        "claimed" => format!("Ticket #{} [claimed]", t.id),
        _ => format!("Ticket #{}", t.id),
    }
}

/// The staff panel pinned in the thread.
fn panel(t: &Ticket) -> Value {
    let claimed = t
        .claimed_by
        .as_ref()
        .map_or("Nobody".into(), |c| format!("<@{c}>"));
    let id = t.id;
    let menu = |kind: &str, label: &str, list: &[&str], current: &str| {
        let options: Vec<Value> = list
            .iter()
            .map(
                |v| json!({"label": format!("{label}: {v}"), "value": v, "default": *v == current}),
            )
            .collect();
        row(vec![
            json!({"type": 3, "custom_id": format!("mod:ticket:{kind}:{id}"), "options": options}),
        ])
    };
    let buttons = if t.status == "closed" {
        vec![button("Reopen", 3, format!("mod:ticket:reopen:{id}"))]
    } else {
        vec![
            if t.status == "claimed" {
                button("Unclaim", 2, format!("mod:ticket:unclaim:{id}"))
            } else {
                button("Claim", 1, format!("mod:ticket:claim:{id}"))
            },
            button("Close", 4, format!("mod:ticket:close:{id}")),
            button("Note", 2, format!("mod:ticket:note:{id}")),
            button("History", 2, format!("mod:ticket:history:{id}")),
        ]
    };
    let mut components = vec![row(buttons)];
    if t.status != "closed" {
        components.push(menu("priority", "Priority", &PRIORITIES, &t.priority));
        components.push(menu("category", "Category", &CATEGORIES, &t.category));
    }
    json!({"embeds": [{"title": format!("Ticket #{id}"), "color": 0x58_65F2, "fields": [
        {"name": "Member", "value": format!("<@{}>", t.user_id), "inline": true},
        {"name": "Status", "value": t.status, "inline": true},
        {"name": "Claimed by", "value": claimed, "inline": true},
        {"name": "Category", "value": t.category, "inline": true},
        {"name": "Priority", "value": t.priority, "inline": true},
        {"name": "Opened", "value": format!("<t:{}:R>", t.created_at.timestamp()), "inline": true},
    ]}], "components": components})
}

/// Opens a ticket: the row, the private thread (the mod team is mentioned so they're added), the
/// panel, the first message, and a DM to the member. Err(why) when it can't.
async fn open(
    app: &App,
    h: &Home,
    user: &str,
    content: &str,
    attached: &[String],
    category: &str,
) -> Res<Result<i64, &'static str>> {
    let Some(channel) = home::id(&h.s, "tickets") else {
        return Ok(Err("Tickets aren't set up yet."));
    };
    let id: Option<i64> = sqlx::query_scalar("INSERT INTO discord_tickets(user_id,category) VALUES($1,$2) ON CONFLICT (user_id) WHERE status<>'closed' DO NOTHING RETURNING id")
        .bind(user).bind(category).fetch_optional(&app.db).await?;
    let Some(id) = id else {
        return Ok(Err(
            "You already have an open ticket. Message this bot to add to it.",
        ));
    };
    let thread = call(
        app,
        Method::POST,
        &format!("/channels/{channel}/threads"),
        Some(json!({"name": format!("Ticket #{id}"), "type": 12, "invitable": false, "auto_archive_duration": 10080})),
    )
    .await;
    let Some(thread) = thread
        .filter(|(status, _)| *status < 300)
        .and_then(|(_, t)| t["id"].as_str().map(String::from))
    else {
        sqlx::query("DELETE FROM discord_tickets WHERE id=$1")
            .bind(id)
            .execute(&app.db)
            .await?;
        return Ok(Err("The ticket couldn't be opened. Try again in a minute."));
    };
    sqlx::query("UPDATE discord_tickets SET thread_id=$2 WHERE id=$1")
        .bind(id)
        .bind(&thread)
        .execute(&app.db)
        .await?;
    let Some(t) = ticket(app, id).await? else {
        return Ok(Err("The ticket couldn't be opened. Try again in a minute."));
    };
    let mut body = panel(&t);
    if let Some(role) = home::id(&h.s, "mod_team") {
        body["content"] = json!(format!("<@&{role}> new {category} ticket."));
        body["allowed_mentions"] = json!({"roles": [role]});
    }
    if let Some(message) = post(app, &thread, body).await {
        done(
            app,
            Method::PUT,
            &format!("/channels/{thread}/pins/{message}"),
            None,
        )
        .await;
    }
    if category == "appeal" {
        post(app, &thread, json!({"content": "Filed as an **appeal** (the member is timed out or mentioned a timeout)."})).await;
    }
    post(
        app,
        &thread,
        json!({"content": format!("**Member:** {}", with_files(content, attached))}),
    )
    .await;
    record(app, id, user, "user", content, attached).await?;
    dm(app, user, json!({"embeds": [embed(&format!("Ticket #{id} opened"), 0x58_65F2,
        "The moderators have your message. Reply here to add to it; their answers arrive here too.")]})).await;
    note(
        app,
        h,
        "Ticket opened",
        0x58_65F2,
        format!("<@{user}> opened ticket #{id} ({category}) in <#{thread}>."),
    )
    .await;
    Ok(Ok(id))
}

/// A DM to the bot: added to the member's open ticket, or opens one.
pub(super) async fn direct(app: &App, h: &Home, d: &Value) -> Res<()> {
    let Some(user) = d["author"]["id"].as_str() else {
        return Ok(());
    };
    if d["author"]["bot"] == true || home::id(&h.s, "tickets").is_none() {
        return Ok(());
    }
    let content = d["content"].as_str().unwrap_or_default();
    let attached = files(d);
    if let Some(t) = open_ticket(app, user).await? {
        if let Some(thread) = &t.thread_id {
            post(
                app,
                thread,
                json!({"content": format!("**Member:** {}", with_files(content, &attached))}),
            )
            .await;
        }
        return record(app, t.id, user, "user", content, &attached).await;
    }
    // Only members of the server can open tickets.
    let Some(m) = actions::member(app, h, user).await else {
        return Ok(());
    };
    if too_soon(app, user).await? {
        dm(app, user, json!({"content": ONE_AN_HOUR})).await;
        return Ok(());
    }
    let timed_out = m["communication_disabled_until"]
        .as_str()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| t.to_utc() > Utc::now());
    let lower = content.to_lowercase();
    let appeal = timed_out
        || ["appeal", "timeout", "timed out"]
            .iter()
            .any(|w| lower.contains(w));
    let category = if appeal { "appeal" } else { "support" };
    if let Err(why) = open(app, h, user, content, &attached, category).await? {
        dm(app, user, json!({"content": why})).await;
    }
    Ok(())
}

/// A staff message in a ticket thread goes to the member by DM.
pub(super) async fn staff_reply(
    app: &App,
    d: &Value,
    author: &str,
    channel: &str,
    content: &str,
) -> Res<()> {
    let row: Option<(i64, String, bool)> = sqlx::query_as(
        "SELECT id,user_id,dm_blocked FROM discord_tickets WHERE thread_id=$1 AND status<>'closed'",
    )
    .bind(channel)
    .fetch_optional(&app.db)
    .await?;
    let Some((id, user, blocked)) = row else {
        return Ok(());
    };
    let attached = files(d);
    record(app, id, author, "staff", content, &attached).await?;
    if !blocked
        && !dm(
            app,
            &user,
            json!({"content": format!("**Staff:** {}", with_files(content, &attached))}),
        )
        .await
    {
        sqlx::query("UPDATE discord_tickets SET dm_blocked=true WHERE id=$1")
            .bind(id)
            .execute(&app.db)
            .await?;
        post(
            app,
            channel,
            json!({"content": "The member has DMs closed, so they won't see replies here."}),
        )
        .await;
    }
    Ok(())
}

/// /ticket subject [category].
pub(super) async fn command(app: &App, h: &Home, i: &Interaction) -> Res<Value> {
    if home::id(&h.s, "tickets").is_none() {
        return Ok(private("Tickets aren't set up yet. Ask a moderator."));
    }
    if let Some(t) = open_ticket(app, &i.user).await? {
        return Ok(private(format!(
            "You already have ticket #{} open. Message this bot to add to it.",
            t.id
        )));
    }
    if too_soon(app, &i.user).await? {
        return Ok(private(ONE_AN_HOUR));
    }
    let category = i
        .option("category")
        .and_then(Value::as_str)
        .filter(|c| CATEGORIES.contains(c))
        .unwrap_or("support");
    let subject = i
        .option("subject")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(private(
        match open(app, h, &i.user, subject, &[], category).await? {
            Ok(id) => format!(
                "Ticket #{id} is open. The moderators will answer by DM, so keep DMs from this server on."
            ),
            Err(why) => why.into(),
        },
    ))
}

/// Closes a ticket: archived and locked, the member told, the transcript posted.
async fn close(app: &App, h: &Home, id: i64, by: Option<&str>, reason: &str) -> Res<bool> {
    let closed = sqlx::query("UPDATE discord_tickets SET status='closed',closed_at=now(),closed_by=$2,close_reason=$3 WHERE id=$1 AND status<>'closed'")
        .bind(id).bind(by).bind(reason).execute(&app.db).await?.rows_affected() > 0;
    let Some(t) = ticket(app, id).await?.filter(|_| closed) else {
        return Ok(false);
    };
    if let Some(thread) = &t.thread_id {
        done(
            app,
            Method::PATCH,
            &format!("/channels/{thread}"),
            Some(json!({"name": title(&t), "archived": true, "locked": true})),
        )
        .await;
    }
    dm(
        app,
        &t.user_id,
        json!({"embeds": [embed(&format!("Ticket #{id} closed"), 0x95_A5A6,
        format!("**Reason:** {reason}\nMessage this bot again if you need anything else."))]}),
    )
    .await;
    transcript(app, h, &t, by, reason).await?;
    note(
        app,
        h,
        "Ticket closed",
        0x95_A5A6,
        format!(
            "Ticket #{id} closed {}: {reason}",
            by.map_or("automatically".into(), |b| format!("by <@{b}>"))
        ),
    )
    .await;
    Ok(true)
}

async fn transcript(app: &App, h: &Home, t: &Ticket, by: Option<&str>, reason: &str) -> Res<()> {
    let Some(channel) = home::id(&h.s, "transcripts") else {
        return Ok(());
    };
    let lines: Vec<(String, String, String, Value, DateTime<Utc>)> = sqlx::query_as("SELECT author_id,kind,content,attachments,created_at FROM discord_ticket_messages WHERE ticket_id=$1 ORDER BY id")
        .bind(t.id).fetch_all(&app.db).await?;
    let mut text = format!(
        "Ticket #{} transcript\nMember: {}\nOpened: {}\nClosed: {}\nCategory: {}\nPriority: {}\nReason: {reason}\n\n--- Messages ---\n\n",
        t.id,
        t.user_id,
        t.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        t.category,
        t.priority
    );
    for (author, kind, content, attached, at) in &lines {
        let label = match kind.as_str() {
            "note" => "NOTE (staff only)",
            "staff" => "STAFF",
            _ => "MEMBER",
        };
        text.push_str(&format!(
            "[{}] {label} {author}: {content}\n",
            at.format("%Y-%m-%d %H:%M:%S")
        ));
        if let Some(a) = attached.as_array().filter(|a| !a.is_empty()) {
            let urls: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            text.push_str(&format!("  Attachments: {}\n", urls.join(", ")));
        }
    }
    let by = by.map_or("automatically".into(), |b| format!("by <@{b}>"));
    let body = json!({"embeds": [embed(&format!("Ticket #{} transcript", t.id), 0x95_A5A6, format!(
        "**Member:** <@{}>\n**Closed:** {by}\n**Reason:** {reason}\n**Messages:** {}", t.user_id, lines.len()))]});
    upload(
        app,
        &channel,
        body,
        &format!("ticket-{}-transcript.txt", t.id),
        &text,
    )
    .await;
    Ok(())
}

/// The panel's buttons, menus and forms.
pub(super) async fn component(
    app: &App,
    h: &Home,
    i: &Interaction,
    action: &str,
    target: &str,
) -> Res<Value> {
    let id: i64 = target.parse().unwrap_or(0);
    let Some(t) = ticket(app, id).await? else {
        return Ok(private("That ticket doesn't exist."));
    };
    let by = i.user.as_str();
    let refreshed = async || -> Res<Value> {
        let Some(t) = ticket(app, id).await? else {
            return Ok(private("That ticket doesn't exist."));
        };
        if let Some(thread) = &t.thread_id {
            done(
                app,
                Method::PATCH,
                &format!("/channels/{thread}"),
                Some(json!({"name": title(&t)})),
            )
            .await;
        }
        Ok(update(panel(&t)))
    };
    Ok(match action {
        "claim" | "unclaim" => {
            let changed = sqlx::query("UPDATE discord_tickets SET status=CASE WHEN $2 THEN 'claimed' ELSE 'open' END,claimed_by=CASE WHEN $2 THEN $3 END WHERE id=$1 AND status<>'closed'")
                .bind(id).bind(action == "claim").bind(by).execute(&app.db).await?.rows_affected();
            if changed == 0 {
                return Ok(private("That ticket is closed."));
            }
            refreshed().await?
        }
        "priority" | "category" => {
            let choice = i.data["values"][0].as_str().unwrap_or_default();
            let (allowed, sql): (&[&str], &str) = if action == "priority" {
                (
                    &PRIORITIES,
                    "UPDATE discord_tickets SET priority=$2 WHERE id=$1",
                )
            } else {
                (
                    &CATEGORIES,
                    "UPDATE discord_tickets SET category=$2 WHERE id=$1",
                )
            };
            if !allowed.contains(&choice) {
                return Ok(private("Pick one of the options."));
            }
            sqlx::query(sql)
                .bind(id)
                .bind(choice)
                .execute(&app.db)
                .await?;
            refreshed().await?
        }
        "close" => form(
            format!("mod:ticket:closing:{id}"),
            &format!("Close ticket #{id}"),
            &[("reason", "Reason for closing", true, true)],
        ),
        "closing" => {
            let reason = field(i, "reason").unwrap_or("No reason given");
            if close(app, h, id, Some(by), reason).await? {
                private(format!("Ticket #{id} closed."))
            } else {
                private("That ticket is already closed.")
            }
        }
        "reopen" => {
            let reopened = sqlx::query("UPDATE discord_tickets SET status='open',closed_at=NULL,closed_by=NULL,close_reason=NULL,claimed_by=NULL,active_at=now(),warned_at=NULL
                WHERE id=$1 AND status='closed' AND NOT EXISTS(SELECT 1 FROM discord_tickets o WHERE o.user_id=$2 AND o.status<>'closed')")
                .bind(id).bind(&t.user_id).execute(&app.db).await?.rows_affected();
            if reopened == 0 {
                return Ok(private(
                    "It's open already, or the member has another ticket open.",
                ));
            }
            if let Some(thread) = &t.thread_id {
                done(
                    app,
                    Method::PATCH,
                    &format!("/channels/{thread}"),
                    Some(json!({"archived": false, "locked": false})),
                )
                .await;
            }
            refreshed().await?
        }
        "note" => form(
            format!("mod:ticket:noting:{id}"),
            &format!("Note on ticket #{id}"),
            &[(
                "note",
                "Internal note (the member never sees it)",
                true,
                true,
            )],
        ),
        "noting" => {
            let text = field(i, "note").unwrap_or_default();
            record(app, id, by, "note", text, &[]).await?;
            private(format!("Note added to ticket #{id}. Only staff see it."))
        }
        "history" => {
            let past: Vec<(i64, String, String, DateTime<Utc>)> = sqlx::query_as("SELECT id,status,category,created_at FROM discord_tickets WHERE user_id=$1 ORDER BY id DESC LIMIT 15")
                .bind(&t.user_id).fetch_all(&app.db).await?;
            let lines: Vec<String> = past
                .iter()
                .map(|(n, status, category, at)| {
                    format!("**#{n}** {category}, {status} (<t:{}:R>)", at.timestamp())
                })
                .collect();
            json!({"type": 4, "data": {"flags": 64, "embeds": [embed("Ticket history", 0x58_65F2,
                format!("<@{}>\n{}", t.user_id, lines.join("\n")))], "allowed_mentions": {"parse": []}}})
        }
        _ => private("That button isn't in use anymore."),
    })
}

/// Inactivity: a warning after 5 days without a message from anyone, closed after 7.
pub(super) async fn tick(app: &App, h: &Home) -> Res<()> {
    let warn: Vec<(i64, String)> = sqlx::query_as("UPDATE discord_tickets SET warned_at=now() WHERE status<>'closed' AND warned_at IS NULL AND active_at<now()-interval '5 days' RETURNING id,user_id")
        .fetch_all(&app.db).await?;
    for (id, user) in warn {
        dm(app, &user, json!({"embeds": [embed(&format!("Ticket #{id} is quiet"), 0xF3_9C12,
            "Nobody has written in this ticket for 5 days. It closes in 2 days unless someone replies.")]})).await;
    }
    let stale: Vec<i64> = sqlx::query_scalar("SELECT id FROM discord_tickets WHERE status<>'closed' AND warned_at IS NOT NULL AND active_at<now()-interval '7 days'")
        .fetch_all(&app.db).await?;
    for id in stale {
        close(app, h, id, None, "Closed after 7 days without activity").await?;
    }
    Ok(())
}
