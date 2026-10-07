//! Module 3 viewer integrity (docs/LIVE_STREAMS.md, "Viewer integrity"). Every playback lease gets
//! a level from its heartbeats and a background rescore: Pending, Counted (public count), Trusted
//! (public count plus tiers, payouts and influence) or Excluded. Suspicious viewers are only ever
//! not counted; penalties need staff review of an integrity case. Raw IP addresses are never stored.
use crate::{
    App,
    profiles::{self, Fail, Res},
    safety, security as sec,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgConnection;
use std::net::IpAddr;

/// Weights and thresholds. The defaults are the safe example values for tests and local
/// development; production loads its private values from `INTEGRITY_TUNING_FILE` (JSON).
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct Tuning {
    pub pending_seconds: f64,
    pub trusted_visible_seconds: f64,
    pub metronome_min_intervals: i32,
    pub metronome_jitter_ms: f64,
    pub metronome_risk: f32,
    pub network_limit: i64,
    pub network_risk: f32,
    pub exclude_risk: f32,
    pub trust_max_risk: f32,
    pub ahead_strikes_hard: i32,
    pub spike_min: i64,
    pub spike_factor: f64,
    pub spike_grace_seconds: i64,
    pub provisional_seconds: i64,
    pub case_min_raw: i64,
    pub case_excluded_share: f64,
    pub case_windows: i64,
}
impl Default for Tuning {
    fn default() -> Self {
        Self {
            pending_seconds: 60.0,
            trusted_visible_seconds: 120.0,
            metronome_min_intervals: 6,
            metronome_jitter_ms: 3.0,
            metronome_risk: 0.6,
            network_limit: 8,
            network_risk: 0.6,
            exclude_risk: 1.0,
            trust_max_risk: 0.5,
            ahead_strikes_hard: 2,
            spike_min: 20,
            spike_factor: 4.0,
            spike_grace_seconds: 600,
            provisional_seconds: 300,
            case_min_raw: 10,
            case_excluded_share: 0.5,
            case_windows: 3,
        }
    }
}
impl Tuning {
    pub fn from_env() -> std::result::Result<Self, String> {
        match std::env::var("INTEGRITY_TUNING_FILE") {
            Ok(path) if !path.is_empty() => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|_| "INTEGRITY_TUNING_FILE is not readable")?;
                serde_json::from_str(&text).map_err(|_| "INTEGRITY_TUNING_FILE is not valid".into())
            }
            _ => Ok(Self::default()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Level {
    Pending,
    Counted,
    Trusted,
    Excluded,
}
impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Pending => "pending",
            Level::Counted => "counted",
            Level::Trusted => "trusted",
            Level::Excluded => "excluded",
        }
    }
}

/// What one lease looks like to the scorer.
#[derive(Default)]
pub struct Inputs {
    pub age_seconds: f64,
    pub signed_in: bool,
    pub verified: bool,
    pub turnstile_ok: bool,
    pub hard: bool,
    pub provisional: bool,
    pub interval_count: i32,
    pub interval_mean: f64,
    pub interval_m2: f64,
    pub visible_seconds: f64,
    /// Live leases on the same broadcast from the same network prefix, this one included.
    pub same_network: i64,
}

/// No single soft signal excludes a viewer: exclusion needs risk from two signals (or hard
/// evidence). One soft signal only keeps a session from becoming Trusted.
pub fn assess(t: &Tuning, i: &Inputs) -> (Level, f32, Vec<&'static str>) {
    let mut risk = 0.0;
    let mut flags = Vec::new();
    if i.interval_count >= t.metronome_min_intervals.max(2) {
        let deviation = (i.interval_m2 / f64::from(i.interval_count - 1)).sqrt();
        if deviation < t.metronome_jitter_ms {
            risk += t.metronome_risk;
            flags.push("metronomic");
        }
    }
    if i.same_network > t.network_limit {
        risk += t.network_risk;
        flags.push("network_concentration");
    }
    if i.hard {
        flags.push("media_ahead_of_clock");
        return (Level::Excluded, risk, flags);
    }
    if !(i.turnstile_ok || (i.signed_in && i.verified)) {
        flags.push("check_pending");
        return (Level::Pending, risk, flags);
    }
    if i.provisional {
        flags.push("spike");
        return (Level::Pending, risk, flags);
    }
    if i.age_seconds < t.pending_seconds {
        return (Level::Pending, risk, flags);
    }
    if risk >= t.exclude_risk {
        return (Level::Excluded, risk, flags);
    }
    if i.signed_in
        && i.verified
        && i.visible_seconds >= t.trusted_visible_seconds
        && risk < t.trust_max_risk
    {
        (Level::Trusted, risk, flags)
    } else {
        (Level::Counted, risk, flags)
    }
}

