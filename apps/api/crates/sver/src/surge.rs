//! Module 7 CrowdSync, phase 3 (docs/CROWDSYNC.md "Faction Rally and emote combos", "Surge").
//! Crowd-made moments that count distinct real viewers, never money: a faction rally meter, an
//! emote shower when 5 distinct accounts send the same emote within 5 seconds, and Surge, which
//! starts when enough distinct viewers take part within about a minute. None of it touches MAGNet;
//! a `!rally` chat message counts toward faction influence only as the ordinary message it is.
use crate::{
    App, boards,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use axum_extra::extract::cookie::CookieJar;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

/// The current minute bucket (one participation or rally per person per minute).
const MINUTE: &str = "floor(extract(epoch FROM now())/60)::bigint";
/// A real viewer ($2) on the channel's ($1) live broadcast, never the owner.
const WATCHING: &str = "broadcasts b JOIN playback_leases l ON l.broadcast_id=b.id AND l.viewer_key='u:'||$2 AND l.expires_at>now() AND l.level IN ('counted','trusted')";
const WATCHING_WHERE: &str = "b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') AND $1<>$2";

// Surge tuning (**Proposed** in the spec).
const SURGE_SECONDS: i64 = 300;
const SURGE_MAX_SECONDS: i64 = 600;
const LEVEL_SECONDS: i64 = 75;
const COOLDOWN_MINUTES: i32 = 30;
const AWARD_PER_LEVEL: i32 = 20;
const DAILY_AWARD_CAP: i32 = 200;
const CELEBRATIONS: [&str; 5] = ["confetti", "stars", "hearts", "fireworks", "fireworks"];

/// Distinct participants needed: 10, scaled down for small channels (60% of the current audience,
/// at least 3), so a 5-viewer stream can still start one.
pub fn threshold(viewers: i64) -> i64 {
    ((viewers * 3 + 4) / 5).clamp(3, 10)
}

/// One participation per real viewer per minute (chat, rallies, presses, tributes, Skills and
/// subscriptions call this); anyone not watching the live broadcast simply doesn't count.
pub(crate) async fn participated(db: &mut PgConnection, channel: &str, user: &str) -> Res<()> {
    // MINUTE, WATCHING and WATCHING_WHERE are fixed SQL; the channel and user are bound.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO surge_participation(broadcast_id,user_id,minute) SELECT b.id,$2,{MINUTE} FROM {WATCHING} WHERE {WATCHING_WHERE} ON CONFLICT DO NOTHING"
    )))
    .bind(channel)
    .bind(user)
    .execute(db)
    .await?;
    Ok(())
}

// ---- Faction Rally ----

