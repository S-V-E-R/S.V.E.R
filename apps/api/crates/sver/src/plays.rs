//! Dedicated Plays voting. The separate game project consumes short-lived commands over HTTP.
use crate::{
    App, auth, moderation,
    profiles::{self, Fail, Res},
    security, streams,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};

const COMMANDS: [&str; 8] = ["up", "down", "left", "right", "a", "b", "start", "select"];
#[derive(FromRow)]
struct Runtime {
    channel_id: String,
    game: String,
    ready: bool,
    heartbeat_at: Option<DateTime<Utc>>,
    input_mode: String,
    last_round: i64,
    last_command: Option<String>,
    last_chosen_at: Option<DateTime<Utc>>,
}
impl Runtime {
    fn connected(&self, now: DateTime<Utc>) -> bool {
        self.ready
            && self
                .heartbeat_at
                .is_some_and(|t| t > now - chrono::Duration::seconds(10))
    }
}
async fn clock(db: &mut PgConnection) -> Res<(DateTime<Utc>, i64)> {
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(db)
        .await?;
    Ok((now, now.timestamp().div_euclid(5)))
}
pub async fn is_channel(db: &mut PgConnection, channel: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM plays_runtime WHERE enabled AND channel_id=$1)",
    )
    .bind(channel)
    .fetch_one(db)
    .await?)
}
async fn channel(app: &App, name: &str) -> Res<String> {
    let mut db = app.db.acquire().await?;
    let user = profiles::eligible_by_name(&mut db, name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    if !is_channel(&mut db, &user.id).await? {
        return Err(Fail::missing());
    }
    Ok(user.id)
}
/// Whether `user` is watching: a Counted or Trusted, signed-in and verified session on the Plays
/// broadcast (docs/PLAYS.md "Who controls the game"). Bots and chat-only accounts have none.
async fn watching(db: &mut PgConnection, broadcast: &str, user: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playback_leases WHERE broadcast_id=$1 AND viewer_key='u:'||$2 AND level IN ('counted','trusted') AND signed_in AND verified AND expires_at>clock_timestamp())")
        .bind(broadcast).bind(user).fetch_one(db).await?)
}
/// The part of GET /plays that's the same for everyone, per channel name: (when, channel,
/// live broadcast, body). Every viewer polls once a second, so it's built at most once a second
/// and again after each bridge beat.
type Shared = (std::time::Instant, String, Option<String>, Value);
static SHARED: std::sync::Mutex<Option<std::collections::HashMap<String, Shared>>> =
    std::sync::Mutex::new(None);
