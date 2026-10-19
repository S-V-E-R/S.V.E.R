//! The Discord Gateway (docs/DISCORD_BOT.md "How it runs"): one connection for the community bot
//! on the home server. It runs only with `DISCORD_GATEWAY=1`, and only while this process holds the
//! `discord-gateway` advisory lock, so a rehearsal or rollback API never runs a second bot.
//!
//! The connection resumes after a drop and identifies afresh after a restart. Events go to
//! `dispatch` on their own task, so a slow handler never delays a heartbeat.
use super::{bot_token, call, commands, home, levels, moderation, quests};
use crate::App;
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Guilds, Guild Members, Guild Moderation, Guild Voice States, Guild Presences, Guild Messages,
/// Guild Message Reactions, Direct Messages and Message Content.
pub const INTENTS: u64 =
    1 | 1 << 1 | 1 << 2 | 1 << 7 | 1 << 8 | 1 << 9 | 1 << 10 | 1 << 12 | 1 << 15;

pub fn enabled() -> bool {
    std::env::var("DISCORD_GATEWAY").as_deref() == Ok("1") && bot_token().is_some()
}

/// Commands for the live connection (presence updates, member requests), sent between heartbeats.
static OUTBOX: Mutex<Option<mpsc::UnboundedSender<Value>>> = Mutex::new(None);
/// Sends a Gateway command; false while there's no connection.
pub fn send(command: Value) -> bool {
    OUTBOX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|tx| tx.send(command).is_ok())
}