/// Records a rally for a faction member watching the live broadcast, at most once a minute.
/// Returns whether it counted.
pub(crate) async fn rally_in(db: &mut PgConnection, channel: &str, user: &str) -> Res<bool> {
    // MINUTE, WATCHING and WATCHING_WHERE are fixed SQL; the channel and user are bound.
    Ok(sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO rallies(broadcast_id,user_id,faction,minute) SELECT b.id,$2,f.faction,{MINUTE}
            FROM faction_members f, {WATCHING} WHERE f.user_id=$2 AND {WATCHING_WHERE} ON CONFLICT DO NOTHING"
    )))
    .bind(channel)
    .bind(user)
    .execute(db)
    .await?
    .rows_affected()
        == 1)
}
/// Rallies per faction on the channel's live broadcast; null when it's offline.
pub(crate) async fn rally_meter(db: &mut PgConnection, channel: &str) -> Res<Value> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('myria',count(r.*) FILTER (WHERE r.faction='myria'),'aetheron',count(r.*) FILTER (WHERE r.faction='aetheron'),'glint',count(r.*) FILTER (WHERE r.faction='glint'))
        FROM broadcasts b LEFT JOIN rallies r ON r.broadcast_id=b.id WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') GROUP BY b.id")
        .bind(channel)
        .fetch_optional(db)
        .await?
        .unwrap_or(Value::Null))
}
pub(crate) async fn publish_rally(app: &App, channel: &str) -> Res<()> {
    let rally = rally_meter(&mut *app.db.acquire().await?, channel).await?;
    app.chat
        .publish(channel, None, 0, json!({"type": "rally", "rally": rally}));
    Ok(())
}
/// POST /api/channels/{username}/rally: the rally button (also a board control and `!rally`).
async fn rally(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let channel = {
        let mut db = app.db.acquire().await?;
        profiles::eligible_by_name(&mut db, &name)
            .await?
            .ok_or_else(Fail::channel_missing)?
            .id
    };
    rally_for(&app, &channel, &user).await?;
    Ok(Json(json!({"rallied": true})))
}
pub(crate) async fn rally_for(app: &App, channel: &str, user: &crate::auth::User) -> Res<()> {
    if channel == user.id {
        return Err(Fail::bad("Your viewers rally on your stream."));
    }
    boards::real_viewer(app, channel, user).await?;
    let member: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_members WHERE user_id=$1)")
            .bind(&user.id)
            .fetch_one(&app.db)
            .await?;
    if !member {
        return Err(Fail::bad("Join a faction to rally for it."));
    }
    sec::reserve(app, vec![format!("rally:{}", user.id)], 5, 60).await?;
    let mut tx = app.db.begin().await?;
    if !rally_in(&mut tx, channel, &user.id).await? {
        return Err(Fail::conflict(
            "You've rallied this minute. Rally again soon.",
        ));
    }
    participated(&mut tx, channel, &user.id).await?;
    tx.commit().await?;
    publish_rally(app, channel).await
}

// ---- Emote combos ----

#[derive(Default)]
struct Combos {
    /// (emote code, user, when) in the last 5 seconds.
    hits: Vec<(String, String, Instant)>,
    last: Option<Instant>,
}
// ponytail: per-instance memory, like the chat hub; a second API replica would need shared state.
static COMBOS: LazyLock<Mutex<HashMap<String, Combos>>> = LazyLock::new(Default::default);
const COMBO_WINDOW: Duration = Duration::from_secs(5);
const COMBO_COOLDOWN: Duration = Duration::from_secs(10);
const COMBO_PEOPLE: usize = 5;

/// Notes the emote-like words of a chat message; when 5 distinct accounts sent the same channel
/// emote within 5 seconds (and none played in the last 10), an emote shower plays on stream.
/// Chat already requires a verified account.
pub(crate) async fn combo(app: &App, channel: &str, user: &str, body: &str) -> Res<()> {
    let mut words: Vec<&str> = body
        .split_whitespace()
        .filter(|w| (3..=20).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_alphanumeric()))
        .collect();
    words.sort_unstable();
    words.dedup();
    words.truncate(10);
    if words.is_empty() {
        return Ok(());
    }
    let ready = {
        let mut all = COMBOS.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let combos = all.entry(channel.to_string()).or_default();
        combos.hits.retain(|h| now - h.2 < COMBO_WINDOW);
        for word in &words {
            combos.hits.push((word.to_string(), user.to_string(), now));
        }
        if combos.last.is_some_and(|t| now - t < COMBO_COOLDOWN) {
            None
        } else {
            words.into_iter().find(|w| {
                combos
                    .hits
                    .iter()
                    .filter(|h| h.0 == *w)
                    .map(|h| h.1.as_str())
                    .collect::<HashSet<_>>()
                    .len()
                    >= COMBO_PEOPLE
            })
        }
    };
    let Some(code) = ready else {
        return Ok(());
    };
    let emote = crate::emotes::catalog(app, channel)
        .await?
        .into_iter()
        .find(|e| e["code"] == code);
    let Some(emote) = emote else {
        return Ok(());
    };
    {
        let mut all = COMBOS.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(c) = all.get_mut(channel) {
            if c.last.is_some_and(|t| t.elapsed() < COMBO_COOLDOWN) {
                return Ok(());
            }
            c.last = Some(Instant::now());
            c.hits.retain(|h| h.0 != code);
        }
    }
    boards::play(
        app,
        channel,
        None,
        json!({"effect": "emote-shower", "image": emote["image"]["56"], "label": code,
            "caption": format!("Emote combo: {code}")}),
    )
    .await
}

