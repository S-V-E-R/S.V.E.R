//! MAGNet Hype (docs/MAGNET.md "MAGNet Hype"): a Global lane and one lane per genre, each moving
//! viewers between live streams. Switches alternate between a moment (a stream having a moment
//! relative to its own normal) and a fair turn (the stream that has waited longest). Viewer count,
//! follower totals and money are never inputs: `Candidate` has no field for them.
use crate::{
    App,
    discovery::{self, Stream},
    profiles::{self, Fail, Res},
    safety,
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

/// Timing and scoring. Safe example values live here; production reads `MAGNET_TUNING_FILE`.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct Tuning {
    pub tick_seconds: i64,
    pub hold_seconds: i64,
    pub max_seconds: i64,
    pub moment_gap_seconds: i64,
    pub cooldown_seconds: i64,
    pub countdown_seconds: i64,
    pub min_live_seconds: i64,
    /// A moment is a signal at least this many times the stream's own normal rate.
    pub moment_ratio: f64,
    /// ... and it must beat the current stream's score by this factor.
    pub beat_factor: f64,
    /// Smallest burst that counts, so one or two people can't create a moment.
    pub min_chatters: f64,
    pub min_follows: f64,
    pub raid_score: f64,
    /// The streamer's flag adds this, only alongside another signal at least `flag_elevated`.
    pub flag_bonus: f64,
    pub flag_elevated: f64,
    pub flag_cooldown_seconds: i64,
}
impl Default for Tuning {
    fn default() -> Self {
        Self {
            tick_seconds: 10,
            hold_seconds: 45,
            max_seconds: 480,
            moment_gap_seconds: 120,
            cooldown_seconds: 1800,
            countdown_seconds: 5,
            min_live_seconds: 60,
            moment_ratio: 3.0,
            beat_factor: 1.5,
            min_chatters: 3.0,
            min_follows: 2.0,
            raid_score: 10.0,
            flag_bonus: 1.0,
            flag_elevated: 1.5,
            flag_cooldown_seconds: 600,
        }
    }
}
impl Tuning {
    pub fn from_env() -> std::result::Result<Self, String> {
        match std::env::var("MAGNET_TUNING_FILE") {
            Ok(path) if !path.is_empty() => {
                let text = std::fs::read_to_string(path)
                    .map_err(|_| "MAGNET_TUNING_FILE is not readable")?;
                serde_json::from_str(&text).map_err(|_| "MAGNET_TUNING_FILE is not valid".into())
            }
            _ => Ok(Self::default()),
        }
    }
}

/// Room signals for one stream, each against its own recent baseline.
#[derive(Clone, Debug, Default)]
pub struct Signals {
    pub chatters: f64,
    pub chat_base: f64,
    pub follows: f64,
    pub follow_base: f64,
    pub raided_by: Option<String>,
    pub flagged: bool,
}
impl Signals {
    /// The moment score and its one-line reason (None when nothing is elevated).
    pub fn score(&self, t: &Tuning) -> (f64, Option<String>) {
        let chat = if self.chatters >= t.min_chatters {
            self.chatters / self.chat_base.max(1.0)
        } else {
            0.0
        };
        let follow = if self.follows >= t.min_follows {
            self.follows / self.follow_base.max(1.0)
        } else {
            0.0
        };
        let mut best = (0.0, None);
        if chat > best.0 {
            best = (chat, Some("Chat is going off".to_string()));
        }
        if follow > best.0 {
            best = (follow, Some("New follows are pouring in".to_string()));
        }
        if let Some(raider) = &self.raided_by
            && t.raid_score > best.0
        {
            best = (t.raid_score, Some(format!("Just raided by {raider}")));
        }
        // A flag only ever adds to another elevated signal.
        if self.flagged && best.0 >= t.flag_elevated {
            best.0 += t.flag_bonus;
        }
        best
    }
}

/// One eligible stream in a lane, or a merged co-stream as one unit.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// The stream shown when the unit is featured (a merged co-stream's host when eligible).
    pub broadcast: String,
    /// Every broadcast in the unit, `broadcast` included.
    pub members: Vec<String>,
    pub score: f64,
    pub moment: Option<String>,
    /// When it was last featured on this lane (still running counts as now). For a merged
    /// co-stream, the longest wait among its members.
    pub last_featured: Option<DateTime<Utc>>,
}
impl Candidate {
    fn has(&self, broadcast: &str) -> bool {
        self.members.iter().any(|m| m == broadcast)
    }
}

#[derive(Clone, Debug, Default)]
pub struct LaneState {
    pub current: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub last_kind: Option<String>,
    pub last_moment_at: Option<DateTime<Utc>>,
    pub forced: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Hold,
    /// Nothing eligible is live: the lane shows "nothing live".
    Clear,
    Switch {
        broadcast: String,
        kind: &'static str,
        reason: String,
    },
}

/// The stream that has waited longest on this lane; never-featured streams first. Streams in
/// their cooldown are skipped unless nothing else can be shown.
fn fair_pick<'a>(
    cands: &'a [Candidate],
    current: Option<&str>,
    now: DateTime<Utc>,
    t: &Tuning,
) -> Option<&'a Candidate> {
    let others: Vec<&Candidate> = cands
        .iter()
        .filter(|c| current.is_none_or(|id| !c.has(id)))
        .collect();
    let cooled = |c: &&Candidate| {
        c.last_featured
            .is_none_or(|at| now - at >= Duration::seconds(t.cooldown_seconds))
    };
    let pool: Vec<&Candidate> = if others.iter().any(cooled) {
        others.into_iter().filter(cooled).collect()
    } else {
        others
    };
    pool.into_iter()
        .min_by(|a, b| (a.last_featured, &a.broadcast).cmp(&(b.last_featured, &b.broadcast)))
}
fn fair_reason(c: &Candidate, now: DateTime<Utc>) -> String {
    match c.last_featured {
        None => "Fair turn: first time on MAGNet".into(),
        Some(at) if at.date_naive() < now.date_naive() => {
            "Fair turn: hasn't been featured today".into()
        }
        Some(_) => "Fair turn: waited longest".into(),
    }
}

