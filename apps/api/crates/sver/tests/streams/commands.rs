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
    e.sql("DELETE FROM outside_chat_messages WHERE platform='bot'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='cm-b'")
        .await;
}
