//! Live events (docs/DEVELOPER_PLATFORM.md §2, Mixer's "Constellation"): one WebSocket,
//! `/api/events`, where apps subscribe to topics. Events are written to the `events` outbox in the
//! same transaction as the change, drained every second onto the in-process hub, and replayed
//! after a client's last event ID for 5 minutes. Events never carry email, IP addresses or
//! internal IDs; follower names and subscriptions are private topics.
use crate::{
    App,
    profiles::{Fail, Res},
};
use axum::{
    Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

/// Public channel topics (client ID only) and private ones (owner or moderator, `events:private`).
const PUBLIC: [&str; 3] = ["live", "follows", "raids"];
const PRIVATE: [&str; 3] = ["follows:detail", "subs", "tributes"];
const MAX_TOPICS: usize = 200;
const MAX_CONNECTIONS: usize = 10;
/// The hub channel the drain publishes on.
const HUB: &str = "events";
type Row = (i64, String, Value, chrono::DateTime<chrono::Utc>);
type Person = (String, Vec<String>);

fn event((id, topic, data, at): Row) -> Value {
    json!({"type": "event", "id": id, "topic": topic, "data": data, "at": at})
}
/// Writes a channel event (`channel:{username}:{kind}`) in the caller's transaction.
pub async fn emit(
    db: &mut PgConnection,
    channel: &str,
    kind: &str,
    data: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO events(topic,data) SELECT 'channel:'||lower(username)||':'||$2,$3 FROM users WHERE id=$1")
        .bind(channel)
        .bind(kind)
        .bind(data)
        .execute(db)
        .await?;
    Ok(())
}

static CURSOR: AtomicI64 = AtomicI64::new(-1);
/// Every second: new outbox rows go out on the hub (one API instance, as for chat).
pub async fn drain(app: &App) -> Res<()> {
    if CURSOR.load(Ordering::Relaxed) < 0 {
        let start: i64 = sqlx::query_scalar("SELECT coalesce(max(id),0) FROM events")
            .fetch_one(&app.db)
            .await?;
        CURSOR.store(start, Ordering::Relaxed);
    }
    let rows: Vec<Row> =
        sqlx::query_as("SELECT id,topic,data,at FROM events WHERE id>$1 ORDER BY id LIMIT 1000")
            .bind(CURSOR.load(Ordering::Relaxed))
            .fetch_all(&app.db)
            .await?;
    for row in rows {
        let id = row.0;
        app.chat.publish(HUB, None, id, event(row));
        CURSOR.store(id, Ordering::Relaxed);
    }
    Ok(())
}
/// Retention: events older than 10 minutes go.
pub async fn prune(app: &App) -> Res<()> {
    sqlx::query("DELETE FROM events WHERE at<now()-interval '10 minutes'")
        .execute(&app.db)
        .await?;
    Ok(())
}

/// Whether a topic exists and this connection may subscribe to it.
async fn allowed(app: &App, topic: &str, user: Option<&Person>) -> Res<bool> {
    let Some((name, kind)) = topic
        .strip_prefix("channel:")
        .and_then(|rest| rest.split_once(':'))
    else {
        return Ok(false);
    };
    if PUBLIC.contains(&kind) {
        return Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM channel_users WHERE lower(username)=$1 AND eligible)",
        )
        .bind(name)
        .fetch_one(&app.db)
        .await?);
    }
    let Some((user, _)) = user.filter(|(_, s)| s.iter().any(|g| g == "events:private")) else {
        return Ok(false);
    };
    if !PRIVATE.contains(&kind) {
        return Ok(false);
    }
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_users c WHERE lower(c.username)=$1 AND c.eligible AND (c.id=$2 OR EXISTS(SELECT 1 FROM channel_moderators m WHERE m.channel_id=c.id AND m.user_id=$2)))")
        .bind(name).bind(user).fetch_one(&app.db).await?)
}

static CONNECTIONS: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Mutex::default);
struct Slot(String);
impl Drop for Slot {
    fn drop(&mut self) {
        let mut open = CONNECTIONS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = open.get_mut(&self.0) {
            *n = n.saturating_sub(1);
        }
    }
}

