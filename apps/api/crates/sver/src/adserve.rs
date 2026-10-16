//! Ad network step 2 (docs/ADVERTISERS.md): approved ads served in streamer breaks, the Beacons
//! feed and the site slot; pacing so every campaign finishes on time (and runs on past its end
//! date until it's whole); single-use impression tickets that count only real, watching people;
//! and the streamer's ads switch and breaks. Money moves per verified impression in step 3.
use crate::{
    App, auth,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    response::Redirect,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::net::SocketAddr;

/// Break lengths a streamer can start, and the gap between one break's end and the next start.
const BREAK_SECONDS: [i64; 3] = [30, 60, 90];
const BREAK_GAP_MINUTES: i64 = 10;
/// Visible, advancing time before an impression counts: 2 s of video, 1 s of a picture.
const VIDEO_MS: i64 = 2000;
const IMAGE_MS: i64 = 1000;
/// A ticket is decided within this long or expires.
const TICKET_MINUTES: i64 = 10;
/// Per person: the same ad at most once per 10 minutes, one campaign at most 3 times a day.
const REPEAT_MINUTES: i64 = 10;
const DAILY_CAP: i64 = 3;

fn browser_ok(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}
fn viewer_key(user: Option<&auth::User>, browser: &str) -> String {
    match user {
        Some(user) => format!("u:{}", user.id),
        None => format!("b:{}", sec::digest(browser)),
    }
}

/// Pacing: how far a campaign is behind the even schedule that finishes on its end date, as a
/// share of its order. Past its end date it always goes first (the make-good). None means it has
/// met today's target, or hasn't started, and waits.
pub fn behind(
    impressions: i64,
    delivered: i64,
    starts: NaiveDate,
    ends: NaiveDate,
    today: NaiveDate,
) -> Option<f64> {
    if delivered >= impressions || today < starts {
        return None;
    }
    if today > ends {
        return Some(f64::INFINITY);
    }
    let days = (ends - starts).num_days() + 1;
    let through_today = (today - starts).num_days() + 1;
    let target = (impressions as f64 * through_today as f64 / days as f64).ceil();
    ((delivered as f64) < target).then(|| (target - delivered as f64) / impressions as f64)
}

/// A viewer's lease on a broadcast counts toward ads only at Counted or Trusted.
async fn counted(db: &mut PgConnection, broadcast: &str, key: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playback_leases WHERE broadcast_id=$1 AND viewer_key=$2 AND expires_at>now() AND level IN ('counted','trusted'))")
        .bind(broadcast).bind(key).fetch_one(db).await?)
}
/// The break running on a channel's live broadcast, if any: (break, broadcast, end).
async fn running_break(
    db: &mut PgConnection,
    channel: &str,
) -> Res<Option<(String, String, DateTime<Utc>)>> {
    Ok(sqlx::query_as("SELECT a.id,a.broadcast_id,a.ends_at FROM ad_breaks a JOIN broadcasts b ON b.id=a.broadcast_id AND b.state IN ('LIVE','RECONNECTING') WHERE a.channel_id=$1 AND a.ends_at>now() ORDER BY a.starts_at DESC LIMIT 1")
        .bind(channel).fetch_optional(db).await?)
}
/// For /live: the running break, for anyone but the streamer and the channel's subscribers.
pub async fn for_live(app: &App, channel: &str, viewer: Option<&str>) -> Res<Value> {
    if viewer == Some(channel) {
        return Ok(Value::Null);
    }
    if let Some(v) = viewer
        && crate::subs::active_tier(app, channel, v).await?.is_some()
    {
        return Ok(Value::Null);
    }
    Ok(
        match running_break(&mut *app.db.acquire().await?, channel).await? {
            Some((id, _, ends)) => json!({"id": id, "ends_at": ends}),
            None => Value::Null,
        },
    )
}