/// One engine tick for one lane. Pure: the same inputs always give the same decision.
pub fn decide(lane: &LaneState, cands: &[Candidate], now: DateTime<Utc>, t: &Tuning) -> Decision {
    if cands.is_empty() {
        return if lane.current.is_some() {
            Decision::Clear
        } else {
            Decision::Hold
        };
    }
    let current = lane
        .current
        .as_deref()
        .and_then(|id| cands.iter().find(|c| c.has(id)));
    // Staff force: show it as soon as it's eligible, and hold it until released.
    if let Some(forced) = lane.forced.as_deref()
        && let Some(unit) = cands.iter().find(|c| c.has(forced))
    {
        return if lane.current.as_deref().is_some_and(|id| unit.has(id)) {
            Decision::Hold
        } else {
            Decision::Switch {
                broadcast: forced.into(),
                kind: "forced",
                reason: "Staff pick".into(),
            }
        };
    }
    let Some(current) = current else {
        // Nothing featured yet, or the featured stream ended or became ineligible.
        let Some(pick) = fair_pick(cands, None, now, t) else {
            return Decision::Hold;
        };
        let (kind, reason) = if cands.len() == 1 {
            ("only", "The only stream live in this lane".to_string())
        } else if lane.current.is_some() {
            ("fallback", "The previous stream ended".to_string())
        } else {
            ("fair", fair_reason(pick, now))
        };
        return Decision::Switch {
            broadcast: pick.broadcast.clone(),
            kind,
            reason,
        };
    };
    let held = lane.since.map_or(Duration::zero(), |s| now - s);
    if cands.len() == 1 || held < Duration::seconds(t.hold_seconds) {
        return Decision::Hold;
    }
    // Moments and fair turns alternate.
    let moment_due = lane.last_kind.as_deref() != Some("moment")
        && lane
            .last_moment_at
            .is_none_or(|at| now - at >= Duration::seconds(t.moment_gap_seconds));
    if moment_due {
        let cooled = |c: &&Candidate| {
            c.last_featured
                .is_none_or(|at| now - at >= Duration::seconds(t.cooldown_seconds))
        };
        let best = cands
            .iter()
            .filter(|c| !c.has(&current.broadcast) && c.moment.is_some())
            .filter(cooled)
            .filter(|c| c.score >= t.moment_ratio && c.score >= current.score * t.beat_factor)
            .max_by(|a, b| {
                a.score
                    .total_cmp(&b.score)
                    .then(b.broadcast.cmp(&a.broadcast))
            });
        if let Some(best) = best {
            return Decision::Switch {
                broadcast: best.broadcast.clone(),
                kind: "moment",
                reason: best.moment.clone().unwrap_or_default(),
            };
        }
    }
    if held >= Duration::seconds(t.max_seconds)
        && let Some(pick) = fair_pick(cands, Some(&current.broadcast), now, t)
    {
        return Decision::Switch {
            broadcast: pick.broadcast.clone(),
            kind: "fair",
            reason: fair_reason(pick, now),
        };
    }
    Decision::Hold
}