/// The /24 (IPv4) or /48 (IPv6) network a viewer is on.
pub fn network(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v) => {
            let o = v.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            format!("{:x}:{:x}:{:x}::/48", s[0], s[1], s[2])
        }
    }
}
/// Keyed hash, re-keyed every 30 days so it can't be reversed or linked across months.
pub fn keyed(app: &App, label: &str, value: &str) -> String {
    let epoch = Utc::now().timestamp().div_euclid(30 * 86_400);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&app.config.key).expect("HMAC accepts any key length");
    mac.update(format!("viewer-integrity:{epoch}:{label}:{value}").as_bytes());
    URL_SAFE_NO_PAD.encode(&mac.finalize().into_bytes()[..16])
}

/// What a heartbeat reports besides its identity.
pub struct Report<'a> {
    pub visible: bool,
    pub media_time: Option<f64>,
    pub turnstile: Option<&'a str>,
}

#[derive(sqlx::FromRow)]
struct Lease {
    level: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    last_beat_at: Option<DateTime<Utc>>,
    turnstile_ok: bool,
    hard_excluded: bool,
    interval_count: i32,
    interval_mean: f64,
    interval_m2: f64,
    visible_seconds: f64,
    last_media_time: Option<f64>,
    ahead_strikes: i32,
    provisional_until: Option<DateTime<Utc>>,
}

