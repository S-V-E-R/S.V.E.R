//! Module 7 CrowdSync, part 2 (docs/CROWDSYNC.md "Outputs", "Game SDK"): the integration
//! gateway. The S.V.E.R bridge (OBS on the streamer's PC) and games connect here with a scoped
//! token from Creator Studio, never to the database, Redis or internal services. Both receive the
//! channel's board presses, joystick moves and board changes; a game can also send board state
//! back (labels, availability, goal progress), which viewers see at once.
use crate::{
    App, boards,
    chat::Event,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Path, Query, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{delete, get},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_TOKENS: i64 = 10;
/// Messages a client may send per second.
const MESSAGES_PER_SECOND: u32 = 10;
const FRAME: usize = 16 * 1024;

// ---- Creator Studio: tokens ----

async fn list_json(app: &App, channel: &str) -> Res<Value> {
    Ok(sqlx::query_scalar("SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'kind',kind,'name',name,'created_at',created_at,'last_used_at',last_used_at) ORDER BY created_at),'[]') FROM integration_tokens WHERE channel_id=$1 AND revoked_at IS NULL")
        .bind(channel)
        .fetch_one(&app.db)
        .await?)
}
fn gateway_url(app: &App) -> String {
    format!(
        "{}/api/integrations/ws",
        app.config.origin.replacen("http", "ws", 1)
    )
}
/// GET /api/me/integrations
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    Ok(Json(
        json!({"tokens": list_json(&app, &user.id).await?, "gateway": gateway_url(&app), "max": MAX_TOKENS}),
    ))
}
#[derive(Deserialize)]
pub struct Create {
    kind: String,
    name: String,
}
/// POST /api/me/integrations: a new scoped token, shown once (only its digest is stored).
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Create>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    if !["bridge", "game"].contains(&input.kind.as_str()) {
        return Err(Fail::field("kind", "Choose the OBS bridge or a game."));
    }
    let name = input.name.trim();
    if !(1..=40).contains(&name.chars().count()) {
        return Err(Fail::field("name", "Names are 1–40 characters."));
    }
    crate::text::filter(name, "name")?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('integrations:'||$1, 7))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integration_tokens WHERE channel_id=$1 AND revoked_at IS NULL",
    )
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    if active >= MAX_TOKENS {
        return Err(Fail::conflict("You can have up to 10 connections."));
    }
    let token = format!("sver_{}_{}", &input.kind[..1], sec::token());
    sqlx::query(
        "INSERT INTO integration_tokens(id,channel_id,kind,name,token_hash) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(profiles::new_id())
    .bind(&user.id)
    .bind(&input.kind)
    .bind(name)
    .bind(sec::digest(&token))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"tokens": list_json(&app, &user.id).await?, "gateway": gateway_url(&app), "token": token}),
    ))
}
/// DELETE /api/me/integrations/{id}: revoked at once; an open connection closes within 30 seconds.
async fn revoke(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let revoked = sqlx::query("UPDATE integration_tokens SET revoked_at=now() WHERE id=$1 AND channel_id=$2 AND revoked_at IS NULL")
        .bind(&id)
        .bind(&user.id)
        .execute(&app.db)
        .await?
        .rows_affected();
    if revoked == 0 {
        return Err(Fail::missing());
    }
    Ok(Json(json!({"tokens": list_json(&app, &user.id).await?})))
}

// ---- The gateway ----

