//! Module 6 Support, part 3 (docs/SUPPORT.md "Engagement Valor"): per-channel loyalty points
//! earned by verified viewers (real playback, chatting, a one-time follow bonus) and spent on the
//! channel's rewards. No cash value; a channel ban freezes earning and spending there.
use crate::{
    App, moderation,
    profiles::{self, Fail, Res},
    security as sec, text,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::sync::atomic::{AtomicI64, Ordering};

/// Earning rates. Safe example values live here; production reads `ENGAGEMENT_TUNING_FILE`.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct Tuning {
    /// Points for each `watch_seconds` of real playback.
    pub watch_points: i64,
    pub watch_seconds: i64,
    /// Points for chatting, at most once per `chat_cooldown_seconds`.
    pub chat_points: i64,
    pub chat_cooldown_seconds: i64,
    /// One-time bonus for following a channel.
    pub follow_bonus: i64,
    /// Starting cost of the built-in "highlight my message" reward.
    pub highlight_cost: i32,
}
impl Default for Tuning {
    fn default() -> Self {
        Self {
            watch_points: 10,
            watch_seconds: 300,
            chat_points: 5,
            chat_cooldown_seconds: 300,
            follow_bonus: 300,
            highlight_cost: 500,
        }
    }
}
impl Tuning {
    pub fn from_env() -> std::result::Result<Self, String> {
        match std::env::var("ENGAGEMENT_TUNING_FILE") {
            Ok(path) if !path.is_empty() => {
                let text = std::fs::read_to_string(path)
                    .map_err(|_| "ENGAGEMENT_TUNING_FILE is not readable")?;
                serde_json::from_str(&text)
                    .map_err(|_| "ENGAGEMENT_TUNING_FILE is not valid".into())
            }
            _ => Ok(Self::default()),
        }
    }
}

// ---- Earning ----

static LAST_WATCH: AtomicI64 = AtomicI64::new(0);
/// Called from the 5-second media loop; awards watch points at most once a minute.
pub async fn tick(app: &App) -> Res<()> {
    let now = Utc::now().timestamp();
    if now - LAST_WATCH.load(Ordering::Relaxed) < 60 {
        return Ok(());
    }
    LAST_WATCH.store(now, Ordering::Relaxed);
    award_watch(app).await
}
/// Watch points for real playback: a signed-in, verified viewer whose lease viewer integrity
/// counts, on a live stream, once per `watch_seconds` per channel.
pub async fn award_watch(app: &App) -> Res<()> {
    let t = &app.config.engagement;
    sqlx::query("INSERT INTO engagement(channel_id,user_id,balance,earned,last_watch_at)
        SELECT DISTINCT b.owner_id,u.id,$1::bigint,$1::bigint,now()
        FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id JOIN users u ON l.viewer_key='u:'||u.id
        WHERE l.expires_at>now() AND l.level IN ('counted','trusted') AND b.state IN ('LIVE','RECONNECTING')
            AND u.email_verified AND u.deleted_at IS NULL AND u.id<>b.owner_id
            AND NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=b.owner_id AND r.user_id=u.id AND r.kind='ban')
        ON CONFLICT(channel_id,user_id) DO UPDATE SET balance=engagement.balance+EXCLUDED.balance,
            earned=engagement.earned+EXCLUDED.earned, last_watch_at=now()
        WHERE engagement.last_watch_at IS NULL OR engagement.last_watch_at<=now()-make_interval(secs=>$2)")
        .bind(t.watch_points).bind((t.watch_seconds - 5).max(0) as f64)
        .execute(&app.db).await?;
    Ok(())
}
/// Chat points (inside the message's transaction; chat already requires a verified, unbanned
/// sender).
pub(crate) async fn chatted(
    tx: &mut PgConnection,
    t: &Tuning,
    channel: &str,
    user: &str,
) -> Res<()> {
    if channel == user || t.chat_points <= 0 {
        return Ok(());
    }
    sqlx::query("INSERT INTO engagement(channel_id,user_id,balance,earned,last_chat_at) VALUES($1,$2,$3,$3,now())
        ON CONFLICT(channel_id,user_id) DO UPDATE SET balance=engagement.balance+$3, earned=engagement.earned+$3, last_chat_at=now()
        WHERE engagement.last_chat_at IS NULL OR engagement.last_chat_at<=now()-make_interval(secs=>$4)")
        .bind(channel).bind(user).bind(t.chat_points).bind(t.chat_cooldown_seconds as f64)
        .execute(tx).await?;
    Ok(())
}
/// The one-time follow bonus (inside the follow's transaction); refollowing never pays again.
pub(crate) async fn followed(
    tx: &mut PgConnection,
    t: &Tuning,
    channel: &str,
    user: &str,
) -> Res<()> {
    sqlx::query("INSERT INTO engagement(channel_id,user_id,balance,earned,follow_bonus)
        SELECT $1,$2,$3,$3,true FROM users u WHERE u.id=$2 AND u.email_verified AND $1<>$2
            AND NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=$1 AND r.user_id=$2 AND r.kind='ban')
        ON CONFLICT(channel_id,user_id) DO UPDATE SET balance=engagement.balance+$3, earned=engagement.earned+$3, follow_bonus=true
        WHERE NOT engagement.follow_bonus")
        .bind(channel).bind(user).bind(t.follow_bonus)
        .execute(tx).await?;
    Ok(())
}

// ---- Rewards ----

#[derive(sqlx::FromRow)]
struct Reward {
    id: String,
    name: String,
    cost: i32,
    cooldown_seconds: i32,
    per_stream_limit: Option<i32>,
    prompt: Option<String>,
}
const REWARD: &str =
    "SELECT id,name,cost,cooldown_seconds,per_stream_limit,prompt FROM channel_rewards";
/// A reward as JSON, with when its cooldown ends.
const REWARD_JSON: &str = "jsonb_build_object('id',r.id,'kind',r.kind,'name',r.name,'cost',r.cost,'cooldown_seconds',r.cooldown_seconds,
    'per_stream_limit',r.per_stream_limit,'prompt',r.prompt,'enabled',r.enabled,
    'ready_at',(SELECT max(d.created_at)+make_interval(secs=>r.cooldown_seconds) FROM reward_redemptions d WHERE d.reward_id=r.id AND d.status<>'refunded' AND r.cooldown_seconds>0))";

