//! Moderation on the home server (docs/DISCORD_BOT.md "Moderation and safety") against a fake
//! Discord API and the fake Gateway: automod hits (and the duplicate window), deleted and edited
//! message evidence, a strike escalating to an approved mute with its 30-minute re-ping, protected
//! roles, a timed ban lifting, the new-account filter, a raid lockdown and unlock, a scheduled
//! unlock, and a ticket opened by DM through close with its transcript. The tuning values here are
//! examples for the test, not the server's.
use super::*;
use axum::http::{Method, Uri};

type Calls = Arc<Mutex<Vec<String>>>;

/// Every call is logged as "METHOD path body"; the few answers that matter are canned.
async fn fake(
    State(calls): State<Calls>,
    method: Method,
    uri: Uri,
    body: String,
) -> (StatusCode, Json<Value>) {
    let path = uri.path().to_string();
    calls
        .lock()
        .unwrap()
        .push(format!("{method} {path} {body}"));
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let answer = match (method.as_str(), parts.as_slice()) {
        ("GET", ["guilds", "900", "members", "555"]) => {
            return (StatusCode::NOT_FOUND, Json(json!({"code": 10007})));
        }
        // Discord failing (556) or rate limiting even the retry (557).
        ("GET", ["guilds", "900", "members", "556"]) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({})));
        }
        ("GET", ["guilds", "900", "members", "557"]) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"retry_after": 0})),
            );
        }
        // 777 has the protected role; 778 is on the mod team.
        ("GET", ["guilds", "900", "members", user]) => json!({"user": {"id": user},
            "roles": match *user { "777" => vec!["60"], "778" => vec!["50"], _ => vec![] }, "joined_at": "2020-01-01T00:00:00Z"}),
        // 44 owns the server and holds no roles.
        ("GET", ["guilds", "900"]) => json!({"owner_id": "44", "roles": [
            {"id": "900", "position": 0, "permissions": "0"},
            {"id": "60", "position": 1, "permissions": "0"},
            {"id": "50", "position": 5, "permissions": "1099511627776"},
            {"id": "55", "position": 6, "permissions": "1099511635990"},
        ]}),
        ("GET", ["guilds", "900", "channels"]) => json!([
            {"id": "10", "type": 0, "permission_overwrites": []},
            {"id": "12", "type": 0, "permission_overwrites": [{"id": "900", "type": 0, "allow": "0", "deny": "1024"}]},
            {"id": "11", "type": 2, "permission_overwrites": []},
        ]),
        ("POST", ["users", "@me", "channels"]) => {
            let to: Value = serde_json::from_str(&body).unwrap();
            json!({"id": format!("dm-{}", to["recipient_id"].as_str().unwrap())})
        }
        // "Ticket #N" gets thread tN.
        ("POST", ["channels", _, "threads"]) => {
            let named: Value = serde_json::from_str(&body).unwrap();
            let n = named["name"].as_str().unwrap().rsplit('#').next().unwrap();
            json!({"id": format!("t{n}")})
        }
        ("POST", ["channels", _, "messages"]) => json!({"id": "m1"}),
        _ => json!({}),
    };
    (StatusCode::OK, Json(answer))
}

fn taken(calls: &Calls) -> Vec<String> {
    std::mem::take(&mut *calls.lock().unwrap())
}
/// The one call containing every needle.
fn find(calls: &[String], needles: &[&str]) -> String {
    let hits: Vec<&String> = calls
        .iter()
        .filter(|c| needles.iter().all(|n| c.contains(n)))
        .collect();
    assert_eq!(hits.len(), 1, "{needles:?} in {calls:#?}");
    hits[0].clone()
}
fn none(calls: &[String], needle: &str) {
    assert!(
        !calls.iter().any(|c| c.contains(needle)),
        "{needle} in {calls:#?}"
    );
}