#[derive(Deserialize)]
struct ServeInput {
    placement: String,
    browser_id: String,
    #[serde(default)]
    channel: Option<String>,
}
#[derive(sqlx::FromRow)]
struct Candidate {
    id: String,
    creative_id: String,
    impressions: i64,
    delivered: i64,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
    audience: String,
    teen_safe: bool,
}
/// A signed-in account in good standing (feed and site impressions need one).
async fn standing(db: &mut PgConnection, user: &auth::User) -> Res<bool> {
    Ok(user.email_verified
        && profiles::channel_user_by_id(db, &user.id)
            .await?
            .is_some_and(|c| c.eligible))
}

/// POST /api/ads/serve: picks the approved ad most behind its schedule that fits this viewer and
/// place, and hands it over with a single-use ticket. `{"ad": null}` when nothing should show.
async fn serve(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<ServeInput>,
) -> Res<Json<Value>> {
    if !browser_ok(&input.browser_id)
        || !matches!(input.placement.as_str(), "break" | "feed" | "site")
    {
        return Err(Fail::bad("Invalid ad request."));
    }
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("ad-serve:{ip}")], 120, 60).await?;
    let user = profiles::viewer(&app, &jar).await?;
    let key = viewer_key(user.as_ref(), &input.browser_id);
    sec::reserve(&app, vec![format!("ad-serve-viewer:{key}")], 20, 60).await?;
    let none = || Ok(Json(json!({"ad": null})));
    let mut db = app.db.acquire().await?;
    // Context: who's watching, where, and what the place is about.
    type Context = (Option<String>, Option<String>, Vec<String>, Vec<String>);
    let (channel, break_id, genres, languages): Context = if input.placement == "break" {
        let Some(name) = input.channel.as_deref() else {
            return Err(Fail::bad("Invalid ad request."));
        };
        let Some(owner) = profiles::eligible_by_name(&mut db, name).await? else {
            return none();
        };
        let Some((id, broadcast, _)) = running_break(&mut db, &owner.id).await? else {
            return none();
        };
        if let Some(u) = &user
            && (u.id == owner.id
                || crate::moderation::banned(&app, &owner.id, &u.id).await?
                || crate::subs::active_tier(&app, &owner.id, &u.id)
                    .await?
                    .is_some())
        {
            return none();
        }
        if !counted(&mut db, &broadcast, &key).await? {
            return none();
        }
        let (genre, language): (Option<String>, Option<String>) = sqlx::query_as("SELECT c.genre,s.language FROM stream_settings s LEFT JOIN stream_categories c ON c.id=s.category_id WHERE s.owner_id=$1")
            .bind(&owner.id).fetch_optional(&mut *db).await?.unwrap_or((None, None));
        (
            Some(owner.id),
            Some(id),
            genre.into_iter().collect(),
            language.into_iter().collect(),
        )
    } else {
        let Some(u) = &user else { return none() };
        if !standing(&mut db, u).await? {
            return none();
        }
        // ponytail: "genres the viewer watches" is read from the channels they follow; switch to
        // watch history once it's recorded per viewer.
        let genres: Vec<String> = sqlx::query_scalar("SELECT DISTINCT c.genre FROM follows f JOIN stream_settings s ON s.owner_id=f.following_id JOIN stream_categories c ON c.id=s.category_id WHERE f.follower_id=$1 AND c.genre IS NOT NULL")
            .bind(&u.id).fetch_all(&mut *db).await?;
        let languages: Vec<String> =
            sqlx::query_scalar("SELECT coalesce(languages,'{}') FROM users WHERE id=$1")
                .bind(&u.id)
                .fetch_one(&mut *db)
                .await?;
        (None, None, genres, languages)
    };
    let age = match &user {
        Some(u) => crate::support::age(&mut db, &u.id).await?,
        None => None,
    };
    let adult = age.is_some_and(|a| a >= 18);
    sqlx::query("INSERT INTO ad_opportunities(day,placement,count) VALUES(current_date,$1,1) ON CONFLICT(day,placement) DO UPDATE SET count=ad_opportunities.count+1")
        .bind(&input.placement).execute(&mut *db).await?;
    // REPEAT_MINUTES and DAILY_CAP are constants; values are bound.
    let candidates: Vec<Candidate> = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT c.id,c.creative_id,c.impressions::bigint AS impressions,c.delivered::bigint AS delivered,c.starts_on,c.ends_on,c.audience,a.teen_safe FROM ad_campaigns c JOIN ad_creatives a ON a.id=c.creative_id AND a.status='approved' WHERE c.status='running' AND c.placement=$1 AND c.delivered<c.impressions AND (cardinality(c.genres)=0 OR c.genres && $2) AND (cardinality(c.languages)=0 OR c.languages && $3) AND NOT EXISTS(SELECT 1 FROM ad_serves s WHERE s.viewer_key=$4 AND s.creative_id=c.creative_id AND s.served_at>now()-interval '{REPEAT_MINUTES} minutes' AND s.outcome IS DISTINCT FROM 'rejected') AND (SELECT count(*) FROM ad_serves s WHERE s.viewer_key=$4 AND s.campaign_id=c.id AND s.served_at>now()-interval '1 day' AND s.outcome IS DISTINCT FROM 'rejected')<{DAILY_CAP}")))
        .bind(&input.placement).bind(&genres).bind(&languages).bind(&key)
        .fetch_all(&mut *db).await?;
    let today = Utc::now().date_naive();
    // Adults see any approved ad their targeting allows; anyone else only teen-safe, all-ages ads.
    let chosen = candidates
        .into_iter()
        .filter(|c| adult || (c.audience == "all" && c.teen_safe))
        .filter_map(|c| {
            behind(c.impressions, c.delivered, c.starts_on, c.ends_on, today).map(|b| (b, c))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let Some((_, campaign)) = chosen else {
        return none();
    };
    let id = profiles::new_id();
    sqlx::query("INSERT INTO ad_serves(id,campaign_id,creative_id,placement,viewer_key,user_id,channel_id,break_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&id).bind(&campaign.id).bind(&campaign.creative_id).bind(&input.placement).bind(&key)
        .bind(user.as_ref().map(|u| &u.id)).bind(&channel).bind(&break_id)
        .execute(&mut *db).await?;
    let ad = crate::adnet::creative_media(&app, &mut db, &campaign.creative_id).await?;
    let video = ad["video"].is_string();
    Ok(Json(json!({"ad": {
        "ticket": id, "kind": if video { "video" } else { "image" },
        "src": if video { ad["video"].clone() } else { ad["picture"].clone() },
        "poster": ad["picture"], "title": ad["title"], "advertiser": ad["advertiser"],
        "click": format!("/api/ads/click/{id}"), "min_ms": if video { VIDEO_MS } else { IMAGE_MS },
        "duration_ms": ad["duration_ms"],
    }})))
}

#[derive(Deserialize)]
struct Report {
    ticket: String,
    browser_id: String,
    visible_ms: i64,
    #[serde(default)]
    visible: bool,
}
type Serve = (
    String,
    String,
    String,
    String,
    Option<String>,
    DateTime<Utc>,
    Option<String>,
    String,
);
/// POST /api/ads/impressions: the player's report for a ticket. The server decides once whether
/// it was a real, watching person: the ticket is fresh and theirs, the time has really passed, the
/// session still counts, and the frequency caps hold. Only then does the campaign count it.
async fn impression(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Report>,
) -> Res<Json<Value>> {
    if !browser_ok(&input.browser_id) {
        return Err(Fail::bad("Invalid ad report."));
    }
    let user = profiles::viewer(&app, &jar).await?;
    let key = viewer_key(user.as_ref(), &input.browser_id);
    let mut tx = app.db.begin().await?;
    let serve: Option<Serve> = sqlx::query_as("SELECT s.campaign_id,s.creative_id,s.placement,s.viewer_key,s.break_id,s.served_at,s.outcome,a.format FROM ad_serves s JOIN ad_creatives a ON a.id=s.creative_id WHERE s.id=$1 FOR UPDATE OF s")
        .bind(&input.ticket).fetch_optional(&mut *tx).await?;
    let Some((campaign, creative, placement, owner_key, break_id, served_at, outcome, format)) =
        serve
    else {
        return Err(Fail::missing());
    };
    if owner_key != key {
        return Err(Fail::missing());
    }
    if outcome.is_some() {
        return Ok(Json(json!({"counted": false, "reason": "duplicate"})));
    }
    let min = if format.starts_with("video") {
        VIDEO_MS
    } else {
        IMAGE_MS
    };
    let age_ms = (Utc::now() - served_at).num_milliseconds();
    let session = match placement.as_str() {
        "break" => {
            let broadcast: Option<String> =
                sqlx::query_scalar("SELECT broadcast_id FROM ad_breaks WHERE id=$1")
                    .bind(&break_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            match broadcast {
                Some(b) => counted(&mut tx, &b, &key).await?,
                None => false,
            }
        }
        _ => match &user {
            Some(u) => standing(&mut tx, u).await?,
            None => false,
        },
    };
    // REPEAT_MINUTES is a constant; values are bound.
    let repeats: (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT count(*) FILTER (WHERE creative_id=$2 AND decided_at>now()-interval '{REPEAT_MINUTES} minutes'),count(*) FILTER (WHERE campaign_id=$3 AND decided_at>now()-interval '1 day') FROM ad_serves WHERE viewer_key=$1 AND outcome='verified'")))
        .bind(&key).bind(&creative).bind(&campaign).fetch_one(&mut *tx).await?;
    let mut reason = if age_ms > TICKET_MINUTES * 60_000 {
        Some("expired")
    } else if !input.visible {
        Some("hidden")
    } else if input.visible_ms < min || age_ms < min {
        Some("too_short")
    } else if !session {
        Some("unverified_session")
    } else if repeats.0 > 0 || repeats.1 >= DAILY_CAP {
        Some("frequency_cap")
    } else {
        None
    };
    if reason.is_none() {
        let counted: Option<(i32, i32, i32)> = sqlx::query_as("UPDATE ad_campaigns SET delivered=delivered+1,status=CASE WHEN delivered+1>=impressions THEN 'delivered' ELSE status END,updated_at=now() WHERE id=$1 AND status='running' AND delivered<impressions RETURNING delivered,impressions,amount_cents")
            .bind(&campaign).fetch_optional(&mut *tx).await?;
        match counted {
            Some((k, impressions, amount)) => {
                let (channel, user_id): (Option<String>, Option<String>) =
                    sqlx::query_as("SELECT channel_id,user_id FROM ad_serves WHERE id=$1")
                        .bind(&input.ticket)
                        .fetch_one(&mut *tx)
                        .await?;
                let value = impression_value(amount.into(), impressions.into(), k.into());
                settle(
                    &mut tx,
                    &input.ticket,
                    &campaign,
                    &placement,
                    value,
                    channel.as_deref(),
                    user_id.as_deref(),
                    break_id.as_deref(),
                    &key,
                )
                .await?;
            }
            None => reason = Some("campaign_full"),
        }
    }
    sqlx::query("UPDATE ad_serves SET outcome=$2,reason=$3,decided_at=now() WHERE id=$1")
        .bind(&input.ticket)
        .bind(if reason.is_none() {
            "verified"
        } else {
            "rejected"
        })
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"counted": reason.is_none(), "reason": reason})))
}