type SignalRow = (
    String,
    bool,
    f64,
    f64,
    f64,
    f64,
    Option<String>,
    bool,
    Option<DateTime<Utc>>,
    bool,
);
/// Eligible streams for a lane, with their signals. Eligible: live 60+ seconds and not
/// reconnecting, an active category (the lane's genre for genre lanes), not opted out, an eligible
/// channel and no open integrity case.
async fn candidates(
    db: &mut PgConnection,
    lane: &str,
    t: &Tuning,
) -> Res<Vec<(Stream, Candidate, Signals)>> {
    let streams = discovery::live_streams(&mut *db).await?;
    let rows: Vec<SignalRow> = sqlx::query_as(
        "SELECT b.id,
          b.state='LIVE' AND b.started_at<=now()-make_interval(secs=>$2)
            AND NOT coalesce((SELECT opt_out FROM magnet_settings WHERE user_id=b.owner_id),false)
            AND NOT EXISTS(SELECT 1 FROM integrity_cases ic WHERE ic.owner_id=b.owner_id AND ic.status='OPEN')
            AND ($1='global' OR EXISTS(SELECT 1 FROM stream_settings s JOIN stream_categories c ON c.id=s.category_id WHERE s.owner_id=b.owner_id AND c.active AND c.genre=$1))
            AND NOT EXISTS(SELECT 1 FROM stream_settings s JOIN stream_categories c ON c.id=s.category_id WHERE s.owner_id=b.owner_id AND NOT c.active),
          (SELECT coalesce(sum(CASE WHEN u.created_at>now()-interval '7 days' THEN 0.5 ELSE 1 END),0) FROM (SELECT DISTINCT m.author_id FROM chat_messages m WHERE m.channel_id=b.owner_id AND m.author_id<>b.owner_id AND m.deleted_at IS NULL AND m.origin IS NULL AND m.squad_id IS NULL AND m.created_at>now()-interval '60 seconds') a JOIN users u ON u.id=a.author_id AND u.email_verified)::float8,
          (SELECT count(DISTINCT (m.author_id, date_trunc('minute', m.created_at))) FROM chat_messages m JOIN users u ON u.id=m.author_id AND u.email_verified WHERE m.channel_id=b.owner_id AND m.author_id<>b.owner_id AND m.origin IS NULL AND m.squad_id IS NULL AND m.created_at<=now()-interval '60 seconds' AND m.created_at>greatest(b.started_at,now()-interval '31 minutes'))::float8
            / greatest(1.0, extract(epoch FROM (now()-interval '60 seconds')-greatest(b.started_at,now()-interval '31 minutes'))::float8/60.0),
          (SELECT count(*) FROM follows f JOIN users u ON u.id=f.follower_id AND u.email_verified WHERE f.following_id=b.owner_id AND f.created_at>now()-interval '60 seconds')::float8,
          (SELECT count(*) FROM follows f JOIN users u ON u.id=f.follower_id AND u.email_verified WHERE f.following_id=b.owner_id AND f.created_at<=now()-interval '60 seconds' AND f.created_at>greatest(b.started_at,now()-interval '31 minutes'))::float8
            / greatest(1.0, extract(epoch FROM (now()-interval '60 seconds')-greatest(b.started_at,now()-interval '31 minutes'))::float8/60.0),
          (SELECT c.display_name FROM raids r JOIN channel_users c ON c.id=r.raider_id WHERE r.target_broadcast_id=b.id AND r.status='moved' AND r.execute_at>now()-interval '2 minutes' ORDER BY r.execute_at DESC LIMIT 1),
          EXISTS(SELECT 1 FROM magnet_flags g WHERE g.broadcast_id=b.id AND g.at>now()-interval '2 minutes'),
          (SELECT max(coalesce(f.ended_at,now())) FROM magnet_features f WHERE f.lane=$1 AND f.owner_id=b.owner_id),
          EXISTS(SELECT 1 FROM plays_runtime pr WHERE pr.enabled AND pr.channel_id=b.owner_id)
         FROM broadcasts b WHERE b.id=ANY($3)",
    )
    .bind(lane)
    .bind(t.min_live_seconds as f64)
    .bind(streams.iter().map(|s| s.broadcast_id.clone()).collect::<Vec<_>>())
    .fetch_all(&mut *db)
    .await?;
    // S.V.E.R Plays and other system channels are featured only when nothing else is live.
    let others = rows.iter().any(|r| r.1 && !r.9);
    let mut out = Vec::new();
    for (
        id,
        eligible,
        chatters,
        chat_base,
        follows,
        follow_base,
        raided_by,
        flagged,
        last,
        system,
    ) in rows
    {
        if !eligible || (system && others) {
            continue;
        }
        let Some(stream) = streams.iter().find(|s| s.broadcast_id == id) else {
            continue;
        };
        let signals = Signals {
            chatters,
            chat_base,
            follows,
            follow_base,
            raided_by,
            flagged,
        };
        let (score, moment) = signals.score(t);
        out.push((
            stream.clone(),
            Candidate {
                members: vec![id.clone()],
                broadcast: id,
                score,
                moment,
                last_featured: last,
            },
            signals,
        ));
    }
    // A merged co-stream is one candidate (docs/MAGNET.md "Co-streams on MAGNet"): its eligible
    // members only, moments from the shared chat and all members' follows, the longest wait.
    for squad in crate::squads::active(&mut *db).await? {
        if squad.mode != "MERGED" {
            continue;
        }
        let (unit, rest): (Vec<_>, Vec<_>) = out
            .into_iter()
            .partition(|o| squad.members.iter().any(|m| m.1 == o.1.broadcast));
        out = rest;
        // Members are host first, so the first eligible one is shown.
        let mut unit = unit;
        unit.sort_by_key(|o| squad.members.iter().position(|m| m.1 == o.1.broadcast));
        if unit.len() < 2 {
            out.extend(unit);
            continue;
        }
        let ids: Vec<String> = squad.members.iter().map(|m| m.0.clone()).collect();
        let (chatters, chat_base): (f64, f64) = sqlx::query_as(
            "SELECT
              (SELECT coalesce(sum(CASE WHEN u.created_at>now()-interval '7 days' THEN 0.5 ELSE 1 END),0) FROM (SELECT DISTINCT m.author_id FROM chat_messages m WHERE m.squad_id=s.id AND m.author_id<>ALL($2) AND m.deleted_at IS NULL AND m.origin IS NULL AND m.created_at>now()-interval '60 seconds') a JOIN users u ON u.id=a.author_id AND u.email_verified)::float8,
              (SELECT count(DISTINCT (m.author_id, date_trunc('minute', m.created_at))) FROM chat_messages m JOIN users u ON u.id=m.author_id AND u.email_verified WHERE m.squad_id=s.id AND m.author_id<>ALL($2) AND m.origin IS NULL AND m.created_at<=now()-interval '60 seconds' AND m.created_at>greatest(s.created_at,now()-interval '31 minutes'))::float8
                / greatest(1.0, extract(epoch FROM (now()-interval '60 seconds')-greatest(s.created_at,now()-interval '31 minutes'))::float8/60.0)
             FROM squads s WHERE s.id=$1",
        )
        .bind(&squad.id)
        .bind(&ids)
        .fetch_one(&mut *db)
        .await?;
        let signals = Signals {
            chatters,
            chat_base,
            follows: unit.iter().map(|o| o.2.follows).sum(),
            follow_base: unit.iter().map(|o| o.2.follow_base).sum(),
            raided_by: unit.iter().find_map(|o| o.2.raided_by.clone()),
            flagged: unit.iter().any(|o| o.2.flagged),
        };
        let (score, moment) = signals.score(t);
        let last_featured = if unit.iter().any(|o| o.1.last_featured.is_none()) {
            None
        } else {
            unit.iter().filter_map(|o| o.1.last_featured).min()
        };
        let candidate = Candidate {
            broadcast: unit[0].1.broadcast.clone(),
            members: unit.iter().map(|o| o.1.broadcast.clone()).collect(),
            score,
            moment,
            last_featured,
        };
        out.push((unit[0].0.clone(), candidate, signals));
    }
    Ok(out)
}

/// Runs with the 5-second media pass; each lane ticks every `tick_seconds`.
pub async fn tick(app: &App) -> Res<()> {
    let t = &app.config.magnet;
    sqlx::query("INSERT INTO magnet_lanes(id) SELECT 'global' UNION ALL SELECT id FROM faction_genres ON CONFLICT DO NOTHING")
        .execute(&app.db)
        .await?;
    let lanes: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM magnet_lanes WHERE coalesce(ticked_at<now()-make_interval(secs=>$1),true)",
    )
    .bind(t.tick_seconds as f64)
    .fetch_all(&app.db)
    .await?;
    for lane in lanes {
        // If the engine fails, the lane holds its current stream.
        if tick_lane(app, &lane).await.is_err() {
            eprintln!("magnet_event=tick outcome=hold");
        }
    }
    sqlx::query("DELETE FROM magnet_decisions WHERE at<now()-interval '7 days'")
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM magnet_flags WHERE at<now()-interval '1 day'")
        .execute(&app.db)
        .await?;
    Ok(())
}