#[derive(Deserialize)]
pub struct Connect {
    client_id: String,
    #[serde(default)]
    access_token: Option<String>,
}
/// GET /api/events?client_id=…[&access_token=…]: query parameters, because browsers can't set
/// headers on a WebSocket.
async fn socket(
    State(app): State<App>,
    Query(input): Query<Connect>,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    let user =
        crate::devapps::identify(&app, &input.client_id, input.access_token.as_deref()).await?;
    let key = format!(
        "{}:{}",
        input.client_id,
        user.as_ref().map_or("-", |u| u.0.as_str())
    );
    {
        let mut open = CONNECTIONS.lock().unwrap_or_else(|e| e.into_inner());
        let n = open.entry(key.clone()).or_default();
        if *n >= MAX_CONNECTIONS {
            return Err(Fail::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "Up to 10 connections per app and person.",
            ));
        }
        *n += 1;
    }
    let slot = Slot(key);
    Ok(upgrade
        .max_message_size(16 * 1024)
        .on_upgrade(move |ws| session(app, user, slot, ws)))
}
async fn reply(ws: &mut WebSocket, value: Value) -> bool {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}
async fn session(app: App, user: Option<Person>, _slot: Slot, mut ws: WebSocket) {
    let mut events = app.chat.subscribe();
    let mut topics: HashSet<String> = HashSet::new();
    loop {
        tokio::select! {
            incoming = events.recv() => match incoming {
                Ok(e) if e.channel == HUB => {
                    if e.payload["topic"].as_str().is_some_and(|t| topics.contains(t)) && !reply(&mut ws, e.payload.clone()).await {
                        return;
                    }
                }
                Ok(_) => {}
                // Missed events: the client resubscribes with `since` to replay them.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if !reply(&mut ws, json!({"type": "resync"})).await { return; }
                }
                Err(_) => return,
            },
            incoming = ws.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let Ok(call) = serde_json::from_str::<Value>(&text) else {
                        if !reply(&mut ws, json!({"type": "reply", "error": "Send JSON."})).await { return; }
                        continue;
                    };
                    let mut out = json!({"type": "reply", "id": call["id"]});
                    match handle(&app, user.as_ref(), &mut topics, &call).await {
                        Ok((result, replay)) => {
                            out["result"] = result;
                            if !reply(&mut ws, out).await { return; }
                            for event in replay {
                                if !reply(&mut ws, event).await { return; }
                            }
                        }
                        Err(why) => {
                            out["error"] = json!(why);
                            if !reply(&mut ws, out).await { return; }
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
    }
}
/// One method call: subscribe (with an optional `since` to replay), unsubscribe or ping.
async fn handle(
    app: &App,
    user: Option<&Person>,
    topics: &mut HashSet<String>,
    call: &Value,
) -> Result<(Value, Vec<Value>), String> {
    if call["type"] != "method" {
        return Err("Send {\"type\":\"method\",…}.".into());
    }
    let asked: Vec<String> = call["params"]["topics"]
        .as_array()
        .map(|t| {
            t.iter()
                .filter_map(|x| x.as_str())
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default();
    match call["method"].as_str() {
        Some("ping") => Ok((json!("pong"), Vec::new())),
        Some("unsubscribe") => {
            for topic in &asked {
                topics.remove(topic);
            }
            Ok((
                json!({"topics": topics.iter().collect::<Vec<_>>()}),
                Vec::new(),
            ))
        }
        Some("subscribe") => {
            let (mut added, mut refused) = (Vec::new(), Vec::new());
            for topic in asked {
                let ok = topics.len() < MAX_TOPICS
                    && allowed(app, &topic, user)
                        .await
                        .map_err(|_| "Try again.".to_string())?;
                if ok {
                    topics.insert(topic.clone());
                    added.push(topic);
                } else {
                    refused.push(topic);
                }
            }
            let mut replay = Vec::new();
            if let Some(since) = call["params"]["since"].as_i64() {
                let rows: Vec<Row> = sqlx::query_as("SELECT id,topic,data,at FROM events WHERE id>$1 AND topic=ANY($2) AND at>now()-interval '5 minutes' ORDER BY id LIMIT 1000")
                    .bind(since).bind(&added).fetch_all(&app.db).await.map_err(|_| "Try again.".to_string())?;
                replay = rows.into_iter().map(event).collect();
            }
            Ok((json!({"subscribed": added, "refused": refused}), replay))
        }
        _ => Err("Use subscribe, unsubscribe or ping.".into()),
    }
}

pub fn routes() -> Router<App> {
    Router::new().route("/api/events", get(socket))
}