/// Records one heartbeat and rescores its lease. None when the broadcast isn't public.
/// Returns the level and whether the viewer still needs to pass the security check.
#[allow(clippy::too_many_arguments)]
pub async fn record(
    app: &App,
    broadcast: &str,
    owner: &str,
    key: &str,
    signed_in: bool,
    verified: bool,
    ip: IpAddr,
    report: Report<'_>,
) -> Res<Option<(Level, bool)>> {
    let t = &app.config.integrity;
    let exempt = signed_in && verified;
    // Verified before the transaction so the external call never holds the lease lock.
    let passed = match report.turnstile {
        Some(token) if !exempt && !token.is_empty() => {
            sec::turnstile(app, token, "playback", ip).await.is_ok()
        }
        _ => false,
    };
    let mut tx = app.db.begin().await?;
    let live: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM broadcasts WHERE id=$1 AND owner_id=$2 AND state IN ('LIVE','RECONNECTING'))")
        .bind(broadcast).bind(owner).fetch_one(&mut *tx).await?;
    if !live {
        return Ok(None);
    }
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let previous: Option<Lease> = sqlx::query_as("SELECT level,created_at,expires_at,last_beat_at,turnstile_ok,hard_excluded,interval_count,interval_mean,interval_m2,visible_seconds,last_media_time,ahead_strikes,provisional_until FROM playback_leases WHERE broadcast_id=$1 AND viewer_key=$2 FOR UPDATE")
        .bind(broadcast).bind(key).fetch_optional(&mut *tx).await?;
    let mut l = previous.unwrap_or(Lease {
        level: "pending".into(),
        created_at: now,
        expires_at: now,
        last_beat_at: None,
        turnstile_ok: false,
        hard_excluded: false,
        interval_count: 0,
        interval_mean: 0.0,
        interval_m2: 0.0,
        visible_seconds: 0.0,
        last_media_time: None,
        ahead_strikes: 0,
        provisional_until: None,
    });
    // Timing only continues within a live lease; a beat after a lapse starts a new run.
    if let Some(last) = l.last_beat_at
        && l.expires_at > now
    {
        let elapsed = (now - last).num_milliseconds() as f64;
        l.interval_count += 1;
        let delta = elapsed - l.interval_mean;
        l.interval_mean += delta / f64::from(l.interval_count);
        l.interval_m2 += delta * (elapsed - l.interval_mean);
        if report.visible {
            l.visible_seconds += (elapsed / 1000.0).min(15.0);
        }
        // Live media can't play faster than the clock; a large jump ahead means fabricated
        // heartbeats. Going backwards (a transport switch or seek) is ignored.
        if let (Some(before), Some(after)) = (l.last_media_time, report.media_time)
            && after - before > elapsed / 1000.0 * 1.5 + 2.0
        {
            l.ahead_strikes += 1;
        }
    }
    if l.ahead_strikes >= t.ahead_strikes_hard {
        l.hard_excluded = true;
    }
    l.turnstile_ok |= passed;
    let net_hash = keyed(app, "net", &network(ip));
    let ip_hash = keyed(app, "ip", &ip.to_canonical().to_string());
    sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,created_at,expires_at,signed_in,verified,turnstile_ok,hard_excluded,ip_hash,net_hash,beats,last_beat_at,interval_count,interval_mean,interval_m2,visible_seconds,last_media_time,ahead_strikes) VALUES($1,$2,$3,$4+interval '30 seconds',$5,$6,$7,$8,$9,$10,1,$4,$11,$12,$13,$14,$15,$16) ON CONFLICT(broadcast_id,viewer_key) DO UPDATE SET expires_at=EXCLUDED.expires_at,signed_in=EXCLUDED.signed_in,verified=EXCLUDED.verified,turnstile_ok=EXCLUDED.turnstile_ok,hard_excluded=EXCLUDED.hard_excluded,ip_hash=EXCLUDED.ip_hash,net_hash=EXCLUDED.net_hash,beats=playback_leases.beats+1,last_beat_at=EXCLUDED.last_beat_at,interval_count=EXCLUDED.interval_count,interval_mean=EXCLUDED.interval_mean,interval_m2=EXCLUDED.interval_m2,visible_seconds=EXCLUDED.visible_seconds,last_media_time=EXCLUDED.last_media_time,ahead_strikes=EXCLUDED.ahead_strikes")
        .bind(broadcast).bind(key).bind(l.created_at).bind(now).bind(signed_in).bind(verified).bind(l.turnstile_ok).bind(l.hard_excluded)
        .bind(&ip_hash).bind(&net_hash).bind(l.interval_count).bind(l.interval_mean).bind(l.interval_m2).bind(l.visible_seconds).bind(report.media_time).bind(l.ahead_strikes)
        .execute(&mut *tx).await?;
    let same_network: i64 = sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE broadcast_id=$1 AND net_hash=$2 AND expires_at>now()")
        .bind(broadcast).bind(&net_hash).fetch_one(&mut *tx).await?;
    let inputs = Inputs {
        age_seconds: (now - l.created_at).num_milliseconds() as f64 / 1000.0,
        signed_in,
        verified,
        turnstile_ok: l.turnstile_ok,
        hard: l.hard_excluded,
        provisional: l.provisional_until.is_some_and(|p| p > now),
        interval_count: l.interval_count,
        interval_mean: l.interval_mean,
        interval_m2: l.interval_m2,
        visible_seconds: l.visible_seconds,
        same_network,
    };
    let (level, risk, flags) = assess(t, &inputs);
    save_level(&mut tx, broadcast, key, level, risk, &flags).await?;
    if level == Level::Trusted
        && l.level == "trusted"
        && l.expires_at > now
        && report.visible
        && let (Some(user), Some(last), Some(before), Some(after)) = (
            key.strip_prefix("u:"),
            l.last_beat_at,
            l.last_media_time,
            report.media_time,
        )
        && after > before
    {
        let elapsed = (now - last)
            .num_milliseconds()
            .min(((after - before) * 1000.0) as i64);
        crate::factions::watch(app, &mut tx, broadcast, user, elapsed, now).await?;
    }
    tx.commit().await?;
    Ok(Some((level, !exempt && !l.turnstile_ok)))
}