type LaneRow = (
    bool,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
);
/// One lane's tick. Public so tests can drive the engine without waiting for the timer.
pub async fn tick_lane(app: &App, lane: &str) -> Res<()> {
    let t = &app.config.magnet;
    let mut tx = app.db.begin().await?;
    let row: Option<LaneRow> = sqlx::query_as("SELECT enabled,current_broadcast,current_since,last_kind,last_moment_at,forced_broadcast,pending_broadcast,pending_kind,pending_reason,switch_at FROM magnet_lanes WHERE id=$1 FOR UPDATE SKIP LOCKED")
        .bind(lane)
        .fetch_optional(&mut *tx)
        .await?;
    let Some((
        enabled,
        current,
        since,
        last_kind,
        last_moment_at,
        forced,
        pending,
        pending_kind,
        pending_reason,
        switch_at,
    )) = row
    else {
        return Ok(());
    };
    sqlx::query("UPDATE magnet_lanes SET ticked_at=now() WHERE id=$1")
        .bind(lane)
        .execute(&mut *tx)
        .await?;
    if !enabled {
        if current.is_some() || pending.is_some() {
            end_feature(&mut tx, lane).await?;
            sqlx::query("UPDATE magnet_lanes SET current_broadcast=NULL,current_kind=NULL,current_reason=NULL,current_since=NULL,pending_broadcast=NULL,pending_kind=NULL,pending_reason=NULL,switch_at=NULL WHERE id=$1")
                .bind(lane).execute(&mut *tx).await?;
            tx.commit().await?;
            publish(app, lane);
        } else {
            tx.commit().await?;
        }
        return Ok(());
    }
    let found = candidates(&mut tx, lane, t).await?;
    let cands: Vec<Candidate> = found.iter().map(|f| f.1.clone()).collect();
    let now = Utc::now();
    // A countdown in progress: switch when it ends, if the next stream is still eligible.
    if let (Some(next), Some(at)) = (&pending, switch_at) {
        if at <= now {
            if let Some(unit) = cands.iter().find(|c| c.has(next)) {
                commit(
                    &mut tx,
                    lane,
                    next,
                    &unit.members,
                    pending_kind.as_deref().unwrap_or("fair"),
                    pending_reason.as_deref().unwrap_or(""),
                )
                .await?;
            } else {
                sqlx::query("UPDATE magnet_lanes SET pending_broadcast=NULL,pending_kind=NULL,pending_reason=NULL,switch_at=NULL WHERE id=$1")
                    .bind(lane).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            publish(app, lane);
        } else {
            tx.commit().await?;
        }
        return Ok(());
    }
    let state = LaneState {
        current: current.clone(),
        since,
        last_kind,
        last_moment_at,
        forced,
    };
    let decision = decide(&state, &cands, now, t);
    let log = |kind: &str, chosen: Option<&str>, reason: &str| {
        json!({"kind":kind,"chosen":chosen,"reason":reason,"candidates":found.iter().map(|(s,c,g)| json!({
            "broadcast":c.broadcast,"members":c.members,"username":s.username,"score":c.score,"moment":c.moment,"last_featured":c.last_featured,
            "signals":{"chatters":g.chatters,"chat_base":g.chat_base,"follows":g.follows,"follow_base":g.follow_base,"raided_by":g.raided_by,"flagged":g.flagged}})).collect::<Vec<_>>()})
    };
    match &decision {
        Decision::Hold => {
            tx.commit().await?;
            return Ok(());
        }
        Decision::Clear => {
            end_feature(&mut tx, lane).await?;
            sqlx::query("UPDATE magnet_lanes SET current_broadcast=NULL,current_kind=NULL,current_reason=NULL,current_since=NULL WHERE id=$1")
                .bind(lane).execute(&mut *tx).await?;
            record(
                &mut tx,
                lane,
                &log("clear", None, "Nothing eligible is live"),
            )
            .await?;
        }
        Decision::Switch {
            broadcast,
            kind,
            reason,
        } => {
            let showing = current
                .as_deref()
                .is_some_and(|c| cands.iter().any(|x| x.has(c)));
            if showing {
                // Viewers get a countdown with a still of the next stream and a Stay button.
                sqlx::query("UPDATE magnet_lanes SET pending_broadcast=$2,pending_kind=$3,pending_reason=$4,switch_at=now()+make_interval(secs=>$5) WHERE id=$1")
                    .bind(lane).bind(broadcast).bind(kind).bind(reason).bind(t.countdown_seconds as f64).execute(&mut *tx).await?;
            } else {
                let members = cands
                    .iter()
                    .find(|c| c.has(broadcast))
                    .map_or_else(|| vec![broadcast.clone()], |c| c.members.clone());
                commit(&mut tx, lane, broadcast, &members, kind, reason).await?;
            }
            record(&mut tx, lane, &log(kind, Some(broadcast), reason)).await?;
        }
    }
    tx.commit().await?;
    publish(app, lane);
    Ok(())
}
async fn record(db: &mut PgConnection, lane: &str, entry: &Value) -> Res<()> {
    sqlx::query(
        "INSERT INTO magnet_decisions(lane,kind,chosen,reason,candidates) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(lane)
    .bind(entry["kind"].as_str().unwrap_or(""))
    .bind(entry["chosen"].as_str())
    .bind(entry["reason"].as_str().unwrap_or(""))
    .bind(&entry["candidates"])
    .execute(db)
    .await?;
    Ok(())
}
/// A committed handoff explains arrivals for two minutes, including every featured co-stream
/// member. Pending switches and client-supplied lane labels are not evidence of a handoff.
pub async fn explains_burst(db: &mut PgConnection, broadcast: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM magnet_features
        WHERE owner_id=(SELECT owner_id FROM broadcasts WHERE id=$1) AND broadcast_id=$1
        AND started_at>now()-interval '2 minutes' AND started_at<=now())",
    )
    .bind(broadcast)
    .fetch_one(db)
    .await?)
}

