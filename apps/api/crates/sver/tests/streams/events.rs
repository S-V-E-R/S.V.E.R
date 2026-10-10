//! Live events (docs/DEVELOPER_PLATFORM.md §2): public topics with a client ID, private ones only
//! with the owner's `events:private` token, fan-out from the outbox, replay after an ID, ping.
use super::chat::{call, next_json, person};
use super::*;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

pub async fn exercise(e: &Env) {
    let created = e
        .call(
            "POST",
            "/api/me/apps",
            json!({"name":"Alerts Overlay","redirect_uris":["https://example.test/cb"]}),
        )
        .await;
    let client = created["created"]["client_id"]
        .as_str()
        .unwrap()
        .to_string();
    // An owner's token with events:private (as the OAuth flow would issue).
    e.sql("INSERT INTO oauth_grants(id,app_id,user_id,scopes) SELECT 'ev-grant',id,'stream-owner',ARRAY['events:private','chat:write','user:read','whispers:read'] FROM dev_apps WHERE name='Alerts Overlay'").await;
    sqlx::query("INSERT INTO oauth_tokens(token_hash,grant_id,kind,expires_at) VALUES($1,'ev-grant','access',now()+interval '1 hour')")
        .bind(sver::security::digest("ev-owner-token")).execute(&e.app.db).await.unwrap();
    sver::events::drain(&e.app).await.unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let connect = |query: String| async move {
        tokio_tungstenite::connect_async(
            format!("ws://{address}/api/events?{query}")
                .into_client_request()
                .unwrap(),
        )
        .await
    };
    match connect("client_id=sv_nobody".into()).await {
        Err(tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED)
        }
        other => panic!("an unknown app is refused, got {:?}", other.map(|_| ())),
    }
    let send = |text: Value| tungstenite::Message::Text(text.to_string().into());
    let (mut public, _) = connect(format!("client_id={client}")).await.unwrap();
    public.send(send(json!({"type":"method","id":1,"method":"subscribe","params":{"topics":["channel:Streamer:follows","channel:streamer:subs","channel:streamer:nonsense"]}}))).await.unwrap();
    let reply = next_json(&mut public).await;
    assert_eq!(
        (&reply["id"], &reply["result"]["subscribed"]),
        (&json!(1), &json!(["channel:streamer:follows"])),
        "{reply}"
    );
    assert_eq!(
        reply["result"]["refused"].as_array().unwrap().len(),
        2,
        "private needs the owner's token"
    );
    let (mut owner, _) = connect(format!("client_id={client}&access_token=ev-owner-token"))
        .await
        .unwrap();
    owner.send(send(json!({"type":"method","id":"a","method":"subscribe","params":{"topics":["channel:streamer:follows:detail"]}}))).await.unwrap();
    assert_eq!(
        next_json(&mut owner).await["result"]["subscribed"],
        json!(["channel:streamer:follows:detail"])
    );

    // A follow: the count is public, the follower's name only on the private topic.
    let fan = person(e, "ev-fan", "EvFan", true).await;
    call(e, "PUT", "/api/follows/Streamer", Some(&fan), Value::Null).await;
    sver::events::drain(&e.app).await.unwrap();
    let count = next_json(&mut public).await;
    assert_eq!(count["topic"], "channel:streamer:follows");
    assert!(count["data"]["followers"].as_i64().unwrap() >= 1);
    assert!(
        !count.to_string().contains("EvFan"),
        "public events don't name followers"
    );
    let detail = next_json(&mut owner).await;
    assert_eq!(detail["data"]["user"], "EvFan");

    // Replay after an ID, and ping.
    let since = count["id"].as_i64().unwrap() - 1;
    let (mut late, _) = connect(format!("client_id={client}")).await.unwrap();
    late.send(send(json!({"type":"method","id":2,"method":"subscribe","params":{"topics":["channel:streamer:follows"],"since":since}}))).await.unwrap();
    next_json(&mut late).await;
    assert_eq!(
        next_json(&mut late).await["id"],
        count["id"],
        "missed events are replayed"
    );
    late.send(send(json!({"type":"method","id":3,"method":"ping"})))
        .await
        .unwrap();
    assert_eq!(next_json(&mut late).await["result"], "pong");

    // Chat over the socket: an app with chat:write posts as the person; readers need only the
    // client ID. Apps can't spend Valor.
    late.send(send(
        json!({"type":"method","id":4,"method":"subscribe","params":{"topics":["chat:Streamer"]}}),
    ))
    .await
    .unwrap();
    assert_eq!(
        next_json(&mut late).await["result"]["subscribed"],
        json!(["chat:streamer"])
    );
    let say = |id: i64, extra: Value| {
        let mut params = json!({"channel":"streamer","id":uuid::Uuid::new_v4().to_string(),"body":"Hello from an app"});
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        send(json!({"type":"method","id":id,"method":"send","params":params}))
    };
    owner.send(say(5, json!({"tribute": 10}))).await.unwrap();
    assert_eq!(
        next_json(&mut owner).await["error"],
        "Apps can't spend Valor."
    );
    owner.send(say(6, json!({}))).await.unwrap();
    assert_eq!(
        next_json(&mut owner).await["result"]["message"]["body"],
        "Hello from an app"
    );
    let heard = next_json(&mut late).await;
    assert_eq!(
        (
            heard["topic"].as_str(),
            heard["data"]["message"]["body"].as_str()
        ),
        (Some("chat:streamer"), Some("Hello from an app"))
    );
    late.send(say(7, json!({}))).await.unwrap();
    assert_eq!(
        next_json(&mut late).await["error"],
        "Sending chat needs an access token."
    );
    late.send(send(
        json!({"type":"method","id":8,"method":"history","params":{"topic":"chat:streamer"}}),
    ))
    .await
    .unwrap();
    let history = next_json(&mut late).await;
    assert_eq!(
        history["result"]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["body"],
        "Hello from an app"
    );

    // The person's own topics, only with their own token.
    late.send(send(json!({"type":"method","id":9,"method":"subscribe","params":{"topics":["user:streamer:notifications"]}}))).await.unwrap();
    assert_eq!(
        next_json(&mut late).await["result"]["refused"],
        json!(["user:streamer:notifications"])
    );
    owner.send(send(json!({"type":"method","id":10,"method":"subscribe","params":{"topics":["user:streamer:notifications","user:streamer:whispers"]}}))).await.unwrap();
    assert_eq!(
        next_json(&mut owner).await["result"]["subscribed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    e.app.chat.publish(
        "dm:stream-owner",
        None,
        1,
        json!({"type":"dm","with":"EvFan","message":{"body":"psst"}}),
    );
    let whisper = next_json(&mut owner).await;
    assert_eq!(
        (
            whisper["topic"].as_str(),
            whisper["data"]["message"]["body"].as_str()
        ),
        (Some("user:streamer:whispers"), Some("psst"))
    );
    e.sql("INSERT INTO notifications(id,user_id,kind,channel_id,event_key,payload) VALUES('ev-note','stream-owner','sub_ending','stream-owner','ev-note',jsonb_build_object('title','Hi','body','There','url','/'))").await;
    // Notifications are polled every 5 seconds; next_json waits 5 more.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let notice = next_json(&mut owner).await;
    assert_eq!(
        (
            notice["topic"].as_str(),
            notice["data"]["payload"]["title"].as_str()
        ),
        (Some("user:streamer:notifications"), Some("Hi"))
    );
    e.sql("DELETE FROM notifications WHERE id='ev-note'").await;

    // Stream details are a public update event.
    let settings = e.mine().await["settings"].clone();
    e.call(
        "PATCH",
        "/api/me/stream",
        json!({"title":"Events title","category_id":e.call("GET", "/api/categories", Value::Null).await["categories"][0]["id"],"revision":settings["revision"]}),
    )
    .await;
    let updated: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events WHERE topic='channel:streamer:update' AND data->>'title'='Events title')")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(updated);
    sqlx::query("UPDATE stream_settings SET title=$1,category_id=$2,revision=$3 WHERE owner_id='stream-owner'")
        .bind(settings["title"].as_str())
        .bind(settings["category_id"].as_str())
        .bind(settings["revision"].as_i64())
        .execute(&e.app.db)
        .await
        .unwrap();

    // ---- Webhooks for the same events ----
    let url = format!("{}/hook", e.app.config.stripe.api_url);
    let (status, _) = e
        .request(
            "POST",
            "/api/hooks",
            json!({"topics":["channel:evfan:subs"],"url":url}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "another channel's private topic"
    );
    let mine = e
        .call(
            "POST",
            "/api/hooks",
            json!({"topics":["channel:Streamer:follows","channel:streamer:follows:detail"],"url":url}),
        )
        .await;
    let secret = mine["created"]["secret"].as_str().unwrap().to_string();
    // An app registers its own with the person's token, server to server (no Origin, no cookie).
    let http = reqwest::Client::new();
    let api = format!("http://{address}/api/hooks");
    let denied = http
        .post(&api)
        .bearer_auth("ev-owner-token")
        .json(&json!({"topics":["channel:streamer:subs"],"url":url}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        denied.status().as_u16(),
        401,
        "a token needs its app's client ID"
    );
    let by_app = http
        .post(&api)
        .bearer_auth("ev-owner-token")
        .header("sver-client-id", &client)
        .json(&json!({"topics":["channel:streamer:subs"],"url":url}))
        .send()
        .await
        .unwrap();
    assert_eq!(by_app.status().as_u16(), 200);
    let listed: Value = http
        .get(&api)
        .bearer_auth("ev-owner-token")
        .header("sver-client-id", &client)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        listed["hooks"].as_array().unwrap().len(),
        1,
        "an app sees only its own hooks"
    );

    // A follow reaches the person's hook on both topics, signed.
    e.fake.lock().unwrap().hooks.clear();
    let fan2 = person(e, "ev-fan2", "EvFanTwo", true).await;
    call(e, "PUT", "/api/follows/Streamer", Some(&fan2), Value::Null).await;
    sver::events::deliver_hooks(&e.app).await.unwrap();
    {
        let fake = e.fake.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let mut topics: Vec<String> = fake
            .hooks
            .iter()
            .map(|(signature, body)| {
                assert!(sver::stripe::verify(
                    &secret,
                    signature,
                    body.as_bytes(),
                    now
                ));
                serde_json::from_str::<Value>(body).unwrap()["topic"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        topics.sort();
        assert_eq!(
            topics,
            [
                "channel:streamer:follows",
                "channel:streamer:follows:detail"
            ]
        );
    }
    // The owner's private events never show someone they blocked.
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('stream-owner','ev-fan2')")
        .await;
    let mut db = e.app.db.acquire().await.unwrap();
    sver::events::emit(
        &mut db,
        "stream-owner",
        "follows:detail",
        json!({"user":"EvFanTwo"}),
    )
    .await
    .unwrap();
    let shown: i64 = sqlx::query_scalar("SELECT count(*) FROM events WHERE topic='channel:streamer:follows:detail' AND data->>'user'='EvFanTwo'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(shown, 1, "only the follow from before the block");
    e.sql("DELETE FROM user_blocks WHERE blocker_id='stream-owner'")
        .await;
    // Revoking the app removes its hook before anything more is sent.
    e.sql("UPDATE oauth_grants SET revoked_at=now() WHERE id='ev-grant'")
        .await;
    sver::events::emit(
        &mut db,
        "stream-owner",
        "subs",
        json!({"user":"EvFan","tier":1,"months":1}),
    )
    .await
    .unwrap();
    e.fake.lock().unwrap().hooks.clear();
    sver::events::deliver_hooks(&e.app).await.unwrap();
    assert!(e.fake.lock().unwrap().hooks.is_empty());
    let (app_hooks, all_hooks): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE app_id IS NOT NULL), count(*) FROM event_hooks",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!((app_hooks, all_hooks), (0, 1));
    // The 50th failure in a row turns the hook off and tells its owner.
    e.sql("UPDATE event_hooks SET failures=49").await;
    e.fake.lock().unwrap().hook_fail = true;
    sver::events::emit(&mut db, "stream-owner", "follows", json!({"followers":2}))
        .await
        .unwrap();
    sver::events::deliver_hooks(&e.app).await.unwrap();
    e.fake.lock().unwrap().hook_fail = false;
    let (off, told): (bool, bool) = sqlx::query_as("SELECT disabled_at IS NOT NULL, EXISTS(SELECT 1 FROM notifications WHERE kind='hook_disabled' AND user_id='stream-owner') FROM event_hooks")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(off && told);
    drop(db);
    server.abort();
    for statement in [
        "DELETE FROM notifications WHERE kind='hook_disabled'",
        "DELETE FROM event_hooks",
        "DELETE FROM dev_apps",
        "DELETE FROM follows WHERE follower_id IN ('ev-fan','ev-fan2')",
    ] {
        e.sql(statement).await;
    }
}
