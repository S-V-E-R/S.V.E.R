//! Automod, the message evidence and the safety layer. Automod runs its checks in the legacy order
//! (media from new or unverified members, media quarantine, NSFW, mass spam, spam, blocked links,
//! caps, duplicates, banned words); a hit deletes the message, records a strike, logs it and checks
//! escalation. Each rate check counts its own window over the stored messages, so the duplicate
//! window no longer collapses to the spam window.
use super::{Home, actions, clip, created, done, files, note, post, tickets, upload};
use crate::{App, discord::home, profiles::Res};
use chrono::{Duration, Utc};
use reqwest::Method;
use serde_json::{Value, json};

const WARN: u32 = 0xE6_7E22;
const SAFETY: u32 = 0xF3_9C12;

/// A message someone wrote (a plain message or a reply), not a system notice such as "pinned a
/// message" or a join. An edit without its type is taken as written.
fn written(d: &Value) -> bool {
    matches!(d["type"].as_u64(), None | Some(0 | 19))
}

/// A new message on the home server: kept as evidence, then the ban trap, automod, the safety
/// layer and ticket replies. True when it was removed.
pub(super) async fn message(app: &App, h: &Home, d: &Value) -> Res<bool> {
    let (Some(id), Some(channel), Some(author)) = (
        d["id"].as_str(),
        d["channel_id"].as_str(),
        d["author"]["id"].as_str(),
    ) else {
        return Ok(false);
    };
    if d["author"]["bot"] == true || !d["webhook_id"].is_null() || !written(d) {
        return Ok(false);
    }
    let content = d["content"].as_str().unwrap_or_default();
    sqlx::query("INSERT INTO discord_messages(id,channel_id,author_id,content,attachments) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(id).bind(channel).bind(author).bind(content).bind(json!(files(d))).execute(&app.db).await?;
    if home::id(&h.s, "ban_trap").as_deref() == Some(channel) {
        actions::trapped(app, h, channel, id, author).await?;
        return Ok(true);
    }
    if let Some(hit) = check(app, h, d, author, content, false).await? {
        punish(app, h, d, author, hit).await?;
        return Ok(true);
    }
    safety(app, h, d, author, channel, content).await?;
    tickets::staff_reply(app, d, author, channel, content).await?;
    Ok(false)
}

/// An edited message: logged with its old text, then scanned again.
pub(super) async fn edited(app: &App, h: &Home, d: &Value) -> Res<()> {
    let (Some(id), Some(content)) = (d["id"].as_str(), d["content"].as_str()) else {
        return Ok(());
    };
    // The stored text before the edit, and who wrote it.
    let before: Option<(String, String)> = sqlx::query_as("WITH old AS (SELECT content,author_id FROM discord_messages WHERE id=$1 FOR UPDATE)
        UPDATE discord_messages m SET content=$2,edited_at=now() FROM old WHERE m.id=$1 RETURNING old.content,old.author_id")
        .bind(id).bind(content).fetch_optional(&app.db).await?;
    let author = d["author"]["id"]
        .as_str()
        .map(String::from)
        .or_else(|| before.as_ref().map(|b| b.1.clone()));
    let Some(author) = author else {
        return Ok(());
    };
    // Bots, and updates that only add link previews, aren't edits.
    if d["author"]["bot"] == true || !written(d) || before.as_ref().is_some_and(|b| b.0 == content)
    {
        return Ok(());
    }
    let channel = d["channel_id"].as_str().unwrap_or_default();
    note(app, h, "Message edited", 0x34_98DB, format!(
        "**Member:** <@{author}>\n**Channel:** <#{channel}>\n**Before:**\n```{}```\n**After:**\n```{}```",
        before.map_or("[not kept]".into(), |b| clip(&b.0, 900)), clip(content, 900))).await;
    if let Some(hit) = check(app, h, d, &author, content, true).await? {
        punish(app, h, d, &author, hit).await?;
    }
    Ok(())
}

/// Deleted messages: kept 30 days as evidence and logged, unless the bot deleted them itself
/// (automod, purge), which logged them already.
pub(super) async fn deleted(app: &App, h: &Home, ids: &[Value], bulk: bool) -> Res<()> {
    let ids: Vec<&str> = ids.iter().filter_map(Value::as_str).collect();
    let rows: Vec<(String, String, String, Value)> = sqlx::query_as("UPDATE discord_messages SET deleted_at=now() WHERE id=ANY($1) AND deleted_at IS NULL RETURNING author_id,channel_id,content,attachments")
        .bind(&ids).fetch_all(&app.db).await?;
    let list = |a: &Value| -> Vec<String> {
        a.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|u| u.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    match rows.as_slice() {
        [] => {}
        [(author, channel, content, attachments)] if !bulk => {
            let files = list(attachments);
            let files = if files.is_empty() {
                String::new()
            } else {
                format!("\n**Attachments:**\n{}", files.join("\n"))
            };
            note(
                app,
                h,
                "Message deleted",
                0x95_A5A6,
                format!(
                    "**Member:** <@{author}>\n**Channel:** <#{channel}>\n```{}```{files}",
                    clip(content, 1500)
                ),
            )
            .await;
        }
        _ => {
            let mut people: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
            people.sort_unstable();
            people.dedup();
            let transcript: Vec<String> = rows
                .iter()
                .enumerate()
                .map(|(n, (author, channel, content, attachments))| {
                    format!(
                        "[{}] {author} in {channel}\n{content}\nAttachments: {}\n",
                        n + 1,
                        list(attachments).join(", ")
                    )
                })
                .collect();
            if let Some(to) = home::id(&h.s, "mod_log") {
                let body = json!({"embeds": [super::embed("Messages deleted", 0x95_A5A6, format!(
                    "**Messages:** {}\n**Members:** {}\nThe deleted messages are attached.", rows.len(), people.len()))]});
                upload(
                    app,
                    &to,
                    body,
                    "deleted-messages.txt",
                    &transcript.join("\n"),
                )
                .await;
            }
        }
    }
    Ok(())
}

/// What automod found.
enum Hit {
    Rule {
        rule: &'static str,
        detail: String,
        restrict: bool,
    },
    MassSpam(i64),
}
fn rule(rule: &'static str, detail: impl Into<String>, restrict: bool) -> Option<Hit> {
    Some(Hit::Rule {
        rule,
        detail: detail.into(),
        restrict,
    })
}

/// Messages by `author` in the last `seconds` (the new one included), optionally only this text.
async fn recent(app: &App, author: &str, seconds: i64, same: Option<&str>) -> Res<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM discord_messages WHERE author_id=$1 AND created_at>now()-make_interval(secs=>$2) AND ($3::text IS NULL OR content=$3)")
        .bind(author).bind(seconds as f64).bind(same).fetch_one(&app.db).await?)
}

/// `word` as a whole word or phrase in `text` (both lowercase).
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}
fn urls(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .map(|w| w.trim_start_matches('<').trim_end_matches('>'))
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
}
/// A link to one of the allowed sites (or a subdomain of one).
fn allowed(url: &str, sites: &[String]) -> bool {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
        .unwrap_or_default();
    sites.iter().any(|s| {
        let s = s
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        host == s || host.ends_with(&format!(".{s}"))
    })
}
fn media(file: &Value) -> bool {
    let kind = file["content_type"].as_str().unwrap_or_default();
    let name = file["filename"].as_str().unwrap_or_default().to_lowercase();
    kind.starts_with("image/")
        || kind.starts_with("video/")
        || [
            ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tiff", ".svg", ".mp4", ".webm",
            ".mov", ".avi", ".mkv", ".wmv", ".flv",
        ]
        .iter()
        .any(|e| name.ends_with(e))
}
/// The share of ASCII letters that are capitals, in percent.
fn caps(text: &str) -> i64 {
    let letters: Vec<char> = text.chars().filter(char::is_ascii_alphabetic).collect();
    if letters.is_empty() {
        return 0;
    }
    (letters.iter().filter(|c| c.is_ascii_uppercase()).count() * 100 / letters.len()) as i64
}

