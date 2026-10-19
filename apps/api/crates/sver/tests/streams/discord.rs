//! The Discord bot (docs/COMMUNITY.md "Discord bot") against a fake Discord API: adding it to a
//! server, Studio's choices and checks, the go-live post, and role sync that only ever takes back
//! roles it gave.
use super::*;

type Calls = Arc<Mutex<Vec<String>>>;

fn fake(calls: Calls) -> Router {
    let log = move |line: String| calls.lock().unwrap().push(line);
    let (a, b, c, d) = (log.clone(), log.clone(), log.clone(), log.clone());
    let (f, g) = (log.clone(), log.clone());
    let (h, k) = (log.clone(), log);
    Router::new()
        .route(
            "/channels/{id}/messages",
            post(move |Path(id): Path<String>, body: String| async move {
                h(format!("SEND {id} {body}"));
                Json(json!({"id": "m2"}))
            }),
        )
        .route(
            "/users/@me/channels",
            post(|| async { Json(json!({"id": "dm1"})) }),
        )
        .route(
            "/webhooks/{app}/{token}/messages/@original",
            axum::routing::patch(move |headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
                let kind = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").to_string();
                let png = body.windows(4).any(|w| w == b"\x89PNG");
                k(format!("FINISH {kind} png={png}"));
                Json(json!({}))
            }),
        )
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
    // SAFETY: only this test reads this variable.
    unsafe { std::env::set_var("DISCORD_CDN", format!("http://{address}")) };
    // The seeded holidays stay off here, so a run on one of their dates sees no multiplier.
    e.sql("UPDATE discord_holidays SET active=false").await;
    levels(e, &calls).await;
    factions(e).await;
    super::discord_moderation::exercise(e).await;
}