/// The k-th impression's value in tenths of a cent: the running total's step, so a fully delivered
/// campaign moves exactly what was paid.
pub fn impression_value(amount_cents: i64, impressions: i64, k: i64) -> i64 {
    let total = |n: i64| amount_cents * 10 * n / impressions.max(1);
    total(k) - total(k - 1)
}
/// Splits one verified impression (docs/ADVERTISERS.md "The split"): its value leaves the
/// campaign's prepaid account as ad revenue; the streamer's tier share (breaks, when they can earn)
/// and the viewer's 5% (signed in, 13 or older, and Trusted for breaks) move out of the platform's
/// share as for subscriptions; rounding stays with S.V.E.R.
#[allow(clippy::too_many_arguments)]
async fn settle(
    db: &mut PgConnection,
    serve: &str,
    campaign: &str,
    placement: &str,
    value: i64,
    channel: Option<&str>,
    viewer: Option<&str>,
    break_id: Option<&str>,
    key: &str,
) -> Res<()> {
    let prepaid = format!("usd:ads:prepaid:{campaign}");
    let mut entries: Vec<(String, i64)> = vec![(prepaid, value), ("usd:ads".into(), -value)];
    let mut streamer_share = 0;
    if placement == "break"
        && let Some(channel) = channel
        && crate::support::can_earn(db, channel).await?
    {
        let percent = crate::tiers::split(db, channel).await?;
        streamer_share = value * percent / 100;
        entries.push((format!("usd:earnings:{channel}"), streamer_share));
    }
    let mut viewer_share = 0;
    if let Some(user) = viewer {
        let teen_or_older = crate::support::age(db, user)
            .await?
            .is_some_and(|a| a >= 13);
        let trusted = match break_id {
            Some(id) => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playback_leases l JOIN ad_breaks b ON b.broadcast_id=l.broadcast_id WHERE b.id=$1 AND l.viewer_key=$2 AND l.level='trusted' AND l.expires_at>now())")
                .bind(id).bind(key).fetch_one(&mut *db).await?,
            None => true,
        };
        if teen_or_older && trusted {
            viewer_share = value * 5 / 100;
            entries.push((format!("usd:adv:{user}"), viewer_share));
        }
    }
    entries.push(("usd:platform".into(), -(streamer_share + viewer_share)));
    let borrowed: Vec<(&str, &str, i64)> = entries
        .iter()
        .map(|(a, n)| (a.as_str(), "usd", *n))
        .collect();
    crate::ledger::post(
        db,
        "ad_impression",
        &format!("ad:{serve}"),
        json!({"campaign": campaign, "placement": placement, "channel": channel, "viewer": viewer,
            "value": value, "streamer": streamer_share, "viewers": viewer_share}),
        &borrowed,
    )
    .await?;
    Ok(())
}

