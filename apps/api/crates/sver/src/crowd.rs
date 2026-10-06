//! Module 7 CrowdSync, phase 2 (docs/CROWDSYNC.md "Polls and predictions", "Counter widgets").
//! One poll system: polls (one vote each, no weighting) and predictions (Engagement Valor stakes
//! only, never Purchased Valor or money; winners split the pool by stake; cancelling refunds).
//! Windows close a few seconds after their end so viewers whose video runs behind get the same
//! window. Counters (shiny, deaths, win/loss, custom) are updated by the owner and moderators from
//! the page or a chat command, and shown to viewers and in the OBS overlay.
use crate::{
    App, auth, boards,
    moderation::{self, Role},
    profiles::{self, Fail, Res},
    security as sec, text,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// Votes are accepted this long after a window ends (CDN viewers run 3–5 seconds behind).
pub const VOTE_GRACE: i64 = 8;
/// The most Engagement Valor one viewer can stake on one prediction (**Proposed** in the spec).
pub const MAX_STAKE: i32 = 10_000;
const MAX_COUNTERS: i64 = 10;

#[derive(sqlx::FromRow)]
struct Poll {
    id: String,
    kind: String,
    question: String,
    options: sqlx::types::Json<Vec<String>>,
    ends_at: DateTime<Utc>,
    status: String,
    winner: Option<i16>,
}
const POLL: &str = "SELECT id,kind,question,options,ends_at,status,winner FROM polls";

async fn channel(app: &App, name: &str) -> Res<String> {
    let mut conn = app.db.acquire().await?;
    Ok(profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id)
}

/// A poll's public state with live results (vote counts, and pools for predictions).
async fn poll_json(db: &mut PgConnection, id: &str) -> Res<Value> {
    // POLL is fixed SQL; the id is bound.
    let p: Poll = sqlx::query_as(sqlx::AssertSqlSafe(format!("{POLL} WHERE id=$1")))
        .bind(id)
        .fetch_one(&mut *db)
        .await?;
    let tallies: Vec<(i16, i64, i64)> = sqlx::query_as("SELECT option,count(*),coalesce(sum(stake),0)::bigint FROM poll_votes WHERE poll_id=$1 GROUP BY option")
        .bind(id)
        .fetch_all(&mut *db)
        .await?;
    let n = p.options.0.len();
    let (mut counts, mut pools) = (vec![0i64; n], vec![0i64; n]);
    for (option, count, pool) in tallies {
        if let Some(i) = usize::try_from(option).ok().filter(|i| *i < n) {
            counts[i] = count;
            pools[i] = pool;
        }
    }
    let prediction = p.kind == "prediction";
    Ok(json!({
        "id": p.id, "kind": p.kind, "question": p.question, "options": p.options.0,
        "ends_at": p.ends_at, "grace_seconds": VOTE_GRACE, "status": p.status, "winner": p.winner,
        "counts": counts, "pools": if prediction { json!(pools) } else { Value::Null },
    }))
}
async fn publish_poll(app: &App, channel: &str, id: &str) -> Res<()> {
    let poll = poll_json(&mut *app.db.acquire().await?, id).await?;
    app.chat
        .publish(channel, None, 0, json!({"type": "poll", "poll": poll}));
    Ok(())
}

pub(crate) async fn counters_json(db: &mut PgConnection, channel: &str) -> Res<Value> {
    Ok(sqlx::query_scalar("SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'kind',kind,'label',label,'value',value,'extra',extra,'odds',odds) ORDER BY created_at),'[]') FROM counters WHERE channel_id=$1")
        .bind(channel)
        .fetch_one(db)
        .await?)
}
pub(crate) async fn publish_counters(app: &App, channel: &str) -> Res<()> {
    let counters = counters_json(&mut *app.db.acquire().await?, channel).await?;
    app.chat.publish(
        channel,
        None,
        0,
        json!({"type": "counters", "counters": counters}),
    );
    Ok(())
}

/// GET /api/channels/{username}/crowd: the running (or just finished) poll and prediction, the
/// viewer's own vote and stake, their Engagement Valor, and the channel's counters.
async fn state(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let mut out =
        json!({"counters": counters_json(&mut db, &channel).await?, "max_stake": MAX_STAKE});
    for kind in ["poll", "prediction"] {
        let id: Option<String> = sqlx::query_scalar("SELECT id FROM polls WHERE channel_id=$1 AND kind=$2 AND (status IN ('open','locked') OR closed_at>now()-interval '2 minutes') ORDER BY created_at DESC LIMIT 1")
            .bind(&channel).bind(kind).fetch_optional(&mut *db).await?;
        let Some(id) = id else {
            out[kind] = Value::Null;
            continue;
        };
        let mut poll = poll_json(&mut db, &id).await?;
        if let Some(v) = &viewer {
            let mine: Option<(i16, i32, Option<i32>)> = sqlx::query_as(
                "SELECT option,stake,payout FROM poll_votes WHERE poll_id=$1 AND user_id=$2",
            )
            .bind(&id)
            .bind(&v.id)
            .fetch_optional(&mut *db)
            .await?;
            poll["mine"] = mine.map_or(Value::Null, |(option, stake, payout)| {
                json!({"option": option, "stake": stake, "payout": payout})
            });
        }
        out[kind] = poll;
    }
    drop(db);
    let (mut balance, mut role) = (None, None);
    if let Some(v) = &viewer {
        if v.id != channel {
            balance = Some(sqlx::query_scalar::<_, i64>("SELECT coalesce((SELECT balance FROM engagement WHERE channel_id=$1 AND user_id=$2),0)")
                .bind(&channel).bind(&v.id).fetch_one(&app.db).await?);
        }
        role = moderation::role_of(&app, &channel, v).await?;
    }
    out["balance"] = json!(balance);
    out["can_run"] = json!(role.is_some());
    out["can_resolve"] = json!(matches!(role, Some(Role::Owner | Role::Staff)));
    // Phase 3: the rally meter, the running Surge and the viewer's faction (for the rally button).
    let mut db = app.db.acquire().await?;
    out["rally"] = crate::surge::rally_meter(&mut db, &channel).await?;
    out["surge"] = crate::surge::current(&mut db, &channel).await?;
    out["faction"] = match &viewer {
        Some(v) => json!(
            sqlx::query_scalar::<_, String>("SELECT faction FROM faction_members WHERE user_id=$1")
                .bind(&v.id)
                .fetch_optional(&mut *db)
                .await?
        ),
        None => Value::Null,
    };
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct Start {
    kind: String,
    question: String,
    options: Vec<String>,
    seconds: i64,
}
/// POST /api/channels/{username}/polls: the owner or a moderator starts a poll (2–5 options) or a
/// prediction (2–10 outcomes) on the live stream.
async fn start(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Start>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (actor, role) = moderation::actor(&app, &jar, &channel).await?;
    let prediction = match input.kind.as_str() {
        "poll" => false,
        "prediction" => true,
        _ => return Err(Fail::field("kind", "Choose a poll or a prediction.")),
    };
    let question = input.question.trim();
    if !(1..=120).contains(&question.chars().count()) {
        return Err(Fail::field(
            "question",
            "Ask a question of 1–120 characters.",
        ));
    }
    text::filter(question, "question")?;
    let options: Vec<String> = input.options.iter().map(|o| o.trim().to_string()).collect();
    let most = if prediction { 10 } else { 5 };
    if options.len() < 2 || options.len() > most {
        return Err(Fail::bad(if prediction {
            "A prediction has 2 to 10 outcomes."
        } else {
            "A poll has 2 to 5 options."
        }));
    }
    for (i, option) in options.iter().enumerate() {
        if !(1..=40).contains(&option.chars().count()) {
            return Err(Fail::field("options", "Each option is 1–40 characters."));
        }
        if options[..i].iter().any(|o| o.eq_ignore_ascii_case(option)) {
            return Err(Fail::field("options", "Options must be different."));
        }
        text::filter(option, "options")?;
    }
    if !(15..=1800).contains(&input.seconds) {
        return Err(Fail::field(
            "seconds",
            "Choose a duration of 15 seconds to 30 minutes.",
        ));
    }
    let (broadcast, _) = boards::live(&app, &channel)
        .await?
        .ok_or_else(|| Fail::conflict("Polls and predictions run while the stream is live."))?;
    sec::reserve(&app, vec![format!("poll-start:{channel}")], 20, 3600).await?;
    let id = profiles::new_id();
    let mut tx = app.db.begin().await?;
    let inserted = sqlx::query("INSERT INTO polls(id,channel_id,kind,question,options,created_by,broadcast_id,ends_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()+make_interval(secs=>$8)) ON CONFLICT DO NOTHING")
        .bind(&id).bind(&channel).bind(&input.kind).bind(question).bind(sqlx::types::Json(&options))
        .bind(&actor.id).bind(&broadcast).bind(input.seconds as f64)
        .execute(&mut *tx).await?.rows_affected();
    if inserted == 0 {
        return Err(Fail::conflict(if prediction {
            "A prediction is already running."
        } else {
            "A poll is already running."
        }));
    }
    let action = if prediction {
        "prediction_start"
    } else {
        "poll_start"
    };
    moderation::log(
        &mut tx,
        &channel,
        &actor.id,
        role,
        action,
        None,
        None,
        json!({"poll": id}),
        question,
    )
    .await?;
    tx.commit().await?;
    publish_poll(&app, &channel, &id).await?;
    Ok(Json(json!({"id": id})))
}

#[derive(Deserialize)]
pub struct Vote {
    option: i16,
    #[serde(default)]
    stake: i32,
}
/// POST /api/channels/{username}/polls/{id}/vote: one vote per real viewer; a prediction's stake
/// comes out of the channel's Engagement Valor in the same transaction.
async fn vote(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
    Json(input): Json<Vote>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let channel = channel(&app, &name).await?;
    if channel == user.id {
        return Err(Fail::bad("You run this poll, so you can't vote in it."));
    }
    boards::real_viewer(&app, &channel, &user).await?;
    sec::reserve(&app, vec![format!("poll-vote:{}", user.id)], 10, 10).await?;
    let mut tx = app.db.begin().await?;
    // POLL is fixed SQL; values are bound.
    let poll: Poll = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{POLL} WHERE id=$1 AND channel_id=$2 FOR UPDATE"
    )))
    .bind(&id)
    .bind(&channel)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Fail::missing)?;
    if poll.status != "open" || Utc::now() > poll.ends_at + Duration::seconds(VOTE_GRACE) {
        return Err(Fail::conflict("Voting has closed."));
    }
    if usize::try_from(input.option).map_or(true, |o| o >= poll.options.0.len()) {
        return Err(Fail::field("option", "Choose one of the options."));
    }
    let stake = if poll.kind == "prediction" {
        if !(1..=MAX_STAKE).contains(&input.stake) {
            return Err(Fail::field("stake", "Stake 1 to 10,000 Engagement Valor."));
        }
        input.stake
    } else {
        0
    };
    let inserted = sqlx::query("INSERT INTO poll_votes(poll_id,user_id,option,stake) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(&id).bind(&user.id).bind(input.option).bind(stake)
        .execute(&mut *tx).await?.rows_affected();
    if inserted == 0 {
        return Err(Fail::conflict("You've already voted."));
    }
    if stake > 0 {
        let paid = sqlx::query("UPDATE engagement SET balance=balance-$3 WHERE channel_id=$1 AND user_id=$2 AND balance>=$3")
            .bind(&channel).bind(&user.id).bind(i64::from(stake))
            .execute(&mut *tx).await?.rows_affected();
        if paid == 0 {
            return Err(Fail::conflict("You don't have enough Engagement Valor."));
        }
    }
    tx.commit().await?;
    publish_poll(&app, &channel, &id).await?;
    Ok(Json(json!({"voted": true})))
}