/// Levels and ranks, quests and achievements (docs/DISCORD_BOT.md), driven through the same
/// Gateway dispatch the live connection uses.
async fn levels(e: &Env, calls: &Calls) {
    use sver::discord::gateway::{dispatch, minute};
    let taken = || std::mem::take(&mut *calls.lock().unwrap());
    let xp = |user: &'static str| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT coalesce((SELECT total FROM discord_xp WHERE discord_user=$1),0)",
        )
        .bind(user)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
    };
    let say = |user: &'static str, content: &'static str| {
        json!({"id": "x", "guild_id": "900", "channel_id": "10", "content": content,
            "author": {"id": user}, "member": {"roles": []}})
    };
    let ready = || e.sql("UPDATE discord_xp SET message_at=now()-interval '2 minutes'");
    e.sql(r#"UPDATE discord_home SET settings=settings||'{"leveling":true,"rank_up":"12","achievements":"12","factions_channel":"12"}'"#).await;
    e.sql("UPDATE discord_ranks SET roles=jsonb_build_object('none','40'||rank,'myria','41'||rank,'aetheron','42'||rank,'glint','43'||rank)").await;
    e.sql("INSERT INTO discord_members(discord_user,faction) VALUES('800','myria')")
        .await;
    taken();

    // A message earns 15–25 XP and Myria chat points; the first rank comes silently with it.
    dispatch(&e.app, "MESSAGE_CREATE", say("800", "hello there")).await;
    let first = xp("800").await;
    assert!((15..=25).contains(&first), "{first}");
    let points: i64 = sqlx::query_scalar("SELECT coalesce(sum(points),0) FROM discord_points WHERE discord_user='800' AND faction='myria' AND kind='chat'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(points, first, "counted once, not twice");
    assert_eq!(taken(), ["PUT 800 411"]);
    // Within a minute, too short, a bot or another server: nothing.
    dispatch(&e.app, "MESSAGE_CREATE", say("800", "hello again")).await;
    ready().await;
    dispatch(&e.app, "MESSAGE_CREATE", say("800", "hey")).await;
    let mut bot = say("800", "beep boop beep");
    bot["author"]["bot"] = json!(true);
    dispatch(&e.app, "MESSAGE_CREATE", bot).await;
    let mut elsewhere = say("800", "hello elsewhere");
    elsewhere["guild_id"] = json!("901");
    dispatch(&e.app, "MESSAGE_CREATE", elsewhere).await;
    assert_eq!(xp("800").await, first);

    // A faction multiplier is for that faction only.
    e.sql("INSERT INTO discord_multipliers(name,factor,kind,target) VALUES('Glint day',3,'faction','glint'),('Myria day',2,'faction','myria')").await;
    dispatch(&e.app, "MESSAGE_CREATE", say("800", "doubled for myria")).await;
    let gain = xp("800").await - first;
    assert!((30..=50).contains(&gain), "{gain}");
    e.sql("DELETE FROM discord_multipliers").await;

    // The daily cap, after multipliers.
    e.sql("UPDATE discord_xp SET day_xp=1995 WHERE discord_user='800'")
        .await;
    ready().await;
    let before = xp("800").await;
    dispatch(&e.app, "MESSAGE_CREATE", say("800", "capped message")).await;
    assert_eq!(xp("800").await - before, 5);

    // Promotion: the rank role is swapped for the faction's next one, and it's announced.
    e.sql("UPDATE discord_xp SET total=495,day_xp=0 WHERE discord_user='800'")
        .await;
    ready().await;
    taken();
    dispatch(&e.app, "MESSAGE_CREATE", say("800", "promotion time")).await;
    let changes = taken();
    assert!(
        changes.contains(&"DELETE 800 411".to_string()),
        "{changes:?}"
    );
    assert!(changes.contains(&"PUT 800 412".to_string()), "{changes:?}");
    assert!(
        changes
            .iter()
            .any(|c| c.starts_with("SEND 12") && c.contains("Sentinel-II")),
        "{changes:?}"
    );
    // No XP in a channel staff turned it off for.
    e.sql(r#"UPDATE discord_home SET settings=settings||'{"xp_off":["10"]}'"#)
        .await;
    ready().await;
    let before = xp("800").await;
    dispatch(&e.app, "MESSAGE_CREATE", say("800", "no xp in here")).await;
    assert_eq!(xp("800").await, before);
    e.sql("UPDATE discord_home SET settings=settings-'xp_off'")
        .await;

    // Reactions: 5 XP, never on your own message.
    let react = |user: &str, author: &str| json!({"guild_id": "900", "channel_id": "10", "user_id": user, "message_author_id": author, "member": {"roles": []}});
    dispatch(&e.app, "MESSAGE_REACTION_ADD", react("801", "800")).await;
    dispatch(&e.app, "MESSAGE_REACTION_ADD", react("801", "800")).await;
    assert_eq!(xp("801").await, 5, "once per 30 seconds");
    dispatch(&e.app, "MESSAGE_REACTION_ADD", react("804", "804")).await;
    assert_eq!(xp("804").await, 0);

    // Voice: 3 XP a minute with company, 2 more while someone else streams; nothing alone.
    let voice = |user: &str, channel: Value, stream: bool| json!({"guild_id": "900", "user_id": user, "channel_id": channel, "self_stream": stream, "member": {"roles": []}});
    dispatch(
        &e.app,
        "VOICE_STATE_UPDATE",
        voice("800", json!("11"), false),
    )
    .await;
    dispatch(
        &e.app,
        "VOICE_STATE_UPDATE",
        voice("801", json!("11"), true),
    )
    .await;
    let due = || e.sql("UPDATE discord_voice SET credited_at=now()-interval '2 minutes'");
    due().await;
    let (a, b) = (xp("800").await, xp("801").await);
    minute(&e.app).await;
    assert_eq!(xp("800").await - a, 5, "voice and watching");
    assert_eq!(
        xp("801").await - b,
        3,
        "a streamer doesn't watch themselves"
    );
    dispatch(
        &e.app,
        "VOICE_STATE_UPDATE",
        voice("801", Value::Null, false),
    )
    .await;
    due().await;
    let a = xp("800").await;
    minute(&e.app).await;
    assert_eq!(xp("800").await, a, "alone");
    let minutes: (i32, i32) = sqlx::query_as(
        "SELECT voice_minutes,watch_minutes FROM discord_xp WHERE discord_user='800'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(minutes, (1, 1));
    // Connecting again rebuilds who is in voice from the server itself.
    dispatch(
        &e.app,
        "GUILD_CREATE",
        json!({"id": "900", "name": "Test Server", "afk_channel_id": "13",
        "voice_states": [{"user_id": "802", "channel_id": "11"}], "members": []}),
    )
    .await;
    let present: Vec<String> = sqlx::query_scalar("SELECT discord_user FROM discord_voice")
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    assert_eq!(present, ["802"]);

    // A weekly quest: complete once per week, rewarded with XP and faction points, and a DM.
    e.sql("INSERT INTO discord_members(discord_user,faction) VALUES('803','glint')")
        .await;
    e.sql(r#"INSERT INTO discord_quests(name,period,needs,xp,points) VALUES('Chatter','weekly','{"chat_messages":2}',100,10)"#).await;
    taken();
    for text in ["first message", "second message", "third message"] {
        ready().await;
        dispatch(&e.app, "MESSAGE_CREATE", say("803", text)).await;
    }
    let done: i64 = sqlx::query_scalar("SELECT count(*) FROM discord_quest_progress WHERE discord_user='803' AND completed_at IS NOT NULL")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(done, 1);
    assert!(xp("803").await >= 100 + 45);
    let quest_points: i64 = sqlx::query_scalar("SELECT coalesce(sum(points),0) FROM discord_points WHERE discord_user='803' AND kind='quest'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(quest_points, 10);
    assert!(
        taken()
            .iter()
            .any(|c| c.starts_with("SEND dm1") && c.contains("Chatter"))
    );

    // Achievements unlock from the numbers and are posted to their channel.
    e.sql("UPDATE discord_xp SET total=10000 WHERE discord_user='800'")
        .await;
    sver::discord::quests::check(&e.app, "800").await.unwrap();
    taken();
    minute(&e.app).await;
    assert!(
        taken()
            .iter()
            .any(|c| c.starts_with("SEND 12") && c.contains("Rising Star")),
        "level 10"
    );

    // /rank answers at once and then sends the card; /leaderboard lists members; /xpmult is staff-only.
    let ask = |id: &str, name: &str, permissions: &str, options: Value| {
        json!({"id": id, "token": "tk", "type": 2, "guild_id": "900",
            "member": {"user": {"id": "800", "username": "eight"}, "permissions": permissions, "roles": []},
            "data": {"name": name, "options": options}})
    };
    taken();
    dispatch(
        &e.app,
        "INTERACTION_CREATE",
        ask("r1", "rank", "0", json!([])),
    )
    .await;
    let mut finished = false;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c == "FINISH multipart/form-data png=true")
        {
            finished = true;
            break;
        }
    }
    let log = taken();
    assert!(finished, "{log:?}");
    assert!(
        log.iter()
            .any(|c| c.starts_with("REPLY r1") && c.contains(r#""type":5"#)),
        "{log:?}"
    );
    dispatch(
        &e.app,
        "INTERACTION_CREATE",
        ask("r2", "leaderboard", "0", json!([])),
    )
    .await;
    dispatch(
        &e.app,
        "INTERACTION_CREATE",
        ask("r3", "xpmult", "0", json!([{"type": 1, "name": "list"}])),
    )
    .await;
    dispatch(&e.app, "INTERACTION_CREATE", ask("r4", "xpmult", "32", json!([{"type": 1, "name": "create", "options": [
        {"name": "name", "value": "Weekend"}, {"name": "factor", "value": 1.5}, {"name": "for", "value": "event"}]}]))).await;
    let mut replies = Vec::new();
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        replies = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("REPLY r"))
            .cloned()
            .collect();
        if replies.len() == 3 {
            break;
        }
    }
    let reply = |id: &str| {
        replies
            .iter()
            .find(|r| r.starts_with(&format!("REPLY {id} ")))
            .cloned()
            .unwrap_or_default()
    };
    assert!(reply("r2").contains("<@800>"), "{replies:?}");
    assert!(reply("r3").contains("Only staff"), "{replies:?}");
    assert!(reply("r4").contains("Weekend"), "{replies:?}");
    let factor: f64 =
        sqlx::query_scalar("SELECT factor::float8 FROM discord_multipliers WHERE name='Weekend'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(factor, 1.5);
}

/// Every call as "METHOD path body", against members 910 (linked, holds role 66 by hand) and 912
/// (linked, unverified); anyone else isn't in the server.
async fn fake_home(
    State(calls): State<Calls>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    body: String,
) -> (StatusCode, Json<Value>) {
    let path = uri.path().to_string();
    calls
        .lock()
        .unwrap()
        .push(format!("{method} {path} {body}"));
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let answer = match (method.as_str(), parts.as_slice()) {
        ("GET", ["users", "@me"]) => json!({"id": "500"}),
        ("GET", ["guilds", "900", "members", "500"]) => json!({"roles": ["30"]}),
        ("GET", ["guilds", "900", "members", "910"]) => {
            json!({"user": {"id": "910"}, "roles": ["66"], "nick": null})
        }
        ("GET", ["guilds", "900", "members", "912"]) => json!({"user": {"id": "912"}, "roles": []}),
        ("GET", ["guilds", "900", "members", _]) => {
            return (StatusCode::NOT_FOUND, Json(json!({"code": 10007})));
        }
        ("GET", ["guilds", "900", "channels"]) => json!([
            {"id": "10", "name": "general", "type": 0}, {"id": "11", "name": "voice", "type": 2},
            {"id": "14", "name": "category", "type": 4}, {"id": "15", "name": "off-topic", "type": 0}]),
        ("GET", ["guilds", "900", "roles"]) => json!([
            {"id": "900", "name": "@everyone", "position": 0, "permissions": "0"},
            {"id": "61", "name": "Myria", "position": 1}, {"id": "62", "name": "Aetheron", "position": 1},
            {"id": "63", "name": "Glint", "position": 1}, {"id": "64", "name": "Wolfsworn", "position": 1},
            {"id": "65", "name": "Founder", "position": 1}, {"id": "66", "name": "Level 1", "position": 1},
            {"id": "67", "name": "Level 5", "position": 1}, {"id": "69", "name": "Admin", "position": 9},
            {"id": "30", "name": "S.V.E.R", "position": 5, "managed": true, "permissions": "268435456"}]),
        ("POST", ["channels", _, "messages"]) => json!({"id": "m1"}),
        _ => json!({}),
    };
    (StatusCode::OK, Json(answer))
}

/// Factions and the S.V.E.R sync (docs/DISCORD_BOT.md "Factions in Discord", "Account link and
/// sync"): influence, `/faction join`, seasons, holidays, territories, role and level sync,
/// nicknames and the Discord rank on profiles. The tuning here is the repo's example values.
async fn factions(e: &Env) {
    use sver::discord::{
        commands::interaction, factions, gateway::dispatch, gateway::minute, sync,
    };
    let calls: Calls = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new().fallback(fake_home).with_state(calls.clone());
    tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    // SAFETY: only the Discord tests read these variables, one at a time.
    unsafe { std::env::set_var("DISCORD_API", format!("http://{address}")) };
    let taken = || std::mem::take(&mut *calls.lock().unwrap());
    let has = |c: &[String], needle: &str| c.iter().any(|l| l.contains(needle));
    let count = |sql: &'static str| async move {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    // A command or button, answered before this returns; the logged answer comes back.
    let ask = async |id: &str, user: &str, staff: bool, kind: u64, data: Value| {
        interaction(&e.app, json!({"id": id, "token": "tk", "type": kind, "guild_id": "900", "channel_id": "10",
            "member": {"user": {"id": user}, "permissions": if staff { "32" } else { "0" }, "roles": []}, "data": data}))
            .await;
        calls
            .lock()
            .unwrap()
            .iter()
            .find(|l| l.starts_with(&format!("POST /interactions/{id}/")))
            .cloned()
            .unwrap_or_default()
    };
    let sub = |name: &str, sub: &str, options: Value| json!({"name": name, "options": [{"type": 1, "name": sub, "options": options}]});
    // Background syncs after a command settle before the next look.
    let settle = || tokio::time::sleep(std::time::Duration::from_millis(400));

    // Staff map channels to genres: the server's text or voice channels, real genres only.
    let staff = super::bans::staff(e, "fx-staff", "FxStaff").await;
    let put = async |settings: Value| {
        super::chat::call(
            e,
            "PUT",
            "/api/admin/discord",
            Some(&staff),
            json!({"guild_id": "900", "settings": settings}),
        )
        .await
    };
    for bad in [
        json!({"channel_genres": {"10": "no_such_genre"}}),
        json!({"channel_genres": {"14": "fighting"}}),
        json!({"channel_genres": {"99": "fighting"}}),
    ] {
        let (status, body) = put(bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(body["field"], "channel_genres", "{bad}");
    }
    let (status, view) = put(json!({"channel_genres": {"10": "fighting", "11": "fighting"}, "rank_up": "10", "factions_channel": "10",
        "myria": "61", "aetheron": "62", "glint": "63", "sver_wolfsworn": "64", "sver_founder": "65"}))
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert!(
        view["genres"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "fighting")
    );

    // A S.V.E.R war week with territories.
    e.sql("INSERT INTO faction_seasons(number,starts_at,ends_at,next_starts_at) VALUES(1,now()-interval '1 hour',now()+interval '30 days',now()+interval '37 days')").await;
    e.sql("INSERT INTO faction_weeks(season_id,starts_at,ends_at) SELECT id,now()-interval '1 hour',now()+interval '6 days' FROM faction_seasons").await;
    e.sql("INSERT INTO faction_territories(season_id,genre,holder) SELECT s.id,g.id,g.home FROM faction_seasons s,faction_genres g").await;
    linked(e, "910", "fx-linked", "glint").await;
    linked(e, "911", "fx-away", "myria").await;
    linked(e, "912", "fx-unverified", "myria").await;
    e.sql("UPDATE users SET email_verified=false WHERE id='fx-unverified'")
        .await;

    // Influence: a voice minute in a mapped voice channel, for a linked and verified member.
    e.sql(r#"UPDATE discord_home SET settings=settings||'{"leveling":true}'"#)
        .await;
    let voice = |user: &str| json!({"guild_id": "900", "user_id": user, "channel_id": "11", "member": {"roles": []}});
    dispatch(&e.app, "VOICE_STATE_UPDATE", voice("910")).await;
    dispatch(&e.app, "VOICE_STATE_UPDATE", voice("913")).await;
    e.sql("UPDATE discord_voice SET credited_at=now()-interval '2 minutes'")
        .await;
    minute(&e.app).await;
    assert_eq!(
        count("SELECT count(*) FROM faction_influence WHERE source='discord' AND user_id='fx-linked' AND points=1").await,
        1
    );
    e.sql("DELETE FROM discord_voice").await;
    // Chat in a mapped channel too, up to the daily cap; replays, unmapped channels, unverified and
    // unlinked members earn nothing.
    let say = |id: String, user: &str, channel: &str| json!({"id": id, "guild_id": "900", "channel_id": channel, "content": "for the war", "author": {"id": user}, "member": {"roles": []}});
    for n in 0..12 {
        dispatch(&e.app, "MESSAGE_CREATE", say(format!("fx{n}"), "910", "10")).await;
    }
    dispatch(&e.app, "MESSAGE_CREATE", say("fx0".into(), "910", "10")).await;
    dispatch(&e.app, "MESSAGE_CREATE", say("fx-off".into(), "910", "15")).await;
    dispatch(&e.app, "MESSAGE_CREATE", say("fx-unv".into(), "912", "10")).await;
    dispatch(&e.app, "MESSAGE_CREATE", say("fx-unl".into(), "913", "10")).await;
    let (rows, points): (i64, i64) = sqlx::query_as("SELECT count(*),coalesce(sum(points),0)::bigint FROM faction_influence WHERE source='discord'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!((rows, points), (13, 10), "the cap is 10");
    assert_eq!(
        count("SELECT count(*) FROM faction_influence WHERE source='discord' AND (user_id<>'fx-linked' OR genre<>'fighting' OR faction<>'glint')").await,
        0
    );
    settle().await;
    taken();

    // /faction join: an unlinked member chooses; switching asks first.
    let join = |f: &str| sub("faction", "join", json!([{"name": "faction", "value": f}]));
    let reply = ask("j1", "913", false, 2, join("myria")).await;
    assert!(reply.contains("Welcome to **Myria**"), "{reply}");
    let c = taken();
    for call in [
        "PUT /guilds/900/members/913/roles/61",
        "DELETE /guilds/900/members/913/roles/62",
        "DELETE /guilds/900/members/913/roles/63",
    ] {
        assert!(has(&c, call), "{call} in {c:#?}");
    }
    assert_eq!(count("SELECT count(*) FROM discord_achievements WHERE discord_user='913' AND key='faction_joined'").await, 1);
    let reply = ask("j2", "913", false, 2, join("glint")).await;
    assert!(
        reply.contains("fac:join:glint") && reply.contains("Switch from **Myria**"),
        "{reply}"
    );
    assert!(
        !has(&taken(), "/roles/63"),
        "nothing changes before the confirm"
    );
    let reply = ask(
        "j3",
        "913",
        false,
        3,
        json!({"custom_id": "fac:cancel", "component_type": 2}),
    )
    .await;
    assert!(reply.contains("Nothing changed"), "{reply}");
    let reply = ask(
        "j4",
        "913",
        false,
        3,
        json!({"custom_id": "fac:join:glint", "component_type": 2}),
    )
    .await;
    assert!(
        reply.contains(r#""type":7"#) && reply.contains("Welcome to **Glint**"),
        "{reply}"
    );
    let c = taken();
    assert!(
        has(&c, "PUT /guilds/900/members/913/roles/63")
            && has(&c, "DELETE /guilds/900/members/913/roles/61"),
        "{c:#?}"
    );
    assert_eq!(
        count("SELECT count(*) FROM discord_members WHERE discord_user='913' AND faction='glint'")
            .await,
        1
    );
    // A linked member's S.V.E.R faction wins.
    let reply = ask("j5", "910", false, 2, join("myria")).await;
    assert!(
        reply.contains("comes from your S.V.E.R account") && reply.contains("Glint"),
        "{reply}"
    );
    assert_eq!(
        count("SELECT count(*) FROM discord_members WHERE discord_user='910' AND faction='glint'")
            .await,
        1
    );
    settle().await;
    taken();

    // Seasons: points inside the window count; the winner gets 1.5× for 14 days when it ends.
    let reply = ask(
        "s1",
        "42",
        true,
        2,
        sub(
            "season",
            "start",
            json!([{"name": "name", "value": "Autumn"}, {"name": "days", "value": 30}]),
        ),
    )
    .await;
    assert!(reply.contains("Season 1, **Autumn**"), "{reply}");
    let reply = ask(
        "s2",
        "42",
        false,
        2,
        sub(
            "season",
            "start",
            json!([{"name": "name", "value": "Mine"}, {"name": "days", "value": 3}]),
        ),
    )
    .await;
    assert!(reply.contains("Only staff"), "{reply}");
    e.sql("UPDATE discord_seasons SET starts_at=now()-interval '1 day'")
        .await;
    e.sql("INSERT INTO discord_points(discord_user,faction,kind,points,at) VALUES('913','myria','chat',50,now()),('910','glint','chat',20,now()),('912','aetheron','chat',1000,now()-interval '2 days')").await;
    let reply = ask(
        "s3",
        "913",
        false,
        2,
        sub("faction", "leaderboard", json!([])),
    )
    .await;
    assert!(
        reply.contains("<@913>") && !reply.contains("<@912>"),
        "only this season: {reply}"
    );
    let reply = ask("s4", "913", false, 2, sub("faction", "stats", json!([]))).await;
    assert!(
        reply.contains("Season 1: Autumn") && !reply.contains("1,0"),
        "before the season: {reply}"
    );
    minute(&e.app).await;
    assert_eq!(
        count("SELECT count(*) FROM discord_seasons WHERE ended_at IS NULL").await,
        1,
        "not due yet"
    );
    e.sql("UPDATE discord_seasons SET ends_at=now()-interval '1 second'")
        .await;
    taken();
    minute(&e.app).await;
    assert_eq!(
        count("SELECT count(*) FROM discord_seasons WHERE winner='myria' AND ended_at IS NOT NULL")
            .await,
        1
    );
    assert_eq!(count("SELECT count(*) FROM discord_multipliers WHERE kind='faction' AND target='myria' AND factor=1.5 AND ends_at BETWEEN now()+interval '13 days' AND now()+interval '15 days'").await, 1);
    let c = taken();
    assert!(
        c.iter()
            .any(|l| l.starts_with("POST /channels/10/messages") && l.contains("**Myria** wins")),
        "{c:#?}"
    );
    minute(&e.app).await;
    assert!(!has(&taken(), "wins"), "announced once");

    // A holiday runs once on its day: the multiplier until midnight UTC, points, a post.
    e.sql("INSERT INTO discord_holidays(faction,name,month,day,factor,points) VALUES('glint','Test Day',extract(month FROM now() AT TIME ZONE 'UTC'),extract(day FROM now() AT TIME ZONE 'UTC'),2,7)").await;
    minute(&e.app).await;
    minute(&e.app).await;
    let c = taken();
    assert_eq!(
        c.iter().filter(|l| l.contains("Test Day")).count(),
        1,
        "{c:#?}"
    );
    assert_eq!(count("SELECT count(*) FROM discord_multipliers WHERE name='Test Day' AND target='glint' AND ends_at>now() AND ends_at<=now()+interval '1 day'").await, 1);
    assert_eq!(
        count("SELECT count(*) FROM discord_points WHERE kind='holiday' AND points=7").await,
        count("SELECT count(*) FROM discord_members WHERE faction='glint'").await
    );
    let reply = ask("h1", "42", true, 2, sub("holiday", "list", json!([]))).await;
    assert!(
        reply.contains("Test Day") && reply.contains("Winter Luminance"),
        "{reply}"
    );
    let reply = ask("h2", "42", true, 2, sub("holiday", "create", json!([{"name": "name", "value": "Bad"}, {"name": "faction", "value": "glint"},
        {"name": "month", "value": 2}, {"name": "day", "value": 30}, {"name": "factor", "value": 2}, {"name": "points", "value": 1}]))).await;
    assert!(reply.contains("a real date"), "{reply}");

    // Territories: the first look records; a change of hands is posted once.
    factions::territory_posts(&e.app).await.unwrap();
    assert!(!has(&taken(), "taken control"));
    e.sql("UPDATE faction_territories SET holder='glint' WHERE genre='fighting'")
        .await;
    factions::territory_posts(&e.app).await.unwrap();
    factions::territory_posts(&e.app).await.unwrap();
    let c = taken();
    assert_eq!(
        c.iter()
            .filter(|l| l.contains("**Glint** has taken control of **Fighting**!"))
            .count(),
        1,
        "{c:#?}"
    );
    let reply = ask("t1", "913", false, 2, json!({"name": "territories"})).await;
    assert!(reply.contains("**Fighting** · Glint"), "{reply}");

    // Role sync: Wolfsworn and the level band; the band role 910 already holds isn't the bot's.
    let reply = ask(
        "l1",
        "42",
        true,
        2,
        sub(
            "levelroles",
            "add",
            json!([{"name": "level", "value": 1}, {"name": "role", "value": "66"}]),
        ),
    )
    .await;
    assert!(reply.contains("<@&66>"), "{reply}");
    let reply = ask(
        "l2",
        "42",
        true,
        2,
        sub(
            "levelroles",
            "add",
            json!([{"name": "level", "value": 5}, {"name": "role", "value": "69"}]),
        ),
    )
    .await;
    assert!(reply.contains("below the bot"), "{reply}");
    ask(
        "l3",
        "42",
        true,
        2,
        sub(
            "levelroles",
            "add",
            json!([{"name": "level", "value": 5}, {"name": "role", "value": "67"}]),
        ),
    )
    .await;
    let reply = ask(
        "n1",
        "910",
        false,
        2,
        sub(
            "link",
            "nickname",
            json!([{"name": "state", "value": "on"}]),
        ),
    )
    .await;
    assert!(reply.contains("follows your S.V.E.R username"), "{reply}");
    let reply = ask("n2", "913", false, 2, sub("link", "sver", json!([]))).await;
    assert!(
        reply.contains("/account") && reply.contains("isn't linked"),
        "{reply}"
    );
    let reply = ask("n3", "910", false, 2, sub("link", "status", json!([]))).await;
    assert!(reply.contains("linked to **fx-linked**"), "{reply}");
    settle().await;
    e.sql("INSERT INTO platform_subscriptions(user_id,tier,paid_through) VALUES('fx-linked',1,now()+interval '1 month')").await;
    e.sql("UPDATE discord_members SET sver_level=NULL WHERE discord_user='910'")
        .await;
    taken();
    sync::all(&e.app).await.unwrap();
    let c = taken();
    assert!(has(&c, "PUT /guilds/900/members/910/roles/64"), "{c:#?}");
    assert!(!has(&c, "/members/910/roles/66"), "already held: {c:#?}");
    assert!(
        has(&c, r#"PATCH /guilds/900/members/910 {"nick":"fx-linked"}"#),
        "{c:#?}"
    );
    assert!(
        !has(&c, "PATCH /guilds/900/members/912"),
        "nickname is opt-in"
    );
    assert!(!has(&c, "/members/911/roles"), "not in the server");
    assert!(
        !has(&c, "reached **Level"),
        "the first level seen is only recorded"
    );
    assert_eq!(count("SELECT count(*) FROM discord_achievements WHERE discord_user='910' AND key='sver_subscriber'").await, 1);
    // Level 5 and a lapsed subscription: the new band is given, the bot's grant taken back, the
    // band role it never gave left alone, and the level-up announced.
    e.sql(
        "INSERT INTO xp_days(user_id,day,source,xp) VALUES('fx-linked',current_date,'watch',800)",
    )
    .await;
    e.sql("UPDATE platform_subscriptions SET paid_through=now()-interval '1 minute' WHERE user_id='fx-linked'").await;
    sync::one(&e.app, "910").await.unwrap();
    let c = taken();
    assert!(has(&c, "PUT /guilds/900/members/910/roles/67"), "{c:#?}");
    assert!(has(&c, "DELETE /guilds/900/members/910/roles/64"), "{c:#?}");
    assert!(
        !has(&c, "DELETE /guilds/900/members/910/roles/66"),
        "{c:#?}"
    );
    assert!(
        c.iter().any(|l| l.starts_with("POST /channels/10/messages")
            && l.contains("<@910> reached **Level 5** on S.V.E.R!")),
        "{c:#?}"
    );
    // Unlinked: everything the sync gave is taken back.
    e.sql("DELETE FROM identities WHERE subject='910'").await;
    sync::all(&e.app).await.unwrap();
    assert!(has(&taken(), "DELETE /guilds/900/members/910/roles/67"));
    assert_eq!(
        count("SELECT count(*) FROM discord_home_grants WHERE discord_user='910'").await,
        0
    );

    // The Discord rank on S.V.E.R: only for a linked account, and only when its owner shows it.
    let (status, _) = e
        .request(
            "PUT",
            "/api/me/discord-rank",
            json!({"shown": true}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    e.sql(
        "INSERT INTO identities(provider,subject,user_id) VALUES('discord','914','stream-owner')",
    )
    .await;
    e.sql("INSERT INTO discord_xp(discord_user,total) VALUES('914',600)")
        .await;
    e.sql("INSERT INTO discord_members(discord_user,faction) VALUES('914','aetheron')")
        .await;
    let mine = e.call("GET", "/api/me/discord-rank", Value::Null).await;
    assert_eq!(
        mine,
        json!({"linked": true, "shown": false, "rank": {"name": "Seeker", "level": 2, "faction": "aetheron"}})
    );
    let channel = || e.call("GET", "/api/channels/Streamer", Value::Null);
    assert_eq!(channel().await["channel"]["discord_rank"], Value::Null);
    e.call("PUT", "/api/me/discord-rank", json!({"shown": true}))
        .await;
    assert_eq!(channel().await["channel"]["discord_rank"]["name"], "Seeker");
    e.call("PUT", "/api/me/discord-rank", json!({"shown": false}))
        .await;
    assert_eq!(channel().await["channel"]["discord_rank"], Value::Null);
}

/// A fake Gateway: sends what the test hands it and reports what the bot sends. `{"close": code}`
/// closes the connection with that code; the next connection starts with Hello again.
pub(super) async fn fake_gateway() -> (
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
    assert_eq!(
        wait_for("COMMANDS").await,
        format!(
            "COMMANDS app1 900 {}",
            sver::discord::commands::definitions().len()
        )
    );
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
    // Up to 20 seconds: the full suite runs beside other heavy tests.
    for _ in 0..400 {
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