async fn save_level(
    db: &mut PgConnection,
    broadcast: &str,
    key: &str,
    level: Level,
    risk: f32,
    flags: &[&str],
) -> Res<()> {
    sqlx::query("UPDATE playback_leases SET level=$3,risk=$4,flags=$5,ever_counted=ever_counted OR $3 IN ('counted','trusted') WHERE broadcast_id=$1 AND viewer_key=$2")
        .bind(broadcast).bind(key).bind(level.name()).bind(risk).bind(flags)
        .execute(db).await?;
    Ok(())
}

/// Public Module 4 trust gate: owner previews never create leases, and expired leases never earn.
pub async fn trusted(db: &mut PgConnection, broadcast: &str, user: Option<&str>) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playback_leases WHERE broadcast_id=$1 AND level='trusted' AND signed_in AND verified AND expires_at>clock_timestamp() AND ($2::text IS NULL OR viewer_key='u:'||$2))")
        .bind(broadcast).bind(user).fetch_one(db).await?)
}

/// (raw, counted, trusted, excluded, pending) for a broadcast's live leases. "Counted" is the
/// public number and includes Trusted sessions.
pub async fn counts(db: &mut PgConnection, broadcast: &str) -> Res<(i64, i64, i64, i64, i64)> {
    Ok(sqlx::query_as("SELECT count(*), count(*) FILTER (WHERE level IN ('counted','trusted')), count(*) FILTER (WHERE level='trusted'), count(*) FILTER (WHERE level='excluded'), count(*) FILTER (WHERE level='pending') FROM playback_leases WHERE broadcast_id=$1 AND expires_at>now()")
        .bind(broadcast).fetch_one(db).await?)
}

#[derive(sqlx::FromRow)]
struct Scored {
    viewer_key: String,
    age_seconds: f64,
    signed_in: bool,
    verified: bool,
    turnstile_ok: bool,
    hard_excluded: bool,
    provisional: bool,
    interval_count: i32,
    interval_mean: f64,
    interval_m2: f64,
    visible_seconds: f64,
    same_network: i64,
}