/// Every channel has the built-in highlight reward (created on first look).
async fn ensure_highlight(db: &mut PgConnection, channel: &str, t: &Tuning) -> Res<()> {
    sqlx::query("INSERT INTO channel_rewards(id,channel_id,kind,name,cost) VALUES($1,$2,'highlight','Highlight my message',$3) ON CONFLICT (channel_id) WHERE kind='highlight' DO NOTHING")
        .bind(profiles::new_id()).bind(channel).bind(t.highlight_cost)
        .execute(db).await?;
    Ok(())
}
async fn channel(app: &App, name: &str) -> Res<String> {
    let mut conn = app.db.acquire().await?;
    Ok(profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id)
}

/// Spends points on `reward` inside `tx`: a channel ban freezes spending; the reward's cooldown
/// and per-stream limit hold under concurrent redemptions (the reward row is locked). Returns
/// false when redemption `id` already exists, so a retried request changes nothing.
async fn redeem_in(
    tx: &mut PgConnection,
    reward: &Reward,
    channel: &str,
    user: &str,
    id: &str,
    input: Option<&str>,
    done: bool,
) -> Res<bool> {
    sqlx::query("SELECT 1 FROM channel_rewards WHERE id=$1 FOR UPDATE")
        .bind(&reward.id)
        .execute(&mut *tx)
        .await?;
    let repeat: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM reward_redemptions WHERE id=$1 AND user_id=$2)",
    )
    .bind(id)
    .bind(user)
    .fetch_one(&mut *tx)
    .await?;
    if repeat {
        return Ok(false);
    }
    let banned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND kind='ban')")
        .bind(channel).bind(user).fetch_one(&mut *tx).await?;
    if banned {
        return Err(Fail::denied(
            "You can't spend Engagement Valor in this channel.",
        ));
    }
    if reward.cooldown_seconds > 0 {
        let ready: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT max(created_at)+make_interval(secs=>$2) FROM reward_redemptions WHERE reward_id=$1 AND status<>'refunded'")
            .bind(&reward.id).bind(f64::from(reward.cooldown_seconds)).fetch_one(&mut *tx).await?;
        if let Some(ready) = ready.filter(|r| *r > Utc::now()) {
            return Err(Fail {
                retry: Some((ready - Utc::now()).num_seconds().max(1)),
                ..Fail::conflict("That reward is cooling down.")
            });
        }
    }
    if let Some(limit) = reward.per_stream_limit {
        // The current (or most recent) stream's redemptions.
        let used: i64 = sqlx::query_scalar("SELECT count(*) FROM reward_redemptions WHERE reward_id=$1 AND status<>'refunded' AND created_at>=coalesce((SELECT max(started_at) FROM broadcasts WHERE owner_id=$2),'-infinity')")
            .bind(&reward.id).bind(channel).fetch_one(&mut *tx).await?;
        if used >= i64::from(limit) {
            return Err(Fail::conflict(
                "That reward has reached its limit for this stream.",
            ));
        }
    }
    let paid = sqlx::query("UPDATE engagement SET balance=balance-$3 WHERE channel_id=$1 AND user_id=$2 AND balance>=$3")
        .bind(channel).bind(user).bind(i64::from(reward.cost))
        .execute(&mut *tx).await?.rows_affected();
    if paid == 0 {
        return Err(Fail::conflict("You don't have enough Engagement Valor."));
    }
    sqlx::query("INSERT INTO reward_redemptions(id,reward_id,channel_id,user_id,name,cost,input,status,resolved_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,CASE WHEN $9 THEN now() END)")
        .bind(id).bind(&reward.id).bind(channel).bind(user).bind(&reward.name).bind(reward.cost)
        .bind(input).bind(if done { "done" } else { "pending" }).bind(done)
        .execute(&mut *tx).await?;
    Ok(true)
}