#[derive(Deserialize)]
pub struct TokenQuery {
    token: Option<String>,
}
struct Connection {
    id: String,
    /// This socket, so a game's reconnect isn't undone by the old socket closing.
    session: String,
    channel: String,
    username: String,
    kind: String,
}
/// GET /api/integrations/ws: the token comes in `Authorization: Bearer …` (native apps) or
/// `?token=` (browser games, which can't set headers). There's no origin check: the scoped token
/// is the only credential, and it reaches one channel's board events and state.
async fn socket(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<TokenQuery>,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("integration-connect:{ip}")], 30, 60).await?;
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned)
        .or(query.token)
        .ok_or_else(|| Fail::new(StatusCode::UNAUTHORIZED, "A token is required."))?;
    let found: Option<(String, String, String, String)> = sqlx::query_as(
        "UPDATE integration_tokens t SET last_used_at=now() FROM channel_users u
        WHERE t.token_hash=$1 AND t.revoked_at IS NULL AND u.id=t.channel_id AND u.eligible
        RETURNING t.id, t.channel_id, u.username, t.kind",
    )
    .bind(sec::digest(&token))
    .fetch_optional(&app.db)
    .await?;
    let Some((id, channel, username, kind)) = found else {
        return Err(Fail::new(
            StatusCode::UNAUTHORIZED,
            "That token isn't valid.",
        ));
    };
    let connection = Connection {
        id,
        session: profiles::new_id(),
        channel,
        username,
        kind,
    };
    Ok(upgrade
        .max_message_size(FRAME)
        .max_frame_size(FRAME)
        .on_upgrade(move |ws| session(app, connection, ws)))
}
async fn send(ws: &mut WebSocket, value: Value) -> bool {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}
async fn board(app: &App, channel: &str) -> Res<Value> {
    boards::snapshot(&mut *app.db.acquire().await?, channel).await
}
async fn session(app: App, c: Connection, ws: WebSocket) {
    if c.kind == "game" {
        // The board shows "Starting…" until this game says ready.
        game(
            &app,
            &c,
            "game_session=$2, game_seen_at=now(), game_ready=false",
        )
        .await;
    }
    run(&app, &c, ws).await;
    if c.kind == "game" {
        game(
            &app,
            &c,
            "game_session=NULL, game_seen_at=NULL, game_ready=false",
        )
        .await;
    }
}
/// Updates this game session's board fields and reloads open boards.
async fn game(app: &App, c: &Connection, set: &str) {
    let sql = format!(
        "UPDATE boards SET {set} WHERE channel_id=$1 AND (game_session IS NULL OR game_session=$2 OR $3)"
    );
    // A new session takes over; the others only touch their own.
    let takeover = set.starts_with("game_session=$2");
    let done = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(&c.channel)
        .bind(&c.session)
        .bind(takeover)
        .execute(&app.db)
        .await;
    if matches!(done, Ok(r) if r.rows_affected() > 0) {
        boards::changed(app, &c.channel);
    }
}
async fn run(app: &App, c: &Connection, mut ws: WebSocket) {
    let mut events = app.chat.subscribe();
    let Ok(snapshot) = board(app, &c.channel).await else {
        return;
    };
    let mut hello = json!({"type": "hello", "kind": c.kind, "channel": c.username, "protocol": 1});
    merge(&mut hello, snapshot);
    if !send(&mut ws, hello).await {
        return;
    }
    let mut check = tokio::time::interval(Duration::from_secs(30));
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    check.tick().await;
    let (mut window, mut sent) = (Instant::now(), 0u32);
    loop {
        tokio::select! {
            _ = check.tick() => {
                // A revoked token (or an account that can no longer stream) closes the connection.
                let still = sqlx::query("UPDATE integration_tokens t SET last_used_at=now() FROM channel_users u WHERE t.id=$1 AND t.revoked_at IS NULL AND u.id=t.channel_id AND u.eligible")
                    .bind(&c.id).execute(&app.db).await;
                if !matches!(still, Ok(r) if r.rows_affected() == 1) {
                    let _ = ws.send(Message::Close(Some(CloseFrame { code: 4001, reason: "revoked".into() }))).await;
                    return;
                }
                if c.kind == "game" {
                    let _ = sqlx::query("UPDATE boards SET game_seen_at=now() WHERE channel_id=$1 AND game_session=$2")
                        .bind(&c.channel).bind(&c.session).execute(&app.db).await;
                }
            }
            event = events.recv() => match event {
                Ok(e) if forward(&e, &c.channel) => {
                    let mut out = e.payload.clone();
                    if out["type"] == "board" {
                        // Published or paused: send the whole board again.
                        let Ok(snapshot) = board(app, &c.channel).await else { return; };
                        merge(&mut out, snapshot);
                    }
                    if !send(&mut ws, out).await { return; }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = ws.send(Message::Close(Some(CloseFrame { code: 4000, reason: "resync".into() }))).await;
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            incoming = ws.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if window.elapsed() >= Duration::from_secs(1) {
                        (window, sent) = (Instant::now(), 0);
                    }
                    sent += 1;
                    let reply = if sent > MESSAGES_PER_SECOND {
                        json!({"type": "error", "message": "Too many messages. At most 10 a second."})
                    } else {
                        handle(app, c, &text).await
                    };
                    if !send(&mut ws, reply).await { return; }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}
fn merge(target: &mut Value, from: Value) {
    if let (Some(t), Value::Object(f)) = (target.as_object_mut(), from) {
        t.extend(f);
    }
}
/// Board presses and effects, joystick moves, board changes and game-set state.
fn forward(e: &Arc<Event>, channel: &str) -> bool {
    e.channel == channel
        && matches!(
            e.payload["type"].as_str(),
            Some("board_effect" | "board_input" | "board" | "board_state" | "board_result")
        )
}
/// One message from a client: "ping", or (games only) "state" with control changes, "ready" when
/// the game is listening, and "cap" with the most inputs per second it wants (`per_second`); games
/// and bridges both send "capture" or "release" for a held press (`press`); games set viewer
/// "groups" (`by`, `screens`).
async fn handle(app: &App, c: &Connection, text: &str) -> Value {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return json!({"type": "error", "message": "Send JSON."});
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    match message["type"].as_str() {
        Some("ping") => json!({"type": "pong", "id": id}),
        Some("state") if c.kind == "game" => {
            let Some(controls) = message["controls"].as_object() else {
                return json!({"type": "error", "id": id, "message": "\"controls\" is an object of control changes."});
            };
            let result = async {
                let mut tx = app.db.begin().await?;
                let snapshot = boards::apply_state(&mut tx, &c.channel, controls).await?;
                tx.commit().await?;
                Ok::<_, Fail>(snapshot)
            }
            .await;
            match result {
                Ok(snapshot) => {
                    // Viewers' panels and every connected integration see the change.
                    app.chat.publish(
                        &c.channel,
                        None,
                        0,
                        json!({"type": "board_state", "state": snapshot["state"], "goals": snapshot["goals"]}),
                    );
                    json!({"type": "ack", "id": id})
                }
                Err(fail) => json!({"type": "error", "id": id, "message": fail.message}),
            }
        }
        Some("ready") if c.kind == "game" => {
            game(app, c, "game_ready=true, game_seen_at=now()").await;
            json!({"type": "ack", "id": id})
        }
        Some("cap") if c.kind == "game" => {
            let cap = match &message["per_second"] {
                Value::Null => None,
                n => match n.as_i64().and_then(|n| i16::try_from(n).ok()) {
                    Some(n) => Some(n),
                    None => {
                        return json!({"type": "error", "id": id, "message": "\"per_second\" is 1–100, or null for no cap."});
                    }
                },
            };
            if let Err(message) = boards::check_cap(cap) {
                return json!({"type": "error", "id": id, "message": message});
            }
            match sqlx::query("UPDATE boards SET input_cap=$2 WHERE channel_id=$1")
                .bind(&c.channel)
                .bind(cap)
                .execute(&app.db)
                .await
            {
                Ok(_) => json!({"type": "ack", "id": id}),
                Err(_) => json!({"type": "error", "id": id, "message": "Try again."}),
            }
        }
        Some(kind @ ("capture" | "release")) => {
            let Some(press) = message["press"].as_str() else {
                return json!({"type": "error", "id": id, "message": "\"press\" is the press ID."});
            };
            match boards::settle(app, &c.channel, press, kind == "capture").await {
                Ok(()) => json!({"type": "ack", "id": id}),
                Err(fail) => json!({"type": "error", "id": id, "message": fail.message}),
            }
        }
        Some("groups") if c.kind == "game" => {
            match boards::set_groups(app, &c.channel, message["by"].as_str(), &message["screens"])
                .await
            {
                Ok(()) => json!({"type": "ack", "id": id}),
                Err(fail) => json!({"type": "error", "id": id, "message": fail.message}),
            }
        }
        Some("state" | "ready" | "cap" | "groups") => {
            json!({"type": "error", "id": id, "message": "Only game connections can change the board."})
        }
        _ => json!({"type": "error", "id": id, "message": "Unknown message type."}),
    }
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/integrations", get(mine).post(create))
        .route("/api/me/integrations/{id}", delete(revoke))
        .route("/api/integrations/ws", get(socket))
}
