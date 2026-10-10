//! Phase 4 ads, design first (docs/ADS.md; Joe's decision October 10, 2026: no ads and no provider
//! yet). Staff run test ad breaks on a live broadcast and settle them with synthetic revenue; the
//! split (streamer by creator tier, 5% to viewers as Ad Valor, the rest to S.V.E.R) posts to the
//! ledger's separate test book, whose accounts start with `adtest:` and never reach payouts.
use crate::{
    App, ledger,
    profiles::{Fail, Res, new_id},
    safety, tiers,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};

/// The ledger book for test settlements. A live book needs a provider, Joe's approval and its own
/// migration (the `ad_segments.book` check allows only 'test').
const BOOK: &str = "adtest";
/// Viewers' share of ad revenue, in percent (docs/SUPPORT.md "Creator tiers").
const VIEWER_PERCENT: i64 = 5;
/// The largest synthetic revenue staff can settle one segment with, in cents.
const MAX_TEST_CENTS: i64 = 100_000;

/// Splits revenue (tenths of a cent): the streamer's tier share, an equal share of the viewers'
/// pool per viewer, and S.V.E.R keeps the rest, including rounding and the pool when nobody counted.
pub fn split(revenue: i64, streamer_percent: i64, viewers: i64) -> (i64, i64, i64) {
    let streamer = revenue * streamer_percent / 100;
    let each = if viewers > 0 {
        revenue * VIEWER_PERCENT / 100 / viewers
    } else {
        0
    };
    (streamer, each, revenue - streamer - each * viewers)
}

async fn view(app: &App) -> Res<Json<Value>> {
    let segments: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(x ORDER BY x->>'created_at' DESC),'[]') FROM (SELECT jsonb_build_object('id',s.id,'channel',u.username,'created_at',s.created_at,'viewers',(SELECT count(*) FROM ad_segment_viewers v WHERE v.segment_id=s.id),'revenue',s.revenue,'split',s.split,'settled_at',s.settled_at) x FROM ad_segments s JOIN users u ON u.id=s.owner_id ORDER BY s.created_at DESC LIMIT 50) t")
        .fetch_one(&app.db).await?;
    let balances: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(jsonb_build_object('account',regexp_replace(b.account,'^adtest:usd:(adv|earnings):.*$','\\1'),'user',u.username,'balance',b.balance) ORDER BY b.balance DESC),'[]') FROM (SELECT account,sum(amount)::bigint balance FROM ledger_entries WHERE account LIKE 'adtest:usd:adv:%' OR account LIKE 'adtest:usd:earnings:%' GROUP BY account) b JOIN users u ON u.id=regexp_replace(b.account,'^adtest:usd:(adv|earnings):','')")
        .fetch_one(&app.db).await?;
    Ok(Json(
        json!({"book": BOOK, "segments": segments, "balances": balances, "max_test_cents": MAX_TEST_CENTS}),
    ))
}
/// GET /api/admin/ads
async fn ads(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    view(&app).await
}

