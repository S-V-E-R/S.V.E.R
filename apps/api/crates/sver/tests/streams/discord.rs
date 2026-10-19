//! The Discord bot (docs/COMMUNITY.md "Discord bot") against a fake Discord API: adding it to a
//! server, Studio's choices and checks, the go-live post, and role sync that only ever takes back
//! roles it gave.
use super::*;

type Calls = Arc<Mutex<Vec<String>>>;

fn fake(calls: Calls) -> Router {
    let log = move |line: String| calls.lock().unwrap().push(line);
    let (a, b, c, d) = (log.clone(), log.clone(), log.clone(), log.clone());
    let (f, g) = (log.clone(), log);
    Router::new()
        .route(
            "/applications/{app}/guilds/{guild}/commands",
            axum::routing::put(
                move |Path((app, guild)): Path<(String, String)>, Json(body): Json<Value>| async move {
                    f(format!("COMMANDS {app} {guild} {}", body.as_array().map_or(0, Vec::len)));
                    Json(json!([]))
                },
            ),
        )
        .route(
            "/interactions/{id}/{token}/callback",
            post(move |Path((id, _)): Path<(String, String)>, body: String| async move {
                g(format!("REPLY {id} {body}"));
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/oauth2/token",
            post(|body: String| async move {
                if body.contains("code=good") {
                    Json(json!({"guild": {"id": "900", "name": "Test Server"}}))
                } else {
                    Json(json!({"error": "invalid_grant"}))
                }
            }),
        )
        .route(
            "/guilds/900/channels",
            get(|| async {
                Json(json!([{"id": "10", "name": "live", "type": 0}, {"id": "11", "name": "voice", "type": 2}]))
            }),
        )
        .route(
            "/guilds/900/roles",
            get(|| async {
                Json(json!([
                    {"id": "900", "name": "@everyone", "position": 0},
                    {"id": "20", "name": "Subs", "position": 1},
                    {"id": "21", "name": "Myria", "position": 2},
                    {"id": "22", "name": "Guild", "position": 3},
                    {"id": "23", "name": "Admin", "position": 9},
                    {"id": "30", "name": "S.V.E.R", "position": 5, "managed": true, "permissions": "268435456"}
                ]))
            }),
        )
        .route("/users/@me", get(|| async { Json(json!({"id": "500"})) }))
        .route(
            "/guilds/900/members/500",
            get(|| async { Json(json!({"roles": ["30"]})) }),
        )
        .route(
            "/channels/10/messages",
            post(move |body: String| async move {
                a(format!("POST message {body}"));
                Json(json!({"id": "m1"}))
            }),
        )
        .route(
            "/guilds/900/members/{user}/roles/{role}",
            axum::routing::put(move |Path((user, role)): Path<(String, String)>| async move {
                match user.as_str() {
                    "701" => (StatusCode::NOT_FOUND, Json(json!({"code": 10007}))),
                    "702" => (StatusCode::FORBIDDEN, Json(json!({"code": 50013}))),
                    _ => {
                        b(format!("PUT {user} {role}"));
                        (StatusCode::NO_CONTENT, Json(Value::Null))
                    }
                }
            })
            .delete(move |Path((user, role)): Path<(String, String)>| async move {
                c(format!("DELETE {user} {role}"));
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/users/@me/guilds/900",
            delete(move || async move {
                d("LEAVE 900".into());
                StatusCode::NO_CONTENT
            }),
        )
}

async fn linked(e: &Env, discord: &str, id: &str, faction: &str) {
    sqlx::query("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES($1,$1||'@example.test',$1,true,'1990-01-01')")
        .bind(id).execute(&e.app.db).await.unwrap();
    sqlx::query("INSERT INTO identities(provider,subject,user_id) VALUES('discord',$1,$2)")
        .bind(discord)
        .bind(id)
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at) VALUES($1,$2,now(),now())")
        .bind(id).bind(faction).execute(&e.app.db).await.unwrap();
}
async fn sweep(e: &Env) {
    e.sql("UPDATE discord_servers SET synced_at=NULL").await;
    sver::discord::tick(&e.app).await.unwrap();
}

pub async fn exercise(e: &Env) {
    let calls: Calls = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = fake(calls.clone());
    tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let taken = |calls: &Calls| std::mem::take(&mut *calls.lock().unwrap());

    // Off until the bot token is set.
    assert_eq!(
        e.call("GET", "/api/me/discord", Value::Null).await["available"],
        false
    );
    // SAFETY: only this test reads these variables.
    unsafe {
        std::env::set_var("DISCORD_API", format!("http://{address}"));
        std::env::set_var("DISCORD_BOT_TOKEN", "synthetic-bot-token");
    }
    assert_eq!(
        e.call("GET", "/api/me/discord", Value::Null).await,
        json!({"available": true, "server": null})
    );

    // Adding it: Discord's page with only the permissions the bot needs, and a sealed state.
    let url = e.call("POST", "/api/me/discord/install", Value::Null).await["url"]
        .as_str()
        .unwrap()
        .to_string();
    let url = url::Url::parse(&url).unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["permissions"], "268454912");
    assert_eq!(query["scope"], "bot");
    let state = &query["state"];
    let back = |code: &str, state: &str| {
        format!(
            "/api/me/discord/callback?code={code}&state={}",
            url::form_urlencoded::byte_serialize(state.as_bytes()).collect::<String>()
        )
    };
    let linked_rows = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM discord_servers")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    // A forged state, or a code Discord refuses, links nothing.
    e.request(
        "GET",
        &back("good", "v1.forged.state"),
        Value::Null,
        true,
        true,
        false,
    )
    .await;
    e.request("GET", &back("bad", state), Value::Null, true, true, false)
        .await;
    assert_eq!(linked_rows().await, 0);
    let (status, _) = e
        .request("GET", &back("good", state), Value::Null, true, true, false)
        .await;
    assert!(status.is_redirection());
    assert_eq!(linked_rows().await, 1);

    // Studio's choices: text channels only; roles minus @everyone and the bot's own, with the
    // ones above the bot marked as not givable.
    let mine = e.call("GET", "/api/me/discord", Value::Null).await;
    assert_eq!(mine["server"]["guild_name"], "Test Server");
    assert_eq!(
        mine["server"]["channels"],
        json!([{"id": "10", "name": "live"}])
    );
    let roles = mine["server"]["roles"].as_array().unwrap();
    assert_eq!(roles.len(), 4);
    assert_eq!(
        roles.iter().find(|r| r["id"] == "23").unwrap()["givable"],
        false
    );
    assert_eq!(
        roles.iter().find(|r| r["id"] == "21").unwrap()["givable"],
        true
    );
    for bad in [
        json!({"post_channel": "11"}),
        json!({"post_channel": "10", "sub_role": "23"}),
        json!({"post_channel": "10", "faction_roles": {"myria": "999"}}),
        json!({"post_channel": "10", "faction_roles": {"wolves": "21"}}),
    ] {
        let (status, _) = e
            .request("PUT", "/api/me/discord", bad.clone(), true, true, false)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    e.call("PUT", "/api/me/discord", json!({"post_channel": "10", "sub_role": "20", "guild_role": "22", "faction_roles": {"myria": "21"}}))
        .await;

    // The test post reaches the chosen channel and can't ping anyone.
    e.call("POST", "/api/me/discord/test", Value::Null).await;
    let posted = taken(&calls);
    assert!(
        posted[0].contains(r#""allowed_mentions":{"parse":[]}"#),
        "{posted:?}"
    );

    // Role sync: a subscriber in Myria gets both roles; someone not in the server is skipped.
    linked(e, "700", "dc-fan", "myria").await;
    linked(e, "701", "dc-away", "myria").await;
    e.sql("INSERT INTO channel_subs(channel_id,user_id,tier,paid_through,months) VALUES('stream-owner','dc-fan',1,now()+interval '1 month',1)").await;
    sweep(e).await;
    let mut changes = taken(&calls);
    changes.sort();
    assert_eq!(changes, ["PUT 700 20", "PUT 700 21"]);
    // Nothing changes on the next sweep; a lapsed subscription takes back only that role.
    sweep(e).await;
    assert!(taken(&calls).is_empty());
    e.sql("UPDATE channel_subs SET paid_through=now()-interval '1 minute' WHERE user_id='dc-fan'")
        .await;
    sweep(e).await;
    assert_eq!(taken(&calls), ["DELETE 700 20"]);
    // A new subscription doesn't wait for the 15-minute sweep: its event marks the server due.
    e.sql("UPDATE channel_subs SET paid_through=now()+interval '1 month' WHERE user_id='dc-fan'")
        .await;
    sver::discord::tick(&e.app).await.unwrap();
    assert!(taken(&calls).is_empty(), "not due yet");
    let mut tx = e.app.db.begin().await.unwrap();
    sver::events::emit(
        &mut tx,
        "stream-owner",
        "subs",
        serde_json::json!({"user": "dc-fan", "tier": 1}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    sver::discord::tick(&e.app).await.unwrap();
    assert_eq!(taken(&calls), ["PUT 700 20"]);
    e.sql("UPDATE channel_subs SET paid_through=now()-interval '1 minute' WHERE user_id='dc-fan'")
        .await;
    sweep(e).await;
    assert_eq!(taken(&calls), ["DELETE 700 20"]);

    // Go-live: one post per broadcast with the title and link, under the go-live alert throttle.
    e.sql("UPDATE stream_settings SET title='Speedrun night' WHERE owner_id='stream-owner'")
        .await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('dc-live','stream-owner','dc-live',1,'LIVE','s','v','c',now(),now(),now())").await;
    sver::alerts::fan_out(&e.app).await.unwrap();
    sver::alerts::fan_out(&e.app).await.unwrap();
    sver::discord::tick(&e.app).await.unwrap();
    let posts = taken(&calls);
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(posts[0].contains("is live on S.V.E.R"));
    assert!(posts[0].contains("/Streamer"));
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='dc-live'")
        .await;

    // A role the bot can't give (hierarchy) shows the fix in Studio and pauses syncing.
    linked(e, "702", "dc-high", "myria").await;
    sweep(e).await;
    let problem = e.call("GET", "/api/me/discord", Value::Null).await["server"]["problem"].clone();
    assert!(
        problem.as_str().unwrap().contains("drag the S.V.E.R role"),
        "{problem}"
    );
    taken(&calls);

    // Removing it takes back its own roles and leaves the server.
    e.call("DELETE", "/api/me/discord", Value::Null).await;
    let mut cleanup = taken(&calls);
    cleanup.sort();
    assert_eq!(cleanup, ["DELETE 700 21", "LEAVE 900"]);
    assert_eq!(linked_rows().await, 0);

    gateway(e, &calls).await;
}

/// A fake Gateway: sends what the test hands it and reports what the bot sends. `{"close": code}`
/// closes the connection with that code; the next connection starts with Hello again.
async fn fake_gateway() -> (
    String,
    tokio::sync::mpsc::UnboundedSender<Value>,
    tokio::sync::mpsc::UnboundedReceiver<Value>,
) {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (to_bot, mut outgoing) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let (heard, from_bot) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let hello = json!({"op": 10, "d": {"heartbeat_interval": 60_000}});
            ws.send(Message::Text(hello.to_string().into()))
                .await
                .unwrap();
            loop {
                tokio::select! {
                    Some(frame) = outgoing.recv() => {
                        if let Some(code) = frame["close"].as_u64() {
                            let close = CloseFrame { code: CloseCode::from(code as u16), reason: "".into() };
                            let _ = ws.send(Message::Close(Some(close))).await;
                            break;
                        }
                        ws.send(Message::Text(frame.to_string().into())).await.unwrap();
                    }
                    message = ws.next() => match message {
                        Some(Ok(Message::Text(t))) => { let _ = heard.send(serde_json::from_str(&t).unwrap()); }
                        Some(Ok(_)) => {}
                        _ => break,
                    },
                }
            }
        }
    });
    (url, to_bot, from_bot)
}

/// The community bot (docs/DISCORD_BOT.md "How it runs"): identify, events, the staff settings,
/// slash commands, heartbeats on request, resuming, and a fatal close.
async fn gateway(e: &Env, calls: &Calls) {
    let taken = || std::mem::take(&mut *calls.lock().unwrap());
    let wait_for = |prefix: &'static str| async move {
        for _ in 0..100 {
            if let Some(line) = calls.lock().unwrap().iter().find(|l| l.starts_with(prefix)) {
                return line.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("no {prefix} call: {:?}", calls.lock().unwrap());
    };
    assert!(
        !sver::discord::gateway::enabled(),
        "off without DISCORD_GATEWAY"
    );
    let (url, to_bot, mut from_bot) = fake_gateway().await;
    // SAFETY: only this test reads these variables.
    unsafe {
        std::env::set_var("DISCORD_GATEWAY", "1");
        std::env::set_var("DISCORD_GATEWAY_URL", &url);
    }
    assert!(sver::discord::gateway::enabled());
    let bot = tokio::spawn(sver::discord::gateway::run(e.app.clone()));
    let next = |from_bot: &mut tokio::sync::mpsc::UnboundedReceiver<Value>| {
        let frame = from_bot.try_recv();
        frame.ok()
    };
    let heard = async |from_bot: &mut tokio::sync::mpsc::UnboundedReceiver<Value>| {
        tokio::time::timeout(std::time::Duration::from_secs(10), from_bot.recv())
            .await
            .expect("the bot sent nothing")
            .unwrap()
    };

    // Identify with the bot token and the community intents.
    let identify = heard(&mut from_bot).await;
    assert_eq!(identify["op"], 2);
    assert_eq!(identify["d"]["token"], "synthetic-bot-token");
    assert_eq!(identify["d"]["intents"], sver::discord::gateway::INTENTS);
    let dispatch = |s: u64, t: &str, d: Value| json!({"op": 0, "s": s, "t": t, "d": d});
    to_bot
        .send(dispatch(1, "READY", json!({"session_id": "s1", "resume_gateway_url": url,
            "application": {"id": "app1"}, "user": {"id": "500", "username": "S.V.E.R"}, "guilds": [{"id": "900"}]})))
        .unwrap();
    to_bot
        .send(dispatch(
            2,
            "GUILD_CREATE",
            json!({"id": "900", "name": "Test Server"}),
        ))
        .unwrap();
    for _ in 0..100 {
        let seen: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_guilds WHERE guild_id='900')")
                .fetch_one(&e.app.db)
                .await
                .unwrap();
        if seen {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    // No home server yet, so no commands were registered.
    assert!(taken().iter().all(|l| !l.starts_with("COMMANDS")));

    // Staff choose the home server. Channels and roles must be the server's own, of the right
    // kind, and roles the bot gives must sit below its own.
    let staff = super::bans::staff(e, "dc-staff", "DcStaff").await;
    let put = async |body: Value| {
        super::chat::call(e, "PUT", "/api/admin/discord", Some(&staff), body).await
    };
    for (bad, field) in [
        (json!({"guild_id": "901", "settings": {}}), "guild_id"),
        (
            json!({"guild_id": "900", "settings": {"mod_log": "11"}}),
            "mod_log",
        ),
        (
            json!({"guild_id": "900", "settings": {"myria": "23"}}),
            "myria",
        ),
        (
            json!({"guild_id": "900", "settings": {"zone": "Mars/Base"}}),
            "zone",
        ),
        (
            json!({"guild_id": "900", "settings": {"wolves": "10"}}),
            "settings",
        ),
    ] {
        let (status, body) = put(bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(body["field"], field, "{bad}");
    }
    let outsider = super::chat::call(e, "GET", "/api/admin/discord", None, Value::Null).await;
    assert_eq!(outsider.0, StatusCode::NOT_FOUND);
    let (status, view) = put(json!({"guild_id": "900", "settings": {"mod_log": "10", "myria": "21", "leveling": false, "zone": "America/New_York"}})).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["home"]["settings"]["mod_log"], "10");
    assert_eq!(view["home"]["settings"]["leveling"], false);
    assert_eq!(view["health"]["gateway"]["connected"], true);
    let missing = view["health"]["home"]["missing"].as_array().unwrap();
    assert!(missing.iter().any(|m| m == "Ban Members"), "{missing:?}");
    assert!(!missing.iter().any(|m| m == "Manage Roles"), "{missing:?}");
    assert_eq!(
        view["guilds"],
        json!([{"id": "900", "name": "Test Server"}])
    );
    // Saving registers the slash commands on that server.
    assert_eq!(wait_for("COMMANDS").await, "COMMANDS app1 900 2");
    taken();

    // /health for staff, refused for members, and the commands only answer on the home server.
    let ask = |s: u64, id: &str, guild: &str, permissions: &str, name: &str| {
        dispatch(
            s,
            "INTERACTION_CREATE",
            json!({"id": id, "token": "tk", "type": 2, "guild_id": guild,
            "member": {"user": {"id": "42"}, "permissions": permissions, "roles": []}, "data": {"name": name}}),
        )
    };
    to_bot.send(ask(3, "i1", "900", "32", "health")).unwrap();
    let reply = wait_for("REPLY i1").await;
    assert!(reply.contains("S.V.E.R bot health"), "{reply}");
    assert!(reply.contains(r#""flags":64"#), "{reply}");
    to_bot.send(ask(4, "i2", "900", "0", "health")).unwrap();
    assert!(wait_for("REPLY i2").await.contains("Only staff"));
    to_bot.send(ask(5, "i3", "901", "32", "help")).unwrap();
    assert!(
        wait_for("REPLY i3")
            .await
            .contains("work on the S.V.E.R server")
    );
    to_bot.send(ask(6, "i4", "900", "0", "help")).unwrap();
    let help = wait_for("REPLY i4").await;
    assert!(
        help.contains("/help") && !help.contains("/health"),
        "{help}"
    );

    // A heartbeat when Discord asks for one, with the last sequence number.
    while next(&mut from_bot).is_some() {}
    to_bot.send(json!({"op": 1})).unwrap();
    let beat = heard(&mut from_bot).await;
    assert_eq!(beat, json!({"op": 1, "d": 6}));

    // Reconnect: the bot comes back and resumes its session where it left off.
    to_bot.send(json!({"op": 7})).unwrap();
    let resume = heard(&mut from_bot).await;
    assert_eq!(resume["op"], 6);
    assert_eq!(resume["d"]["session_id"], "s1");
    assert_eq!(resume["d"]["seq"], 6);
    to_bot
        .send(json!({"op": 0, "s": 7, "t": "RESUMED", "d": {}}))
        .unwrap();

    // Refused intents stop the bot with the fix shown to staff.
    to_bot.send(json!({"close": 4014})).unwrap();
    let mut problem = None;
    for _ in 0..100 {
        problem = sqlx::query_scalar::<_, Option<String>>("SELECT problem FROM discord_gateway")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
        if problem.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        problem.unwrap_or_default().contains("Developer Portal"),
        "the intents problem is recorded"
    );
    bot.abort();
    // SAFETY: only this test reads these variables.
    unsafe {
        std::env::remove_var("DISCORD_GATEWAY");
        std::env::remove_var("DISCORD_GATEWAY_URL");
    }
}