async fn event(e: &Env, kind: &str, data: Value) {
    sver::discord::gateway::dispatch(&e.app, kind, data).await;
}
fn message(id: &str, user: &str, content: &str) -> Value {
    json!({"id": id, "channel_id": "30", "guild_id": "900", "author": {"id": user},
        "member": {"roles": []}, "content": content, "attachments": []})
}
/// Timeouts, kick, ban, manage messages and manage channels.
const MODERATOR: &str = "1099511635990";
/// Timeouts only.
const HELPER: &str = "1099511627776";
/// Anyone with permissions here is on the mod team (role 50).
/// Moderators hold role 55 (those permissions server-wide), helpers role 50 (timeouts only).
async fn ask(e: &Env, id: &str, kind: u64, user: &str, permissions: &str, data: Value) {
    let roles = match permissions {
        MODERATOR => json!(["55"]),
        HELPER => json!(["50"]),
        _ => json!([]),
    };
    ask_as(e, id, kind, user, permissions, roles, data).await;
}
/// `permissions` is what the interaction carries (channel overwrites included).
async fn ask_as(
    e: &Env,
    id: &str,
    kind: u64,
    user: &str,
    permissions: &str,
    roles: Value,
    data: Value,
) {
    sver::discord::commands::interaction(&e.app, json!({"id": id, "token": "tk", "type": kind, "guild_id": "900",
        "channel_id": "30", "member": {"user": {"id": user}, "permissions": permissions, "roles": roles}, "data": data}))
        .await;
}
/// Where a deferred answer lands: an edit of the message, or a private follow-up.
const EDITED: &str = "PATCH /webhooks/app1/tk/messages/@original";
const FOLLOWUP: &str = "POST /webhooks/app1/tk ";
fn command(sub: &str, options: Value) -> Value {
    json!({"name": "mod", "options": [{"type": 1, "name": sub, "options": options}]})
}
fn button(id: String) -> Value {
    json!({"custom_id": id, "component_type": 2})
}
fn form(id: &str, key: &str, value: &str) -> Value {
    json!({"custom_id": id, "components": [{"type": 1, "components": [{"type": 4, "custom_id": key, "value": value}]}]})
}
async fn count(e: &Env, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&e.app.db).await.unwrap()
}
async fn tick(e: &Env) {
    sver::discord::moderation::tick(&e.app).await.unwrap();
}
/// A Discord id made now: a brand-new account.
fn new_account() -> String {
    let ms = chrono::Utc::now().timestamp_millis() as u64 - 1_420_070_400_000;
    ((ms << 22) | 1).to_string()
}