#[derive(Deserialize)]
pub struct Close {
    action: String,
    winner: Option<i16>,
}
/// POST /api/channels/{username}/polls/{id}/close: "end" ends a poll or locks a prediction early;
/// "resolve" (the owner, or staff) pays a prediction's winners; "cancel" stops either one and
/// refunds every stake.
async fn close(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
    Json(input): Json<Close>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (actor, role) = moderation::actor(&app, &jar, &channel).await?;
    let mut tx = app.db.begin().await?;
    // POLL is fixed SQL; values are bound.
    let poll: Poll = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{POLL} WHERE id=$1 AND channel_id=$2 FOR UPDATE"
    )))
    .bind(&id)
    .bind(&channel)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Fail::missing)?;
    let running = poll.status == "open" || poll.status == "locked";
    let prediction = poll.kind == "prediction";
    match input.action.as_str() {
        "end" if poll.status == "open" => {
            sqlx::query("UPDATE polls SET status=CASE WHEN kind='poll' THEN 'ended' ELSE 'locked' END, ends_at=least(ends_at,now()), closed_at=CASE WHEN kind='poll' THEN now() END WHERE id=$1")
                .bind(&id).execute(&mut *tx).await?;
        }
        "resolve" if prediction && running => {
            if !matches!(role, Role::Owner | Role::Staff) {
                return Err(Fail::denied("Only the streamer resolves a prediction."));
            }
            let winner = input
                .winner
                .filter(|w| usize::try_from(*w).is_ok_and(|w| w < poll.options.0.len()))
                .ok_or_else(|| Fail::field("winner", "Choose the outcome that happened."))?;
            settle(&mut tx, &id, &channel, Some(winner)).await?;
            sqlx::query(
                "UPDATE polls SET status='resolved', winner=$2, closed_at=now() WHERE id=$1",
            )
            .bind(&id)
            .bind(winner)
            .execute(&mut *tx)
            .await?;
        }
        "cancel" if running => {
            settle(&mut tx, &id, &channel, None).await?;
            sqlx::query("UPDATE polls SET status='cancelled', closed_at=now() WHERE id=$1")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
        "end" | "resolve" | "cancel" => return Err(Fail::stale()),
        _ => return Err(Fail::field("action", "Unknown action.")),
    }
    let action = format!("{}_{}", poll.kind, input.action);
    moderation::log(
        &mut tx,
        &channel,
        &actor.id,
        role,
        &action,
        None,
        None,
        json!({"poll": id, "winner": input.winner}),
        &poll.question,
    )
    .await?;
    tx.commit().await?;
    publish_poll(&app, &channel, &id).await?;
    Ok(Json(json!({"ok": true})))
}