async fn end_feature(db: &mut PgConnection, lane: &str) -> Res<()> {
    sqlx::query("UPDATE magnet_features SET ended_at=now() WHERE lane=$1 AND ended_at IS NULL")
        .bind(lane)
        .execute(db)
        .await?;
    Ok(())
}
async fn commit(
    db: &mut PgConnection,
    lane: &str,
    broadcast: &str,
    members: &[String],
    kind: &str,
    reason: &str,
) -> Res<()> {
    end_feature(&mut *db, lane).await?;
    // One row per member of a merged co-stream: all share the cooldown and see it in history,
    // and each gets its own spotlight clip suggestion.
    for member in members {
        let feature = profiles::new_id();
        let owner:Option<String>=sqlx::query_scalar("INSERT INTO magnet_features(id,lane,broadcast_id,owner_id,kind,reason) SELECT $1,$2,id,owner_id,$3,$4 FROM broadcasts WHERE id=$5 RETURNING owner_id")
            .bind(&feature).bind(lane).bind(kind).bind(reason).bind(member).fetch_optional(&mut *db).await?;
        if let Some(owner) = owner {
            crate::videos::spotlight(db, &owner, &feature).await?;
        }
    }
    sqlx::query("UPDATE magnet_lanes SET current_broadcast=$2,current_kind=$3,current_reason=$4,current_since=now(),pending_broadcast=NULL,pending_kind=NULL,pending_reason=NULL,switch_at=NULL,
        last_kind=CASE WHEN $3 IN ('moment','fair') THEN $3 ELSE last_kind END,
        last_moment_at=CASE WHEN $3='moment' THEN now() ELSE last_moment_at END WHERE id=$1")
        .bind(lane).bind(broadcast).bind(kind).bind(reason).execute(&mut *db).await?;
    Ok(())
}
/// Tells open Hype pages to refresh now (they also poll).
fn publish(app: &App, lane: &str) {
    app.chat
        .publish(&format!("magnet:{lane}"), None, 0, json!({"type":"magnet"}));
}

/// The lane's display name.
async fn lane_name(db: &mut PgConnection, lane: &str) -> Res<Option<String>> {
    if lane == "global" {
        return Ok(Some("Global".into()));
    }
    Ok(sqlx::query_scalar(
        "SELECT g.name FROM faction_genres g JOIN magnet_lanes l ON l.id=g.id WHERE g.id=$1",
    )
    .bind(lane)
    .fetch_optional(db)
    .await?)
}

/// GET /api/magnet: every lane and what it's showing.
async fn lanes(State(app): State<App>) -> Res<Json<Value>> {
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',l.id,'name',CASE WHEN l.id='global' THEN 'Global' ELSE g.name END,'enabled',l.enabled,'featuring',c.display_name) FROM magnet_lanes l LEFT JOIN faction_genres g ON g.id=l.id LEFT JOIN broadcasts b ON b.id=l.current_broadcast LEFT JOIN channel_users c ON c.id=b.owner_id ORDER BY l.id<>'global', g.position NULLS FIRST, l.id")
        .fetch_all(&app.db)
        .await?;
    Ok(Json(json!({"lanes":items})))
}

type LaneView = (
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
);
/// GET /api/magnet/{lane}: the featured stream, any countdown, and for a viewer who can't watch
/// the featured channel (banned, timed out or blocked), a holding card instead.
async fn lane(
    State(app): State<App>,
    jar: CookieJar,
    Path(lane): Path<String>,
) -> Res<Json<Value>> {
    let now = Utc::now();
    let mut db = app.db.acquire().await?;
    let name = lane_name(&mut db, &lane).await?.ok_or_else(Fail::missing)?;
    let (enabled, current, kind, reason, since, pending, pending_reason, switch_at): LaneView = sqlx::query_as(
        "SELECT enabled,current_broadcast,current_kind,current_reason,current_since,pending_broadcast,pending_reason,switch_at FROM magnet_lanes WHERE id=$1")
        .bind(&lane).fetch_one(&mut *db).await?;
    let streams = discovery::live_streams(&mut db).await?;
    let find = |id: &Option<String>| {
        id.as_ref()
            .and_then(|id| streams.iter().find(|s| &s.broadcast_id == id))
    };
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut featured = Value::Null;
    if let Some(s) = find(&current) {
        let mut card = discovery::card(&app, s, now);
        let features: i64 =
            sqlx::query_scalar("SELECT count(*) FROM magnet_features WHERE owner_id=$1")
                .bind(&s.owner_id)
                .fetch_one(&mut *db)
                .await?;
        if features <= 1 {
            card["label"] = json!("First feature");
        }
        // Co-streams: a merged one is featured as a unit with tabs for its featured members; a
        // separate one links to the other members ("Co-streaming with …").
        let squad = crate::squads::active_of(&mut db, &s.owner_id).await?;
        let unit: Vec<String> = sqlx::query_scalar(
            "SELECT broadcast_id FROM magnet_features WHERE lane=$1 AND ended_at IS NULL",
        )
        .bind(&lane)
        .fetch_all(&mut *db)
        .await?;
        let merged = squad.as_ref().is_some_and(|q| q.mode == "MERGED");
        let owners: Vec<String> = match &squad {
            Some(q) if merged => q.members.iter().map(|m| m.0.clone()).collect(),
            _ => vec![s.owner_id.clone()],
        };
        let squad_id = squad.as_ref().filter(|_| merged).map(|q| q.id.clone());
        let squad = squad.map(|q| {
            let members: Vec<Value> = q
                .members
                .iter()
                .filter(|m| {
                    if merged {
                        unit.contains(&m.1)
                    } else {
                        m.1 != s.broadcast_id
                    }
                })
                .filter_map(|m| streams.iter().find(|x| x.broadcast_id == m.1))
                .map(|x| discovery::card(&app, x, now))
                .collect();
            json!({"mode":q.mode,"members":members})
        });
        // A channel ban, a timeout or a block hides the stream (no playback session) and pauses
        // sending in MAGNet chat until MAGNet moves on. In a merged co-stream any member's counts,
        // as in its shared chat. Signed-out viewing can't be blocked.
        let holding = match &viewer {
            Some(v) => sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=ANY($1) AND user_id=$2 AND (kind='ban' OR until>now())) OR EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$2 AND blocked_id=ANY($1)) OR (blocker_id=ANY($1) AND blocked_id=$2)) OR EXISTS(SELECT 1 FROM squad_restrictions WHERE squad_id=$3 AND user_id=$2 AND (kind='ban' OR until>now()))")
                .bind(&owners).bind(&v.id).bind(squad_id.as_deref()).fetch_one(&mut *db).await?,
            None => false,
        };
        let moves_on_by = since.map(|at| at + Duration::seconds(app.config.magnet.max_seconds));
        // Charity streams say so in MAGNet's reason (Shine); it never changes scoring.
        let reason = match (&s.charity, reason.clone()) {
            (Some(_), Some(r)) => Some(format!("{r} · Charity stream")),
            (Some(_), None) => Some("Charity stream".to_string()),
            (None, r) => r,
        };
        featured = json!({"stream":card,"kind":kind,"reason":reason,"since":since,
            "moves_on_by":moves_on_by,"holding":holding,"squad":squad});
    }
    let next = find(&pending).map(|s| {
        json!({"stream":discovery::card(&app, s, now),"reason":pending_reason,"switch_at":switch_at})
    });
    // Other streams in this lane, for the holding card.
    let others: Vec<Value> = streams
        .iter()
        .filter(|s| Some(&s.broadcast_id) != current.as_ref())
        .filter(|s| lane == "global" || s.genre.as_deref() == Some(lane.as_str()))
        .take(6)
        .map(|s| discovery::card(&app, s, now))
        .collect();
    Ok(Json(
        json!({"id":lane,"name":name,"enabled":enabled,"featured":featured,"next":next,"others":others,"as_of":now}),
    ))
}

