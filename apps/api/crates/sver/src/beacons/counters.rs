//! Counters (docs/BEACONS.md "Counts"). Every counter needs a session (an account or a guest
//! browser session) and is rate-limited. A view counts after 3 seconds of visible, advancing
//! playback from a session viewer integrity counts, at most once per Beacon per day.
use super::*;
use crate::integrity::{self, Inputs, Level};
use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    routing::{post, put},
};
use std::net::SocketAddr;

pub const VIEW_SECONDS: f64 = 3.0;

fn browser(value: &str) -> Res<()> {
    if !(16..=64).contains(&value.len())
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Fail::bad("Invalid playback session."));
    }
    Ok(())
}
fn viewer_key(user: Option<&auth::User>, browser: &str) -> String {
    match user {
        Some(user) => format!("u:{}", user.id),
        None => format!("b:{}", security::digest(browser)),
    }
}

#[derive(Deserialize)]
struct Beat {
    browser_id: String,
    media_time: f64,
    #[serde(default)]
    visible: bool,
    #[serde(default)]
    age_ack: bool,
    turnstile: Option<String>,
}
#[derive(sqlx::FromRow)]
struct Lease {
    updated_at: DateTime<Utc>,
    media_time: f64,
    watched_seconds: f64,
    interval_count: i32,
    interval_mean: f64,
    interval_m2: f64,
    turnstile_ok: bool,
    counted: bool,
    completed: bool,
}
/// POST /api/beacons/{id}/beat
async fn beat(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Beat>,
) -> Res<Json<Value>> {
    browser(&input.browser_id)?;
    if !input.media_time.is_finite() || input.media_time < 0.0 {
        return Err(Fail::bad("Invalid playback heartbeat."));
    }
    let (beacon, user) = watched(&app, &jar, &id, input.age_ack).await?;
    if beacon.status != "PUBLISHED" {
        return Ok(Json(json!({"recorded":false})));
    }
    let length = beacon.duration_ms as f64 / 1000.0;
    if input.media_time > length + 2.0 {
        return Err(Fail::bad("Playback time exceeds this Beacon."));
    }
    let owner = beacon.owner_id.clone().unwrap_or_default();
    if user.as_ref().is_some_and(|u| u.id == owner) {
        return Ok(Json(json!({"recorded":false})));
    }
    let tuning = &app.config.beacons.tuning;
    let ip = security::client_ip(&app, peer, &headers);
    security::reserve(
        &app,
        vec![format!("beacon-beat:{ip}")],
        tuning.beats_per_ip_minute,
        60,
    )
    .await?;
    let key = viewer_key(user.as_ref(), &input.browser_id);
    security::reserve(
        &app,
        vec![format!("beacon-viewer:{key}")],
        tuning.beats_per_viewer_minute,
        60,
    )
    .await?;
    let verified = match &user {
        Some(user) => {
            user.email_verified
                && profiles::channel_user_by_id(&mut *app.db.acquire().await?, &user.id)
                    .await?
                    .is_some_and(|u| u.eligible)
        }
        None => false,
    };
    let passed = match input.turnstile {
        Some(token) if !verified && !token.is_empty() => {
            security::turnstile(&app, &token, "playback", ip)
                .await
                .is_ok()
        }
        _ => false,
    };
    let network = integrity::keyed(&app, "beacon-net", &integrity::network(ip));
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO beacon_playback(beacon_id,viewer_key,day,network,media_time) VALUES($1,$2,current_date,$3,$4) ON CONFLICT DO NOTHING")
        .bind(&id).bind(&key).bind(&network).bind(input.media_time).execute(&mut *tx).await?;
    let mut lease: Lease = sqlx::query_as("SELECT * FROM beacon_playback WHERE beacon_id=$1 AND viewer_key=$2 AND day=current_date FOR UPDATE")
        .bind(&id)
        .bind(&key)
        .fetch_one(&mut *tx)
        .await?;
    let now = Utc::now();
    let elapsed = (now - lease.updated_at).num_milliseconds() as f64 / 1000.0;
    let advanced = input.media_time - lease.media_time;
    // Only visible playback that actually advances earns watch time. Seeks and loops don't.
    if elapsed > 0.0
        && elapsed <= 30.0
        && advanced > 0.0
        && advanced <= elapsed * 2.0 + 1.0
        && input.visible
    {
        lease.watched_seconds += elapsed.min(advanced).min(15.0);
        lease.interval_count += 1;
        let delta = elapsed * 1000.0 - lease.interval_mean;
        lease.interval_mean += delta / f64::from(lease.interval_count);
        lease.interval_m2 += delta * (elapsed * 1000.0 - lease.interval_mean);
    }
    lease.turnstile_ok |= passed;
    let same_network:i64=sqlx::query_scalar("SELECT count(*) FROM beacon_playback WHERE beacon_id=$1 AND network=$2 AND updated_at>now()-interval '30 seconds'").bind(&id).bind(&network).fetch_one(&mut *tx).await?;
    let mut integrity = app.config.integrity.clone();
    // Same identity and risk checks as live viewing, over the Beacon's 3-second window.
    integrity.pending_seconds = VIEW_SECONDS;
    let (level, _, _) = integrity::assess(
        &integrity,
        &Inputs {
            age_seconds: lease.watched_seconds,
            signed_in: user.is_some(),
            verified,
            turnstile_ok: lease.turnstile_ok,
            interval_count: lease.interval_count,
            interval_mean: lease.interval_mean,
            interval_m2: lease.interval_m2,
            visible_seconds: lease.watched_seconds,
            same_network,
            ..Default::default()
        },
    );
    let count =
        lease.watched_seconds >= VIEW_SECONDS && matches!(level, Level::Counted | Level::Trusted);
    let complete = count && lease.watched_seconds >= length * 0.9;
    sqlx::query("UPDATE beacon_playback SET network=$3,updated_at=$4,media_time=$5,watched_seconds=$6,interval_count=$7,interval_mean=$8,interval_m2=$9,turnstile_ok=$10,counted=counted OR $11,completed=completed OR $12 WHERE beacon_id=$1 AND viewer_key=$2 AND day=current_date")
        .bind(&id).bind(&key).bind(network).bind(now).bind(input.media_time).bind(lease.watched_seconds).bind(lease.interval_count).bind(lease.interval_mean).bind(lease.interval_m2).bind(lease.turnstile_ok).bind(count).bind(complete).execute(&mut *tx).await?;
    if count && !lease.counted {
        sqlx::query("UPDATE beacons SET views=views+1 WHERE id=$1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    if complete && !lease.completed {
        sqlx::query("INSERT INTO beacon_events(beacon_id,viewer_key,kind,owner_id) VALUES($1,$2,'COMPLETION',$3) ON CONFLICT DO NOTHING")
            .bind(&id).bind(&key).bind(&owner).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"recorded":true,"counted":count,"needs_turnstile":!verified&&!lease.turnstile_ok}),
    ))
}