/// Background pass, run with the other jobs: spike windows, rescoring (so sessions can recover),
/// per-minute snapshots, integrity cases and retention.
pub async fn tick(app: &App) -> Res<()> {
    let t = app.config.integrity.clone();
    let live: Vec<(String, String, f64)> = sqlx::query_as("SELECT id, owner_id, extract(epoch FROM now()-started_at)::float8 FROM broadcasts WHERE state IN ('LIVE','RECONNECTING')")
        .fetch_all(&app.db).await?;
    for (broadcast, owner, running) in live {
        let mut tx = app.db.begin().await?;
        // A burst of arrivals not explained by the stream just starting gets a provisional window.
        // Go-live alerts go out as the stream starts, inside this grace window.
        // Raids and committed MAGNet handoffs explain their arrival bursts.
        if running >= t.spike_grace_seconds as f64
            && !crate::raids::explains_burst(&mut tx, &broadcast).await?
            && !crate::magnet::explains_burst(&mut tx, &broadcast).await?
        {
            let (arrivals, baseline): (i64, i64) = sqlx::query_as("SELECT count(*) FILTER (WHERE created_at>now()-interval '60 seconds' AND provisional_until IS NULL), count(*) FILTER (WHERE created_at<=now()-interval '60 seconds' AND created_at>now()-interval '6 minutes') FROM playback_leases WHERE broadcast_id=$1")
                .bind(&broadcast).fetch_one(&mut *tx).await?;
            let per_minute = (baseline as f64 / 5.0).max(1.0);
            if arrivals >= t.spike_min && arrivals as f64 > t.spike_factor * per_minute {
                sqlx::query("UPDATE playback_leases SET provisional_until=now()+make_interval(secs=>$2) WHERE broadcast_id=$1 AND created_at>now()-interval '60 seconds' AND provisional_until IS NULL")
                    .bind(&broadcast).bind(t.provisional_seconds as f64).execute(&mut *tx).await?;
            }
        }
        // ponytail: one UPDATE per live lease every pass; batch it if audiences reach thousands.
        let leases: Vec<Scored> = sqlx::query_as("SELECT l.viewer_key, extract(epoch FROM now()-l.created_at)::float8 AS age_seconds, l.signed_in, l.verified, l.turnstile_ok, l.hard_excluded, coalesce(l.provisional_until>now(),false) AS provisional, l.interval_count, l.interval_mean, l.interval_m2, l.visible_seconds, (SELECT count(*) FROM playback_leases n WHERE n.broadcast_id=l.broadcast_id AND n.net_hash=l.net_hash AND n.expires_at>now()) AS same_network FROM playback_leases l WHERE l.broadcast_id=$1 AND l.expires_at>now()")
            .bind(&broadcast).fetch_all(&mut *tx).await?;
        for l in &leases {
            let inputs = Inputs {
                age_seconds: l.age_seconds,
                signed_in: l.signed_in,
                verified: l.verified,
                turnstile_ok: l.turnstile_ok,
                hard: l.hard_excluded,
                provisional: l.provisional,
                interval_count: l.interval_count,
                interval_mean: l.interval_mean,
                interval_m2: l.interval_m2,
                visible_seconds: l.visible_seconds,
                same_network: l.same_network,
            };
            let (level, risk, flags) = assess(&t, &inputs);
            save_level(&mut tx, &broadcast, &l.viewer_key, level, risk, &flags).await?;
        }
        let recent: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_snapshots WHERE broadcast_id=$1 AND taken_at>now()-interval '55 seconds')")
            .bind(&broadcast).fetch_one(&mut *tx).await?;
        if !recent {
            let (raw, counted, trusted, excluded, pending) = counts(&mut tx, &broadcast).await?;
            sqlx::query("INSERT INTO integrity_snapshots(broadcast_id,raw,counted,trusted,excluded,pending) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(&broadcast).bind(raw as i32).bind(counted as i32).bind(trusted as i32).bind(excluded as i32).bind(pending as i32)
                .execute(&mut *tx).await?;
            open_case(&mut tx, &t, &broadcast, &owner).await?;
        }
        tx.commit().await?;
    }
    // Retention: session rows 30 days after the broadcast ends, snapshots 1 year, closed cases
    // 1 year after their decision.
    sqlx::query("DELETE FROM playback_leases l USING broadcasts b WHERE b.id=l.broadcast_id AND b.ended_at<now()-interval '30 days'")
        .execute(&app.db).await?;
    sqlx::query("DELETE FROM integrity_snapshots WHERE taken_at<now()-interval '1 year'")
        .execute(&app.db)
        .await?;
    sqlx::query(
        "DELETE FROM integrity_cases WHERE status<>'OPEN' AND decided_at<now()-interval '1 year'",
    )
    .execute(&app.db)
    .await?;
    Ok(())
}