#[derive(Default)]
struct Session {
    id: Option<String>,
    seq: Option<u64>,
    resume_url: Option<String>,
}
/// How a connection ended.
enum End {
    /// Reconnect and resume the session.
    Resume,
    /// Reconnect with a new session.
    Fresh,
    /// Discord refused the bot itself (token, intents); waits before trying again.
    Fatal(&'static str),
}

/// Forever: take the lead when no other process has it, then keep a connection open.
pub async fn run(app: App) {
    loop {
        if lead(&app).await.is_err() {
            eprintln!("discord_event=gateway outcome=retry");
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

async fn lead(app: &App) -> Result<(), sqlx::Error> {
    // The lock lives as long as this connection, which is taken out of the pool for it.
    let mut lock = app.db.acquire().await?.detach();
    let held: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext('discord-gateway'))")
        .fetch_one(&mut lock)
        .await?;
    if !held {
        return Ok(());
    }
    let (events, received) = mpsc::channel(1024);
    let handler = tokio::spawn(handle(app.clone(), received));
    let ticker = tokio::spawn(ticker(app.clone()));
    let keeper = async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if sqlx::query("SELECT 1").execute(&mut lock).await.is_err() {
                return;
            }
        }
    };
    tokio::pin!(keeper);
    let mut session = Session::default();
    let mut failures = 0u32;
    loop {
        let end = tokio::select! {
            end = connect(app, &mut session, &events) => end,
            () = &mut keeper => break,
        };
        *OUTBOX.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let (wait, problem) = match end {
            Ok(End::Resume) => {
                failures = 0;
                (1, None)
            }
            Ok(End::Fresh) => {
                session = Session::default();
                (1 + jitter(4), None)
            }
            Ok(End::Fatal(why)) => {
                session = Session::default();
                (600, Some(why))
            }
            Err(()) => {
                failures += 1;
                ((1u64 << failures.min(6)).min(60), None)
            }
        };
        status(app, false, problem).await;
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
    handler.abort();
    ticker.abort();
    Ok(())
}

/// Every minute, while this process runs the bot: voice XP and achievement posts.
async fn ticker(app: App) {
    let mut every = tokio::time::interval(Duration::from_secs(60));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        every.tick().await;
        // While disconnected nobody's voice state is current, so nothing is credited.
        let connected: bool =
            sqlx::query_scalar("SELECT coalesce((SELECT connected FROM discord_gateway),false)")
                .fetch_one(&app.db)
                .await
                .unwrap_or(false);
        if connected {
            minute(&app).await;
        }
    }
}
pub async fn minute(app: &App) {
    logged("voice_minute", levels::minute(app).await);
    logged("achievements", quests::announce(app).await);
}
fn logged(what: &str, result: crate::profiles::Res<()>) {
    if result.is_err() {
        eprintln!("discord_event={what} outcome=retry");
    }
}

fn jitter(below: u64) -> u64 {
    (uuid::Uuid::new_v4().as_u128() as u64) % below.max(1)
}

async fn gateway_url(app: &App) -> String {
    if let Ok(url) = std::env::var("DISCORD_GATEWAY_URL") {
        return url;
    }
    match call(app, Method::GET, "/gateway/bot", None).await {
        Some((200, body)) => body["url"]
            .as_str()
            .unwrap_or("wss://gateway.discord.gg")
            .into(),
        _ => "wss://gateway.discord.gg".into(),
    }
}

async fn connect(
    app: &App,
    session: &mut Session,
    events: &mpsc::Sender<(String, Value)>,
) -> Result<End, ()> {
    let token = bot_token().ok_or(())?;
    let base = match (&session.id, &session.resume_url) {
        (Some(_), Some(url)) => url.clone(),
        _ => gateway_url(app).await,
    };
    let url = format!("{}/?v=10&encoding=json", base.trim_end_matches('/'));
    let (ws, _) = tokio::time::timeout(
        Duration::from_secs(20),
        tokio_tungstenite::connect_async(url),
    )
    .await
    .map_err(drop)?
    .map_err(drop)?;
    let (mut sink, mut stream) = ws.split();
    let text = |v: Value| Message::Text(v.to_string().into());

    // Hello gives the heartbeat interval; the first beat waits a random part of it.
    let hello = tokio::time::timeout(Duration::from_secs(20), next_json(&mut stream))
        .await
        .map_err(drop)?
        .ok_or(())?;
    if hello["op"] != 10 {
        return Err(());
    }
    let every = hello["d"]["heartbeat_interval"]
        .as_u64()
        .unwrap_or(41_250)
        .max(1_000);
    let opening = match (&session.id, session.seq) {
        (Some(id), Some(seq)) => {
            json!({"op": 6, "d": {"token": token, "session_id": id, "seq": seq}})
        }
        _ => json!({"op": 2, "d": {"token": token, "intents": INTENTS,
            "properties": {"os": "linux", "browser": "sver", "device": "sver"}}}),
    };
    sink.send(text(opening)).await.map_err(drop)?;
    let (out, mut outgoing) = mpsc::unbounded_channel();
    *OUTBOX.lock().unwrap_or_else(|e| e.into_inner()) = Some(out);
    let mut beat = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_millis(jitter(every)),
        Duration::from_millis(every),
    );
    let mut acked = true;
    loop {
        tokio::select! {
            _ = beat.tick() => {
                // A beat Discord never acknowledged means the connection is dead.
                if !acked {
                    return Ok(End::Resume);
                }
                acked = false;
                sink.send(text(json!({"op": 1, "d": session.seq}))).await.map_err(drop)?;
            }
            Some(command) = outgoing.recv() => {
                sink.send(text(command)).await.map_err(drop)?;
            }
            message = stream.next() => {
                let message = match message {
                    Some(Ok(m)) => m,
                    _ => return Ok(End::Resume),
                };
                let v: Value = match message {
                    Message::Text(t) => serde_json::from_str(&t).unwrap_or(Value::Null),
                    Message::Close(frame) => return Ok(closed(frame.map(|f| u16::from(f.code)))),
                    _ => continue,
                };
                match v["op"].as_u64() {
                    Some(0) => {
                        if let Some(seq) = v["s"].as_u64() {
                            session.seq = Some(seq);
                        }
                        let kind = v["t"].as_str().unwrap_or_default().to_string();
                        if kind == "READY" {
                            session.id = v["d"]["session_id"].as_str().map(String::from);
                            session.resume_url = v["d"]["resume_gateway_url"].as_str().map(String::from);
                        }
                        if events.send((kind, v["d"].clone())).await.is_err() {
                            return Err(());
                        }
                    }
                    Some(1) => {
                        sink.send(text(json!({"op": 1, "d": session.seq}))).await.map_err(drop)?;
                    }
                    Some(7) => return Ok(End::Resume),
                    Some(9) => {
                        tokio::time::sleep(Duration::from_secs(1 + jitter(4))).await;
                        return Ok(if v["d"] == true { End::Resume } else { End::Fresh });
                    }
                    Some(11) => acked = true,
                    _ => {}
                }
            }
        }
    }
}

async fn next_json<S>(stream: &mut S) -> Option<Value>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(Ok(message)) = stream.next().await {
        if let Message::Text(t) = message {
            return serde_json::from_str(&t).ok();
        }
    }
    None
}