/// GET /api/me/magnet: Studio's MAGNet panel: settings, featured now, history.
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let (opt_out, chat_merge): (bool, bool) = sqlx::query_as("SELECT coalesce(s.opt_out,false),coalesce(s.chat_merge,true) FROM (SELECT 1) one LEFT JOIN magnet_settings s ON s.user_id=$1")
        .bind(&user.id).fetch_one(&mut *db).await?;
    let now: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('lane',l.id,'name',CASE WHEN l.id='global' THEN 'Global' ELSE g.name END,'reason',l.current_reason,'since',l.current_since) FROM magnet_features f JOIN magnet_lanes l ON l.id=f.lane LEFT JOIN faction_genres g ON g.id=l.id WHERE f.owner_id=$1 AND f.ended_at IS NULL")
        .bind(&user.id).fetch_all(&mut *db).await?;
    // What happened during each feature is shown to the streamer only, never used for scoring.
    let history: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('lane',f.lane,'name',CASE WHEN f.lane='global' THEN 'Global' ELSE g.name END,'kind',f.kind,'reason',f.reason,'started_at',f.started_at,'ended_at',f.ended_at,
        'seconds',extract(epoch FROM coalesce(f.ended_at,now())-f.started_at)::int,
        'hype_viewers',(SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=f.broadcast_id AND l.magnet_lane=f.lane AND l.created_at BETWEEN f.started_at AND coalesce(f.ended_at,now())),
        'followed',(SELECT count(*) FROM follows w JOIN playback_leases l ON l.broadcast_id=f.broadcast_id AND l.magnet_lane=f.lane AND l.viewer_key='u:'||w.follower_id WHERE w.following_id=f.owner_id AND w.created_at BETWEEN f.started_at AND coalesce(f.ended_at,now())+interval '10 minutes'),
        'chatted',(SELECT count(DISTINCT m.author_id) FROM chat_messages m JOIN playback_leases l ON l.broadcast_id=f.broadcast_id AND l.magnet_lane=f.lane AND l.viewer_key='u:'||m.author_id WHERE m.channel_id=f.owner_id AND m.squad_id IS NULL AND m.created_at BETWEEN f.started_at AND coalesce(f.ended_at,now())+interval '10 minutes'))
        FROM magnet_features f LEFT JOIN faction_genres g ON g.id=f.lane WHERE f.owner_id=$1 AND f.started_at>now()-interval '30 days' ORDER BY f.started_at DESC LIMIT 50")
        .bind(&user.id).fetch_all(&mut *db).await?;
    Ok(Json(
        json!({"opt_out":opt_out,"chat_merge":chat_merge,"featured_now":now,"history":history}),
    ))
}
#[derive(Deserialize)]
struct Settings {
    opt_out: bool,
    chat_merge: bool,
}
/// PUT /api/me/magnet: opt out of MAGNet Hype (effective at the next tick) and chat merging.
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query("INSERT INTO magnet_settings(user_id,opt_out,chat_merge) VALUES($1,$2,$3) ON CONFLICT(user_id) DO UPDATE SET opt_out=$2,chat_merge=$3")
        .bind(&user.id).bind(input.opt_out).bind(input.chat_merge).execute(&app.db).await?;
    mine(State(app), jar).await
}
/// POST /api/me/magnet/flag: "Flag this moment" (also `/flag` in chat). Counts only together with
/// another elevated signal; once every 10 minutes.
async fn flag(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let broadcast: Option<String> = sqlx::query_scalar(
        "SELECT id FROM broadcasts WHERE owner_id=$1 AND state='LIVE' FOR UPDATE",
    )
    .bind(&user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let broadcast = broadcast.ok_or_else(|| Fail::bad("Go live before flagging a moment."))?;
    let recent: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM magnet_flags WHERE owner_id=$1 AND at>now()-make_interval(secs=>$2))")
        .bind(&user.id).bind(app.config.magnet.flag_cooldown_seconds as f64).fetch_one(&mut *tx).await?;
    if recent {
        return Err(Fail::conflict(
            "You can flag a moment once every 10 minutes.",
        ));
    }
    sqlx::query("INSERT INTO magnet_flags(owner_id,broadcast_id) VALUES($1,$2)")
        .bind(&user.id)
        .bind(&broadcast)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"flagged":true})))
}