/// "Highlight my message": paid inside the chat message's transaction, so the points move only
/// if the message (which passed every chat rule) is stored.
pub(crate) async fn highlight(
    tx: &mut PgConnection,
    channel: &str,
    user: &str,
    message: &str,
) -> Res<()> {
    let reward: Option<Reward> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{REWARD} WHERE channel_id=$1 AND kind='highlight' AND enabled"
    )))
    .bind(channel)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(reward) = reward else {
        return Err(Fail::field(
            "highlight",
            "Highlighted messages aren't available in this chat.",
        ));
    };
    redeem_in(tx, &reward, channel, user, message, None, true).await?;
    Ok(())
}

async fn balance(app: &App, channel: &str, user: &str) -> Res<i64> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT balance FROM engagement WHERE channel_id=$1 AND user_id=$2),0)",
    )
    .bind(channel)
    .bind(user)
    .fetch_one(&app.db)
    .await?)
}

/// GET /api/channels/{username}/rewards: the channel's enabled rewards and the viewer's balance.
async fn list(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    ensure_highlight(
        &mut *app.db.acquire().await?,
        &channel,
        &app.config.engagement,
    )
    .await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    // REWARD_JSON is fixed SQL; the channel is bound.
    let rewards: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REWARD_JSON} FROM channel_rewards r WHERE r.channel_id=$1 AND r.enabled ORDER BY r.kind='custom', r.cost, r.created_at"
    )))
    .bind(&channel)
    .fetch_all(&app.db)
    .await?;
    let balance = match &viewer {
        Some(v) if v.id != channel => Some(balance(&app, &channel, &v.id).await?),
        _ => None,
    };
    Ok(Json(json!({"rewards": rewards, "balance": balance})))
}