/// Pays out a prediction inside `tx`: with a winner, winners split the whole pool in proportion to
/// their stakes (rounding leftovers go one each to the largest, then earliest, stakes); with no
/// winner (cancelled), or when nobody picked the winner, every stake is refunded.
async fn settle(tx: &mut PgConnection, poll: &str, channel: &str, winner: Option<i16>) -> Res<()> {
    let votes: Vec<(String, i16, i32)> = sqlx::query_as("SELECT user_id,option,stake FROM poll_votes WHERE poll_id=$1 AND stake>0 ORDER BY stake DESC, created_at")
        .bind(poll)
        .fetch_all(&mut *tx)
        .await?;
    for (user, payout) in payouts(&votes, winner) {
        sqlx::query("UPDATE poll_votes SET payout=$3 WHERE poll_id=$1 AND user_id=$2")
            .bind(poll)
            .bind(&user)
            .bind(payout)
            .execute(&mut *tx)
            .await?;
        if payout > 0 {
            sqlx::query("INSERT INTO engagement(channel_id,user_id,balance) VALUES($1,$2,$3) ON CONFLICT(channel_id,user_id) DO UPDATE SET balance=engagement.balance+EXCLUDED.balance")
                .bind(channel).bind(&user).bind(i64::from(payout))
                .execute(&mut *tx).await?;
        }
    }
    Ok(())
}
/// (user, payout) for every stake; `votes` are ordered largest stake first, then earliest.
pub fn payouts(votes: &[(String, i16, i32)], winner: Option<i16>) -> Vec<(String, i32)> {
    let pool: i64 = votes.iter().map(|v| i64::from(v.2)).sum();
    let winning: i64 = votes
        .iter()
        .filter(|v| Some(v.1) == winner)
        .map(|v| i64::from(v.2))
        .sum();
    if winner.is_none() || winning == 0 {
        return votes.iter().map(|v| (v.0.clone(), v.2)).collect();
    }
    let mut out: Vec<(String, i64)> = votes
        .iter()
        .map(|v| {
            let share = if Some(v.1) == winner {
                pool * i64::from(v.2) / winning
            } else {
                0
            };
            (v.0.clone(), share)
        })
        .collect();
    let mut left = pool - out.iter().map(|o| o.1).sum::<i64>();
    for (o, v) in out.iter_mut().zip(votes) {
        if left == 0 {
            break;
        }
        if Some(v.1) == winner {
            o.1 += 1;
            left -= 1;
        }
    }
    out.into_iter()
        .map(|(u, p)| (u, i32::try_from(p).unwrap_or(i32::MAX)))
        .collect()
}