/// Opens a staff case when the excluded share stays high across the last few snapshots. At most
/// one case per broadcast every six hours, so a dismissed case isn't immediately reopened.
async fn open_case(db: &mut PgConnection, t: &Tuning, broadcast: &str, owner: &str) -> Res<()> {
    let windows: Vec<(i32, i32)> = sqlx::query_as("SELECT raw, excluded FROM integrity_snapshots WHERE broadcast_id=$1 ORDER BY taken_at DESC LIMIT $2")
        .bind(broadcast).bind(t.case_windows).fetch_all(&mut *db).await?;
    let sustained = windows.len() as i64 >= t.case_windows
        && windows.iter().all(|(raw, excluded)| {
            i64::from(*raw) >= t.case_min_raw
                && f64::from(*excluded) / f64::from(*raw) >= t.case_excluded_share
        });
    if !sustained {
        return Ok(());
    }
    let evidence: Value = sqlx::query_scalar("SELECT jsonb_build_object('windows',(SELECT jsonb_agg(jsonb_build_object('at',taken_at,'raw',raw,'counted',counted,'trusted',trusted,'excluded',excluded,'pending',pending) ORDER BY taken_at DESC) FROM (SELECT * FROM integrity_snapshots WHERE broadcast_id=$1 ORDER BY taken_at DESC LIMIT $2) s),'flags',(SELECT coalesce(jsonb_object_agg(flag,n),'{}') FROM (SELECT unnest(flags) AS flag, count(*) AS n FROM playback_leases WHERE broadcast_id=$1 AND expires_at>now() GROUP BY 1) f),'networks_excluded',(SELECT count(DISTINCT net_hash) FROM playback_leases WHERE broadcast_id=$1 AND level='excluded' AND expires_at>now()),'guests',(SELECT count(*) FROM playback_leases WHERE broadcast_id=$1 AND NOT signed_in AND expires_at>now()))")
        .bind(broadcast).bind(t.case_windows).fetch_one(&mut *db).await?;
    sqlx::query("INSERT INTO integrity_cases(id,broadcast_id,owner_id,evidence) SELECT $1,$2,$3,$4 WHERE NOT EXISTS(SELECT 1 FROM integrity_cases WHERE broadcast_id=$2 AND (status='OPEN' OR opened_at>now()-interval '6 hours'))")
        .bind(profiles::new_id()).bind(broadcast).bind(owner).bind(evidence)
        .execute(db).await?;
    Ok(())
}

/// For Creator Studio after a stream: viewers who watched about a minute or more but were never
/// counted. No identities, networks or reasons.
pub async fn last_broadcast(db: &mut PgConnection, owner: &str) -> Res<Value> {
    let row: Option<(String, DateTime<Utc>)> = sqlx::query_as("SELECT id, ended_at FROM broadcasts WHERE owner_id=$1 AND state='ENDED' ORDER BY ended_at DESC LIMIT 1")
        .bind(owner).fetch_optional(&mut *db).await?;
    let Some((id, ended)) = row else {
        return Ok(Value::Null);
    };
    let not_counted: i64 = sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE broadcast_id=$1 AND NOT ever_counted AND beats>=6")
        .bind(&id).fetch_one(&mut *db).await?;
    Ok(json!({"ended_at": ended, "not_counted": not_counted}))
}

/// GET /api/admin/integrity: open cases first, then recent decisions.
pub async fn admin_cases(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let _staff = safety::staff(&app, &jar).await?;
    let cases: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',c.id,'username',u.username,'broadcast_id',c.broadcast_id,'opened_at',c.opened_at,'status',c.status,'evidence',c.evidence,'hold_payouts',c.hold_payouts,'pause_tier',c.pause_tier,'note',c.note,'decided_at',c.decided_at,'decided_by',(SELECT username FROM users WHERE id=c.decided_by)) FROM integrity_cases c JOIN users u ON u.id=c.owner_id ORDER BY c.status='OPEN' DESC, c.opened_at DESC LIMIT 100")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"cases": cases})))
}

