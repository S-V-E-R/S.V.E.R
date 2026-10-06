//! Module 7 CrowdSync, phase 1: a streamer builds a board, tests it safely and publishes it; only
//! verified viewers on a counted or trusted lease of the live broadcast can press; presses spend
//! Engagement Valor once (idempotent retries) under cooldowns, limits, audiences, goals, channel
//! rules, board blocks, the panic switch and rate limits; effects reach chat sockets and the OBS
//! overlay; webhooks leave through the outbox, signed, with retries.
use super::Env;
use super::chat::{call, id, next_json, person};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::net::SocketAddr;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const CHANNEL: &str = "/api/channels/bdowner/board";
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A press with fresh rate-limit budgets (the rate limits have their own check below).
async fn press(e: &Env, token: &str, body: Value) -> (StatusCode, Value) {
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'board%'")
        .await;
    raw_press(e, token, body).await
}
async fn raw_press(e: &Env, token: &str, body: Value) -> (StatusCode, Value) {
    call(e, "POST", &format!("{CHANNEL}/press"), Some(token), body).await
}
fn hit(control: &str) -> Value {
    json!({"id": id(), "version": 1, "control": control})
}
async fn balance(e: &Env, user: &str) -> i64 {
    sqlx::query_scalar("SELECT coalesce((SELECT balance FROM engagement WHERE channel_id='bd-owner' AND user_id=$1),0)")
        .bind(user).fetch_one(&e.app.db).await.unwrap()
}
async fn count(e: &Env, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&e.app.db).await.unwrap()
}
/// The next socket message of `kind`, skipping others.
async fn until(ws: &mut Socket, kind: &str) -> Value {
    loop {
        let message = next_json(ws).await;
        if message["type"] == kind {
            return message;
        }
    }
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "bd-owner", "BdOwner", true).await;
    let viewer = person(e, "bd-viewer", "BdViewer", true).await;
    let excluded = person(e, "bd-excluded", "BdExcluded", true).await;
    let unverified = person(e, "bd-unverified", "BdUnverified", false).await;
    let moderator = person(e, "bd-mod", "BdMod", true).await;

    // ---- Studio: templates, validation, drafts, checklist, publish ----
    let (status, me) = call(e, "GET", "/api/me/board", Some(&owner), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["templates"].as_array().unwrap().len(), 4);
    assert_eq!(me["checklist"][0]["ok"], false);
    let draft = |board: Value| {
        call(
            e,
            "PUT",
            "/api/me/board/draft",
            Some(&owner),
            json!({"board": board}),
        )
    };
    let bad = json!({"screens": [{"name": "Main", "controls": [{"id": "s", "kind": "joystick", "label": "Move", "cost": 5}]}]});
    assert_eq!(
        draft(bad).await.0,
        StatusCode::BAD_REQUEST,
        "joysticks are free"
    );
    let unknown = json!({"screens": [{"name": "Main", "controls": [{"id": "x", "kind": "button", "label": "X", "sound": "airhorn"}]}]});
    assert_eq!(
        draft(unknown).await.0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown fields"
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/board/publish",
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "nothing to publish"
    );
    let board = json!({"screens": [{"name": "Main", "controls": [
        {"id": "boom", "kind": "button", "label": "Boom", "cost": 50, "cooldown_seconds": 60, "effect": "confetti"},
        {"id": "free", "kind": "button", "label": "Free", "per_stream_limit": 2, "effect": "hearts"},
        {"id": "goal", "kind": "goal", "label": "Goal", "cost": 10, "target": 25, "effect": "fireworks", "width": 4},
        {"id": "say", "kind": "text", "label": "Say", "effect": "spotlight"},
        {"id": "subs", "kind": "button", "label": "Subs", "audience": "subscribers"},
        {"id": "fans", "kind": "button", "label": "Fans", "audience": "followers"},
        {"id": "about", "kind": "label", "label": "Have fun"},
        {"id": "stick", "kind": "joystick", "label": "Move"}
    ]}]});
    let (status, saved) = draft(board.clone()).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["version"], 0);
    let (_, viewing) = call(e, "GET", CHANNEL, Some(&viewer), Value::Null).await;
    assert_eq!(viewing["board"], Value::Null, "drafts are never live");
    let (status, published) = call(
        e,
        "POST",
        "/api/me/board/publish",
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["version"], 1);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/board/publish",
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "no changes since the last publish"
    );
    // Test mode plays the effect without charging or recording anything.
    let (status, tested) = call(
        e,
        "POST",
        "/api/me/board/test",
        Some(&owner),
        json!({"control": "boom"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tested["effect"]["test"], true);
    assert_eq!(tested["effect"]["effect"], "confetti");

    // ---- Webhook settings ----
    let settings = |body: Value| call(e, "PUT", "/api/me/board/settings", Some(&owner), body);
    assert_eq!(
        settings(json!({"moderators_run": false, "webhook_url": "ftp://example.test/x"}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let hook = format!("{}/hook", e.app.config.stripe.api_url);
    let (status, set) = settings(json!({"moderators_run": false, "webhook_url": hook})).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let secret = set["webhook_secret"].as_str().unwrap().to_string();
    assert!(secret.starts_with("whsec_"));
    let (_, again) = settings(json!({"moderators_run": false, "webhook_url": hook})).await;
    assert_eq!(
        again["webhook_secret"],
        Value::Null,
        "an unchanged webhook keeps its secret"
    );

    // ---- Who can press ----
    assert_eq!(
        press(e, &viewer, hit("boom")).await.0,
        StatusCode::CONFLICT,
        "only while live"
    );
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('bd-b','bd-owner','bd-b',1,'LIVE','s','v','c',now()-interval '10 minutes',now(),now(),'skipped')").await;
    assert_eq!(
        press(e, &viewer, hit("boom")).await.0,
        StatusCode::FORBIDDEN,
        "watch first"
    );
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('bd-b','u:bd-viewer',now()+interval '1 minute','counted'),('bd-b','u:bd-excluded',now()+interval '1 minute','excluded'),('bd-b','u:bd-unverified',now()+interval '1 minute','trusted'),('bd-b','u:bd-mod',now()+interval '1 minute','trusted')").await;
    assert_eq!(
        press(e, &excluded, hit("free")).await.0,
        StatusCode::FORBIDDEN,
        "excluded sessions"
    );
    assert_eq!(
        press(e, &unverified, hit("free")).await.0,
        StatusCode::FORBIDDEN,
        "unverified"
    );
    assert_eq!(
        press(e, &owner, hit("free")).await.0,
        StatusCode::BAD_REQUEST,
        "owners use test mode"
    );
    let (status, _) = call(e, "POST", &format!("{CHANNEL}/press"), None, hit("free")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "guests");
    assert_eq!(
        press(e, &viewer, hit("about")).await.0,
        StatusCode::NOT_FOUND,
        "labels"
    );
    let mut stale = hit("free");
    stale["version"] = json!(0);
    assert_eq!(
        press(e, &viewer, stale).await.0,
        StatusCode::CONFLICT,
        "stale version"
    );

    // Effects reach open chats, with stream time.
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
    let open = |path: String| {
        let origin = e.app.config.origin.clone();
        async move {
            let mut request = format!("ws://{address}{path}")
                .into_client_request()
                .unwrap();
            request
                .headers_mut()
                .insert("origin", origin.parse().unwrap());
            tokio_tungstenite::connect_async(request).await
        }
    };
    let (mut chat, _) = open("/api/chat/ws?channel=bdowner".into()).await.unwrap();
    until(&mut chat, "snapshot").await;

    // Engagement Valor: charged once per press, retries are free, cooldowns hold.
    assert_eq!(
        press(e, &viewer, hit("boom")).await.0,
        StatusCode::CONFLICT,
        "can't afford"
    );
    e.sql("INSERT INTO engagement(channel_id,user_id,balance,earned) VALUES('bd-owner','bd-viewer',500,500)").await;
    let first = hit("boom");
    let (status, pressed) = press(e, &viewer, first.clone()).await;
    assert_eq!(status, StatusCode::OK, "{pressed}");
    assert_eq!(pressed["balance"], 450);
    let effect = until(&mut chat, "board_effect").await;
    assert_eq!(effect["effect"], "confetti");
    assert_eq!(effect["user"]["username"], "BdViewer");
    assert!(effect["stream_ms"].as_i64().unwrap() >= 600_000, "{effect}");
    assert_eq!(effect["overlay"], false);
    let (status, repeat) = press(e, &viewer, first).await;
    assert_eq!((status, &repeat["repeat"]), (StatusCode::OK, &json!(true)));
    assert_eq!(
        balance(e, "bd-viewer").await,
        450,
        "a retry is charged once"
    );
    let (status, cooling) = press(e, &viewer, hit("boom")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{cooling}");
    // A per-stream limit counts everyone.
    assert_eq!(press(e, &viewer, hit("free")).await.0, StatusCode::OK);
    assert_eq!(press(e, &moderator, hit("free")).await.0, StatusCode::OK);
    assert_eq!(
        press(e, &viewer, hit("free")).await.0,
        StatusCode::CONFLICT,
        "stream limit"
    );
    // A goal fills (each press adds its cost) and then closes.
    for expected in [10, 20, 25] {
        let (status, goal) = press(e, &viewer, hit("goal")).await;
        assert_eq!(status, StatusCode::OK, "{goal}");
        assert_eq!(goal["goal"]["progress"], expected);
    }
    assert_eq!(
        press(e, &viewer, hit("goal")).await.0,
        StatusCode::CONFLICT,
        "goal complete"
    );
    assert_eq!(balance(e, "bd-viewer").await, 420);
    // Text inputs follow the channel's chat rules.
    let say = |text: &str| json!({"id": id(), "version": 1, "control": "say", "text": text});
    assert_eq!(press(e, &viewer, say(" ")).await.0, StatusCode::BAD_REQUEST);
    e.sql("INSERT INTO chat_settings(channel_id,banned_words) VALUES('bd-owner','{badword}')")
        .await;
    assert_eq!(
        press(e, &viewer, say("a badword here")).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        press(e, &viewer, say("hello stream")).await.0,
        StatusCode::OK
    );
    // Earlier presses' effects are still queued on the socket; find this one.
    let said = loop {
        let effect = until(&mut chat, "board_effect").await;
        if effect["control"] == "say" {
            break effect;
        }
    };
    assert_eq!(said["text"], "hello stream");
    // Audiences.
    assert_eq!(
        press(e, &viewer, hit("subs")).await.0,
        StatusCode::FORBIDDEN
    );
    e.sql("INSERT INTO channel_subs(channel_id,user_id,tier,paid_through,months) VALUES('bd-owner','bd-viewer',1,now()+interval '1 month',1)").await;
    assert_eq!(press(e, &viewer, hit("subs")).await.0, StatusCode::OK);
    assert_eq!(
        press(e, &viewer, hit("fans")).await.0,
        StatusCode::FORBIDDEN
    );
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('bd-viewer','bd-owner')")
        .await;
    assert_eq!(press(e, &viewer, hit("fans")).await.0, StatusCode::OK);

    // Joystick: free, bounded input, relayed (not stored), at most 10 a second.
    let stick = |x: f64| json!({"id": id(), "version": 1, "control": "stick", "x": x, "y": 0.0});
    assert_eq!(
        press(e, &viewer, stick(2.0)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(press(e, &viewer, stick(0.5)).await.0, StatusCode::OK);
    assert_eq!(until(&mut chat, "board_input").await["x"], 0.5);
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'board%'")
        .await;
    let mut limited = 0;
    for _ in 0..12 {
        if raw_press(e, &viewer, stick(0.1)).await.0 == StatusCode::TOO_MANY_REQUESTS {
            limited += 1;
        }
    }
    assert!(limited >= 1, "joystick rate limit");
    // The press rate limit across all controls.
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'board%'")
        .await;
    let mut limited = 0;
    for _ in 0..12 {
        if raw_press(e, &viewer, hit("boom")).await.0 == StatusCode::TOO_MANY_REQUESTS {
            limited += 1;
        }
    }
    assert!(limited >= 1, "press rate limit");

    // Channel rules, board blocks and the panic switch.
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind,until) VALUES('bd-owner','bd-viewer','timeout',now()+interval '5 minutes')").await;
    assert_eq!(
        press(e, &viewer, hit("subs")).await.0,
        StatusCode::FORBIDDEN,
        "timed out"
    );
    e.sql("DELETE FROM channel_restrictions WHERE user_id='bd-viewer'")
        .await;
    e.sql("INSERT INTO channel_moderators(channel_id,user_id) VALUES('bd-owner','bd-mod')")
        .await;
    let blocks = format!("{CHANNEL}/blocks/BdViewer");
    assert_eq!(
        call(e, "PUT", &blocks, Some(&moderator), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN,
        "moderators run the board only when allowed"
    );
    assert_eq!(
        call(e, "PUT", &blocks, Some(&owner), Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(
        press(e, &viewer, hit("subs")).await.0,
        StatusCode::FORBIDDEN,
        "blocked"
    );
    let (_, run) = call(e, "GET", CHANNEL, Some(&owner), Value::Null).await;
    assert_eq!(
        (&run["can_run"], &run["blocks"]),
        (&json!(true), &json!(["BdViewer"]))
    );
    assert_eq!(
        call(e, "DELETE", &blocks, Some(&owner), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "PUT",
            &format!("{CHANNEL}/blocks/BdMod"),
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "moderators can't be blocked"
    );
    settings(json!({"moderators_run": true, "webhook_url": hook})).await;
    let disabled = format!("{CHANNEL}/disabled");
    let panic = |on: bool| {
        call(
            e,
            "PUT",
            &disabled,
            Some(&moderator),
            json!({"disabled": on}),
        )
    };
    assert_eq!(panic(true).await.0, StatusCode::OK);
    until(&mut chat, "board").await;
    assert_eq!(
        press(e, &viewer, hit("subs")).await.0,
        StatusCode::CONFLICT,
        "paused"
    );
    assert_eq!(panic(false).await.0, StatusCode::OK);
    assert_eq!(
        count(e, "SELECT count(*) FROM channel_moderation_log WHERE channel_id='bd-owner' AND action IN ('board_block','board_unblock','board_disable','board_enable')").await,
        4
    );

    // ---- Webhooks: every recorded press is in the outbox, delivered signed. ----
    let presses = count(
        e,
        "SELECT count(*) FROM board_presses WHERE channel_id='bd-owner'",
    )
    .await;
    assert_eq!(
        presses, 9,
        "joystick moves and refused presses aren't recorded"
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM outbox WHERE channel_id='bd-owner'").await,
        presses
    );
    sver::boards::deliver_due(&e.app).await.unwrap();
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM outbox WHERE channel_id='bd-owner' AND delivered_at IS NULL"
        )
        .await,
        0
    );
    {
        let fake = e.fake.lock().unwrap();
        assert_eq!(fake.hooks.len() as i64, presses);
        let now = chrono::Utc::now().timestamp();
        for (signature, body) in &fake.hooks {
            assert!(sver::stripe::verify(
                &secret,
                signature,
                body.as_bytes(),
                now
            ));
            assert_eq!(
                serde_json::from_str::<Value>(body).unwrap()["type"],
                "board.press"
            );
        }
    }
    // A failing endpoint is retried later, not lost.
    e.fake.lock().unwrap().hook_fail = true;
    assert_eq!(press(e, &viewer, hit("subs")).await.0, StatusCode::OK);
    sver::boards::deliver_due(&e.app).await.unwrap();
    let (attempts, waiting): (i32, bool) = sqlx::query_as("SELECT attempts, available_at>now() FROM outbox WHERE channel_id='bd-owner' AND delivered_at IS NULL")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!((attempts, waiting), (1, true));
    e.fake.lock().unwrap().hook_fail = false;
    e.sql(
        "UPDATE outbox SET available_at=now() WHERE channel_id='bd-owner' AND delivered_at IS NULL",
    )
    .await;
    sver::boards::deliver_due(&e.app).await.unwrap();
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM outbox WHERE channel_id='bd-owner' AND delivered_at IS NULL"
        )
        .await,
        0
    );

    // ---- OBS overlay: a private token, effects only; pages then leave effects to it. ----
    assert!(
        open("/api/boards/overlay/ws?token=wrong".into())
            .await
            .is_err()
    );
    let (_, overlay) = call(
        e,
        "POST",
        "/api/me/board/overlay",
        Some(&owner),
        Value::Null,
    )
    .await;
    let token = overlay["url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let (mut obs, _) = open(format!("/api/boards/overlay/ws?token={token}"))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (_, viewing) = call(e, "GET", CHANNEL, Some(&viewer), Value::Null).await;
    assert_eq!(viewing["overlay"], true, "{viewing}");
    assert_eq!(viewing["goals"]["goal"], 25);
    e.sql("DELETE FROM board_presses WHERE control_id='subs'")
        .await;
    assert_eq!(press(e, &viewer, hit("subs")).await.0, StatusCode::OK);
    let shown = until(&mut obs, "board_effect").await;
    assert_eq!(
        (&shown["control"], &shown["overlay"]),
        (&json!("subs"), &json!(true))
    );
    // Test mode reaches the overlay too.
    call(
        e,
        "POST",
        "/api/me/board/test",
        Some(&owner),
        json!({"control": "goal"}),
    )
    .await;
    assert_eq!(until(&mut obs, "board_effect").await["test"], true);
    drop(obs);
    // Publishing a new version starts goals over.
    let mut next = board;
    next["screens"][0]["name"] = json!("Main v2");
    draft(next).await;
    call(
        e,
        "POST",
        "/api/me/board/publish",
        Some(&owner),
        Value::Null,
    )
    .await;
    let (_, viewing) = call(e, "GET", CHANNEL, Some(&viewer), Value::Null).await;
    assert_eq!(
        (&viewing["version"], &viewing["goals"]),
        (&json!(2), &json!({}))
    );
    drop(chat);
    // End the synthetic broadcast outside discovery's 14-day "recently live" window.
    e.sql("UPDATE broadcasts SET state='ENDED',started_at=now()-interval '30 days',ended_at=now()-interval '30 days',end_reason='test',reconnect_deadline=NULL WHERE id='bd-b'").await;
    server.abort();
}
