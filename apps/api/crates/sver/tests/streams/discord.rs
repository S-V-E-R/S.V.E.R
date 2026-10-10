//! The Discord bot (docs/COMMUNITY.md "Discord bot") against a fake Discord API: adding it to a
//! server, Studio's choices and checks, the go-live post, and role sync that only ever takes back
//! roles it gave.
use super::*;

type Calls = Arc<Mutex<Vec<String>>>;

fn fake(calls: Calls) -> Router {
    let log = move |line: String| calls.lock().unwrap().push(line);
    let (a, b, c, d) = (log.clone(), log.clone(), log.clone(), log);
    Router::new()
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
                    {"id": "30", "name": "S.V.E.R", "position": 5, "managed": true}
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
}