#[derive(Deserialize)]
pub struct Break {
    username: String,
}
/// POST /api/admin/ads/test-breaks: starts a test ad break on a live channel and snapshots who
/// counts (trusted, signed-in viewers who aren't the owner and aren't subscribed).
async fn start(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Break>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let live: Option<(String, String)> = sqlx::query_as("SELECT b.id,b.owner_id FROM broadcasts b JOIN users u ON u.id=b.owner_id WHERE lower(u.username)=lower($1) AND b.ended_at IS NULL AND b.state IN ('LIVE','RECONNECTING') ORDER BY b.started_at DESC LIMIT 1")
        .bind(input.username.trim()).fetch_optional(&app.db).await?;
    let (broadcast, owner) = live.ok_or_else(|| Fail::conflict("That channel isn't live."))?;
    let id = new_id();
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO ad_segments(id,broadcast_id,owner_id,created_by) VALUES($1,$2,$3,$4)")
        .bind(&id)
        .bind(&broadcast)
        .bind(&owner)
        .bind(&staff.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO ad_segment_viewers(segment_id,user_id) SELECT $1,u.id FROM playback_leases l JOIN users u ON l.viewer_key='u:'||u.id
        WHERE l.broadcast_id=$2 AND l.level='trusted' AND l.expires_at>now() AND u.id<>$3 AND u.deleted_at IS NULL
        AND NOT EXISTS(SELECT 1 FROM channel_subs s WHERE s.channel_id=$3 AND s.user_id=u.id AND s.paid_through>now())")
        .bind(&id).bind(&broadcast).bind(&owner).execute(&mut *tx).await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "ad_test_break",
        "ad_segment",
        &id,
        &[],
        "Test ad break",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    view(&app).await
}

#[derive(Deserialize)]
pub struct Settle {
    revenue_cents: i64,
}
/// POST /api/admin/ads/test-breaks/{id}/settle: posts the split to the test book, once.
async fn settle(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Settle>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    if !(1..=MAX_TEST_CENTS).contains(&input.revenue_cents) {
        return Err(Fail::field(
            "revenue_cents",
            "Use 1¢ to $1,000 of test revenue.",
        ));
    }
    let mut tx = app.db.begin().await?;
    let owner: String = sqlx::query_scalar(
        "SELECT owner_id FROM ad_segments WHERE id=$1 AND settled_at IS NULL FOR UPDATE",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| Fail::conflict("That break is already settled or doesn't exist."))?;
    let viewers: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM ad_segment_viewers WHERE segment_id=$1")
            .bind(&id)
            .fetch_all(&mut *tx)
            .await?;
    let revenue = input.revenue_cents * 10;
    let percent = tiers::split(&mut tx, &owner).await?;
    let (streamer, each, platform) = split(revenue, percent, viewers.len() as i64);
    let accounts: Vec<String> = viewers
        .iter()
        .map(|v| format!("{BOOK}:usd:adv:{v}"))
        .collect();
    let (earnings, source, keep) = (
        format!("{BOOK}:usd:earnings:{owner}"),
        format!("{BOOK}:usd:ads"),
        format!("{BOOK}:usd:platform"),
    );
    let mut entries: Vec<(&str, &str, i64)> = vec![
        (&source, "usd", -revenue),
        (&earnings, "usd", streamer),
        (&keep, "usd", platform),
    ];
    entries.extend(accounts.iter().map(|a| (a.as_str(), "usd", each)));
    let detail = json!({"segment": id, "channel": owner, "revenue": revenue, "streamer_percent": percent, "viewers": viewers.len(), "per_viewer": each, "platform": platform});
    ledger::post(
        &mut tx,
        "adtest_settlement",
        &format!("{BOOK}:{id}"),
        detail.clone(),
        &entries,
    )
    .await?;
    sqlx::query("UPDATE ad_segments SET revenue=$2,split=$3,settled_at=now() WHERE id=$1")
        .bind(&id)
        .bind(revenue)
        .bind(&detail)
        .execute(&mut *tx)
        .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "ad_test_settle",
        "ad_segment",
        &id,
        &[],
        "Test ad settlement",
        detail,
        false,
    )
    .await?;
    tx.commit().await?;
    view(&app).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/admin/ads", get(ads))
        .route("/api/admin/ads/test-breaks", post(start))
        .route("/api/admin/ads/test-breaks/{id}/settle", post(settle))
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn splits_by_tier_with_rounding_to_the_platform() {
        // $10.00 at Scout (65%) across 3 viewers: 6500 + 3×166 + 3002 = 10000.
        assert_eq!(split(10_000, 65, 3), (6_500, 166, 3_002));
        // Nobody counted: the viewers' pool stays with S.V.E.R.
        assert_eq!(split(10_000, 80, 0), (8_000, 0, 2_000));
        let (s, e, p) = split(12_345, 75, 7);
        assert_eq!(s + e * 7 + p, 12_345);
    }
}