/// Ends polls and locks predictions whose window (plus grace) has passed, and cancels (refunds)
/// predictions nobody resolved within a day. Called from the 5-second media loop.
pub async fn tick(app: &App) -> Res<()> {
    let closed: Vec<(String, String)> = sqlx::query_as("UPDATE polls SET status=CASE WHEN kind='poll' THEN 'ended' ELSE 'locked' END, closed_at=CASE WHEN kind='poll' THEN now() END
        WHERE status='open' AND ends_at<now()-make_interval(secs=>$1) RETURNING id,channel_id")
        .bind(VOTE_GRACE as f64)
        .fetch_all(&app.db)
        .await?;
    let stale: Vec<(String, String)> = sqlx::query_as("SELECT id,channel_id FROM polls WHERE kind='prediction' AND status IN ('open','locked') AND ends_at<now()-interval '1 day'")
        .fetch_all(&app.db)
        .await?;
    for (id, channel) in &stale {
        let mut tx = app.db.begin().await?;
        let still = sqlx::query("UPDATE polls SET status='cancelled', closed_at=now() WHERE id=$1 AND status IN ('open','locked')")
            .bind(id).execute(&mut *tx).await?.rows_affected();
        if still == 1 {
            settle(&mut tx, id, channel, None).await?;
        }
        tx.commit().await?;
    }
    for (id, channel) in closed.iter().chain(&stale) {
        publish_poll(app, channel, id).await?;
    }
    Ok(())
}

// ---- Counters ----

#[derive(Deserialize)]
pub struct CounterInput {
    id: Option<String>,
    kind: Option<String>,
    label: String,
    odds: Option<i32>,
}
fn counter_fields(input: &CounterInput) -> Res<(String, Option<i32>)> {
    let label = input.label.trim();
    if !(1..=40).contains(&label.chars().count()) {
        return Err(Fail::field("label", "Labels are 1–40 characters."));
    }
    text::filter(label, "label")?;
    if input.odds.is_some_and(|o| !(2..=1_000_000).contains(&o)) {
        return Err(Fail::field("odds", "Odds are 1 in 2 to 1 in 1,000,000."));
    }
    Ok((label.to_string(), input.odds))
}
async fn mine(app: &App, user: &auth::User) -> Res<Json<Value>> {
    let counters = counters_json(&mut *app.db.acquire().await?, &user.id).await?;
    Ok(Json(
        json!({"counters": counters, "username": user.username, "max": MAX_COUNTERS}),
    ))
}
/// GET /api/me/counters
async fn list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    mine(&app, &user).await
}
/// POST /api/me/counters: the ID is what moderators type in chat (`!deaths`).
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<CounterInput>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let (label, odds) = counter_fields(&input)?;
    let id = input
        .id
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !(1..=16).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(Fail::field(
            "id",
            "The chat name is 1–16 letters or numbers.",
        ));
    }
    let kind = input.kind.as_deref().unwrap_or("custom");
    if !["shiny", "deaths", "tally", "custom"].contains(&kind) {
        return Err(Fail::field("kind", "Choose a counter type."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('counters:'||$1, 7))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM counters WHERE channel_id=$1")
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_COUNTERS {
        return Err(Fail::conflict("You can have up to 10 counters."));
    }
    let made = sqlx::query("INSERT INTO counters(channel_id,id,kind,label,odds) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(&user.id).bind(&id).bind(kind).bind(label).bind(odds)
        .execute(&mut *tx).await?.rows_affected();
    if made == 0 {
        return Err(Fail::field(
            "id",
            "You already have a counter with that name.",
        ));
    }
    tx.commit().await?;
    publish_counters(&app, &user.id).await?;
    mine(&app, &user).await
}
/// PUT /api/me/counters/{id}: rename, or change a shiny counter's odds.
async fn edit(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<CounterInput>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (label, odds) = counter_fields(&input)?;
    let updated = sqlx::query(
        "UPDATE counters SET label=$3, odds=$4, updated_at=now() WHERE channel_id=$1 AND id=$2",
    )
    .bind(&user.id)
    .bind(&id)
    .bind(label)
    .bind(odds)
    .execute(&app.db)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(Fail::missing());
    }
    publish_counters(&app, &user.id).await?;
    mine(&app, &user).await
}
/// DELETE /api/me/counters/{id}
async fn remove(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let deleted = sqlx::query("DELETE FROM counters WHERE channel_id=$1 AND id=$2")
        .bind(&user.id)
        .bind(&id)
        .execute(&app.db)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(Fail::missing());
    }
    publish_counters(&app, &user.id).await?;
    mine(&app, &user).await
}