#[derive(Deserialize)]
pub struct Redeem {
    id: String,
    input: Option<String>,
}
/// POST /api/channels/{username}/rewards/{id}/redeem: a custom reward goes to the owner's queue.
async fn redeem(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, reward)): Path<(String, String)>,
    Json(input): Json<Redeem>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to redeem rewards.")?;
    let channel = channel(&app, &name).await?;
    if channel == user.id {
        return Err(Fail::bad("You can't redeem your own channel's rewards."));
    }
    if uuid::Uuid::parse_str(&input.id).is_err() {
        return Err(Fail::bad("Invalid request ID."));
    }
    let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1))")
        .bind(&channel).bind(&user.id).fetch_one(&app.db).await?;
    if blocked {
        return Err(Fail::denied("You can't redeem rewards in this channel."));
    }
    sec::reserve(&app, vec![format!("redeem:{}", user.id)], 30, 60).await?;
    let mut tx = app.db.begin().await?;
    let reward: Reward = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{REWARD} WHERE id=$1 AND channel_id=$2 AND enabled AND kind='custom'"
    )))
    .bind(&reward)
    .bind(&channel)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Fail::missing)?;
    let text = match &reward.prompt {
        Some(_) => {
            let text = input.input.as_deref().map(str::trim).unwrap_or_default();
            if !(1..=200).contains(&text.chars().count()) {
                return Err(Fail::field("input", "Enter 1–200 characters."));
            }
            text::filter(text, "input")?;
            Some(text)
        }
        None => None,
    };
    redeem_in(&mut tx, &reward, &channel, &user.id, &input.id, text, false).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"balance": balance(&app, &channel, &user.id).await?}),
    ))
}

// ---- Creator Studio ----

#[derive(Deserialize)]
pub struct RewardInput {
    name: Option<String>,
    cost: i32,
    #[serde(default)]
    cooldown_seconds: i32,
    per_stream_limit: Option<i32>,
    prompt: Option<String>,
    enabled: bool,
}
/// Validated (name, prompt); the name is required for custom rewards only.
fn validate(input: &RewardInput) -> Res<(Option<String>, Option<String>)> {
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if let Some(name) = name {
        if name.chars().count() > 45 {
            return Err(Fail::field("name", "Names are 1–45 characters."));
        }
        text::filter(name, "name")?;
    }
    if !(1..=1_000_000).contains(&input.cost) {
        return Err(Fail::field("cost", "Cost is 1 to 1,000,000."));
    }
    if !(0..=604_800).contains(&input.cooldown_seconds) {
        return Err(Fail::field("cooldown_seconds", "Cooldown is up to 7 days."));
    }
    if input
        .per_stream_limit
        .is_some_and(|l| !(1..=1000).contains(&l))
    {
        return Err(Fail::field(
            "per_stream_limit",
            "The per-stream limit is 1 to 1,000.",
        ));
    }
    let prompt = input
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    if let Some(prompt) = prompt {
        if prompt.chars().count() > 100 {
            return Err(Fail::field("prompt", "Prompts are up to 100 characters."));
        }
        text::filter(prompt, "prompt")?;
    }
    Ok((name.map(str::to_owned), prompt.map(str::to_owned)))
}
async fn studio_list(app: &App, owner: &str) -> Res<Vec<Value>> {
    ensure_highlight(&mut *app.db.acquire().await?, owner, &app.config.engagement).await?;
    // REWARD_JSON is fixed SQL; the owner is bound.
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REWARD_JSON} FROM channel_rewards r WHERE r.channel_id=$1 ORDER BY r.kind='custom', r.created_at"
    )))
    .bind(owner)
    .fetch_all(&app.db)
    .await?)
}
/// GET /api/me/rewards
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    Ok(Json(
        json!({"rewards": studio_list(&app, &user.id).await?, "max_rewards": 50, "username": user.username}),
    ))
}
/// POST /api/me/rewards
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<RewardInput>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let (name, prompt) = validate(&input)?;
    let Some(name) = name else {
        return Err(Fail::field("name", "Give the reward a name."));
    };
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('rewards:'||$1, 6))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_rewards WHERE channel_id=$1")
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= 50 {
        return Err(Fail::conflict("You can have up to 50 rewards."));
    }
    sqlx::query("INSERT INTO channel_rewards(id,channel_id,name,cost,cooldown_seconds,per_stream_limit,prompt,enabled) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(profiles::new_id()).bind(&user.id).bind(name).bind(input.cost).bind(input.cooldown_seconds)
        .bind(input.per_stream_limit).bind(prompt).bind(input.enabled)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"rewards": studio_list(&app, &user.id).await?})))
}
/// PUT /api/me/rewards/{id}: the built-in highlight keeps its name and has no prompt.
async fn update(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<RewardInput>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (name, prompt) = validate(&input)?;
    let updated = sqlx::query("UPDATE channel_rewards SET name=CASE WHEN kind='custom' THEN coalesce($3,name) ELSE name END, cost=$4, cooldown_seconds=$5, per_stream_limit=$6,
            prompt=CASE WHEN kind='custom' THEN $7 END, enabled=$8 WHERE id=$1 AND channel_id=$2")
        .bind(&id).bind(&user.id).bind(name).bind(input.cost).bind(input.cooldown_seconds)
        .bind(input.per_stream_limit).bind(prompt).bind(input.enabled)
        .execute(&app.db).await?.rows_affected();
    if updated == 0 {
        return Err(Fail::missing());
    }
    Ok(Json(json!({"rewards": studio_list(&app, &user.id).await?})))
}
/// DELETE /api/me/rewards/{id}: custom rewards only (past redemptions keep their name and cost).
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let deleted =
        sqlx::query("DELETE FROM channel_rewards WHERE id=$1 AND channel_id=$2 AND kind='custom'")
            .bind(&id)
            .bind(&user.id)
            .execute(&app.db)
            .await?
            .rows_affected();
    if deleted == 0 {
        return Err(Fail::missing());
    }
    Ok(Json(json!({"rewards": studio_list(&app, &user.id).await?})))
}