/// Automod's checks in order. An edit skips the rate and attachment checks.
async fn check(
    app: &App,
    h: &Home,
    d: &Value,
    author: &str,
    content: &str,
    edit: bool,
) -> Res<Option<Hit>> {
    let s = &h.s;
    if !home::on(s, "automod") {
        return Ok(None);
    }
    let roles: Vec<String> = d["member"]["roles"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|r| r.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if home::ids(s, "immune").iter().any(|r| roles.contains(r)) {
        return Ok(None);
    }
    let lower = content.to_lowercase();
    let attached = d["attachments"].as_array().cloned().unwrap_or_default();
    let now = Utc::now();

    if !edit && !attached.is_empty() {
        let new = home::number(s, "new_account_days")
            .zip(created(author))
            .is_some_and(|(days, at)| now - at < Duration::days(days));
        let unverified = home::id(s, "community").is_some_and(|c| !roles.contains(&c));
        if new || unverified {
            return Ok(rule(
                "Unverified media",
                format!(
                    "{} attachment(s) from a {} member",
                    attached.len(),
                    if new { "new" } else { "unverified" }
                ),
                true,
            ));
        }
        let joined = d["member"]["joined_at"]
            .as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok());
        if let (Some(hours), Some(joined)) = (home::number(s, "quarantine_hours"), joined)
            && attached.iter().any(media)
            && now - joined.to_utc() < Duration::hours(hours)
        {
            return Ok(rule(
                "Media quarantine",
                format!(
                    "Media from a member who joined {}h ago (quarantine: {hours}h)",
                    (now - joined.to_utc()).num_hours()
                ),
                true,
            ));
        }
    }

    if home::on(s, "nsfw") {
        let words = home::words(s, "nsfw_words");
        let sites = home::words(s, "nsfw_sites");
        let explicit = |text: &str| {
            let text = text.to_lowercase();
            words.iter().any(|w| has_word(&text, w))
        };
        let explicit_site = |url: &str| {
            let url = url.to_lowercase();
            sites.iter().any(|s| url.contains(s.as_str()))
        };
        if explicit(content) {
            return Ok(rule("NSFW content", "Explicit language", true));
        }
        if let Some(url) = urls(content).find(|u| explicit_site(u)) {
            return Ok(rule("NSFW link", format!("Explicit site: {url}"), true));
        }
        let name = |f: &Value| f["filename"].as_str().unwrap_or_default().to_string();
        if let Some(f) = attached.iter().find(|f| explicit(&name(f))) {
            return Ok(rule(
                "NSFW attachment",
                format!("Suspicious file name: {}", name(f)),
                true,
            ));
        }
        // Discord spoilers media it flagged as explicit.
        if let Some(f) = attached.iter().find(|f| {
            name(f).starts_with("SPOILER_") && media(f) && f["flags"].as_u64().unwrap_or(0) != 0
        }) {
            return Ok(rule(
                "NSFW flagged media",
                format!("Discord-flagged attachment: {}", name(f)),
                true,
            ));
        }
        let embeds = d["embeds"].as_array().cloned().unwrap_or_default();
        if embeds.iter().any(|e| {
            [&e["url"], &e["image"]["url"], &e["thumbnail"]["url"]]
                .iter()
                .any(|u| explicit_site(u.as_str().unwrap_or_default()))
                || explicit(e["title"].as_str().unwrap_or_default())
                || explicit(e["description"].as_str().unwrap_or_default())
        }) {
            return Ok(rule(
                "NSFW embed",
                "Embedded content from an explicit source",
                true,
            ));
        }
    }

    if !edit {
        if let (Some(count), Some(seconds)) = (
            home::number(s, "mass_spam_count"),
            home::number(s, "mass_spam_seconds"),
        ) {
            let sent = recent(app, author, seconds, None).await?;
            if sent >= count {
                return Ok(Some(Hit::MassSpam(sent)));
            }
        }
        if let (Some(count), Some(seconds)) = (
            home::number(s, "spam_count"),
            home::number(s, "spam_seconds"),
        ) {
            let sent = recent(app, author, seconds, None).await?;
            if sent >= count {
                return Ok(rule(
                    "Spam",
                    format!("{sent} messages in {seconds}s"),
                    false,
                ));
            }
        }
    }

    if home::opted(s, "link_block") {
        let sites = home::words(s, "allowed_links");
        if let Some(url) = urls(content).find(|u| !allowed(u, &sites)) {
            return Ok(rule("Blocked link", url.to_string(), false));
        }
    }

    // The first all-caps message in five minutes is allowed.
    if let (Some(percent), Some(length)) = (
        home::number(s, "caps_percent"),
        home::number(s, "caps_length"),
    ) && content.chars().count() as i64 >= length
        && caps(content) >= percent
    {
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM discord_signals WHERE kind='caps' AND user_id=$1 AND created_at>now()-interval '5 minutes'")
            .bind(author).fetch_one(&app.db).await?;
        sqlx::query("INSERT INTO discord_signals(kind,user_id) VALUES('caps',$1)")
            .bind(author)
            .execute(&app.db)
            .await?;
        if before >= 1 {
            return Ok(rule(
                "Excessive caps",
                format!("{}% caps", caps(content)),
                false,
            ));
        }
    }

    if !edit
        && !content.is_empty()
        && let (Some(count), Some(seconds)) = (
            home::number(s, "dupes_count"),
            home::number(s, "dupes_seconds"),
        )
    {
        let same = recent(app, author, seconds, Some(content)).await?;
        if same >= count {
            return Ok(rule(
                "Duplicate messages",
                format!("Same message {same}x"),
                false,
            ));
        }
    }

    if let Some(word) = home::words(s, "banned_words")
        .into_iter()
        .find(|w| lower.contains(w.as_str()))
    {
        return Ok(rule("Banned word", format!("Matched \"{word}\""), false));
    }
    Ok(None)
}

