use super::*;
use crate::integrity::{self, Inputs, Level};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    routing::post,
};
use std::net::SocketAddr;

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
}
async fn beat(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Beat>,
) -> Res<Json<Value>> {
    if !(16..=64).contains(&input.browser_id.len())
        || !input
            .browser_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !input.media_time.is_finite()
        || input.media_time < 0.0
    {
        return Err(Fail::bad("Invalid playback heartbeat."));
    }
    let (video, user) = watched(&app, &jar, &id, input.age_ack).await?;
    if input.media_time > video.duration_ms as f64 / 1000.0 + 2.0 {
        return Err(Fail::bad("Playback time exceeds this recording."));
    }
    if user
        .as_ref()
        .is_some_and(|u| Some(&u.id) == video.owner_id.as_ref())
    {
        return Ok(Json(json!({"recorded":false})));
    }
    let ip = security::client_ip(&app, peer, &headers);
    security::reserve(
        &app,
        vec![format!("video-beat:{ip}")],
        app.config.videos.tuning.beats_per_ip_minute,
        60,
    )
    .await?;
    let key = match &user {
        Some(user) => format!("u:{}", user.id),
        None => format!("b:{}", security::digest(&input.browser_id)),
    };
    security::reserve(
        &app,
        vec![format!("video-viewer:{key}")],
        app.config.videos.tuning.beats_per_viewer_minute,
        60,
    )
    .await?;
    let verified = if let Some(user) = &user {
        user.email_verified
            && profiles::channel_user_by_id(&mut *app.db.acquire().await?, &user.id)
                .await?
                .is_some_and(|u| u.eligible)
    } else {
        false
    };
    let passed = match input.turnstile {
        Some(token) if !verified && !token.is_empty() => {
            security::turnstile(&app, &token, "playback", ip)
                .await
                .is_ok()
        }
        _ => false,
    };
    let network = integrity::keyed(&app, "video-net", &integrity::network(ip));
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO video_playback(video_id,viewer_key,network,media_time) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(&id).bind(&key).bind(&network).bind(input.media_time).execute(&mut *tx).await?;
    let mut lease: Lease = sqlx::query_as(
        "SELECT * FROM video_playback WHERE video_id=$1 AND viewer_key=$2 FOR UPDATE",
    )
    .bind(&id)
    .bind(&key)
    .fetch_one(&mut *tx)
    .await?;
    let now = Utc::now();
    let elapsed = (now - lease.updated_at).num_milliseconds() as f64 / 1000.0;
    let advanced = input.media_time - lease.media_time;
    // Seeking and a resumed tab don't earn watch time. Ordinary playback may run at up to 2x.
    if elapsed > 0.0
        && elapsed <= 30.0
        && advanced > 0.0
        && advanced <= elapsed * 2.0 + 2.0
        && input.visible
    {
        lease.watched_seconds += elapsed.min(advanced).min(15.0);
        lease.interval_count += 1;
        let delta = elapsed * 1000.0 - lease.interval_mean;
        lease.interval_mean += delta / f64::from(lease.interval_count);
        lease.interval_m2 += delta * (elapsed * 1000.0 - lease.interval_mean);
    }
    lease.turnstile_ok |= passed;
    let same_network:i64=sqlx::query_scalar("SELECT count(*) FROM video_playback WHERE video_id=$1 AND network=$2 AND updated_at>now()-interval '30 seconds'").bind(&id).bind(&network).fetch_one(&mut *tx).await?;
    let mut tuning = app.config.integrity.clone();
    if video.kind == "CLIP" {
        // A short clip can finish before the live-stream observation window. Keep the
        // same identity/risk checks, with a privately tuned minimum of real watch time.
        tuning.pending_seconds = tuning
            .pending_seconds
            .min(app.config.videos.tuning.clip_watch_seconds)
            .min(video.duration_ms as f64 / 1000.0);
    }
    let (level, _, _) = integrity::assess(
        &tuning,
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
    let count = matches!(level, Level::Counted | Level::Trusted);
    sqlx::query("UPDATE video_playback SET network=$3,updated_at=$4,media_time=$5,watched_seconds=$6,interval_count=$7,interval_mean=$8,interval_m2=$9,turnstile_ok=$10,counted=counted OR $11 WHERE video_id=$1 AND viewer_key=$2")
        .bind(&id).bind(&key).bind(network).bind(now).bind(input.media_time).bind(lease.watched_seconds).bind(lease.interval_count).bind(lease.interval_mean).bind(lease.interval_m2).bind(lease.turnstile_ok).bind(count).execute(&mut *tx).await?;
    if count && !lease.counted {
        sqlx::query("UPDATE videos SET views=views+1 WHERE id=$1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"recorded":true,"level":level.name(),"needs_turnstile":!verified&&!lease.turnstile_ok}),
    ))
}
pub fn routes() -> Router<App> {
    Router::new().route("/api/videos/{id}/beat", post(beat))
}