/// GET /api/channels/{username}/redemptions: the owner's and moderators' queue, pending first.
async fn queue(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    moderation::actor(&app, &jar, &channel).await?;
    // Only chip_sql with a literal alias is interpolated; the channel is bound.
    let mut items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT jsonb_build_object('id',d.id,'name',d.name,'cost',d.cost,'input',d.input,'status',d.status,'created_at',d.created_at,'resolved_at',d.resolved_at,'user',{})
        FROM reward_redemptions d JOIN channel_users c ON c.id=d.user_id WHERE d.channel_id=$1 AND (d.status='pending' OR d.created_at>now()-interval '7 days')
        ORDER BY d.status<>'pending', d.created_at DESC LIMIT 200",
        profiles::chip_sql("c")
    )))
    .bind(&channel)
    .fetch_all(&app.db)
    .await?;
    for item in &mut items {
        profiles::hydrate(&app, &mut item["user"]);
    }
    Ok(Json(json!({"items": items})))
}

#[derive(Deserialize)]
pub struct Resolve {
    action: String,
}
/// POST /api/channels/{username}/redemptions/{id}: mark a pending redemption done, or refund it
/// (the points go back).
async fn resolve(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
    Json(input): Json<Resolve>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (actor, _) = moderation::actor(&app, &jar, &channel).await?;
    let refund = match input.action.as_str() {
        "done" => false,
        "refund" => true,
        _ => return Err(Fail::field("action", "Mark it done or refund it.")),
    };
    let mut tx = app.db.begin().await?;
    let row: Option<(String, i32, String)> = sqlx::query_as("SELECT user_id,cost,status FROM reward_redemptions WHERE id=$1 AND channel_id=$2 FOR UPDATE")
        .bind(&id).bind(&channel).fetch_optional(&mut *tx).await?;
    let Some((user, cost, status)) = row else {
        return Err(Fail::missing());
    };
    if status != "pending" {
        return Err(Fail::stale());
    }
    sqlx::query(
        "UPDATE reward_redemptions SET status=$2,resolved_at=now(),resolved_by=$3 WHERE id=$1",
    )
    .bind(&id)
    .bind(if refund { "refunded" } else { "done" })
    .bind(&actor.id)
    .execute(&mut *tx)
    .await?;
    if refund {
        sqlx::query("UPDATE engagement SET balance=balance+$3 WHERE channel_id=$1 AND user_id=$2")
            .bind(&channel)
            .bind(&user)
            .bind(i64::from(cost))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"status": if refund { "refunded" } else { "done" }}),
    ))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/rewards", get(list))
        .route("/api/channels/{username}/rewards/{id}/redeem", post(redeem))
        .route("/api/channels/{username}/redemptions", get(queue))
        .route("/api/channels/{username}/redemptions/{id}", post(resolve))
        .route("/api/me/rewards", get(mine).post(create))
        .route("/api/me/rewards/{id}", put(update).delete(remove))
}