/// Adds to (or sets) a counter's main value or its second number (phase, losses); never below 0.
async fn bump(
    db: &mut PgConnection,
    channel: &str,
    id: &str,
    extra: bool,
    set: bool,
    amount: i64,
) -> Res<bool> {
    Ok(sqlx::query("UPDATE counters SET
            value=CASE WHEN $3 THEN value ELSE least(greatest(CASE WHEN $4 THEN $5 ELSE value+$5 END,0),1000000000) END,
            extra=CASE WHEN $3 THEN least(greatest(CASE WHEN $4 THEN $5 ELSE extra+$5 END,0),1000000000) ELSE extra END,
            updated_at=now() WHERE channel_id=$1 AND id=$2")
        .bind(channel).bind(id).bind(extra).bind(set).bind(amount)
        .execute(db).await?.rows_affected() == 1)
}

#[derive(Deserialize)]
pub struct Update {
    #[serde(default)]
    extra: bool,
    #[serde(default)]
    set: bool,
    amount: i64,
}
/// POST /api/channels/{username}/counters/{id}: the owner and moderators update a counter.
async fn update(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, id)): Path<(String, String)>,
    Json(input): Json<Update>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    moderation::actor(&app, &jar, &channel).await?;
    if !(-1_000_000_000..=1_000_000_000).contains(&input.amount) {
        return Err(Fail::field("amount", "That number is too large."));
    }
    if !bump(
        &mut *app.db.acquire().await?,
        &channel,
        &id,
        input.extra,
        input.set,
        input.amount,
    )
    .await?
    {
        return Err(Fail::missing());
    }
    publish_counters(&app, &channel).await?;
    Ok(Json(json!({"ok": true})))
}