#[derive(Deserialize)]
pub struct Decision {
    outcome: String,
    note: Option<String>,
    #[serde(default)]
    hold_payouts: bool,
    #[serde(default)]
    pause_tier: bool,
}
/// POST /api/admin/integrity/{id}/decision: staff only. Dismissing records nothing against the
/// streamer; "action" records payout and tier holds for Module 6. Strikes go through the user's
/// standing page so they keep the normal appeal path.
pub async fn decide(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let (status, hold, pause) = match input.outcome.as_str() {
        "dismiss" => ("DISMISSED", false, false),
        "action" => ("ACTIONED", input.hold_payouts, input.pause_tier),
        _ => return Err(Fail::field("outcome", "Choose dismiss or action.")),
    };
    let mut tx = app.db.begin().await?;
    let owner: String = sqlx::query_scalar("UPDATE integrity_cases SET status=$2,hold_payouts=$3,pause_tier=$4,note=$5,decided_by=$6,decided_at=now() WHERE id=$1 AND status='OPEN' RETURNING owner_id")
        .bind(&id).bind(status).bind(hold).bind(pause).bind(&note).bind(&actor.id)
        .fetch_optional(&mut *tx).await?
        .ok_or_else(|| Fail::conflict("This case has already been decided."))?;
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "integrity_case_decided",
        "user",
        &owner,
        &[],
        &note,
        json!({"case_id": id, "outcome": status, "hold_payouts": hold, "pause_tier": pause}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"status": status})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/admin/integrity", get(admin_cases))
        .route("/api/admin/integrity/{id}/decision", post(decide))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted_guest() -> Inputs {
        Inputs {
            age_seconds: 90.0,
            turnstile_ok: true,
            same_network: 1,
            ..Inputs::default()
        }
    }

    #[test]
    fn levels() {
        let t = Tuning::default();
        let level = |i: &Inputs| assess(&t, i).0;
        assert_eq!(level(&Inputs::default()), Level::Pending, "unchecked guest");
        assert_eq!(
            level(&Inputs {
                age_seconds: 30.0,
                ..counted_guest()
            }),
            Level::Pending,
            "first minute"
        );
        assert_eq!(level(&counted_guest()), Level::Counted);
        assert_eq!(
            level(&Inputs {
                provisional: true,
                ..counted_guest()
            }),
            Level::Pending,
            "spike window"
        );
        // Signed-in verified viewers skip the check but become Trusted only after two visible minutes.
        let member = Inputs {
            signed_in: true,
            verified: true,
            age_seconds: 90.0,
            same_network: 1,
            ..Inputs::default()
        };
        assert_eq!(level(&member), Level::Counted);
        assert_eq!(
            level(&Inputs {
                visible_seconds: 120.0,
                ..member
            }),
            Level::Trusted
        );
        // A signed-in but unverified viewer still needs the check and is never Trusted.
        assert_eq!(
            level(&Inputs {
                signed_in: true,
                age_seconds: 300.0,
                visible_seconds: 300.0,
                ..Inputs::default()
            }),
            Level::Pending
        );
        assert_eq!(
            level(&Inputs {
                hard: true,
                ..counted_guest()
            }),
            Level::Excluded
        );
    }

    #[test]
    fn one_signal_never_excludes_two_do() {
        let t = Tuning::default();
        // Ten beats a few ms apart: metronomic. Natural browser jitter is far wider.
        let metronomic = Inputs {
            interval_count: 10,
            interval_mean: 10_000.0,
            interval_m2: 9.0 * 1.0,
            ..counted_guest()
        };
        let natural = Inputs {
            interval_count: 10,
            interval_m2: 9.0 * 2500.0,
            ..counted_guest()
        };
        assert_eq!(assess(&t, &natural).0, Level::Counted);
        let (level, _, flags) = assess(&t, &metronomic);
        assert_eq!(
            (level, flags.contains(&"metronomic")),
            (Level::Counted, true),
            "one signal: still counted"
        );
        // A shared household or school network is normal.
        assert_eq!(
            assess(
                &t,
                &Inputs {
                    same_network: 8,
                    ..counted_guest()
                }
            )
            .0,
            Level::Counted
        );
        let both = Inputs {
            same_network: 40,
            ..metronomic
        };
        assert_eq!(assess(&t, &both).0, Level::Excluded);
        // Recovery: once the concentration drops, the session counts again.
        assert_eq!(
            assess(
                &t,
                &Inputs {
                    same_network: 2,
                    ..both
                }
            )
            .0,
            Level::Counted
        );
        // One soft signal blocks Trusted.
        let member = Inputs {
            signed_in: true,
            verified: true,
            visible_seconds: 600.0,
            ..metronomic
        };
        assert_eq!(assess(&t, &member).0, Level::Counted);
    }

    #[test]
    fn networks() {
        assert_eq!(network("203.0.113.77".parse().unwrap()), "203.0.113.0/24");
        assert_eq!(
            network("::ffff:203.0.113.9".parse().unwrap()),
            "203.0.113.0/24"
        );
        assert_eq!(
            network("2001:db8:abcd:12::1".parse().unwrap()),
            "2001:db8:abcd::/48"
        );
    }
}