/// PUT /api/beacons/{id}/like and DELETE to take it back: signed in, one per account, rate-limited.
async fn like(
    State(app): State<App>,
    jar: CookieJar,
    method: axum::http::Method,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (beacon, _) = watched(&app, &jar, &id, true).await?;
    if beacon.status != "PUBLISHED" {
        return Err(Fail::bad("This Beacon isn't published."));
    }
    profiles::rate(
        &app,
        format!("beacon-like:{}", user.id),
        app.config.beacons.tuning.likes_per_user_hour,
        3600,
    )
    .await?;
    let mut tx = app.db.begin().await?;
    let changed = if method == axum::http::Method::PUT {
        sqlx::query(
            "INSERT INTO beacon_likes(beacon_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(&id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64
    } else {
        -(sqlx::query("DELETE FROM beacon_likes WHERE beacon_id=$1 AND user_id=$2")
            .bind(&id)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?
            .rows_affected() as i64)
    };
    let likes: i64 = sqlx::query_scalar(
        "UPDATE beacons SET likes=greatest(likes+$2,0) WHERE id=$1 RETURNING likes",
    )
    .bind(&id)
    .bind(changed)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"liked":method==axum::http::Method::PUT,"likes":likes}),
    ))
}

#[derive(Deserialize)]
struct Session {
    browser_id: String,
    #[serde(default)]
    age_ack: bool,
}
/// POST /api/beacons/{id}/live: the Live now tap. It becomes a live join once the same session
/// is counted on the creator's stream (worker::maintain).
async fn live(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Session>,
) -> Res<Json<Value>> {
    browser(&input.browser_id)?;
    let (beacon, user) = watched(&app, &jar, &id, input.age_ack).await?;
    let owner = beacon.owner_id.clone().ok_or_else(Fail::missing)?;
    let mut db = app.db.acquire().await?;
    let channel = profiles::channel_user_by_id(&mut db, &owner)
        .await?
        .ok_or_else(Fail::missing)?;
    if !crate::playback::is_live(&mut db, &owner).await? {
        return Ok(Json(
            json!({"live":false,"url":format!("/{}", channel.username)}),
        ));
    }
    let key = viewer_key(user.as_ref(), &input.browser_id);
    if beacon.status == "PUBLISHED" && user.as_ref().is_none_or(|u| u.id != owner) {
        security::reserve(
            &app,
            vec![format!("beacon-tap:{key}")],
            app.config.beacons.tuning.taps_per_viewer_hour,
            3600,
        )
        .await?;
        sqlx::query("INSERT INTO beacon_events(beacon_id,viewer_key,kind,owner_id) VALUES($1,$2,'LIVE_TAP',$3) ON CONFLICT(beacon_id,viewer_key,kind,day) DO UPDATE SET created_at=now()")
            .bind(&id).bind(&key).bind(&owner).execute(&mut *db).await?;
    }
    Ok(Json(
        json!({"live":true,"url":format!("/{}/live", channel.username)}),
    ))
}
/// POST /api/beacons/{id}/followed: records a follow made from this Beacon's rail.
async fn followed(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (beacon, _) = watched(&app, &jar, &id, true).await?;
    let owner = beacon.owner_id.clone().ok_or_else(Fail::missing)?;
    // Only a follow that exists and was made moments ago is attributed to the Beacon.
    let recorded = sqlx::query("INSERT INTO beacon_events(beacon_id,viewer_key,kind,owner_id) SELECT $1,'u:'||$2,'FOLLOW',$3 WHERE EXISTS(SELECT 1 FROM follows WHERE follower_id=$2 AND following_id=$3 AND created_at>now()-interval '5 minutes') AND NOT EXISTS(SELECT 1 FROM beacon_events WHERE owner_id=$3 AND viewer_key='u:'||$2 AND kind='FOLLOW') ON CONFLICT DO NOTHING")
        .bind(&id).bind(&user.id).bind(&owner).execute(&app.db).await?.rows_affected();
    Ok(Json(json!({"recorded":recorded==1})))
}
/// PUT /api/beacons/mutes/{username} mutes a creator in the viewer's feed; DELETE unmutes.
async fn mute(
    State(app): State<App>,
    jar: CookieJar,
    method: axum::http::Method,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::rate(
        &app,
        format!("beacon-mute:{}", user.id),
        app.config.beacons.tuning.mutes_per_user_hour,
        3600,
    )
    .await?;
    let mut db = app.db.acquire().await?;
    let channel = profiles::eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    if channel.id == user.id {
        return Err(Fail::bad("You can't mute yourself."));
    }
    if method == axum::http::Method::PUT {
        sqlx::query(
            "INSERT INTO beacon_mutes(user_id,muted_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(&user.id)
        .bind(&channel.id)
        .execute(&mut *db)
        .await?;
    } else {
        sqlx::query("DELETE FROM beacon_mutes WHERE user_id=$1 AND muted_id=$2")
            .bind(&user.id)
            .bind(&channel.id)
            .execute(&mut *db)
            .await?;
    }
    Ok(Json(json!({"muted":method==axum::http::Method::PUT})))
}
pub fn routes() -> axum::Router<App> {
    axum::Router::new()
        .route("/api/beacons/{id}/beat", post(beat))
        .route("/api/beacons/{id}/like", put(like).delete(like))
        .route("/api/beacons/{id}/live", post(live))
        .route("/api/beacons/{id}/followed", post(followed))
        .route("/api/beacons/mutes/{name}", put(mute).delete(mute))
}