// ---- Surge ----

async fn surge_json(db: &mut PgConnection, id: &str) -> Res<Value> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'level',s.level,'threshold',s.threshold,'started_at',s.started_at,'ends_at',s.ends_at,'ended_at',s.ended_at,
            'participants',(SELECT count(DISTINCT p.user_id) FROM surge_participation p WHERE p.broadcast_id=s.broadcast_id AND p.minute>=floor(extract(epoch FROM s.started_at)/60)::bigint-1),
            'awarded',(SELECT count(*) FROM surge_awards a WHERE a.surge_id=s.id))
        FROM surges s WHERE s.id=$1")
    .bind(id)
    .fetch_one(db)
    .await?)
}
/// The channel's running Surge, if any.
pub(crate) async fn current(db: &mut PgConnection, channel: &str) -> Res<Value> {
    let id: Option<String> =
        sqlx::query_scalar("SELECT id FROM surges WHERE channel_id=$1 AND ended_at IS NULL")
            .bind(channel)
            .fetch_optional(&mut *db)
            .await?;
    Ok(match id {
        Some(id) => surge_json(db, &id).await?,
        None => Value::Null,
    })
}
async fn announce(app: &App, channel: &str, id: &str, celebrate: Option<i16>) -> Res<()> {
    let surge = surge_json(&mut *app.db.acquire().await?, id).await?;
    crate::events::emit_after(app, channel, "surge", surge.clone()).await?;
    app.chat
        .publish(channel, None, 0, json!({"type": "surge", "surge": surge}));
    if let Some(level) = celebrate {
        let effect = CELEBRATIONS[usize::try_from(level - 1).unwrap_or(0).min(4)];
        boards::play(
            app,
            channel,
            None,
            json!({"effect": effect, "label": "Surge", "surge": level, "caption": format!("Surge level {level}!")}),
        )
        .await?;
    }
    Ok(())
}

