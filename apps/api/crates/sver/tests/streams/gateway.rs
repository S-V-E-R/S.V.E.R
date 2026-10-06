//! Module 7 CrowdSync, part 2: the integration gateway. Scoped tokens (shown once, revocable)
//! connect the OBS bridge and games; both receive board presses; only a game can send board
//! state back (labels, availability, goal progress), which viewers' presses respect.
use super::Env;
use super::chat::{call, id, next_json, person};
use axum::http::StatusCode;
use futures_util::SinkExt;
use serde_json::{Value, json};
use std::net::SocketAddr;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

const CHANNEL: &str = "/api/channels/gwowner/board";
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next(ws: &mut Socket, kind: &str) -> Value {
    loop {
        let message = next_json(ws).await;
        if message["type"] == kind {
            return message;
        }
    }
}
async fn say(ws: &mut Socket, message: Value) -> Value {
    ws.send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
    loop {
        let reply = next_json(ws).await;
        if matches!(reply["type"].as_str(), Some("ack" | "error" | "pong")) {
            return reply;
        }
    }
}
async fn press(e: &Env, token: &str, control: &str, version: i64) -> StatusCode {
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'board%'")
        .await;
    call(
        e,
        "POST",
        &format!("{CHANNEL}/press"),
        Some(token),
        json!({"id": id(), "version": version, "control": control}),
    )
    .await
    .0
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "gw-owner", "GwOwner", true).await;
    let viewer = person(e, "gw-viewer", "GwViewer", true).await;
    let board = json!({"screens": [{"name": "Game", "controls": [
        {"id": "jump", "kind": "button", "label": "Jump"},
        {"id": "g", "kind": "goal", "label": "Coins", "target": 10, "cost": 0},
        {"id": "info", "kind": "label", "label": "Help the runner"}
    ]}]});
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/me/board/draft",
            Some(&owner),
            json!({"board": board})
        )
        .await
        .0,
        StatusCode::OK
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
        StatusCode::OK
    );
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('gw-b','gw-owner','gw-b',1,'LIVE','s','v','c',now()-interval '3 minutes',now(),now(),'skipped')").await;
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('gw-b','u:gw-viewer',now()+interval '1 minute','counted')").await;

    // ---- Tokens: shown once, listed without their value ----
    let create = |kind: &'static str, name: &'static str| {
        call(
            e,
            "POST",
            "/api/me/integrations",
            Some(&owner),
            json!({"kind": kind, "name": name}),
        )
    };
    assert_eq!(create("robot", "x").await.0, StatusCode::BAD_REQUEST);
    let (status, made) = create("game", "Runner").await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let game_token = made["token"].as_str().unwrap().to_string();
    assert!(game_token.starts_with("sver_g_"));
    let (_, made) = create("bridge", "Studio PC").await;
    let bridge_token = made["token"].as_str().unwrap().to_string();
    let (_, listed) = call(e, "GET", "/api/me/integrations", Some(&owner), Value::Null).await;
    assert_eq!(listed["tokens"].as_array().unwrap().len(), 2);
    assert!(
        !listed.to_string().contains(&game_token),
        "tokens are never listed"
    );
    assert!(
        listed["gateway"]
            .as_str()
            .unwrap()
            .ends_with("/api/integrations/ws")
    );

    // ---- Connecting: bearer header (native) or ?token= (browser games) ----
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
    let connect = |bearer: Option<String>, query: Option<String>| async move {
        let url = match query {
            Some(t) => format!("ws://{address}/api/integrations/ws?token={t}"),
            None => format!("ws://{address}/api/integrations/ws"),
        };
        let mut request = url.into_client_request().unwrap();
        if let Some(t) = bearer {
            request
                .headers_mut()
                .insert("authorization", format!("Bearer {t}").parse().unwrap());
        }
        tokio_tungstenite::connect_async(request).await
    };
    assert!(connect(None, None).await.is_err(), "a token is required");
    assert!(connect(Some("sver_g_wrong".into()), None).await.is_err());
    let (mut game, _) = connect(Some(game_token.clone()), None).await.unwrap();
    let hello = next(&mut game, "hello").await;
    assert_eq!(
        (&hello["kind"], &hello["channel"], &hello["version"]),
        (&json!("game"), &json!("GwOwner"), &json!(1))
    );
    assert_eq!(hello["board"]["screens"][0]["controls"][0]["id"], "jump");
    let (mut bridge, _) = connect(None, Some(bridge_token.clone())).await.unwrap();
    assert_eq!(next(&mut bridge, "hello").await["kind"], "bridge");

    // ---- Presses reach both ----
    assert_eq!(press(e, &viewer, "jump", 1).await, StatusCode::OK);
    for ws in [&mut game, &mut bridge] {
        let pressed = next(ws, "board_effect").await;
        assert_eq!(
            (&pressed["control"], &pressed["user"]["username"]),
            (&json!("jump"), &json!("GwViewer"))
        );
    }

    // ---- A game sets board state; viewers' presses respect it ----
    let state = json!({"type": "state", "id": 1, "controls": {"jump": {"disabled": true, "label": "Jump (cooling)"}, "g": {"progress": 7}}});
    assert_eq!(say(&mut game, state).await, json!({"type": "ack", "id": 1}));
    let changed = next(&mut bridge, "board_state").await;
    assert_eq!(
        (
            &changed["state"]["jump"]["disabled"],
            &changed["goals"]["g"]
        ),
        (&json!(true), &json!(7))
    );
    assert_eq!(
        press(e, &viewer, "jump", 1).await,
        StatusCode::CONFLICT,
        "unavailable"
    );
    let (_, viewing) = call(e, "GET", CHANNEL, Some(&viewer), Value::Null).await;
    assert_eq!(
        (&viewing["state"]["jump"]["label"], &viewing["goals"]["g"]),
        (&json!("Jump (cooling)"), &json!(7))
    );
    for (bad, why) in [
        (json!({"nope": {"disabled": true}}), "unknown control"),
        (json!({"g": {"progress": 11}}), "beyond the target"),
        (json!({"jump": {"progress": 1}}), "progress is for goals"),
        (json!({"jump": {"label": "x".repeat(41)}}), "label too long"),
        (json!({"jump": {"colour": "red"}}), "unknown field"),
    ] {
        let reply = say(&mut game, json!({"type": "state", "controls": bad})).await;
        assert_eq!(reply["type"], "error", "{why}");
    }
    let reply = say(
        &mut bridge,
        json!({"type": "state", "controls": {"jump": {"disabled": false}}}),
    )
    .await;
    assert_eq!(reply["type"], "error", "bridges can't change the board");
    let reply = say(
        &mut game,
        json!({"type": "state", "controls": {"jump": {"disabled": false, "label": null}}}),
    )
    .await;
    assert_eq!(reply["type"], "ack");
    assert_eq!(press(e, &viewer, "jump", 1).await, StatusCode::OK);
    assert_eq!(
        say(&mut game, json!({"type": "ping", "id": "p"})).await,
        json!({"type": "pong", "id": "p"})
    );

    // ---- Publishing a new version resends the board and clears game state ----
    let mut next_board = board;
    next_board["screens"][0]["name"] = json!("Game v2");
    call(
        e,
        "PUT",
        "/api/me/board/draft",
        Some(&owner),
        json!({"board": next_board}),
    )
    .await;
    say(
        &mut game,
        json!({"type": "state", "controls": {"jump": {"disabled": true}}}),
    )
    .await;
    call(
        e,
        "POST",
        "/api/me/board/publish",
        Some(&owner),
        Value::Null,
    )
    .await;
    let republished = next(&mut game, "board").await;
    assert_eq!(
        (&republished["version"], &republished["state"]),
        (&json!(2), &json!({}))
    );

    // ---- At most 10 messages a second ----
    for _ in 0..12 {
        game.send(Message::Text(json!({"type": "ping"}).to_string().into()))
            .await
            .unwrap();
    }
    let mut limited = false;
    for _ in 0..12 {
        if next_json(&mut game).await["type"] == "error" {
            limited = true;
        }
    }
    assert!(limited, "message rate limit");

    // ---- Revoking stops new connections ----
    let id = listed["tokens"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            e,
            "DELETE",
            &format!("/api/me/integrations/{id}"),
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(connect(Some(game_token), None).await.is_err(), "revoked");
    drop(game);
    drop(bridge);
    server.abort();
    e.sql("UPDATE broadcasts SET state='ENDED',started_at=now()-interval '30 days',ended_at=now()-interval '30 days',end_reason='test',reconnect_deadline=NULL WHERE id='gw-b'").await;
}