/// A chat command from the owner or a moderator, inside the message's transaction:
/// `!deaths` or `!deaths +` adds one, `!deaths -` takes one away, `!deaths +5`, `!deaths -2` and
/// `!deaths =10` change it by or set it to a number, `!record win` / `!record loss` count a tally,
/// and `!shiny phase` starts a new phase. Returns whether a counter changed.
pub(crate) async fn chat_command(
    app: &App,
    tx: &mut PgConnection,
    channel: &str,
    user: &auth::User,
    body: &str,
) -> Res<bool> {
    let Some((id, Some((extra, set, amount)))) = parse_command(body) else {
        return Ok(false);
    };
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM counters WHERE channel_id=$1 AND id=$2)")
            .bind(channel)
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
    if !exists || moderation::role_of(app, channel, user).await?.is_none() {
        return Ok(false);
    }
    bump(tx, channel, &id, extra, set, amount).await
}
/// A counter change: (second number instead of the main one, set instead of add, amount).
pub type Change = (bool, bool, i64);
/// (counter id, the change) for a `!id ...` message (None when the words aren't a counter change);
/// None when it isn't a command at all.
pub fn parse_command(body: &str) -> Option<(String, Option<Change>)> {
    let rest = body.trim().strip_prefix('!')?;
    let mut words = rest.split_whitespace();
    let id = words.next()?.to_ascii_lowercase();
    if rest.starts_with(char::is_whitespace)
        || !(1..=16).contains(&id.len())
        || !id.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    let arg = words.next();
    if words.next().is_some() {
        return Some((id, None));
    }
    let op = match arg.map(str::to_ascii_lowercase).as_deref() {
        None | Some("+") | Some("win") => Some((false, false, 1)),
        Some("-") => Some((false, false, -1)),
        Some("loss") | Some("phase") => Some((true, false, 1)),
        Some(a) => {
            let (set, digits) = match a.strip_prefix('=') {
                Some(d) => (true, d),
                None => (false, a),
            };
            digits
                .parse::<i64>()
                .ok()
                .filter(|n| n.abs() <= 1_000_000_000 && (set || digits.starts_with(['+', '-'])))
                .map(|n| (false, set, n))
        }
    };
    Some((id, op))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/crowd", get(state))
        .route("/api/channels/{username}/polls", post(start))
        .route("/api/channels/{username}/polls/{id}/vote", post(vote))
        .route("/api/channels/{username}/polls/{id}/close", post(close))
        .route("/api/channels/{username}/counters/{id}", post(update))
        .route("/api/me/counters", get(list).post(create))
        .route("/api/me/counters/{id}", put(edit).delete(remove))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(user: &str, option: i16, stake: i32) -> (String, i16, i32) {
        (user.into(), option, stake)
    }

    #[test]
    fn winners_split_the_whole_pool_by_stake() {
        // Pool 1000; winners staked 300 and 100 on option 1.
        let votes = [v("a", 0, 600), v("b", 1, 300), v("c", 1, 100)];
        let paid = payouts(&votes, Some(1));
        assert_eq!(
            paid,
            [("a".into(), 0), ("b".into(), 750), ("c".into(), 250)]
        );
        // Rounding leftovers go to the largest stakes; nothing is lost or created.
        let votes = [v("a", 1, 2), v("b", 1, 1), v("c", 0, 1)];
        let paid = payouts(&votes, Some(1));
        assert_eq!(paid.iter().map(|p| p.1).sum::<i32>(), 4);
        assert_eq!(paid, [("a".into(), 3), ("b".into(), 1), ("c".into(), 0)]);
        // Nobody picked the winner, or cancelled: everyone is refunded.
        let refunded = [("a".into(), 2), ("b".into(), 1), ("c".into(), 1)];
        assert_eq!(payouts(&votes, Some(2)), refunded);
        assert_eq!(payouts(&votes, None), refunded);
    }

    #[test]
    fn chat_commands() {
        let p = parse_command;
        assert_eq!(
            p("!deaths"),
            Some(("deaths".into(), Some((false, false, 1))))
        );
        assert_eq!(
            p("!Deaths -"),
            Some(("deaths".into(), Some((false, false, -1))))
        );
        assert_eq!(
            p("!deaths +5"),
            Some(("deaths".into(), Some((false, false, 5))))
        );
        assert_eq!(
            p("!deaths =10"),
            Some(("deaths".into(), Some((false, true, 10))))
        );
        assert_eq!(
            p("!record loss"),
            Some(("record".into(), Some((true, false, 1))))
        );
        assert_eq!(
            p("!shiny phase"),
            Some(("shiny".into(), Some((true, false, 1))))
        );
        assert_eq!(
            p("!deaths 5"),
            Some(("deaths".into(), None)),
            "a bare number is ambiguous"
        );
        assert_eq!(p("!deaths is high"), Some(("deaths".into(), None)));
        assert_eq!(p("deaths"), None);
        assert_eq!(p("! deaths"), None);
    }
}
