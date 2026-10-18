//! Module 6 Support, part 3: Engagement Valor earned by following (once), chatting (cooldown) and
//! real playback (verified viewers on counted leases), spent on rewards (cooldown, per-stream
//! limit, required text, idempotent retries) and the highlight reward; the owner's queue marks
//! redemptions done or refunds them; a channel ban freezes earning and spending.
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

const CHANNEL: &str = "/api/channels/evowner";

async fn balance(e: &Env, user: &str) -> i64 {
    sqlx::query_scalar("SELECT coalesce((SELECT balance FROM engagement WHERE channel_id='ev-owner' AND user_id=$1),0)")
        .bind(user).fetch_one(&e.app.db).await.unwrap()
}
async fn say(e: &Env, token: &str, body: Value) -> (StatusCode, Value) {
    call(e, "POST", &format!("{CHANNEL}/chat"), Some(token), body).await
}
fn reward<'a>(rewards: &'a Value, name: &str) -> &'a str {
    rewards
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap()["id"]
        .as_str()
        .unwrap()
}
async fn follow(e: &Env, token: &str, method: &str) -> (StatusCode, Value) {
    call(e, method, "/api/follows/evowner", Some(token), Value::Null).await
}
async fn resolve(e: &Env, owner: &str, id: String, action: &str) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("{CHANNEL}/redemptions/{id}"),
        Some(owner),
        json!({"action": action}),
    )
    .await
}
async fn redeem(e: &Env, token: &str, reward: &str, body: Value) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("{CHANNEL}/rewards/{reward}/redeem"),
        Some(token),
        body,
    )
    .await
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "ev-owner", "EvOwner", true).await;
    let viewer = person(e, "ev-viewer", "EvViewer", true).await;
    let lurker = person(e, "ev-lurker", "EvLurker", false).await;

    // The owner's rewards; the built-in highlight is always there.
    let create = |body: Value| call(e, "POST", "/api/me/rewards", Some(&owner), body);
    assert_eq!(
        create(json!({"name": " ", "cost": 10, "enabled": true}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        create(json!({"name": "Free", "cost": 0, "enabled": true}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, _) =
        create(json!({"name": "Hydrate", "cost": 100, "cooldown_seconds": 60, "enabled": true}))
            .await;
    assert_eq!(status, StatusCode::OK);
    let (status, made) = create(
        json!({"name": "Song request", "cost": 50, "per_stream_limit": 1,
        "prompt": "Which song?", "enabled": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let rewards = made["rewards"].clone();
    assert_eq!(rewards.as_array().unwrap().len(), 3);
    assert_eq!(rewards[0]["kind"], "highlight");
    let (hydrate, song) = (
        reward(&rewards, "Hydrate"),
        reward(&rewards, "Song request"),
    );

    // Following pays once, never again on refollow; unverified viewers don't earn.
    assert_eq!(follow(e, &viewer, "PUT").await.0, StatusCode::OK);
    assert_eq!(balance(e, "ev-viewer").await, 300);
    follow(e, &viewer, "DELETE").await;
    follow(e, &viewer, "PUT").await;
    assert_eq!(balance(e, "ev-viewer").await, 300, "no refollow farming");
    follow(e, &lurker, "PUT").await;
    assert_eq!(balance(e, "ev-lurker").await, 0);
    // Chatting pays once per cooldown.
    assert_eq!(
        say(e, &viewer, json!({"id": id(), "body": "hello"}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(balance(e, "ev-viewer").await, 305);
    say(e, &viewer, json!({"id": id(), "body": "again"})).await;
    assert_eq!(balance(e, "ev-viewer").await, 305);
    // Real playback pays per interval: verified viewers on counted leases of a live stream.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('ev-b','ev-owner','ev-b',1,'LIVE','s','v','c',now()-interval '10 minutes',now(),now(),'skipped')").await;
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('ev-b','u:ev-viewer',now()+interval '1 minute','trusted'),('ev-b','u:ev-lurker',now()+interval '1 minute','trusted')").await;
    sver::engagement::award_watch(&e.app).await.unwrap();
    assert_eq!(balance(e, "ev-viewer").await, 315);
    sver::engagement::award_watch(&e.app).await.unwrap();
    assert_eq!(balance(e, "ev-viewer").await, 315, "once per interval");
    assert_eq!(balance(e, "ev-lurker").await, 0);
    // Account XP (docs/PROGRESSION.md): chat at most once a minute, watching once per interval,
    // and a Scout bonus for 10+ minutes that began early in a new channel's broadcast, once.
    let progression = || async {
        call(e, "GET", "/api/me/progression", Some(&viewer), Value::Null)
            .await
            .1
    };
    assert_eq!(
        progression().await["xp"],
        12,
        "2 for chat (once) + 10 for a minute watched"
    );
    e.sql("UPDATE broadcasts SET started_at=now()-interval '12 minutes' WHERE id='ev-b'")
        .await;
    e.sql("UPDATE playback_leases SET created_at=now()-interval '11 minutes' WHERE broadcast_id='ev-b'").await;
    sver::engagement::award_watch(&e.app).await.unwrap();
    sver::engagement::award_watch(&e.app).await.unwrap();
    let mine = progression().await;
    assert_eq!(
        (&mine["xp"], &mine["level"], &mine["next_xp"]),
        (&json!(62), &json!(1), &json!(100)),
        "{mine}"
    );
    let (_, studio) = call(e, "GET", "/api/me/stream", Some(&owner), Value::Null).await;
    assert_eq!(studio["broadcast"]["scouts"], 1, "{studio}");
    // Daily orders: three on first view, completed from event records by the minute tick.
    let orders = || async {
        call(e, "GET", "/api/me/orders", Some(&viewer), Value::Null)
            .await
            .1
    };
    assert_eq!(orders().await["orders"].as_array().unwrap().len(), 3);
    e.sql("UPDATE daily_orders SET kind=CASE slot WHEN 0 THEN 'watch' ELSE 'poll' END,rarity=0,target=CASE slot WHEN 0 THEN 1 ELSE 5 END WHERE user_id='ev-viewer'").await;
    let before = balance(e, "ev-viewer").await;
    sver::engagement::award_watch(&e.app).await.unwrap();
    let today = orders().await;
    // A Common order also pays 10 Engagement Valor in the channel the viewer was watching.
    assert_eq!(today["orders"][0]["ev"], 10, "{today}");
    assert!(today["orders"][0]["ev_channel"].is_string(), "{today}");
    assert_eq!(balance(e, "ev-viewer").await, before + 10);
    // Take it back out so the reward arithmetic below stays as it was.
    e.sql("UPDATE engagement SET balance=balance-10,earned=earned-10 WHERE channel_id='ev-owner' AND user_id='ev-viewer'").await;
    assert_eq!(
        (
            &today["orders"][0]["done"],
            &today["orders"][0]["xp"],
            &today["orders"][1]["done"],
            &today["streak"],
            &today["week"]
        ),
        (
            &json!(true),
            &json!(40),
            &json!(false),
            &json!(1),
            &json!(1)
        ),
        "{today}"
    );
    let mine = progression().await;
    assert_eq!(mine["level"], 2, "62 + 40 XP");
    assert_eq!(mine["frame"], 0);
    assert!(mine["title"].is_string(), "{mine}");
    let token = viewer.as_str();
    let reroll = |slot: i32| async move {
        call(
            e,
            "POST",
            &format!("/api/me/orders/{slot}/reroll"),
            Some(token),
            Value::Null,
        )
        .await
        .0
    };
    assert_eq!(reroll(0).await, StatusCode::BAD_REQUEST, "done orders stay");
    assert_eq!(reroll(1).await, StatusCode::OK);
    assert_eq!(reroll(2).await, StatusCode::BAD_REQUEST, "one reroll a day");

    // A failed payment leaves its order for the next minute, and never holds up anyone else's.
    // (The viewer's rerolled order is a random draw; keep it out of reach so the reward
    // arithmetic below stays as it was.)
    e.sql("UPDATE daily_orders SET target=1000 WHERE user_id='ev-viewer' AND completed_at IS NULL")
        .await;
    let mut tokens = Vec::new();
    for (id, name) in [("ev-o1", "EvOrders1"), ("ev-o2", "EvOrders2")] {
        tokens.push(person(e, id, name, true).await);
        e.sql(sqlx::AssertSqlSafe(format!("INSERT INTO daily_orders(user_id,day,slot,kind,rarity,target) VALUES('{id}',current_date,0,'watch',0,1),('{id}',current_date,1,'chat',0,1),('{id}',current_date,2,'beacon',0,5)"))).await;
        e.sql(sqlx::AssertSqlSafe(format!("INSERT INTO xp_days(user_id,day,source,xp,minutes) VALUES('{id}',current_date,'watch',10,1)"))).await;
    }
    e.sql("ALTER TABLE xp_days ADD CONSTRAINT payment_fails CHECK (user_id<>'ev-o1' OR source<>'orders') NOT VALID").await;
    sver::engagement::award_watch(&e.app).await.unwrap();
    e.sql("ALTER TABLE xp_days DROP CONSTRAINT payment_fails")
        .await;
    let order = |user: &'static str| async move {
        sqlx::query_as::<_, (bool, Option<i32>)>(
            "SELECT completed_at IS NOT NULL,xp FROM daily_orders WHERE user_id=$1 AND slot=0",
        )
        .bind(user)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
    };
    assert_eq!(order("ev-o1").await, (false, None), "not done, not paid");
    assert_eq!(order("ev-o2").await, (true, Some(40)), "others are paid");
    sver::engagement::award_watch(&e.app).await.unwrap();
    assert_eq!(
        order("ev-o1").await,
        (true, Some(40)),
        "paid the next minute"
    );
    // A done order shows what it paid (no channel today, so XP only); a target of 1 reads naturally.
    let (_, theirs) = call(e, "GET", "/api/me/orders", Some(&tokens[1]), Value::Null).await;
    assert_eq!(
        (
            &theirs["orders"][0]["done"],
            &theirs["orders"][0]["ev"],
            &theirs["orders"][1]["label"]
        ),
        (&json!(true), &json!(0), &json!("Chat in a channel")),
        "{theirs}"
    );

    // Weekly milestones at 7, 14 and 21 (every order 3 a day allows): the 21st order of the week
    // pays all three (100 + 200 + 300 XP) and earns Order Keeper.
    person(e, "ev-keeper", "EvKeeper", true).await;
    e.sql(
        "INSERT INTO daily_orders(user_id,day,slot,kind,rarity,target,completed_at,xp)
        SELECT 'ev-keeper',date_trunc('week',current_date)::date+d,s,'poll',0,1,now(),40
        FROM generate_series(0,6) d, generate_series(0,2) s
        WHERE NOT (date_trunc('week',current_date)::date+d=current_date AND s=0)",
    )
    .await;
    e.sql("INSERT INTO daily_orders(user_id,day,slot,kind,rarity,target) VALUES('ev-keeper',current_date,0,'watch',0,1)").await;
    e.sql("INSERT INTO xp_days(user_id,day,source,xp,minutes) VALUES('ev-keeper',current_date,'watch',10,1)").await;
    sver::engagement::award_watch(&e.app).await.unwrap();
    let milestones: Vec<i16> = sqlx::query_scalar(
        "SELECT orders FROM order_milestones WHERE user_id='ev-keeper' ORDER BY orders",
    )
    .fetch_all(&e.app.db)
    .await
    .unwrap();
    assert_eq!(milestones, [7, 14, 21]);
    let (order_xp, paid): (i32, i32) = sqlx::query_as("SELECT (SELECT xp FROM daily_orders WHERE user_id='ev-keeper' AND day=current_date AND slot=0),(SELECT xp FROM xp_days WHERE user_id='ev-keeper' AND day=current_date AND source='orders')")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(paid - order_xp, 600, "100 + 200 + 300 XP");
    sver::achievements::award(&e.app).await.unwrap();
    let keeper: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_achievements WHERE user_id='ev-keeper' AND key='order_keeper')")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(keeper, "Order Keeper is reachable");

    // Several new channels qualifying at once award only what's left of today's 3 Scout bonuses.
    person(e, "ev-scout", "EvScout", true).await;
    for n in 0..4 {
        person(e, &format!("ev-new{n}"), &format!("EvNew{n}"), true).await;
        e.sql(sqlx::AssertSqlSafe(format!("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('ev-nb{n}','ev-new{n}','ev-nb{n}',1,'LIVE','s','v','c',now()-interval '12 minutes',now(),now(),'skipped')"))).await;
        e.sql(sqlx::AssertSqlSafe(format!("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level,created_at) VALUES('ev-nb{n}','u:ev-scout',now()+interval '1 minute','trusted',now()-interval '11 minutes')"))).await;
    }
    sver::engagement::award_watch(&e.app).await.unwrap();
    let scouted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM scout_awards WHERE user_id='ev-scout'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(scouted, 3);
    e.sql("DELETE FROM playback_leases WHERE viewer_key='u:ev-scout'")
        .await;
    // Ended long enough ago to stay out of discovery's recently-live channels.
    e.sql("UPDATE broadcasts SET state='ENDED',started_at=now()-interval '20 days',ended_at=now()-interval '20 days' WHERE id LIKE 'ev-nb%'")
        .await;

    // The viewer's view of the channel's rewards and balance.
    let (_, listed) = call(
        e,
        "GET",
        &format!("{CHANNEL}/rewards"),
        Some(&viewer),
        Value::Null,
    )
    .await;
    assert_eq!(listed["balance"], 315);
    assert_eq!(listed["rewards"].as_array().unwrap().len(), 3);

    // Redeeming: idempotent by request ID, then the cooldown applies.
    let first = id();
    assert_eq!(
        redeem(e, &viewer, hydrate, json!({"id": first})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        redeem(e, &viewer, hydrate, json!({"id": first})).await.0,
        StatusCode::OK
    );
    assert_eq!(balance(e, "ev-viewer").await, 215);
    assert_eq!(
        redeem(e, &viewer, hydrate, json!({"id": id()})).await.0,
        StatusCode::CONFLICT
    );
    // A required text and a per-stream limit.
    let (status, needs) = redeem(e, &viewer, song, json!({"id": id()})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(needs["field"], "input");
    assert_eq!(
        redeem(
            e,
            &viewer,
            song,
            json!({"id": id(), "input": "Dead of Night"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        redeem(e, &viewer, song, json!({"id": id(), "input": "Another"}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(balance(e, "ev-viewer").await, 165);
    assert_eq!(
        redeem(e, &owner, hydrate, json!({"id": id()})).await.0,
        StatusCode::BAD_REQUEST,
        "not your own rewards"
    );
    // The highlight reward pays for a highlighted chat message, or nothing posts.
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat-%:ev-viewer'")
        .await;
    let (status, poor) = say(
        e,
        &viewer,
        json!({"id": id(), "body": "Look at me", "highlight": true}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{poor}");
    e.sql("UPDATE engagement SET balance=1000 WHERE channel_id='ev-owner' AND user_id='ev-viewer'")
        .await;
    let (status, shown) = say(
        e,
        &viewer,
        json!({"id": id(), "body": "Look at me", "highlight": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["message"]["highlighted"], true);
    assert_eq!(balance(e, "ev-viewer").await, 500);

    // The queue: owner and moderators only; done or refunded once.
    assert_eq!(
        call(
            e,
            "GET",
            &format!("{CHANNEL}/redemptions"),
            Some(&viewer),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, queue) = call(
        e,
        "GET",
        &format!("{CHANNEL}/redemptions"),
        Some(&owner),
        Value::Null,
    )
    .await;
    let items = queue["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    let pending: Vec<&Value> = items.iter().filter(|i| i["status"] == "pending").collect();
    assert_eq!(pending.len(), 2);
    let find = |name: &str| {
        pending.iter().find(|i| i["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        resolve(e, &owner, find("Hydrate"), "refund").await.0,
        StatusCode::OK
    );
    assert_eq!(balance(e, "ev-viewer").await, 600);
    assert_eq!(
        resolve(e, &owner, find("Song request"), "done").await.0,
        StatusCode::OK
    );
    assert_eq!(
        resolve(e, &owner, find("Song request"), "refund").await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(balance(e, "ev-viewer").await, 600);

    // Watch page loyalty: the viewer's standing, and this week's leaders by points earned here.
    // Spending never lowers the week; unverified and banned viewers are left out.
    let (status, mine) = call(
        e,
        "GET",
        &format!("{CHANNEL}/loyalty"),
        Some(&viewer),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    let earned = mine["me"]["earned"].as_i64().unwrap();
    assert_eq!(mine["me"]["balance"], 600);
    assert_eq!(mine["me"]["rank"], 1, "{mine}");
    assert_eq!(mine["me"]["from"], 200);
    assert_eq!(mine["me"]["next"]["name"], "Devoted");
    assert_eq!(mine["leaders"].as_array().unwrap().len(), 1, "{mine}");
    assert_eq!(mine["leaders"][0]["user"]["username"], "EvViewer");
    assert!(mine["leaders"][0]["points"].as_i64().unwrap() >= earned);
    assert_eq!(mine["me"]["week"], mine["leaders"][0]["points"]);
    let (_, theirs) = call(
        e,
        "GET",
        &format!("{CHANNEL}/loyalty"),
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(
        theirs["me"],
        Value::Null,
        "the owner has no standing in their own channel"
    );
    let (_, guest) = call(e, "GET", &format!("{CHANNEL}/loyalty"), None, Value::Null).await;
    assert_eq!(guest["me"], Value::Null);
    // A new week starts the count again from what's earned in it.
    e.sql("UPDATE engagement SET week_of=week_of-7 WHERE channel_id='ev-owner'")
        .await;
    let (_, stale) = call(e, "GET", &format!("{CHANNEL}/loyalty"), None, Value::Null).await;
    assert_eq!(stale["leaders"], json!([]));
    e.sql(
        "UPDATE engagement SET earned=earned+5 WHERE channel_id='ev-owner' AND user_id='ev-viewer'",
    )
    .await;
    let (_, fresh) = call(e, "GET", &format!("{CHANNEL}/loyalty"), None, Value::Null).await;
    assert_eq!(fresh["leaders"][0]["points"], 5, "{fresh}");

    // Report playback problem: fixed choices, one per viewer per broadcast every 10 minutes.
    let path = format!("{CHANNEL}/live/problem");
    let problem = |kind: &str| {
        call(
            e,
            "POST",
            &path,
            None,
            json!({"broadcast_id": "ev-b", "browser_id": "problem-browser-0001", "kind": kind, "transport": "hls", "delay_ms": 2100, "height": 720, "dropped_frames": 3}),
        )
    };
    assert_eq!(problem("bad").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(problem("stutter").await.0, StatusCode::OK);
    assert_eq!(problem("no_audio").await.0, StatusCode::TOO_MANY_REQUESTS);
    let saved: (String, Option<i32>) =
        sqlx::query_as("SELECT kind,height FROM playback_problems WHERE broadcast_id='ev-b'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(saved, ("stutter".into(), Some(720)));

    // A channel ban freezes earning and spending there.
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('ev-owner','ev-viewer','ban')").await;
    let (_, banned) = call(e, "GET", &format!("{CHANNEL}/loyalty"), None, Value::Null).await;
    assert_eq!(
        banned["leaders"],
        json!([]),
        "banned viewers leave the leaders box"
    );
    assert_eq!(
        redeem(e, &viewer, hydrate, json!({"id": id()})).await.0,
        StatusCode::FORBIDDEN
    );
    e.sql("UPDATE engagement SET last_watch_at=now()-interval '1 hour' WHERE user_id='ev-viewer'")
        .await;
    sver::engagement::award_watch(&e.app).await.unwrap();
    assert_eq!(balance(e, "ev-viewer").await, 600);
    e.sql("DELETE FROM channel_restrictions WHERE channel_id='ev-owner'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE id='ev-b'").await;
}