/// Deletes a message the bot removes itself, marking it so its delete event isn't logged twice.
async fn remove(app: &App, channel: &str, id: &str) -> Res<()> {
    sqlx::query("UPDATE discord_messages SET deleted_at=coalesce(deleted_at,now()) WHERE id=$1")
        .bind(id)
        .execute(&app.db)
        .await?;
    done(
        app,
        Method::DELETE,
        &format!("/channels/{channel}/messages/{id}"),
        None,
    )
    .await;
    Ok(())
}

async fn punish(app: &App, h: &Home, d: &Value, author: &str, hit: Hit) -> Res<()> {
    let (id, channel) = (
        d["id"].as_str().unwrap_or_default(),
        d["channel_id"].as_str().unwrap_or_default(),
    );
    let content = d["content"].as_str().unwrap_or_default();
    match hit {
        Hit::Rule {
            rule,
            detail,
            restrict,
        } => {
            remove(app, channel, id).await?;
            let reason = format!("Automod: {rule} ({detail})");
            let strikes = actions::warn_case(app, author, None, &reason).await?;
            note(
                app,
                h,
                "Automod",
                WARN,
                format!(
                    "**Member:** <@{author}>\n**Rule:** {rule}\n**Detail:** {detail}\n**Strikes:** {strikes}\n**Message:**\n```{}```",
                    clip(content, 1000)
                ),
            )
            .await;
            if restrict {
                actions::restrict(app, h, author).await;
            }
            actions::escalate(app, h, author, strikes).await?;
        }
        Hit::MassSpam(count) => {
            // Everything they sent in the last minute goes, then a 24-hour timeout for review.
            let gone: Vec<(String, String, String)> = sqlx::query_as("UPDATE discord_messages SET deleted_at=now() WHERE author_id=$1 AND created_at>now()-interval '60 seconds' AND deleted_at IS NULL RETURNING id,channel_id,content")
                .bind(author).fetch_all(&app.db).await?;
            let mut channels: Vec<&str> = gone.iter().map(|g| g.1.as_str()).collect();
            channels.sort_unstable();
            channels.dedup();
            for c in channels {
                let ids: Vec<&str> = gone
                    .iter()
                    .filter(|g| g.1 == c)
                    .map(|g| g.0.as_str())
                    .collect();
                for chunk in ids.chunks(100) {
                    match chunk {
                        [one] => {
                            done(
                                app,
                                Method::DELETE,
                                &format!("/channels/{c}/messages/{one}"),
                                None,
                            )
                            .await
                        }
                        _ => {
                            done(
                                app,
                                Method::POST,
                                &format!("/channels/{c}/messages/bulk-delete"),
                                Some(json!({"messages": chunk})),
                            )
                            .await
                        }
                    };
                }
            }
            let outcome = match actions::quarantine(app, h, author).await? {
                None => "Timed out for 24 hours, pending review".to_string(),
                Some(why) => format!("Not timed out: {why}"),
            };
            let samples: Vec<String> = gone
                .iter()
                .take(2)
                .map(|g| format!("> {}", clip(&g.2, 100)))
                .collect();
            note(
                app,
                h,
                "Mass spam: action taken",
                0xFF_0000,
                format!(
                    "**Member:** <@{author}>\n**Messages:** {count} in {}s\n**Action:** {outcome}\n**Deleted:** {}\n{}",
                    home::number(&h.s, "mass_spam_seconds").unwrap_or_default(),
                    gone.len(),
                    samples.join("\n")
                ),
            )
            .await;
        }
    }
    Ok(())
}