/// Ends Surges whose time is up (or whose stream ended) and awards their participants; starts or
/// levels up Surges from the last minute's distinct participants. Called from the media loop.
pub async fn tick(app: &App) -> Res<()> {
    let ending: Vec<(String, String, String, i16)> = sqlx::query_as("SELECT s.id,s.channel_id,s.broadcast_id,s.level FROM surges s LEFT JOIN broadcasts b ON b.id=s.broadcast_id
        WHERE s.ended_at IS NULL AND (s.ends_at<now() OR b.state IS NULL OR b.state NOT IN ('LIVE','RECONNECTING'))")
        .fetch_all(&app.db)
        .await?;
    for (id, channel, broadcast, level) in ending {
        let mut tx = app.db.begin().await?;
        let closed =
            sqlx::query("UPDATE surges SET ended_at=now() WHERE id=$1 AND ended_at IS NULL")
                .bind(&id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        if closed == 1 {
            // Everyone who took part gets Engagement Valor for the level reached, within the daily
            // cap; channel bans are excluded.
            sqlx::query("WITH people AS (
                    SELECT DISTINCT p.user_id FROM surge_participation p, surges s WHERE s.id=$1 AND p.broadcast_id=$3 AND p.minute>=floor(extract(epoch FROM s.started_at)/60)::bigint-1
                        AND NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=$2 AND r.user_id=p.user_id AND r.kind='ban')),
                today AS (SELECT user_id, sum(amount)::int AS got FROM surge_awards WHERE channel_id=$2 AND day=current_date GROUP BY user_id),
                given AS (INSERT INTO surge_awards(surge_id,user_id,channel_id,day,amount)
                    SELECT $1, p.user_id, $2, current_date, least($4, $5-coalesce(t.got,0)) FROM people p LEFT JOIN today t ON t.user_id=p.user_id
                    WHERE $5-coalesce(t.got,0)>0 RETURNING user_id, amount)
                INSERT INTO engagement(channel_id,user_id,balance,earned) SELECT $2,user_id,amount,amount FROM given
                ON CONFLICT(channel_id,user_id) DO UPDATE SET balance=engagement.balance+EXCLUDED.balance, earned=engagement.earned+EXCLUDED.earned")
                .bind(&id).bind(&channel).bind(&broadcast)
                .bind(i32::from(level) * AWARD_PER_LEVEL).bind(DAILY_AWARD_CAP)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        if closed == 1 {
            announce(app, &channel, &id, None).await?;
        }
    }
    // MINUTE is fixed SQL.
    let active: Vec<(String, String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT b.id, b.owner_id,
            (SELECT count(DISTINCT p.user_id) FROM surge_participation p WHERE p.broadcast_id=b.id AND p.minute>={MINUTE}-1),
            (SELECT count(*) FROM playback_leases l WHERE l.broadcast_id=b.id AND l.expires_at>now() AND l.level IN ('counted','trusted'))
        FROM broadcasts b WHERE b.state IN ('LIVE','RECONNECTING')
            AND EXISTS(SELECT 1 FROM surge_participation p WHERE p.broadcast_id=b.id AND p.minute>={MINUTE}-1)"
    )))
    .fetch_all(&app.db)
    .await?;
    for (broadcast, channel, recent, viewers) in active {
        let running: Option<(String, i16, i32, i64)> = sqlx::query_as("SELECT id, level, threshold,
                (SELECT count(DISTINCT p.user_id) FROM surge_participation p WHERE p.broadcast_id=s.broadcast_id AND p.minute>=floor(extract(epoch FROM s.started_at)/60)::bigint-1)
            FROM surges s WHERE channel_id=$1 AND ended_at IS NULL")
            .bind(&channel)
            .fetch_optional(&app.db)
            .await?;
        match running {
            Some((id, level, need, people)) => {
                let reached = i16::try_from((people / i64::from(need.max(1))).min(5)).unwrap_or(5);
                if reached > level {
                    sqlx::query("UPDATE surges SET level=$2, ends_at=least(started_at+make_interval(secs=>$3), ends_at+make_interval(secs=>$4)) WHERE id=$1")
                        .bind(&id).bind(reached).bind(SURGE_MAX_SECONDS as f64)
                        .bind((LEVEL_SECONDS * i64::from(reached - level)) as f64)
                        .execute(&app.db).await?;
                    announce(app, &channel, &id, Some(reached)).await?;
                }
            }
            None => {
                let need = threshold(viewers);
                if recent < need {
                    continue;
                }
                let cooling: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM surges WHERE channel_id=$1 AND ended_at>now()-make_interval(mins=>$2))")
                    .bind(&channel).bind(COOLDOWN_MINUTES).fetch_one(&app.db).await?;
                if cooling {
                    continue;
                }
                let id = profiles::new_id();
                let started = sqlx::query("INSERT INTO surges(id,channel_id,broadcast_id,threshold,ends_at) VALUES($1,$2,$3,$4,now()+make_interval(secs=>$5)) ON CONFLICT DO NOTHING")
                    .bind(&id).bind(&channel).bind(&broadcast).bind(need as i32).bind(SURGE_SECONDS as f64)
                    .execute(&app.db).await?.rows_affected();
                if started == 1 {
                    announce(app, &channel, &id, Some(1)).await?;
                }
            }
        }
    }
    Ok(())
}

pub fn routes() -> Router<App> {
    Router::new().route("/api/channels/{username}/rally", post(rally))
}

#[cfg(test)]
mod tests {
    use super::threshold;

    #[test]
    fn small_channels_can_surge() {
        assert_eq!(threshold(0), 3);
        assert_eq!(threshold(5), 3);
        assert_eq!(threshold(8), 5);
        assert_eq!(threshold(14), 9);
        assert_eq!(threshold(500), 10);
    }
}
