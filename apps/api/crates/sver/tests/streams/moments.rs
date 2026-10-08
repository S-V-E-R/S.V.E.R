//! Module 7 CrowdSync, phase 3: Skills (Purchased Valor like a tribute, credited to the streamer,
//! categories the streamer can switch off, nothing sold while effects are paused), Faction Rally
//! (button and `!rally`, faction members watching only, once a minute), emote combos (5 distinct
//! accounts within 5 seconds, then a cooldown) and Surge (distinct participants start and level
//! it, it ends and awards capped Engagement Valor, then cools down).
use super::Env;
use super::chat::{call, id, next_json, person};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::net::SocketAddr;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const CHANNEL: &str = "/api/channels/moowner";
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn say(e: &Env, token: &str, body: Value) -> (StatusCode, Value) {
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat-%'")
        .await;
    call(e, "POST", &format!("{CHANNEL}/chat"), Some(token), body).await
}
fn msg(body: &str) -> Value {
    json!({"id": id(), "body": body})
}
async fn valor(e: &Env, account: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(sum(amount),0)::bigint FROM ledger_entries WHERE account=$1",
    )
    .bind(account)
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}
/// The next socket message of `kind` matching `want`.
async fn next(ws: &mut Socket, kind: &str, want: impl Fn(&Value) -> bool) -> Value {
    loop {
        let message = next_json(ws).await;
        if message["type"] == kind && want(&message) {
            return message;
        }
    }
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "mo-owner", "MoOwner", true).await;
    let mut viewers = Vec::new();
    for n in 1..=8 {
        viewers.push(person(e, &format!("mo-v{n}"), &format!("MoV{n}"), true).await);
    }
    let away = person(e, "mo-away", "MoAway", true).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='mo-owner'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='mo-owner'")
        .await;
    e.sql("INSERT INTO payout_accounts(user_id,stripe_account,details_submitted,payouts_enabled) VALUES('mo-owner','acct_mo_synthetic',true,true)").await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('mo-b','mo-owner','mo-b',1,'LIVE','s','v','c',now()-interval '20 minutes',now(),now(),'skipped')").await;
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) SELECT 'mo-b','u:mo-v'||n,now()+interval '1 minute','counted' FROM generate_series(1,6) n").await;
    e.sql("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at) VALUES('mo-v1','myria',now(),now()),('mo-v2','glint',now(),now()),('mo-away','myria',now(),now())").await;
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
    let mut request = format!("ws://{address}/api/chat/ws?channel=moowner")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", e.app.config.origin.parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    next(&mut ws, "snapshot", |_| true).await;

    // ---- Skills ----
    let (_, catalog) = call(
        e,
        "GET",
        &format!("{CHANNEL}/skills"),
        Some(&viewers[0]),
        Value::Null,
    )
    .await;
    assert_eq!(catalog["skills"].as_array().unwrap().len(), 8);
    assert_eq!(catalog["valor"], 0);
    let mut tx = e.app.db.begin().await.unwrap();
    sver::ledger::post(
        &mut tx,
        "test",
        "mo-fund",
        json!({}),
        &[
            ("valor:issued", "valor", -1000),
            ("valor:mo-v1", "valor", 1000),
        ],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let both = json!({"id": id(), "body": "hi", "skill": "crown", "tribute": 50});
    assert_eq!(
        say(e, &viewers[0], both).await.0,
        StatusCode::BAD_REQUEST,
        "a Skill is its own payment"
    );
    let (status, sent) = say(
        e,
        &viewers[0],
        json!({"id": id(), "body": "wow", "skill": "starfall"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(
        (&sent["message"]["skill"], &sent["message"]["tribute"]),
        (&json!("starfall"), &json!(200))
    );
    assert_eq!(valor(e, "valor:mo-v1").await, 800);
    assert_eq!(
        valor(e, "usd:earnings:mo-owner").await,
        1600,
        "0.8¢ per Valor, in tenths of a cent"
    );
    let played = next(&mut ws, "board_effect", |e| e["skill"] == "starfall").await;
    assert_eq!(
        (&played["effect"], &played["caption"]),
        (&json!("stars"), &json!("MoV1 played Starfall"))
    );
    // Streamers switch categories off; nothing is sold while effects are paused.
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/skills",
        Some(&owner),
        json!({"disabled": ["fullscreen"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/me/skills",
            Some(&owner),
            json!({"disabled": ["nope"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        say(
            e,
            &viewers[0],
            json!({"id": id(), "body": "again", "skill": "quake"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    e.sql(
        "INSERT INTO boards(channel_id,draft,disabled) VALUES('mo-owner','{\"screens\":[]}',true)",
    )
    .await;
    assert_eq!(
        say(
            e,
            &viewers[0],
            json!({"id": id(), "body": "hey", "skill": "crown"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "paused"
    );
    e.sql("UPDATE boards SET disabled=false WHERE channel_id='mo-owner'")
        .await;
    assert_eq!(
        say(
            e,
            &viewers[0],
            json!({"id": id(), "body": "hey", "skill": "crown"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(valor(e, "valor:mo-v1").await, 750);
    let (status, _) = say(
        e,
        &viewers[1],
        json!({"id": id(), "body": "broke", "skill": "crown"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no Valor");

    // ---- Faction Rally ----
    let rally = format!("{CHANNEL}/rally");
    assert_eq!(
        call(e, "POST", &rally, Some(&viewers[2]), Value::Null)
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "not in a faction"
    );
    assert_eq!(
        call(e, "POST", &rally, Some(&away), Value::Null).await.0,
        StatusCode::FORBIDDEN,
        "not watching"
    );
    assert_eq!(
        call(e, "POST", &rally, Some(&owner), Value::Null).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(e, "POST", &rally, Some(&viewers[0]), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "POST", &rally, Some(&viewers[0]), Value::Null)
            .await
            .0,
        StatusCode::CONFLICT,
        "once a minute"
    );
    assert_eq!(say(e, &viewers[1], msg("!rally")).await.0, StatusCode::OK);
    let meter = next(&mut ws, "rally", |m| m["rally"]["glint"] == 1).await;
    assert_eq!(
        meter["rally"],
        json!({"myria": 1, "aetheron": 0, "glint": 1})
    );
    let (_, crowd) = call(
        e,
        "GET",
        &format!("{CHANNEL}/crowd"),
        Some(&viewers[0]),
        Value::Null,
    )
    .await;
    assert_eq!(
        (&crowd["faction"], &crowd["rally"]["myria"], &crowd["own"]),
        (&json!("myria"), &json!(1), &json!(false))
    );
    // The streamer sees an explanation instead of a rally button.
    let (_, mine) = call(
        e,
        "GET",
        &format!("{CHANNEL}/crowd"),
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(mine["own"], true);

    // ---- Emote combos: 5 distinct accounts within 5 seconds ----
    e.sql("INSERT INTO channel_emotes(id,channel_id,code,image_key) VALUES('mo-emote','mo-owner','MoHype','emotes/mo-hype')").await;
    for token in &viewers[..5] {
        assert_eq!(say(e, token, msg("MoHype MoHype")).await.0, StatusCode::OK);
    }
    let shower = next(&mut ws, "board_effect", |e| e["effect"] == "emote-shower").await;
    assert_eq!(
        (&shower["label"], &shower["caption"]),
        (&json!("MoHype"), &json!("Emote combo: MoHype"))
    );
    // Ordinary words never shower, and the cooldown holds.
    for token in &viewers[..5] {
        say(e, token, msg("hello")).await;
    }
    say(e, &viewers[5], msg("MoHype")).await;

    // ---- Surge ----
    // Six distinct real viewers took part this minute; with six watching the threshold is 4.
    sver::surge::tick(&e.app).await.unwrap();
    let started = next(&mut ws, "surge", |_| true).await;
    assert_eq!(
        (&started["surge"]["level"], &started["surge"]["threshold"]),
        (&json!(1), &json!(4))
    );
    assert_eq!(
        next(&mut ws, "board_effect", |e| e["surge"] == 1).await["caption"],
        "Surge level 1!"
    );
    // Two more viewers take part: 8 distinct participants reach level 2.
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('mo-b','u:mo-v7',now()+interval '1 minute','trusted'),('mo-b','u:mo-v8',now()+interval '1 minute','trusted')").await;
    say(e, &viewers[6], msg("joining")).await;
    say(e, &viewers[7], msg("me too")).await;
    say(e, &away, msg("not watching")).await;
    sver::surge::tick(&e.app).await.unwrap();
    let (_, crowd) = call(
        e,
        "GET",
        &format!("{CHANNEL}/crowd"),
        Some(&viewers[0]),
        Value::Null,
    )
    .await;
    assert_eq!(
        (&crowd["surge"]["level"], &crowd["surge"]["participants"]),
        (&json!(2), &json!(8))
    );
    // It ends and awards each participant (level × 20), within the daily cap; bans are excluded.
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('mo-owner','mo-v8','ban')").await;
    e.sql("INSERT INTO surges(id,channel_id,broadcast_id,threshold,ends_at,ended_at,started_at) VALUES('mo-old','mo-owner','mo-b',4,now(),now()-interval '2 days',now()-interval '2 days')").await;
    e.sql("INSERT INTO surge_awards(surge_id,user_id,channel_id,day,amount) VALUES('mo-old','mo-v1','mo-owner',current_date,190)").await;
    e.sql("UPDATE surges SET ends_at=now()-interval '1 second' WHERE channel_id='mo-owner' AND ended_at IS NULL").await;
    sver::surge::tick(&e.app).await.unwrap();
    let awards: Vec<(String, i32)> = sqlx::query_as("SELECT a.user_id,a.amount FROM surge_awards a WHERE a.channel_id='mo-owner' AND a.surge_id<>'mo-old' ORDER BY a.user_id")
        .fetch_all(&e.app.db).await.unwrap();
    let expected: Vec<(String, i32)> = (1..=7)
        .map(|n| (format!("mo-v{n}"), if n == 1 { 10 } else { 40 }))
        .collect();
    assert_eq!(
        awards, expected,
        "capped for mo-v1, none for the banned mo-v8 or mo-away"
    );
    next(&mut ws, "surge", |m| !m["surge"]["ended_at"].is_null()).await;
    // A 30-minute cooldown: plenty of participation doesn't start another.
    for token in &viewers[..6] {
        say(e, token, msg("still here")).await;
    }
    sver::surge::tick(&e.app).await.unwrap();
    let running: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM surges WHERE channel_id='mo-owner' AND ended_at IS NULL",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(running, 0);

    drop(ws);
    server.abort();
    e.sql("UPDATE broadcasts SET state='ENDED',started_at=now()-interval '30 days',ended_at=now()-interval '30 days',end_reason='test',reconnect_deadline=NULL WHERE id='mo-b'").await;
}