/// GET /api/ads/click/{ticket}: counts the click once, then goes to the advertiser's page.
async fn click(State(app): State<App>, Path(ticket): Path<String>) -> Res<Redirect> {
    let link: Option<String> = sqlx::query_scalar("UPDATE ad_serves s SET clicked_at=coalesce(s.clicked_at,now()) FROM ad_creatives a WHERE s.id=$1 AND a.id=s.creative_id RETURNING a.link")
        .bind(&ticket).fetch_optional(&app.db).await?;
    link.map(|l| Redirect::to(&l)).ok_or_else(Fail::missing)
}

// ---- Streamers ----

/// An account's ad money: the total and the last 30 days, in tenths of a cent.
async fn earnings(db: &mut PgConnection, account: &str) -> Res<Value> {
    let total: i64 = sqlx::query_scalar("SELECT coalesce(sum(e.amount),0)::bigint FROM ledger_entries e JOIN ledger_transactions t ON t.id=e.transaction_id WHERE e.account=$1 AND e.unit='usd' AND t.kind='ad_impression'")
        .bind(account).fetch_one(&mut *db).await?;
    let days: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('day',d,'tenths',tenths,'impressions',n) FROM (SELECT t.created_at::date d,sum(e.amount)::bigint tenths,count(*) n FROM ledger_entries e JOIN ledger_transactions t ON t.id=e.transaction_id WHERE e.account=$1 AND e.unit='usd' AND t.kind='ad_impression' AND t.created_at>now()-interval '30 days' GROUP BY 1 ORDER BY 1 DESC) x")
        .bind(account).fetch_all(&mut *db).await?;
    Ok(json!({"total_tenths": total, "days": days}))
}
/// GET /api/me/ad-valor: the viewer's Ad Valor (their 5% of the ads they saw).
async fn ad_valor(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let account = format!("usd:adv:{}", user.id);
    Ok(Json(json!({
        "balance_tenths": crate::ledger::balance(&mut db, &account, "usd").await?,
        "earned": earnings(&mut db, &account).await?,
    })))
}
/// The staff view of ad money: gross, streamers, viewers and S.V.E.R by placement, and why
/// impressions were rejected. For /api/admin/adnet.
pub(crate) async fn revenue(db: &mut PgConnection) -> Res<Value> {
    let placements: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('placement',t.detail->>'placement','impressions',count(*),'gross',sum((t.detail->>'value')::bigint),'streamers',sum((t.detail->>'streamer')::bigint),'viewers',sum((t.detail->>'viewers')::bigint)) FROM ledger_transactions t WHERE t.kind='ad_impression' GROUP BY t.detail->>'placement' ORDER BY 1")
        .fetch_all(&mut *db).await?;
    let rejected: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('reason',reason,'count',count(*)) FROM ad_serves WHERE outcome='rejected' AND decided_at>now()-interval '30 days' GROUP BY reason ORDER BY count(*) DESC")
        .fetch_all(&mut *db).await?;
    Ok(json!({"placements": placements, "rejected": rejected}))
}