pub async fn exercise(e: &Env) {
    let calls: Calls = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new().fallback(fake).with_state(calls.clone());
    tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    // SAFETY: only the Discord tests read these variables, one at a time.
    unsafe { std::env::set_var("DISCORD_API", format!("http://{address}")) };
    e.sql(r#"UPDATE discord_home SET settings='{"mod_log": "10", "mod_team": "50", "restricted": "51", "immune": ["52"],
        "protected": ["60"], "tickets": "20", "transcripts": "21", "banned_words": ["examplebadword"],
        "dupes_count": 3, "dupes_seconds": 300, "spam_count": 6, "spam_seconds": 2,
        "mute_at": 2, "mute_minutes": 30, "kick_at": 4, "ban_at": 6, "new_account_days": 3}'"#)
        .await;

    // Automod: a banned word is deleted, struck and logged; an immune role skips it.
    event(
        e,
        "MESSAGE_CREATE",
        message("8001", "700", "this has ExampleBadWord in it"),
    )
    .await;
    let c = taken(&calls);
    find(&c, &["DELETE /channels/30/messages/8001"]);
    find(
        &c,
        &[
            "POST /channels/10/messages",
            "Banned word",
            "**Strikes:** 1",
        ],
    );
    assert_eq!(count(e, "SELECT count(*) FROM discord_cases WHERE user_id='700' AND action='warn' AND active AND moderator_id IS NULL").await, 1);
    let mut immune = message("8002", "701", "examplebadword");
    immune["member"]["roles"] = json!(["52"]);
    event(e, "MESSAGE_CREATE", immune).await;
    none(&taken(&calls), "DELETE");
    // Duplicates count over their own window: the third copy goes, though the spam window is
    // shorter than the time between them.
    for id in ["8003", "8004", "8005"] {
        event(e, "MESSAGE_CREATE", message(id, "702", "buy my thing")).await;
        tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    }
    let c = taken(&calls);
    find(&c, &["DELETE /channels/30/messages/8005"]);
    find(&c, &["Duplicate messages", "Same message 3x"]);
    none(&c, "messages/8004");

    // Evidence: a deleted message is logged with its text and kept; an edit is logged and scanned.
    event(e, "MESSAGE_CREATE", message("8010", "703", "hello there")).await;
    event(
        e,
        "MESSAGE_DELETE",
        json!({"id": "8010", "channel_id": "30", "guild_id": "900"}),
    )
    .await;
    find(&taken(&calls), &["Message deleted", "hello there"]);
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM discord_messages WHERE id='8010' AND deleted_at IS NOT NULL"
        )
        .await,
        1
    );
    event(e, "MESSAGE_CREATE", message("8011", "704", "fine words")).await;
    event(
        e,
        "MESSAGE_UPDATE",
        message("8011", "704", "now examplebadword"),
    )
    .await;
    let c = taken(&calls);
    find(&c, &["Message edited", "fine words", "now examplebadword"]);
    find(&c, &["DELETE /channels/30/messages/8011"]);
    // The bot's own deletion isn't logged again when Discord reports it.
    event(
        e,
        "MESSAGE_DELETE",
        json!({"id": "8011", "channel_id": "30", "guild_id": "900"}),
    )
    .await;
    none(&taken(&calls), "Message deleted");

    // Escalation: a second strike reaches the ladder (at least 2: mute), proposed to staff with
    // the member restricted.
    let warn = |user: &str| {
        command(
            "warn",
            json!([{"name": "user", "value": user}, {"name": "reason", "value": "rude"}]),
        )
    };
    ask(e, "i1", 2, "42", MODERATOR, warn("700")).await;
    let c = taken(&calls);
    find(&c, &["POST /interactions/i1/tk/callback", r#""type":5"#]);
    find(
        &c,
        &[
            "PATCH /webhooks/app1/tk/messages/@original",
            "Active strikes: **2**",
        ],
    );
    find(&c, &["POST /channels/dm-700/messages", "warned"]);
    find(&c, &["PUT /guilds/900/members/700/roles/51"]);
    let escalation: i64 = sqlx::query_scalar(
        "SELECT id FROM discord_escalations WHERE user_id='700' AND resolved_at IS NULL",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    let approve = format!("mod:esc:approve:{escalation}");
    find(&c, &["Escalation: approval needed", &approve]);
    // Members can't press it; a moderator approves the mute, with the ladder's length.
    ask(e, "i2", 3, "43", "0", button(approve.clone())).await;
    find(
        &taken(&calls),
        &["POST /interactions/i2/tk/callback", "Only moderators"],
    );
    ask(e, "i3", 3, "42", MODERATOR, button(approve.clone())).await;
    let c = taken(&calls);
    find(&c, &["POST /interactions/i3/tk/callback", r#""type":6"#]);
    find(&c, &[EDITED, "approved by <@42>"]);
    find(
        &c,
        &[
            "PATCH /guilds/900/members/700 ",
            "communication_disabled_until",
        ],
    );
    find(&c, &["DELETE /guilds/900/members/700/roles/51"]);
    assert_eq!(count(e, "SELECT count(*) FROM discord_cases WHERE user_id='700' AND action='mute' AND expires_at BETWEEN now()+interval '29 minutes' AND now()+interval '31 minutes'").await, 1);
    ask(e, "i4", 3, "42", MODERATOR, button(approve)).await;
    find(&taken(&calls), &[FOLLOWUP, "already handled"]);

    // An escalation nobody touches pings the mod team again after 30 minutes, once; the timer is a
    // row, so a restart doesn't lose it.
    ask(e, "i5", 2, "42", MODERATOR, warn("705")).await;
    ask(e, "i6", 2, "42", MODERATOR, warn("705")).await;
    taken(&calls);
    tick(e).await;
    none(&taken(&calls), "waited 30 minutes");
    e.sql(
        "UPDATE discord_escalations SET created_at=now()-interval '31 minutes' WHERE user_id='705'",
    )
    .await;
    tick(e).await;
    find(
        &taken(&calls),
        &[
            "POST /channels/10/messages",
            "<@&50>",
            "waited 30 minutes",
            r#""roles":["50"]"#,
        ],
    );
    tick(e).await;
    none(&taken(&calls), "waited 30 minutes");

    // Protected roles can't be moderated.
    ask(
        e,
        "i7",
        2,
        "42",
        MODERATOR,
        command("kick", json!([{"name": "user", "value": "777"}])),
    )
    .await;
    let c = taken(&calls);
    find(&c, &["PATCH /webhooks", "protected role"]);
    none(&c, "DELETE /guilds/900/members/777");
    // Nor staff, nor yourself, nor the bot; and each action needs its own permission.
    for (n, user, permissions, answer) in [
        ("i7a", "778", MODERATOR, "is staff"),
        ("i7b", "42", MODERATOR, "can't moderate yourself"),
        ("i7c", "500", MODERATOR, "That's the bot"),
        ("i7i", "44", MODERATOR, "owns the server"),
        ("i7d", "706", HELPER, "needs the Ban Members permission"),
    ] {
        ask(
            e,
            n,
            2,
            "42",
            permissions,
            command("ban", json!([{"name": "user", "value": user}])),
        )
        .await;
        let c = taken(&calls);
        find(&c, &[EDITED, answer]);
        none(&c, "/bans/");
    }
    ask(
        e,
        "i7e",
        2,
        "42",
        HELPER,
        command("purge", json!([{"name": "count", "value": 5}])),
    )
    .await;
    find(
        &taken(&calls),
        &[EDITED, "needs the Manage Messages permission"],
    );
    // Manage Channels on one channel (an overwrite) isn't Manage Channels for the server.
    ask_as(
        e,
        "i7f",
        2,
        "42",
        MODERATOR,
        json!(["50"]),
        command("lockdown", json!([])),
    )
    .await;
    let c = taken(&calls);
    find(&c, &[EDITED, "needs the Manage Channels permission"]);
    none(&c, "/permissions/");
    // The owner moderates without a role, and is never restricted.
    ask_as(
        e,
        "i7j",
        2,
        "44",
        "8",
        json!([]),
        command("purge", json!([{"name": "count", "value": 5}])),
    )
    .await;
    let c = taken(&calls);
    find(&c, &["GET /channels/30/messages"]);
    none(&c, "needs the");
    for (n, text) in [("8030", "examplebadword"), ("8031", "examplebadword again")] {
        event(e, "MESSAGE_CREATE", message(n, "44", text)).await;
    }
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM discord_escalations WHERE user_id='44'"
        )
        .await,
        1
    );
    none(&taken(&calls), "/members/44/roles/51");
    // When Discord can't say who someone is (an error, a rate limit), nothing is done to them.
    for (n, user) in [("i7g", "556"), ("i7h", "557")] {
        ask(
            e,
            n,
            2,
            "42",
            MODERATOR,
            command("ban", json!([{"name": "user", "value": user}])),
        )
        .await;
        let c = taken(&calls);
        find(&c, &[EDITED, "Discord didn't answer"]);
        none(&c, "/bans/");
    }

    // A timed ban lifts when it ends, once.
    ask(
        e,
        "i8",
        2,
        "42",
        MODERATOR,
        command(
            "ban",
            json!([{"name": "user", "value": "706"},
        {"name": "duration", "value": "1h"}, {"name": "reason", "value": "cool off"}]),
        ),
    )
    .await;
    let c = taken(&calls);
    find(&c, &["PUT /guilds/900/bans/706"]);
    find(&c, &["Mod action: ban", "cool off"]);
    tick(e).await;
    none(&taken(&calls), "DELETE /guilds/900/bans/706");
    e.sql("UPDATE discord_cases SET expires_at=now()-interval '1 second' WHERE user_id='706' AND action='ban'").await;
    tick(e).await;
    let c = taken(&calls);
    find(&c, &["DELETE /guilds/900/bans/706"]);
    find(&c, &["Ban lifted", "706"]);
    tick(e).await;
    none(&taken(&calls), "DELETE /guilds/900/bans/706");
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM discord_cases WHERE user_id='706' AND action='unban'"
        )
        .await,
        1
    );
    // A later permanent ban isn't lifted by the earlier timed one.
    for (n, length) in [("i8a", Some("1h")), ("i8b", None)] {
        let mut options = vec![json!({"name": "user", "value": "709"})];
        options.extend(length.map(|l| json!({"name": "duration", "value": l})));
        ask(e, n, 2, "42", MODERATOR, command("ban", json!(options))).await;
    }
    e.sql("UPDATE discord_cases SET expires_at=now()-interval '1 second' WHERE user_id='709' AND expires_at IS NOT NULL").await;
    taken(&calls);
    tick(e).await;
    none(&taken(&calls), "DELETE /guilds/900/bans/709");

    // A length change waits on the member's ban lock and leaves a ban the timed lift claimed
    // meanwhile alone.
    ask(
        e,
        "i8c",
        2,
        "42",
        MODERATOR,
        command(
            "ban",
            json!([{"name": "user", "value": "711"}, {"name": "duration", "value": "1h"}]),
        ),
    )
    .await;
    taken(&calls);
    let mut held = e.app.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('discord-ban:711', 0))")
        .execute(&mut *held)
        .await
        .unwrap();
    let change = command(
        "duration",
        json!([{"name": "user", "value": "711"}, {"name": "duration", "value": "7d"}]),
    );
    tokio::join!(ask(e, "i8d", 2, "42", MODERATOR, change), async {
        let mut waiting = false;
        for _ in 0..500 {
            waiting = count(
                e,
                "SELECT count(*) FROM pg_locks WHERE locktype='advisory' AND NOT granted
                AND ((classid::bigint<<32)|objid::bigint)=hashtextextended('discord-ban:711', 0)",
            )
            .await
                > 0;
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(waiting, "the length change didn't wait for the ban lock");
        e.sql("UPDATE discord_cases SET lifted_at=now() WHERE user_id='711' AND action='ban'")
            .await;
        held.commit().await.unwrap();
    });
    let c = taken(&calls);
    find(&c, &[EDITED, "has no active mute or ban"]);
    none(&c, "now ends");
    assert_eq!(count(e, "SELECT count(*) FROM discord_cases WHERE user_id='711' AND expires_at<now()+interval '2 hours'").await, 1);

    // The ban trap bans whoever writes there, but not for a system notice such as a pin.
    e.sql(r#"UPDATE discord_home SET settings=settings||'{"ban_trap": "40"}'"#)
        .await;
    let mut pinned = message("8020", "42", "");
    pinned["channel_id"] = json!("40");
    pinned["type"] = json!(6);
    event(e, "MESSAGE_CREATE", pinned).await;
    none(&taken(&calls), "/bans/");
    let mut trapped = message("8021", "710", "hi");
    trapped["channel_id"] = json!("40");
    event(e, "MESSAGE_CREATE", trapped).await;
    let c = taken(&calls);
    find(&c, &["PUT /guilds/900/bans/710", "604800"]);
    find(&c, &["Ban trap", "710"]);
    // A moderator posting a warning there loses the message, never their place.
    let mut warning = message("8022", "778", "don't type here");
    warning["channel_id"] = json!("40");
    event(e, "MESSAGE_CREATE", warning).await;
    let c = taken(&calls);
    none(&c, "/bans/");
    find(&c, &["DELETE /channels/40/messages/8022"]);
    find(&c, &["Ban trap", "Not banned", "is staff"]);

    // The new-account filter holds a brand-new account for staff.
    let fresh = new_account();
    event(
        e,
        "GUILD_MEMBER_ADD",
        json!({"guild_id": "900", "user": {"id": fresh}}),
    )
    .await;
    let c = taken(&calls);
    find(&c, &[&format!("PUT /guilds/900/members/{fresh}/roles/51")]);
    find(
        &c,
        &["New account held", &format!("mod:new:approve:{fresh}")],
    );
    ask(
        e,
        "i9",
        3,
        "42",
        MODERATOR,
        button(format!("mod:new:approve:{fresh}")),
    )
    .await;
    let c = taken(&calls);
    find(
        &c,
        &[&format!("DELETE /guilds/900/members/{fresh}/roles/51")],
    );
    find(&c, &[EDITED, "approved by <@42>"]);

    // Anti-raid: the third join in the window locks every text channel for @everyone, keeping each
    // channel's overwrite; later joins are held; unlocking restores exactly what was there.
    e.sql("DELETE FROM discord_joins").await;
    e.sql(r#"UPDATE discord_home SET settings=settings||'{"raid_joins": 3, "raid_seconds": 60}'"#)
        .await;
    for user in ["801", "802", "803"] {
        event(
            e,
            "GUILD_MEMBER_ADD",
            json!({"guild_id": "900", "user": {"id": user}}),
        )
        .await;
    }
    let c = taken(&calls);
    find(
        &c,
        &[
            "PUT /channels/10/permissions/900",
            r#""deny":"274877908992""#,
        ],
    );
    find(
        &c,
        &[
            "PUT /channels/12/permissions/900",
            r#""deny":"274877910016""#,
        ],
    );
    none(&c, "/channels/11/permissions");
    let raid: i64 = sqlx::query_scalar(
        "SELECT id FROM discord_lockdowns WHERE raid_joins=3 AND unlocked_at IS NULL",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    let unlock = format!("mod:raid:unlock:{raid}");
    find(
        &c,
        &[
            "Nobody can send messages",
            &unlock,
            &format!("mod:raid:banall:{raid}"),
        ],
    );
    find(&c, &["<@&50>", "Raid detected"]);
    let held = sver::discord::moderation::event(
        &e.app,
        "GUILD_MEMBER_ADD",
        &json!({"guild_id": "900", "user": {"id": "804"}}),
    )
    .await;
    assert!(held, "joins during a raid are held");
    none(&taken(&calls), "permissions");
    // "Ban all" bans exactly the joins that set the raid off, not the one held after, and only
    // for a moderator who can ban.
    let banall = format!("mod:raid:banall:{raid}");
    ask(e, "i10a", 3, "42", HELPER, button(banall.clone())).await;
    find(
        &taken(&calls),
        &[FOLLOWUP, "needs the Ban Members permission"],
    );
    ask(e, "i10b", 3, "42", MODERATOR, button(banall)).await;
    let mut c = vec![];
    for _ in 0..100 {
        c.extend(taken(&calls));
        if c.iter().any(|l| l.contains("Raid accounts banned")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    for user in ["801", "802", "803"] {
        find(&c, &[&format!("PUT /guilds/900/bans/{user} ")]);
    }
    none(&c, "/bans/804");
    find(&c, &["Raid accounts banned", "banned 3 of 3"]);
    ask(e, "i10", 3, "42", MODERATOR, button(unlock)).await;
    let c = taken(&calls);
    find(&c, &["DELETE /channels/10/permissions/900"]);
    find(
        &c,
        &["PUT /channels/12/permissions/900", r#""deny":"1024""#],
    );
    find(&c, &[EDITED, "Server unlocked by <@42>"]);

    // A manual lockdown with a length unlocks itself from the tick.
    ask(
        e,
        "i11",
        2,
        "42",
        MODERATOR,
        command(
            "lockdown",
            json!([{"name": "channel", "value": "10"},
        {"name": "duration", "value": "30m"}]),
        ),
    )
    .await;
    let c = taken(&calls);
    find(&c, &["PUT /channels/10/permissions/900"]);
    find(
        &c,
        &["PATCH /webhooks", "Locked 1 channel(s) for 30 minute(s)"],
    );
    e.sql("UPDATE discord_lockdowns SET unlock_at=now()-interval '1 second' WHERE unlocked_at IS NULL").await;
    tick(e).await;
    let c = taken(&calls);
    find(&c, &["DELETE /channels/10/permissions/900"]);
    find(&c, &["Lockdown lifted"]);

    tickets(e, &calls).await;
    // SAFETY: as above.
    unsafe { std::env::remove_var("DISCORD_API") };
}

/// A ticket opened by DM through the Gateway, answered, claimed, noted, closed with a transcript;
/// the one-an-hour limit; and the inactivity warning and close.
async fn tickets(e: &Env, calls: &Calls) {
    let (url, to_bot, mut from_bot) = super::discord::fake_gateway().await;
    // SAFETY: only the Discord tests read these variables, one at a time.
    unsafe {
        std::env::set_var("DISCORD_GATEWAY", "1");
        std::env::set_var("DISCORD_GATEWAY_URL", &url);
    }
    let bot = tokio::spawn(sver::discord::gateway::run(e.app.clone()));
    // The earlier connection's lock goes with its session; the bot retries for it.
    let identify = tokio::time::timeout(std::time::Duration::from_secs(70), from_bot.recv())
        .await
        .expect("the bot identified")
        .unwrap();
    assert_eq!(identify["op"], 2);
    let dispatch = |s: u64, t: &str, d: Value| json!({"op": 0, "s": s, "t": t, "d": d});
    to_bot
        .send(dispatch(1, "READY", json!({"session_id": "s2", "resume_gateway_url": url,
            "application": {"id": "app1"}, "user": {"id": "500", "username": "S.V.E.R"}, "guilds": []})))
        .unwrap();
    let direct = |id: &str, text: &str| json!({"id": id, "channel_id": "dm-707", "author": {"id": "707"}, "content": text, "attachments": []});
    to_bot
        .send(dispatch(
            2,
            "MESSAGE_CREATE",
            direct("9001", "I need help with my account"),
        ))
        .unwrap();
    let mut c = vec![];
    for _ in 0..200 {
        c.extend(taken(calls));
        if c.iter().any(|l| l.contains("Ticket opened")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    find(
        &c,
        &["POST /channels/20/threads", r#""type":12"#, "Ticket #1"],
    );
    find(
        &c,
        &[
            "POST /channels/t1/messages",
            "<@&50> new support ticket",
            "mod:ticket:claim:1",
        ],
    );
    find(
        &c,
        &[
            "POST /channels/t1/messages",
            "**Member:** I need help with my account",
        ],
    );
    find(&c, &["POST /channels/dm-707/messages", "Ticket #1 opened"]);
    find(&c, &["PUT /channels/t1/pins/m1"]);
    bot.abort();
    // SAFETY: as above.
    unsafe {
        std::env::remove_var("DISCORD_GATEWAY");
        std::env::remove_var("DISCORD_GATEWAY_URL");
    }

    // More DMs land in the thread; a staff reply in the thread goes back by DM.
    event(e, "MESSAGE_CREATE", direct("9002", "It's about my login")).await;
    find(
        &taken(calls),
        &[
            "POST /channels/t1/messages",
            "**Member:** It's about my login",
        ],
    );
    let mut reply = message("9003", "42", "We're on it");
    reply["channel_id"] = json!("t1");
    reply["member"]["roles"] = json!(["52"]);
    event(e, "MESSAGE_CREATE", reply).await;
    find(
        &taken(calls),
        &["POST /channels/dm-707/messages", "**Staff:** We're on it"],
    );

    // Claim, priority, an internal note, then close with a reason and a transcript.
    ask(
        e,
        "t1c",
        3,
        "42",
        MODERATOR,
        button("mod:ticket:claim:1".into()),
    )
    .await;
    let c = taken(calls);
    find(&c, &["PATCH /channels/t1", "Ticket #1 [claimed]"]);
    find(&c, &[EDITED, "mod:ticket:unclaim:1"]);
    ask(
        e,
        "t1p",
        3,
        "42",
        MODERATOR,
        json!({"custom_id": "mod:ticket:priority:1", "component_type": 3, "values": ["high"]}),
    )
    .await;
    find(
        &taken(calls),
        &[EDITED, r#""name":"Priority","value":"high""#],
    );
    ask(
        e,
        "t1n",
        5,
        "42",
        MODERATOR,
        form("mod:ticket:noting:1", "note", "Checked their account"),
    )
    .await;
    find(&taken(calls), &[FOLLOWUP, "Only staff see it"]);
    ask(
        e,
        "t1x",
        5,
        "42",
        MODERATOR,
        form("mod:ticket:closing:1", "reason", "Solved"),
    )
    .await;
    let c = taken(calls);
    find(
        &c,
        &[
            "PATCH /channels/t1",
            r#""archived":true"#,
            "[CLOSED] Ticket #1",
        ],
    );
    find(
        &c,
        &[
            "POST /channels/dm-707/messages",
            "Ticket #1 closed",
            "Solved",
        ],
    );
    let transcript = find(
        &c,
        &["POST /channels/21/messages", "ticket-1-transcript.txt"],
    );
    for line in [
        "MEMBER 707: I need help with my account",
        "MEMBER 707: It's about my login",
        "STAFF 42: We're on it",
        "NOTE (staff only) 42: Checked their account",
        "Priority: high",
        "Reason: Solved",
    ] {
        assert!(transcript.contains(line), "{line} in {transcript}");
    }
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM discord_tickets WHERE id=1 AND status='closed'"
        )
        .await,
        1
    );

    // One new ticket an hour.
    event(e, "MESSAGE_CREATE", direct("9004", "one more thing")).await;
    let c = taken(calls);
    find(
        &c,
        &["POST /channels/dm-707/messages", "one ticket an hour"],
    );
    none(&c, "/threads");

    // /ticket opens one too; 5 quiet days bring a warning, 7 close it.
    ask(
        e,
        "t2",
        2,
        "708",
        "0",
        json!({"name": "ticket", "options": [{"name": "subject", "value": "Report a bug"},
        {"name": "category", "value": "report"}]}),
    )
    .await;
    find(
        &taken(calls),
        &[
            "PATCH /webhooks/app1/tk/messages/@original",
            "Ticket #2 is open",
        ],
    );
    e.sql("UPDATE discord_tickets SET active_at=now()-interval '6 days' WHERE id=2")
        .await;
    tick(e).await;
    find(
        &taken(calls),
        &["POST /channels/dm-708/messages", "closes in 2 days"],
    );
    tick(e).await;
    none(&taken(calls), "closes in 2 days");
    e.sql("UPDATE discord_tickets SET active_at=now()-interval '8 days' WHERE id=2")
        .await;
    tick(e).await;
    let c = taken(calls);
    find(&c, &["POST /channels/dm-708/messages", "Ticket #2 closed"]);
    find(
        &c,
        &[
            "POST /channels/21/messages",
            "ticket-2-transcript.txt",
            "MEMBER 708: Report a bug",
        ],
    );
}
