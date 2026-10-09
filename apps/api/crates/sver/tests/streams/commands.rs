//! Chat commands and the channel bot (docs/COMMUNITY.md): custom commands, timed messages, /help.
use super::*;

async fn bot_lines(e: &Env, channel: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT body FROM outside_chat_messages WHERE channel_id=$1 AND platform='bot' ORDER BY seq")
        .bind(channel)
        .fetch_all(&e.app.db)
        .await
        .unwrap()
}

/// The owner says !hello (the chat rate limit is reset so the test can send quickly).
async fn hello(e: &Env) {
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat%stream-owner%'")
        .await;
    e.call(
        "POST",
        "/api/channels/Streamer/chat",
        json!({"id": chat::id(), "body": "!hello"}),
    )
    .await;
}

pub async fn exercise(e: &Env) {
    let saved = e
        .call("PUT", "/api/me/commands/Hello", json!({"reply":"Hi {user}, welcome to {channel}! Followers: {followers}.","access":"everyone","cooldown_seconds":0}))
        .await;
    assert_eq!(saved["commands"][0]["name"], "hello");
    for (name, body) in [
        (
            "marker",
            json!({"reply":"x","access":"everyone","cooldown_seconds":0}),
        ),
        (
            "bad-name",
            json!({"reply":"x","access":"everyone","cooldown_seconds":0}),
        ),
        (
            "long",
            json!({"reply":"x".repeat(301),"access":"everyone","cooldown_seconds":0}),
        ),
        (
            "who",
            json!({"reply":"x","access":"admins","cooldown_seconds":0}),
        ),
    ] {
        let (status, _) = e
            .request(
                "PUT",
                &format!("/api/me/commands/{name}"),
                body,
                true,
                true,
                false,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
    }

    // !hello gets the bot's reply, variables filled in, after the message; it shows in chat history.
    let before = bot_lines(e, "stream-owner").await.len();
    hello(e).await;
    let lines = bot_lines(e, "stream-owner").await;
    assert_eq!(lines.len(), before + 1);
    let line = lines.last().unwrap();
    assert!(
        line.starts_with("Hi ") && line.contains("welcome to") && !line.contains('{'),
        "{line}"
    );
    let read = e
        .call("GET", "/api/channels/Streamer/chat", Value::Null)
        .await;
    let shown = read["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["outside"]["platform"] == "bot")
        .unwrap()
        .clone();
    assert!(
        ["PYRE", "ECHO", "FAVOR", "VOLK"].contains(&shown["outside"]["name"].as_str().unwrap()),
        "{shown}"
    );
    // A cooldown keeps it quiet.
    e.call(
        "PUT",
        "/api/me/commands/hello",
        json!({"reply":"Hi again","access":"everyone","cooldown_seconds":60}),
    )
    .await;
    hello(e).await;
    hello(e).await;
    assert_eq!(
        bot_lines(e, "stream-owner").await.len(),
        before + 2,
        "cooldown"
    );
    let help = e
        .call("GET", "/api/channels/Streamer/commands", Value::Null)
        .await;
    assert_eq!(help["commands"][0]["name"], "hello");
    e.call(
        "PUT",
        "/api/me/commands/bot",
        json!({"bot":"volk","personality":"battle"}),
    )
    .await;
    assert_eq!(
        e.call("GET", "/api/me/commands", Value::Null).await["bot"]["name"],
        "VOLK"
    );
    e.call("DELETE", "/api/me/commands/hello", Value::Null)
        .await;

    // Timers: at most 5, every 10+ minutes; a due timer posts once, and only after chat activity.
    for n in 0..5 {
        e.call(
            "POST",
            "/api/me/timers",
            json!({"body": format!("Timer {n}"), "every_minutes": 10}),
        )
        .await;
    }
    let (status, _) = e
        .request(
            "POST",
            "/api/me/timers",
            json!({"body":"Six","every_minutes":10}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = e
        .request(
            "PATCH",
            "/api/me/timers/none",
            json!({"every_minutes":5}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "at least 10 minutes");
    e.sql("DELETE FROM chat_timers WHERE channel_id='stream-owner'")
        .await;

    e.sql("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES('cm-owner','cm@example.test','CmOwner',true,'1990-01-01')").await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('cm-b','cm-owner','cm-b',1,'LIVE','s','v','c',now()-interval '20 minutes',now(),now(),'skipped')").await;
    e.sql("INSERT INTO chat_timers(id,channel_id,body,every_minutes) VALUES('cm-t','cm-owner','Follow {channel}! Live for {uptime}.',10)").await;
    sver::commands::tick(&e.app).await.unwrap();
    assert!(
        bot_lines(e, "cm-owner").await.is_empty(),
        "no chat since the stream started"
    );
    e.sql("INSERT INTO chat_messages(id,channel_id,author_id,body) VALUES('cm-m','cm-owner','cm-owner','hello chat')").await;
    sver::commands::tick(&e.app).await.unwrap();
    sver::commands::tick(&e.app).await.unwrap();
    let lines = bot_lines(e, "cm-owner").await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("Follow CmOwner! Live for 20m"),
        "{}",
        lines[0]
    );
    // A counted raid on a live channel gets the bot's line (VOLK, Chill, for a faction-less channel).
    sver::bot::record_raid(&e.app, "cm-owner", "Raider", 7)
        .await
        .unwrap();
    sver::bot::drain(&e.app).await.unwrap();
    assert_eq!(
        bot_lines(e, "cm-owner").await.last().unwrap(),
        "Welcome, Raider and company (7)."
    );
    part_two(e).await;
    e.sql("DELETE FROM outside_chat_messages WHERE platform='bot'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='cm-b'")
        .await;
}

/// Part 2: AutoMod and its ladder, follow events, giveaways and the command import.
async fn part_two(e: &Env) {
    let viewer = chat::person(e, "cb-viewer", "CbViewer", true).await;
    let say = |text: &'static str| {
        let viewer = viewer.clone();
        async move {
            e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat%cb-viewer%'")
                .await;
            chat::call(
                e,
                "POST",
                "/api/channels/Streamer/chat",
                Some(&viewer),
                json!({"id": chat::id(), "body": text}),
            )
            .await
        }
    };
    e.call(
        "PUT",
        "/api/me/automod",
        json!({"caps":true,"repeats":true,"spam":false,"ladder":[0,60]}),
    )
    .await;
    let (status, held) = say("THIS IS SO LOUD RIGHT NOW").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{held}");
    assert!(held["error"].as_str().unwrap().contains("caps"));
    let (status, out) = say("STILL VERY LOUD OVER HERE").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{out}");
    assert!(out["error"].as_str().unwrap().contains("timed you out"));
    let timed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id='stream-owner' AND user_id='cb-viewer' AND kind='timeout' AND until>now())")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(timed);
    assert!(
        bot_lines(e, "stream-owner")
            .await
            .last()
            .unwrap()
            .contains("CbViewer"),
        "the bot announces it"
    );
    let logged: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_moderation_log WHERE channel_id='stream-owner' AND action='automod_timeout')")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(logged);
    e.sql("DELETE FROM channel_restrictions WHERE user_id='cb-viewer'")
        .await;
    assert_eq!(say("a calm message").await.0, StatusCode::OK);
    assert_eq!(
        say("a calm message").await.0,
        StatusCode::FORBIDDEN,
        "the same message again is a repeat (second strike)"
    );
    e.sql("DELETE FROM channel_restrictions WHERE user_id='cb-viewer'")
        .await;
    e.call(
        "PUT",
        "/api/me/automod",
        json!({"caps":false,"repeats":false,"spam":false,"ladder":[0]}),
    )
    .await;
    let (status, _) = e
        .request(
            "PUT",
            "/api/me/automod",
            json!({"caps":true,"repeats":false,"spam":false,"ladder":[5]}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a timeout under 10 seconds"
    );

    // A follow is recorded for the bot to greet.
    chat::call(
        e,
        "PUT",
        "/api/follows/Streamer",
        Some(&viewer),
        Value::Null,
    )
    .await;
    let followed: i64 = sqlx::query_scalar("SELECT count(*) FROM bot_events WHERE channel_id='stream-owner' AND kind='follow' AND name='CbViewer'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(followed, 1);
    e.sql("DELETE FROM bot_events").await;

    // A giveaway: only viewers with a Counted session enter; the end picks one of them.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) SELECT 'gv-b','stream-owner','gv-b',1,'LIVE','s','v','c',now(),now(),now(),'skipped' WHERE NOT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id='stream-owner' AND state<>'ENDED')").await;
    let live: String = sqlx::query_scalar("SELECT id FROM broadcasts WHERE owner_id='stream-owner' AND state IN ('LIVE','RECONNECTING')")
        .fetch_one(&e.app.db).await.unwrap();
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat%stream-owner%'")
        .await;
    e.call(
        "POST",
        "/api/channels/Streamer/chat",
        json!({"id": chat::id(), "body": "!giveaway start pickme"}),
    )
    .await;
    assert!(
        bot_lines(e, "stream-owner")
            .await
            .last()
            .unwrap()
            .contains("Type pickme")
    );
    assert_eq!(say("pickme").await.0, StatusCode::OK);
    let entries: i64 = sqlx::query_scalar("SELECT count(*) FROM giveaway_entries")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(entries, 0, "not watching, so not entered");
    sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES($1,'u:cb-viewer',now()+interval '1 minute','counted')")
        .bind(&live).execute(&e.app.db).await.unwrap();
    assert_eq!(say("PickMe").await.0, StatusCode::OK);
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat%stream-owner%'")
        .await;
    e.call(
        "POST",
        "/api/channels/Streamer/chat",
        json!({"id": chat::id(), "body": "!giveaway end"}),
    )
    .await;
    assert_eq!(
        bot_lines(e, "stream-owner").await.last().unwrap(),
        "The giveaway winner is CbViewer! (1 entered.) Congratulations!"
    );
    sqlx::query("DELETE FROM playback_leases WHERE viewer_key='u:cb-viewer'")
        .execute(&e.app.db)
        .await
        .unwrap();
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='gv-b'")
        .await;

    // Import: commands are created, unknown variables flagged, bad lines skipped.
    let report = e
        .call(
            "POST",
            "/api/me/commands-import",
            json!({"text":"!discord Join: https://discord.gg/x $(user)
!hug $(user) hugs $(count)
not a command
!marker reserved"}),
        )
        .await;
    assert_eq!(report["imported"], json!(["discord", "hug"]), "{report}");
    assert_eq!(report["flagged"][0]["variables"], json!(["$(count)"]));
    assert_eq!(report["skipped"].as_array().unwrap().len(), 2);
    e.sql("DELETE FROM chat_commands WHERE channel_id='stream-owner'")
        .await;
}