/// GET /api/me/ads: the ads switch, whether the channel can earn, and recent breaks.
async fn studio(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let enabled: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT enabled FROM channel_ads WHERE channel_id=$1),false)",
    )
    .bind(&user.id)
    .fetch_one(&mut *db)
    .await?;
    let breaks: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',b.id,'starts_at',b.starts_at,'ends_at',b.ends_at,'impressions',(SELECT count(*) FROM ad_serves s WHERE s.break_id=b.id AND s.outcome='verified')) FROM ad_breaks b WHERE b.channel_id=$1 ORDER BY b.starts_at DESC LIMIT 10")
        .bind(&user.id).fetch_all(&mut *db).await?;
    // BREAK_GAP_MINUTES is a constant; values are bound.
    let next: Option<DateTime<Utc>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT max(ends_at)+interval '{BREAK_GAP_MINUTES} minutes' FROM ad_breaks WHERE channel_id=$1"
    )))
    .bind(&user.id)
    .fetch_one(&mut *db)
    .await?;
    Ok(Json(json!({
        "enabled": enabled, "can_earn": crate::support::can_earn(&mut db, &user.id).await?,
        "live": crate::streams::live_broadcast(&mut db, &user.id).await?.is_some(),
        "running": running_break(&mut db, &user.id).await?.map(|(id, _, ends)| json!({"id": id, "ends_at": ends})),
        "next_break_at": next.filter(|n| *n > Utc::now()), "lengths": BREAK_SECONDS, "breaks": breaks,
        "earnings": earnings(&mut db, &format!("usd:earnings:{}", user.id)).await?,
    })))
}
#[derive(Deserialize)]
struct Switch {
    enabled: bool,
}
/// PUT /api/me/ads: ads are off until the streamer turns them on; only channels that can earn can.
async fn switch(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Switch>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    if input.enabled && !crate::support::can_earn(&mut db, &user.id).await? {
        return Err(Fail::denied(
            "Set up payouts first: ad earnings go to your payout balance.",
        ));
    }
    sqlx::query("INSERT INTO channel_ads(channel_id,enabled) VALUES($1,$2) ON CONFLICT(channel_id) DO UPDATE SET enabled=$2,updated_at=now()")
        .bind(&user.id).bind(input.enabled).execute(&mut *db).await?;
    Ok(Json(json!({"saved": true})))
}
#[derive(Deserialize)]
struct NewBreak {
    seconds: i64,
}
/// POST /api/me/ads/breaks: the streamer starts a 30, 60 or 90-second break on their live
/// broadcast. Never automatic, never a pre-roll; at least 10 minutes between breaks.
async fn start_break(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewBreak>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if !BREAK_SECONDS.contains(&input.seconds) {
        return Err(Fail::field(
            "seconds",
            "Choose a 30, 60 or 90-second break.",
        ));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ad_break:'||$1,0))")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let enabled: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT enabled FROM channel_ads WHERE channel_id=$1),false)",
    )
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    if !enabled || !crate::support::can_earn(&mut tx, &user.id).await? {
        return Err(Fail::denied("Turn ads on in Creator Studio first."));
    }
    let Some(broadcast) = crate::streams::live_broadcast(&mut tx, &user.id).await? else {
        return Err(Fail::conflict("Go live first."));
    };
    // BREAK_GAP_MINUTES is a constant; values are bound.
    let recent: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT EXISTS(SELECT 1 FROM ad_breaks WHERE channel_id=$1 AND ends_at>now()-interval '{BREAK_GAP_MINUTES} minutes')")))
        .bind(&user.id).fetch_one(&mut *tx).await?;
    if recent {
        return Err(Fail::conflict(
            "Leave at least 10 minutes between ad breaks.",
        ));
    }
    let id = profiles::new_id();
    let ends: DateTime<Utc> = sqlx::query_scalar("INSERT INTO ad_breaks(id,channel_id,broadcast_id,ends_at) VALUES($1,$2,$3,now()+make_interval(secs=>$4)) RETURNING ends_at")
        .bind(&id).bind(&user.id).bind(&broadcast).bind(input.seconds as f64)
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id": id, "ends_at": ends})))
}