/// Members a message replies to or mentions (not bots, not the author).
fn targets(d: &Value, author: &str) -> Vec<String> {
    let mut out: Vec<String> = d["mentions"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(Some(&d["referenced_message"]["author"]))
        .filter(|u| u["bot"] != true)
        .filter_map(|u| u["id"].as_str())
        .filter(|id| *id != author)
        .map(String::from)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The different people who mentioned or replied to `target` in the window, the first time they
/// reach `people` in it.
async fn crowd(
    app: &App,
    target: &str,
    alert: &str,
    people: i64,
    seconds: i64,
) -> Res<Option<Vec<String>>> {
    let who: Vec<String> = sqlx::query_scalar("SELECT DISTINCT user_id FROM discord_signals WHERE kind='mention' AND target_id=$1 AND created_at>now()-make_interval(secs=>$2)")
        .bind(target).bind(seconds as f64).fetch_all(&app.db).await?;
    if (who.len() as i64) < people {
        return Ok(None);
    }
    let fresh: bool = sqlx::query_scalar("INSERT INTO discord_signals(kind,user_id,target_id) SELECT $1,'',$2 WHERE NOT EXISTS(SELECT 1 FROM discord_signals WHERE kind=$1 AND target_id=$2 AND created_at>now()-make_interval(secs=>$3)) RETURNING true")
        .bind(alert).bind(target).bind(seconds as f64).fetch_optional(&app.db).await?.unwrap_or(false);
    Ok(fresh.then_some(who))
}

/// The safety layer: alerts for staff only, each off until staff turn it on.
async fn safety(
    app: &App,
    h: &Home,
    d: &Value,
    author: &str,
    channel: &str,
    content: &str,
) -> Res<()> {
    let s = &h.s;
    let lower = content.to_lowercase();
    let link = format!(
        "https://discord.com/channels/{}/{channel}/{}",
        h.guild,
        d["id"].as_str().unwrap_or_default()
    );

    if home::opted(s, "nudges")
        && let (Some(hours), Some(needed)) = (
            home::number(s, "nudge_hours"),
            home::number(s, "nudge_count"),
        )
        && let Some(word) = home::words(s, "nudge_words")
            .into_iter()
            .find(|w| lower.contains(w.as_str()))
    {
        let hours = hours as i32;
        sqlx::query("INSERT INTO discord_signals(kind,user_id,target_id) VALUES('nudge',$1,$2)")
            .bind(author)
            .bind(&word)
            .execute(&app.db)
            .await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM discord_signals WHERE kind='nudge' AND user_id=$1 AND created_at>now()-make_interval(hours=>$2)")
            .bind(author).bind(hours).fetch_one(&app.db).await?;
        if count >= needed {
            let fresh: bool = sqlx::query_scalar("INSERT INTO discord_signals(kind,user_id) SELECT 'nudge-alert',$1 WHERE NOT EXISTS(SELECT 1 FROM discord_signals WHERE kind='nudge-alert' AND user_id=$1 AND created_at>now()-make_interval(hours=>$2)) RETURNING true")
                .bind(author).bind(hours).fetch_optional(&app.db).await?.unwrap_or(false);
            if fresh {
                note(app, h, "Language alert", SAFETY, format!(
                    "<@{author}> used flagged language in <#{channel}>.\n**Flagged:** `{word}`\n**Incidents:** {count} in the last {hours}h\n[Jump to message]({link})")).await;
            }
        }
    }

    let dogpile = home::opted(s, "dogpile");
    let warning = home::opted(s, "early_warning");
    if dogpile || warning {
        for target in targets(d, author) {
            sqlx::query(
                "INSERT INTO discord_signals(kind,user_id,target_id) VALUES('mention',$1,$2)",
            )
            .bind(author)
            .bind(&target)
            .execute(&app.db)
            .await?;
            let windows = [
                (
                    dogpile,
                    "dogpile-alert",
                    home::number(s, "dogpile_users"),
                    home::number(s, "dogpile_seconds"),
                    "Dogpile",
                ),
                (
                    warning,
                    "targeting-alert",
                    home::number(s, "targeting_users"),
                    home::number(s, "targeting_minutes").map(|m| m * 60),
                    "Member targeted",
                ),
            ];
            for (on, alert, people, seconds, title) in windows {
                let (true, Some(people), Some(seconds)) = (on, people, seconds) else {
                    continue;
                };
                if let Some(who) = crowd(app, &target, alert, people, seconds).await? {
                    let who: Vec<String> = who.iter().map(|u| format!("<@{u}>")).collect();
                    note(app, h, title, SAFETY, format!(
                        "<@{target}> is being replied to or mentioned by {} different people in <#{channel}> within {seconds}s.\n**People:** {}\n[Jump to message]({link})",
                        who.len(), who.join(", "))).await;
                }
            }
        }
    }

    if warning {
        let age = created(author).map(|c| Utc::now() - c);
        if let (Some(days), Some(age)) = (home::number(s, "aggression_days"), age)
            && age < Duration::days(days)
            && let Some(word) = home::words(s, "aggression_words")
                .into_iter()
                .find(|w| lower.contains(w.as_str()))
        {
            note(app, h, "New account aggression", 0xE7_4C3C, format!(
                "<@{author}> (account {} days old) used flagged language in <#{channel}>.\n**Flagged:** `{word}`\n[Jump to message]({link})",
                age.num_days())).await;
        }
        if home::words(s, "distress_words")
            .iter()
            .any(|w| lower.contains(w.as_str()))
        {
            note(app, h, "Distress signal", 0xF1_C40F, format!(
                "<@{author}> may need support (<#{channel}>).\n[Jump to message, and review it discreetly]({link})\nDon't acknowledge this publicly; reach out privately if it's right to.")).await;
        }
    }

    if home::opted(s, "support_guard") {
        let mut support = home::ids(s, "support");
        support.extend(home::id(s, "stream_help"));
        if support.iter().any(|c| c == channel) {
            match d["message_reference"]["message_id"].as_str() {
                Some(asked) => {
                    sqlx::query("UPDATE discord_questions SET answered_at=now() WHERE message_id=$1 AND answered_at IS NULL")
                        .bind(asked).execute(&app.db).await?;
                }
                None => {
                    sqlx::query("INSERT INTO discord_questions(message_id,channel_id,user_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                        .bind(d["id"].as_str()).bind(channel).bind(author).execute(&app.db).await?;
                }
            }
        }
    }
    Ok(())
}

/// Unanswered support questions, and what's past keeping: messages after a day (30 days once
/// deleted), joins after a day, signals and questions after 30 days.
pub(super) async fn tick(app: &App, h: &Home) -> Res<()> {
    if home::opted(&h.s, "support_guard")
        && let Some(minutes) = home::number(&h.s, "support_minutes")
    {
        let due: Vec<(String, String, String)> = sqlx::query_as("UPDATE discord_questions SET alerted_at=now() WHERE answered_at IS NULL AND alerted_at IS NULL AND asked_at<now()-make_interval(mins=>$1) RETURNING message_id,channel_id,user_id")
            .bind(minutes as i32).fetch_all(&app.db).await?;
        for (message, channel, user) in due {
            note(app, h, "Unanswered support question", SAFETY, format!(
                "<@{user}> asked in <#{channel}> over {minutes} minutes ago and nobody has replied.\n[Jump to message](https://discord.com/channels/{}/{channel}/{message})", h.guild)).await;
            if let Some(role) = home::id(&h.s, "mentor_role") {
                post(
                    app,
                    &channel,
                    json!({"content": format!("<@&{role}> there's an unanswered question here."),
                    "allowed_mentions": {"roles": [role]}}),
                )
                .await;
            }
        }
    }
    // ponytail: a bounded delete each tick; a nightly job if these tables ever grow large.
    for sql in [
        "DELETE FROM discord_messages WHERE id IN (SELECT id FROM discord_messages WHERE (deleted_at IS NULL AND created_at<now()-interval '1 day') OR deleted_at<now()-interval '30 days' LIMIT 1000)",
        "DELETE FROM discord_joins WHERE joined_at<now()-interval '1 day'",
        "DELETE FROM discord_signals WHERE created_at<now()-interval '30 days'",
        "DELETE FROM discord_questions WHERE asked_at<now()-interval '30 days'",
        "DELETE FROM discord_ticket_messages WHERE ticket_id IN (SELECT id FROM discord_tickets WHERE status='closed' AND closed_at<now()-interval '30 days')",
    ] {
        sqlx::query(sql).execute(&app.db).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_words_links_and_caps() {
        assert!(has_word("that is badword here", "badword"));
        assert!(has_word("badword!", "badword"));
        assert!(!has_word("notbadwordy", "badword"));
        assert!(has_word("a two words b", "two words"));
        let sites = vec!["example.com".to_string()];
        assert!(allowed("https://example.com/x", &sites));
        assert!(allowed("https://cdn.example.com/x", &sites));
        assert!(!allowed("https://badexample.com", &sites));
        assert!(!allowed("https://example.com.evil.test", &sites));
        assert_eq!(urls("see <https://a.test/x> and http://b.test").count(), 2);
        assert_eq!(caps("HELLO there"), 50);
        assert_eq!(caps("1234"), 0);
        let mentions = json!({"mentions": [{"id": "1"}, {"id": "2", "bot": true}, {"id": "9"}],
            "referenced_message": {"author": {"id": "1"}}});
        assert_eq!(targets(&mentions, "9"), ["1"]);
    }
}