/// Discord's close codes: which ones end the session, and which ones mean the bot itself is wrong.
fn closed(code: Option<u16>) -> End {
    match code {
        Some(4004) => End::Fatal("Discord rejected the bot token."),
        Some(4013 | 4014) => End::Fatal(
            "Discord refused the bot's intents. Turn on Server Members, Presence and Message Content for the application in the Discord Developer Portal.",
        ),
        Some(4010..=4012) => End::Fatal("Discord refused the connection settings."),
        Some(4007 | 4009) => End::Fresh,
        _ => End::Resume,
    }
}

/// Records the connection in `discord_gateway` for the staff page and `/health`.
async fn status(app: &App, connected: bool, problem: Option<&str>) {
    let _ = sqlx::query("INSERT INTO discord_gateway(singleton,connected,connected_at,problem) VALUES(true,$1,CASE WHEN $1 THEN now() END,$2)
        ON CONFLICT(singleton) DO UPDATE SET connected=$1,connected_at=CASE WHEN $1 THEN now() ELSE discord_gateway.connected_at END,
        problem=CASE WHEN $1 THEN NULL ELSE coalesce($2,discord_gateway.problem) END")
        .bind(connected).bind(problem).execute(&app.db).await;
}

async fn handle(app: App, mut received: mpsc::Receiver<(String, Value)>) {
    // Message and member events go through moderation in order on their own task, so a flood of
    // them never holds up the rest. It ends once this task drops its sender.
    // ponytail: an unbounded queue; cap it if a flood ever outruns the database.
    let (queue, mut queued) = mpsc::unbounded_channel::<(String, Value)>();
    let worker = app.clone();
    tokio::spawn(async move {
        while let Some((kind, data)) = queued.recv().await {
            dispatch(&worker, &kind, data).await;
        }
    });
    let mut seen = tokio::time::Instant::now() - Duration::from_secs(60);
    while let Some((kind, data)) = received.recv().await {
        // The staff page shows when the last event arrived, to the nearest half minute.
        if seen.elapsed() >= Duration::from_secs(30) {
            seen = tokio::time::Instant::now();
            let _ = sqlx::query("UPDATE discord_gateway SET last_event_at=now()")
                .execute(&app.db)
                .await;
        }
        if moderation::QUEUED.contains(&kind.as_str()) {
            let _ = queue.send((kind, data));
        } else {
            dispatch(&app, &kind, data).await;
        }
    }
}

/// Routes one Gateway event to the feature that handles it.
pub async fn dispatch(app: &App, kind: &str, data: Value) {
    match kind {
        "READY" => {
            status(app, true, None).await;
            let guilds: Vec<&str> = data["guilds"]
                .as_array()
                .map(|g| g.iter().filter_map(|g| g["id"].as_str()).collect())
                .unwrap_or_default();
            let _ = sqlx::query(
                "UPDATE discord_gateway SET application_id=$1,bot_id=$2,bot_name=$3,guild_ids=$4",
            )
            .bind(data["application"]["id"].as_str())
            .bind(data["user"]["id"].as_str())
            .bind(data["user"]["username"].as_str())
            .bind(&guilds)
            .execute(&app.db)
            .await;
            if !commands::register(app).await {
                eprintln!("discord_event=commands outcome=retry");
            }
        }
        "RESUMED" => status(app, true, None).await,
        "GUILD_CREATE" | "GUILD_UPDATE" => {
            home::guild_seen(app, &data).await;
            if kind == "GUILD_CREATE" {
                logged("voice", levels::guild_create(app, &data).await);
            }
        }
        // Moderation first: a message it removed earns no XP.
        "MESSAGE_CREATE" => {
            if !moderation::event(app, kind, &data).await {
                logged("message", levels::message(app, &data).await);
            }
        }
        "MESSAGE_UPDATE" | "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK" | "GUILD_MEMBER_ADD"
        | "GUILD_MEMBER_REMOVE" => {
            moderation::event(app, kind, &data).await;
        }
        "MESSAGE_REACTION_ADD" => logged("reaction", levels::reaction(app, &data).await),
        "VOICE_STATE_UPDATE" => logged("voice", levels::voice_state(app, &data).await),
        "INTERACTION_CREATE" => {
            // Each interaction gets its own task: Discord wants an answer within 3 seconds.
            let app = app.clone();
            tokio::spawn(async move { commands::interaction(&app, data).await });
        }
        _ => {}
    }
}