/// Starts paid campaigns whose ad is approved and start date has come, and expires tickets that
/// were never reported. Called by the jobs loop.
pub async fn tick(app: &App) -> Res<()> {
    sqlx::query("UPDATE ad_campaigns c SET status='running',updated_at=now() WHERE status='paid' AND starts_on<=current_date AND EXISTS(SELECT 1 FROM ad_creatives a WHERE a.id=c.creative_id AND a.status='approved')")
        .execute(&app.db).await?;
    // TICKET_MINUTES is a constant.
    sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE ad_serves SET outcome='rejected',reason='expired',decided_at=now() WHERE outcome IS NULL AND served_at<now()-interval '{TICKET_MINUTES} minutes'")))
        .execute(&app.db).await?;
    Ok(())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/ads/serve", post(serve))
        .route("/api/ads/impressions", post(impression))
        .route("/api/ads/click/{ticket}", get(click))
        .route("/api/me/ads", get(studio).put(switch))
        .route("/api/me/ads/breaks", post(start_break))
        .route("/api/me/ad-valor", get(ad_valor))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pacing_serves_whoever_is_furthest_behind() {
        let d = |n: i64| NaiveDate::from_ymd_opt(2026, 11, 1).unwrap() + chrono::Duration::days(n);
        // 1,000 over 10 days: 100 is today's target on day one.
        assert_eq!(behind(1000, 0, d(0), d(9), d(0)), Some(0.1));
        assert_eq!(
            behind(1000, 100, d(0), d(9), d(0)),
            None,
            "on schedule: waits"
        );
        assert_eq!(behind(1000, 450, d(0), d(9), d(4)), Some(0.05));
        assert_eq!(
            behind(1000, 999, d(0), d(9), d(12)),
            Some(f64::INFINITY),
            "past its end date: first"
        );
        assert_eq!(behind(1000, 1000, d(0), d(9), d(12)), None, "finished");
        assert_eq!(behind(1000, 0, d(3), d(9), d(0)), None, "not started");
    }
    #[test]
    fn impressions_move_exactly_what_was_paid() {
        for (amount, impressions) in [(1000, 2000), (1001, 3000), (500, 1000), (777_777, 123_457)] {
            let total: i64 = (1..=impressions)
                .map(|k| impression_value(amount, impressions, k))
                .sum();
            assert_eq!(total, amount * 10, "{amount} cents over {impressions}");
        }
    }
}