fn forget_shared() {
    if let Ok(mut cache) = SHARED.lock() {
        *cache = None;
    }
}
async fn shared(app: &App, name: &str) -> Res<(String, Option<String>, Value)> {
    if let Ok(cache) = SHARED.lock()
        && let Some((at, channel, live, body)) = cache.as_ref().and_then(|c| c.get(name))
        && at.elapsed() < std::time::Duration::from_secs(1)
    {
        return Ok((channel.clone(), live.clone(), body.clone()));
    }
    let channel = channel(app, name).await?;
    let mut db = app.db.acquire().await?;
    let r: Runtime = sqlx::query_as("SELECT * FROM plays_runtime WHERE enabled AND channel_id=$1")
        .bind(&channel)
        .fetch_one(&mut *db)
        .await?;
    let (now, round) = clock(&mut db).await?;
    let live = streams::live_broadcast(&mut db, &channel).await?;
    let votes:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('command',command,'votes',count(*)) FROM plays_votes WHERE round=$1 GROUP BY command ORDER BY command").bind(round).fetch_all(&mut *db).await?;
    let body = json!({"game":r.game,"round":round,"closes_at":DateTime::from_timestamp((round+1)*5,0),"server_time":now,"connected":r.connected(now)&&live.is_some(),"input_mode":r.input_mode,"votes":votes,"last_command":r.last_command,"last_chosen_at":r.last_chosen_at});
    if let Ok(mut cache) = SHARED.lock() {
        cache.get_or_insert_with(Default::default).insert(
            name.to_string(),
            (
                std::time::Instant::now(),
                channel.clone(),
                live.clone(),
                body.clone(),
            ),
        );
    }
    Ok((channel, live, body))
}
async fn state(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let (_, live, mut body) = shared(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    let (mut my_vote, mut can_vote, mut why) = (None::<String>, false, None);
    if let Some(u) = &viewer {
        let mut db = app.db.acquire().await?;
        my_vote =
            sqlx::query_scalar("SELECT command FROM plays_votes WHERE round=$1 AND user_id=$2")
                .bind(body["round"].as_i64())
                .bind(&u.id)
                .fetch_optional(&mut *db)
                .await?;
        can_vote = u.email_verified
            && match &live {
                Some(b) => watching(&mut db, b, &u.id).await?,
                None => false,
            };
        why = (!can_vote).then_some(if u.email_verified {
            "Your vote counts once you've been watching the stream for a moment."
        } else {
            "Verify your email to vote."
        });
    }
    body["my_vote"] = json!(my_vote);
    body["can_vote"] = json!(can_vote);
    body["why"] = json!(why);
    Ok(Json(body))
}
#[derive(Deserialize)]
struct Vote {
    command: String,
    round: i64,
}
async fn vote(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Vote>,
) -> Res<Json<Value>> {
    if !COMMANDS.contains(&input.command.as_str()) {
        return Err(Fail::bad("Choose a game button."));
    }
    let channel = channel(&app, &name).await?;
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_verified(&user, "Verify your email to control the game.")?;
    let mut db = app.db.acquire().await?;
    if profiles::channel_user_by_id(&mut db, &user.id)
        .await?
        .is_none_or(|u| !u.eligible)
        || profiles::blocked_between(&mut db, &user.id, &channel).await?
    {
        return Err(Fail::denied("You can't control this channel."));
    }
    if let Some(broadcast) = streams::live_broadcast(&mut db, &channel).await?
        && !watching(&mut db, &broadcast, &user.id).await?
    {
        return Err(Fail::denied(
            "Your vote counts once you've been watching the stream for a moment.",
        ));
    }
    drop(db);
    // A button press isn't a chat message: bans and timeouts apply, AutoMod and slow mode don't.
    moderation::check_restriction(&app, &channel, &user.id).await?;
    profiles::rate(&app, format!("plays-vote:{}", user.id), 20, 10).await?;
    let mut tx = app.db.begin().await?;
    if !record(
        &mut tx,
        &channel,
        &user.id,
        &input.command,
        Some(input.round),
    )
    .await?
    {
        return Err(Fail::conflict(
            "Already voted, the round ended, or the game is reconnecting. Refresh and try the next round.",
        ));
    }
    tx.commit().await?;
    Ok(Json(json!({"voted":true})))
}
async fn record(
    db: &mut PgConnection,
    channel: &str,
    user: &str,
    command: &str,
    expected: Option<i64>,
) -> Res<bool> {
    // ponytail: one row serializes one channel's vote boundary; split by channel if Plays expands.
    let r: Option<Runtime> =
        sqlx::query_as("SELECT * FROM plays_runtime WHERE enabled AND channel_id=$1 FOR UPDATE")
            .bind(channel)
            .fetch_optional(&mut *db)
            .await?;
    let Some(r) = r else { return Ok(false) };
    let (now, round) = clock(db).await?;
    if !r.connected(now) || expected.is_some_and(|v| v != round) {
        return Ok(false);
    }
    // Buttons and chat alike: only someone watching the broadcast votes.
    match streams::live_broadcast(db, channel).await? {
        Some(broadcast) if watching(db, &broadcast, user).await? => {}
        _ => return Ok(false),
    }
    Ok(sqlx::query(
        "INSERT INTO plays_votes(round,user_id,command) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
    )
    .bind(round)
    .bind(user)
    .bind(command)
    .execute(db)
    .await?
    .rows_affected()
        == 1)
}
/// Only called for a newly accepted, verified and moderation-checked chat message.
pub async fn chat_vote(
    db: &mut PgConnection,
    channel: &str,
    user: &auth::User,
    body: &str,
) -> Res<()> {
    let command = body.to_ascii_lowercase();
    if COMMANDS.contains(&command.as_str()) {
        record(db, channel, &user.id, &command, None).await?;
    }
    Ok(())
}
#[derive(Deserialize)]
struct Beat {
    ready: bool,
    input_mode: String,
    /// The host watchdog's current problem (frozen picture, restarted runner), if any.
    #[serde(default)]
    problem: Option<String>,
}
async fn bridge(
    State(app): State<App>,
    headers: HeaderMap,
    Json(input): Json<Beat>,
) -> Res<Json<Value>> {
    let key = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|s| s.len() == 43)
        .ok_or_else(Fail::missing)?;
    if !["chat", "rl"].contains(&input.input_mode.as_str()) {
        return Err(Fail::bad("Invalid input mode."));
    }
    let problem = input
        .problem
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    if problem.is_some_and(|p| p.chars().count() > 200 || p.chars().any(char::is_control)) {
        return Err(Fail::bad("Invalid problem report."));
    }
    let mut tx = app.db.begin().await?;
    let r: Runtime =
        sqlx::query_as("SELECT * FROM plays_runtime WHERE enabled AND bridge_hash=$1 FOR UPDATE")
            .bind(security::digest(key))
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Fail::missing)?;
    let (now, round) = clock(&mut tx).await?;
    let eligible = profiles::channel_user_by_id(&mut tx, &r.channel_id)
        .await?
        .is_some_and(|u| u.eligible);
    let live = streams::live_broadcast(&mut tx, &r.channel_id)
        .await?
        .filter(|_| eligible);
    let ready = input.ready && live.is_some();
    // Viewers who keep the AI off: signed-in, verified and Counted or Trusted (guests don't).
    let viewers: i64 = match &live {
        Some(id) => sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE broadcast_id=$1 AND level IN ('counted','trusted') AND signed_in AND verified AND expires_at>clock_timestamp()")
            .bind(id).fetch_one(&mut *tx).await?,
        None => 0,
    };
    // At most once: expired/missed rounds are skipped, never replayed into a later game state.
    // A failed HTTP delivery can lose one input; retrying it could press a destructive button twice.
    // Ties are random, so nobody can predict which tied command wins.
    let command: Option<String> = if ready && r.last_round < round - 1 {
        sqlx::query_scalar("SELECT command FROM plays_votes WHERE round=$1 GROUP BY command ORDER BY count(*) DESC,random() LIMIT 1").bind(round-1).fetch_optional(&mut *tx).await?
    } else {
        None
    };
    sqlx::query("UPDATE plays_runtime SET heartbeat_at=$1,ready=$2,input_mode=$3,last_round=greatest(last_round,$4),last_command=coalesce($5,last_command),last_chosen_at=CASE WHEN $5::text IS NOT NULL THEN $1 ELSE last_chosen_at END WHERE singleton")
        .bind(now).bind(ready).bind(&input.input_mode).bind(round-1).bind(&command).execute(&mut *tx).await?;
    sqlx::query("UPDATE plays_runtime SET problem=$1,problem_since=CASE WHEN $1::text IS NULL THEN NULL ELSE coalesce(problem_since,$2) END WHERE singleton")
        .bind(problem).bind(now).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM plays_votes WHERE round<$1")
        .bind(round - 120)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    forget_shared();
    Ok(Json(
        json!({"round":round-1,"command":command,"viewers":viewers,"live":ready}),
    ))
}
/// Nobody signs in as the Plays channel, so it accepts co-stream invitations itself, but only
/// from its configured host (`plays_runtime.costream_host_id`, set by an operator).
pub async fn accepts_costream(db: &mut PgConnection, channel: &str, host: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plays_runtime WHERE enabled AND channel_id=$1 AND costream_host_id=$2)")
        .bind(channel)
        .bind(host)
        .fetch_one(db)
        .await?)
}
/// Plays is the always-on monitoring stream, so its own outages email staff admins: a problem the
/// host watchdog reported, a bridge that stopped checking in, or no live broadcast, each for more
/// than two minutes. At most one email an hour while it stays unhealthy.
pub async fn tick(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let issue: Option<String> = sqlx::query_scalar(
        "SELECT CASE
            WHEN r.problem IS NOT NULL AND r.problem_since<=now()-interval '2 minutes' THEN r.problem
            WHEN r.heartbeat_at IS NULL OR r.heartbeat_at<=now()-interval '2 minutes' THEN 'The Plays bridge has stopped checking in.'
            WHEN NOT EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=r.channel_id AND (b.state IN ('LIVE','RECONNECTING') OR b.ended_at>now()-interval '2 minutes')) THEN 'The Plays stream is offline.'
         END
         FROM plays_runtime r WHERE r.enabled AND (r.alerted_at IS NULL OR r.alerted_at<=now()-interval '1 hour') FOR UPDATE",
    )
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    let Some(issue) = issue else {
        return Ok(());
    };
    let admins: Vec<String> = sqlx::query_scalar("SELECT r.user_id FROM staff_roles r JOIN users u ON u.id=r.user_id AND u.deleted_at IS NULL WHERE r.role='admin'")
        .fetch_all(&mut *tx).await?;
    let body = format!(
        "{issue}

The host watchdog restarts the game automatically when its picture stops. Check the channel and the server logs if this repeats: {}/admin",
        app.config.origin
    );
    for admin in admins {
        crate::safety::queue_notice_id(
            app,
            &mut tx,
            &admin,
            "S.V.E.R Plays needs attention",
            &body,
        )
        .await?;
    }
    sqlx::query("UPDATE plays_runtime SET alerted_at=now() WHERE singleton")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    eprintln!("plays_event=alert outcome=queued");
    Ok(())
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/plays", get(state).post(vote))
        .route("/api/plays/bridge", post(bridge))
}