/// GET /api/admin/magnet: lanes and the decision log (7 days).
async fn admin(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let lanes: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',l.id,'name',CASE WHEN l.id='global' THEN 'Global' ELSE g.name END,'enabled',l.enabled,'featuring',c.username,'reason',l.current_reason,'since',l.current_since,'forced',fc.username,'next',pc.username,'switch_at',l.switch_at) FROM magnet_lanes l LEFT JOIN faction_genres g ON g.id=l.id LEFT JOIN broadcasts b ON b.id=l.current_broadcast LEFT JOIN channel_users c ON c.id=b.owner_id LEFT JOIN broadcasts fb ON fb.id=l.forced_broadcast LEFT JOIN channel_users fc ON fc.id=fb.owner_id LEFT JOIN broadcasts pb ON pb.id=l.pending_broadcast LEFT JOIN channel_users pc ON pc.id=pb.owner_id ORDER BY l.id<>'global', g.position NULLS FIRST, l.id")
        .fetch_all(&app.db).await?;
    let decisions: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('lane',d.lane,'at',d.at,'kind',d.kind,'chosen',c.username,'reason',d.reason,'candidates',d.candidates) FROM magnet_decisions d LEFT JOIN broadcasts b ON b.id=d.chosen LEFT JOIN channel_users c ON c.id=b.owner_id ORDER BY d.at DESC LIMIT 100")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"lanes":lanes,"decisions":decisions})))
}
async fn audit(
    db: &mut PgConnection,
    staff: &str,
    action: &str,
    lane: &str,
    detail: Value,
) -> Res<()> {
    safety::audit(
        db,
        Some(staff),
        action,
        "magnet_lane",
        lane,
        &[],
        "",
        detail,
        false,
    )
    .await
}
#[derive(Deserialize)]
struct Enable {
    enabled: bool,
}
/// PUT /api/admin/magnet/{lane}: enable or disable one lane.
async fn set_enabled(
    State(app): State<App>,
    jar: CookieJar,
    Path(lane): Path<String>,
    Json(input): Json<Enable>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let found = sqlx::query("UPDATE magnet_lanes SET enabled=$2,ticked_at=NULL WHERE id=$1")
        .bind(&lane)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if found == 0 {
        return Err(Fail::missing());
    }
    let action = if input.enabled {
        "magnet_enable"
    } else {
        "magnet_disable"
    };
    audit(&mut tx, &staff.id, action, &lane, json!({})).await?;
    tx.commit().await?;
    admin(State(app), jar).await
}
#[derive(Deserialize)]
struct Force {
    username: Option<String>,
}
/// PUT /api/admin/magnet/{lane}/force: put a live stream on a lane (null releases it).
async fn force(
    State(app): State<App>,
    jar: CookieJar,
    Path(lane): Path<String>,
    Json(input): Json<Force>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let broadcast = match input
        .username
        .as_deref()
        .map(|n| n.trim().trim_start_matches('@'))
    {
        None | Some("") => None,
        Some(name) => {
            let channel = profiles::eligible_by_name(&mut tx, name)
                .await?
                .ok_or_else(Fail::channel_missing)?;
            let live: Option<String> =
                sqlx::query_scalar("SELECT id FROM broadcasts WHERE owner_id=$1 AND state='LIVE'")
                    .bind(&channel.id)
                    .fetch_optional(&mut *tx)
                    .await?;
            Some(live.ok_or_else(|| Fail::bad("That channel isn't live."))?)
        }
    };
    let found =
        sqlx::query("UPDATE magnet_lanes SET forced_broadcast=$2,ticked_at=NULL WHERE id=$1")
            .bind(&lane)
            .bind(&broadcast)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if found == 0 {
        return Err(Fail::missing());
    }
    let action = if broadcast.is_some() {
        "magnet_force"
    } else {
        "magnet_release"
    };
    audit(
        &mut tx,
        &staff.id,
        action,
        &lane,
        json!({"broadcast": broadcast}),
    )
    .await?;
    tx.commit().await?;
    admin(State(app), jar).await
}
/// POST /api/admin/magnet/stop: emergency stop; every lane is disabled at once.
async fn stop(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE magnet_lanes SET enabled=false,ticked_at=NULL")
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &staff.id,
        "magnet_emergency_stop",
        "all",
        json!({}),
    )
    .await?;
    tx.commit().await?;
    admin(State(app), jar).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/magnet", get(lanes))
        .route("/api/magnet/{lane}", get(lane))
        .route("/api/me/magnet", get(mine).put(save))
        .route("/api/me/magnet/flag", post(flag))
        .route("/api/admin/magnet", get(admin))
        .route("/api/admin/magnet/stop", post(stop))
        .route("/api/admin/magnet/{lane}", put(set_enabled))
        .route("/api/admin/magnet/{lane}/force", put(force))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn t() -> Tuning {
        Tuning::default()
    }
    fn c(id: &str, score: f64, last: Option<i64>, now: DateTime<Utc>) -> Candidate {
        Candidate {
            broadcast: id.into(),
            members: vec![id.into()],
            score,
            moment: (score >= 3.0).then(|| "Chat is going off".to_string()),
            last_featured: last.map(|m| now - Duration::minutes(m)),
        }
    }
    fn lane(current: &str, held: i64, last_kind: &str, now: DateTime<Utc>) -> LaneState {
        LaneState {
            current: Some(current.into()),
            since: Some(now - Duration::seconds(held)),
            last_kind: Some(last_kind.into()),
            last_moment_at: None,
            forced: None,
        }
    }
    fn switched(d: &Decision) -> Option<(&str, &str)> {
        match d {
            Decision::Switch {
                broadcast, kind, ..
            } => Some((broadcast.as_str(), *kind)),
            _ => None,
        }
    }

    #[test]
    fn signals_compare_with_the_streams_own_normal() {
        let t = t();
        // A 3-viewer stream: 3 chatters against a normal of under 1 a minute is a moment.
        let small = Signals {
            chatters: 3.0,
            chat_base: 0.3,
            ..Signals::default()
        };
        assert_eq!(small.score(&t), (3.0, Some("Chat is going off".into())));
        // A big stream with 60 chatters against its normal of 50 is not.
        let big = Signals {
            chatters: 60.0,
            chat_base: 50.0,
            ..Signals::default()
        };
        assert!(big.score(&t).0 < t.moment_ratio);
        // Two people can't make a moment; a flag alone adds nothing.
        let two = Signals {
            chatters: 2.0,
            ..Signals::default()
        };
        assert_eq!(two.score(&t).0, 0.0);
        let alone = Signals {
            flagged: true,
            ..Signals::default()
        };
        assert_eq!(alone.score(&t).0, 0.0);
        let flagged = Signals {
            chatters: 3.0,
            chat_base: 1.5,
            flagged: true,
            ..Signals::default()
        };
        assert_eq!(flagged.score(&t).0, 3.0);
        let raid = Signals {
            raided_by: Some("Raider".into()),
            ..Signals::default()
        };
        assert_eq!(raid.score(&t), (10.0, Some("Just raided by Raider".into())));
    }

    #[test]
    fn holds_alternates_and_moves_on() {
        let now = Utc::now();
        let t = t();
        let quiet = [
            c("a", 0.0, Some(0), now),
            c("b", 0.0, Some(40), now),
            c("x", 5.0, None, now),
        ];
        // Minimum hold: no switch before 45 seconds, even for a moment.
        assert_eq!(
            decide(&lane("a", 30, "fair", now), &quiet, now, &t),
            Decision::Hold
        );
        // After a fair turn the next switch may be a moment.
        assert_eq!(
            switched(&decide(&lane("a", 50, "fair", now), &quiet, now, &t)),
            Some(("x", "moment"))
        );
        // After a moment the next switch must be a fair turn: no back-to-back moments.
        assert_eq!(
            decide(&lane("a", 50, "moment", now), &quiet, now, &t),
            Decision::Hold
        );
        // At most 8 minutes, then a fair turn to the stream that waited longest.
        assert_eq!(
            switched(&decide(&lane("a", 480, "moment", now), &quiet, now, &t)),
            Some(("x", "fair"))
        );
        let waited = [
            c("a", 0.0, Some(0), now),
            c("b", 0.0, Some(40), now),
            c("d", 0.0, Some(90), now),
        ];
        assert_eq!(
            switched(&decide(&lane("a", 480, "moment", now), &waited, now, &t)),
            Some(("d", "fair"))
        );
        // Two minutes between moment switches.
        let mut recent = lane("a", 50, "fair", now);
        recent.last_moment_at = Some(now - Duration::seconds(60));
        assert_eq!(decide(&recent, &quiet, now, &t), Decision::Hold);
        // A moment must clearly beat the current stream.
        let busy = [c("a", 4.0, Some(0), now), c("x", 5.0, None, now)];
        assert_eq!(
            decide(&lane("a", 50, "fair", now), &busy, now, &t),
            Decision::Hold
        );
    }

    #[test]
    fn cooldown_dead_air_and_fallbacks() {
        let now = Utc::now();
        let t = t();
        let three = [
            c("a", 0.0, Some(0), now),
            c("cool", 0.0, Some(10), now),
            c("b", 0.0, Some(20), now),
        ];
        // Featured 10 minutes ago: cooling down, so the fair turn goes to the other stream...
        assert_eq!(
            switched(&decide(&lane("a", 480, "fair", now), &three, now, &t)),
            Some(("b", "fair"))
        );
        // ...unless it's the only other stream (no dead air).
        let alone = [c("a", 0.0, Some(0), now), c("cool", 0.0, Some(10), now)];
        assert_eq!(
            switched(&decide(&lane("a", 480, "fair", now), &alone, now, &t)),
            Some(("cool", "fair"))
        );
        // The only eligible stream holds indefinitely.
        assert_eq!(
            decide(
                &lane("a", 9000, "fair", now),
                &[c("a", 0.0, Some(0), now)],
                now,
                &t
            ),
            Decision::Hold
        );
        // The featured stream ended: fall back to an eligible one at once.
        assert_eq!(
            switched(&decide(&lane("gone", 100, "fair", now), &three, now, &t)),
            Some(("b", "fallback"))
        );
        // Nothing live: clear.
        assert_eq!(
            decide(&lane("gone", 100, "fair", now), &[], now, &t),
            Decision::Clear
        );
        // Staff force wins and holds.
        let mut forced = lane("a", 10, "fair", now);
        forced.forced = Some("cool".into());
        assert_eq!(
            switched(&decide(&forced, &three, now, &t)),
            Some(("cool", "forced"))
        );
        forced.current = Some("cool".into());
        assert_eq!(decide(&forced, &three, now, &t), Decision::Hold);
    }

    #[test]
    fn a_merged_co_stream_is_one_unit() {
        let now = Utc::now();
        let t = t();
        let mut squad = c("host", 0.0, Some(0), now);
        squad.members = vec!["host".into(), "guest".into()];
        let cands = [squad, c("solo", 0.0, Some(60), now)];
        // Showing any member is showing the unit: it holds, and a fair turn skips the whole unit.
        assert_eq!(
            decide(&lane("guest", 100, "fair", now), &cands, now, &t),
            Decision::Hold
        );
        assert_eq!(
            switched(&decide(&lane("guest", 480, "fair", now), &cands, now, &t)),
            Some(("solo", "fair"))
        );
        // Staff forcing one member shows that member and holds while any member is showing.
        let mut forced = lane("solo", 10, "fair", now);
        forced.forced = Some("guest".into());
        assert_eq!(
            switched(&decide(&forced, &cands, now, &t)),
            Some(("guest", "forced"))
        );
        forced.current = Some("host".into());
        assert_eq!(decide(&forced, &cands, now, &t), Decision::Hold);
    }

    #[test]
    fn every_eligible_stream_is_featured_within_the_bound() {
        // Six quiet streams and one that is always having a moment.
        let t = t();
        let start = Utc::now();
        let ids = ["s0", "s1", "s2", "s3", "s4", "s5", "loud"];
        let mut last: HashMap<String, DateTime<Utc>> = HashMap::new();
        let mut state = LaneState::default();
        let mut seen = HashSet::new();
        let mut now = start;
        // Alternation gives a fair turn at least every second switch and a feature lasts at most
        // 8 minutes, so n streams are all reached within 2 * n * 8 minutes.
        let bound = Duration::minutes(2 * ids.len() as i64 * 8);
        while now - start <= bound {
            let cands: Vec<Candidate> = ids
                .iter()
                .map(|id| Candidate {
                    broadcast: id.to_string(),
                    members: vec![id.to_string()],
                    score: if *id == "loud" { 50.0 } else { 0.0 },
                    moment: (*id == "loud").then(|| "Chat is going off".to_string()),
                    last_featured: last.get(*id).copied(),
                })
                .collect();
            if let Decision::Switch {
                broadcast, kind, ..
            } = decide(&state, &cands, now, &t)
            {
                if kind == "moment" || kind == "fair" {
                    state.last_kind = Some(kind.into());
                }
                if kind == "moment" {
                    state.last_moment_at = Some(now);
                }
                seen.insert(broadcast.clone());
                state.current = Some(broadcast);
                state.since = Some(now);
            }
            if let Some(cur) = &state.current {
                last.insert(cur.clone(), now);
            }
            now += Duration::seconds(t.tick_seconds);
        }
        assert_eq!(
            seen.len(),
            ids.len(),
            "every stream featured within the bound: {seen:?}"
        );
    }
}
