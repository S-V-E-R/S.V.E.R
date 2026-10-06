//! Payouts (docs/SUPPORT.md "Payouts"): payday every 2 weeks pays the creator's whole available
//! balance by Stripe transfer to their Express account; between paydays Early Pay withdraws up to
//! 75% of what was earned since the last payday (minus earlier Early Pay), at most once a day,
//! standard (free) or instant (Stripe's 1% fee, paid by the creator). The remaining 25% covers
//! refunds and disputes; a negative balance is recovered from later earnings before any payout.
use crate::{
    App, ledger,
    profiles::{self, Fail, Res},
    security as sec, stripe,
};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::sync::atomic::{AtomicI64, Ordering};

pub const EARLY_PERCENT: i64 = 75;
pub const INSTANT_FEE_PERCENT: i64 = 1;
const PERIOD_DAYS: i64 = 14;

/// The first payday: Friday, October 9, 2026, noon Eastern; then every 14 days.
fn anchor() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 9, 16, 0, 0).unwrap()
}
/// The most recent payday at or before `now` (None before the first).
pub fn last_payday(now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let anchor = anchor();
    (now >= anchor)
        .then(|| anchor + Duration::days(PERIOD_DAYS * ((now - anchor).num_days() / PERIOD_DAYS)))
}
pub fn next_payday(now: DateTime<Utc>) -> DateTime<Utc> {
    last_payday(now).map_or_else(anchor, |p| p + Duration::days(PERIOD_DAYS))
}

fn earnings(user: &str) -> String {
    format!("usd:earnings:{user}")
}
/// The creator's Express account, when payouts are enabled on it.
async fn account(db: &mut PgConnection, user: &str) -> Res<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT stripe_account FROM payout_accounts WHERE user_id=$1 AND payouts_enabled",
    )
    .bind(user)
    .fetch_optional(db)
    .await?)
}
/// An open staff integrity case can hold payouts.
async fn held(db: &mut PgConnection, user: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_cases WHERE owner_id=$1 AND status='OPEN' AND hold_payouts)")
        .bind(user).fetch_one(db).await?)
}
/// Whole cents payable now: earnings (rounded down only here) less payouts still pending.
async fn available(db: &mut PgConnection, user: &str) -> Res<i64> {
    let balance = ledger::balance(db, &earnings(user), "usd").await?;
    let pending: i64 = sqlx::query_scalar("SELECT coalesce(sum(amount_cents+fee_cents),0)::bigint FROM payout_runs WHERE user_id=$1 AND status='pending'")
        .bind(user).fetch_one(db).await?;
    Ok((balance.div_euclid(10) - pending).max(0))
}
/// (Early Pay limit in cents, whether today's Early Pay is used).
async fn early_limit(db: &mut PgConnection, user: &str) -> Res<(i64, bool)> {
    // Before the first payday, everything earned so far counts.
    let since = last_payday(Utc::now());
    let earned: i64 = sqlx::query_scalar("SELECT coalesce(sum(e.amount),0)::bigint FROM ledger_entries e JOIN ledger_transactions t ON t.id=e.transaction_id WHERE e.account=$1 AND e.unit='usd' AND e.amount>0 AND ($2::timestamptz IS NULL OR t.created_at>=$2)")
        .bind(earnings(user)).bind(since).fetch_one(&mut *db).await?;
    let used: i64 = sqlx::query_scalar("SELECT coalesce(sum(amount_cents+fee_cents),0)::bigint FROM payout_runs WHERE user_id=$1 AND kind LIKE 'early%' AND status<>'failed' AND ($2::timestamptz IS NULL OR created_at>=$2)")
        .bind(user).bind(since).fetch_one(&mut *db).await?;
    let today: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_runs WHERE user_id=$1 AND kind LIKE 'early%' AND status<>'failed' AND created_at>=date_trunc('day', now() AT TIME ZONE 'America/New_York') AT TIME ZONE 'America/New_York')")
        .bind(user).fetch_one(&mut *db).await?;
    let cap = earned * EARLY_PERCENT / 100 / 10 - used;
    Ok((cap.min(available(db, user).await?).max(0), today))
}

/// Records a pending run (it counts against limits at once), under the caller's payout lock.
async fn reserve(
    db: &mut PgConnection,
    user: &str,
    kind: &str,
    cents: i64,
    fee: i64,
) -> Res<String> {
    let id = profiles::new_id();
    sqlx::query(
        "INSERT INTO payout_runs(id,user_id,kind,amount_cents,fee_cents) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(&id)
    .bind(user)
    .bind(kind)
    .bind(cents)
    .bind(fee)
    .execute(db)
    .await?;
    Ok(id)
}
/// Moves the money: a transfer to the Express account (idempotent by run), then for instant
/// Early Pay an instant payout from it. Ledger entries post only after Stripe accepts.
async fn execute(
    app: &App,
    id: &str,
    user: &str,
    account: &str,
    kind: &str,
    cents: i64,
    fee: i64,
) -> Res<()> {
    let transfer = stripe::post(
        &app.http,
        &app.config.stripe,
        "transfers",
        &[
            ("amount", cents.to_string()),
            ("currency", "usd".into()),
            ("destination", account.to_string()),
            ("description", "S.V.E.R creator earnings".into()),
            ("metadata[sver_payout]", id.to_string()),
        ],
        Some(&format!("sver-payout-{id}")),
    )
    .await;
    let transfer = match transfer {
        Ok(t) => t,
        Err(error) => {
            sqlx::query(
                "UPDATE payout_runs SET status='failed',error=$2,completed_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(error.message.as_ref())
            .execute(&app.db)
            .await?;
            eprintln!("payout_event=transfer outcome=failed kind={kind}");
            return Err(error);
        }
    };
    let (mut fee, mut payout, mut note) = (fee, None, None);
    if kind == "early_instant" {
        match stripe::post_as(
            &app.http,
            &app.config.stripe,
            Some(account),
            "payouts",
            &[
                ("amount", cents.to_string()),
                ("currency", "usd".into()),
                ("method", "instant".into()),
            ],
            Some(&format!("sver-instant-{id}")),
        )
        .await
        {
            Ok(p) => payout = p["id"].as_str().map(str::to_owned),
            // No fee for an instant payout that didn't happen; the transfer pays out as standard.
            Err(_) => {
                fee = 0;
                note = Some(
                    "Instant payout isn't available for this account, so the money arrives by standard payout and no fee was charged.",
                );
            }
        }
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE payout_runs SET status='paid',transfer_id=$2,payout_id=$3,fee_cents=$4,error=$5,completed_at=now() WHERE id=$1")
        .bind(id).bind(transfer["id"].as_str()).bind(&payout).bind(fee).bind(note)
        .execute(&mut *tx).await?;
    ledger::post(
        &mut tx,
        "payout",
        &format!("payout:{id}"),
        json!({"run": id, "kind": kind, "cents": cents, "fee_cents": fee, "transfer": transfer["id"]}),
        &[
            (&earnings(user), "usd", -(cents + fee) * 10),
            ("usd:paid_out", "usd", cents * 10),
            ("usd:platform", "usd", fee * 10),
        ],
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

static LAST_MINUTE: AtomicI64 = AtomicI64::new(0);
/// From the media loop: starts the payday run once its period begins.
pub async fn tick(app: &App) -> Res<()> {
    let now = Utc::now().timestamp();
    if now - LAST_MINUTE.load(Ordering::Relaxed) < 60 {
        return Ok(());
    }
    LAST_MINUTE.store(now, Ordering::Relaxed);
    match last_payday(Utc::now()) {
        Some(period) => payday(app, period).await,
        None => Ok(()),
    }
}
/// Pays every enabled, unheld creator their whole available balance, once per period.
// ponytail: a crash mid-run leaves the rest of that period unpaid until the next payday (their
// balances carry over); add a resumable run if that ever matters.
pub async fn payday(app: &App, period: DateTime<Utc>) -> Res<()> {
    let started = sqlx::query(
        "INSERT INTO support_runs(kind,period) VALUES('payday',$1) ON CONFLICT DO NOTHING",
    )
    .bind(period)
    .execute(&app.db)
    .await?
    .rows_affected();
    if started == 0 {
        return Ok(());
    }
    let creators: Vec<(String, String)> =
        sqlx::query_as("SELECT user_id,stripe_account FROM payout_accounts WHERE payouts_enabled")
            .fetch_all(&app.db)
            .await?;
    let mut paid = 0;
    for (user, account) in creators {
        let mut tx = app.db.begin().await?;
        ledger::lock(&mut tx, &format!("payout:{user}")).await?;
        if held(&mut tx, &user).await? {
            continue;
        }
        let cents = available(&mut tx, &user).await?;
        if cents < 1 {
            continue;
        }
        let id = reserve(&mut tx, &user, "payday", cents, 0).await?;
        tx.commit().await?;
        if execute(app, &id, &user, &account, "payday", cents, 0)
            .await
            .is_ok()
        {
            paid += 1;
        }
    }
    eprintln!("payout_event=payday paid={paid}");
    Ok(())
}

async fn summary_json(app: &App, user: &str) -> Res<Value> {
    let mut db = app.db.acquire().await?;
    let (limit, today) = early_limit(&mut db, user).await?;
    let pending: i64 = sqlx::query_scalar("SELECT coalesce(sum(amount_cents+fee_cents),0)::bigint FROM payout_runs WHERE user_id=$1 AND status='pending'")
        .bind(user).fetch_one(&mut *db).await?;
    let history: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'kind',kind,'amount_cents',amount_cents,'fee_cents',fee_cents,'status',status,'error',error,'created_at',created_at) FROM payout_runs WHERE user_id=$1 ORDER BY created_at DESC LIMIT 20")
        .bind(user).fetch_all(&mut *db).await?;
    Ok(json!({
        "available_cents": available(&mut db, user).await?,
        "pending_cents": pending,
        "next_payday": next_payday(Utc::now()),
        "early": {"limit_cents": limit, "used_today": today, "percent": EARLY_PERCENT, "instant_fee_percent": INSTANT_FEE_PERCENT},
        "enabled": account(&mut db, user).await?.is_some(),
        "held": held(&mut db, user).await?,
        "history": history,
    }))
}
/// GET /api/me/payouts/summary: balance, next payday, Early Pay and history.
async fn summary(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    Ok(Json(summary_json(&app, &user.id).await?))
}

#[derive(Deserialize)]
pub struct Early {
    method: String,
}
/// POST /api/me/payouts/early: withdraws the whole Early Pay limit now.
async fn early(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Early>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let kind = match input.method.as_str() {
        "standard" => "early_standard",
        "instant" => "early_instant",
        _ => return Err(Fail::field("method", "Choose standard or instant.")),
    };
    sec::reserve(&app, vec![format!("early-pay:{}", user.id)], 5, 3600).await?;
    let mut tx = app.db.begin().await?;
    ledger::lock(&mut tx, &format!("payout:{}", user.id)).await?;
    let Some(account) = account(&mut tx, &user.id).await? else {
        return Err(Fail::conflict("Finish payout setup first."));
    };
    if held(&mut tx, &user.id).await? {
        return Err(Fail::denied(
            "Payouts are on hold while a staff review is open.",
        ));
    }
    let (limit, today) = early_limit(&mut tx, &user.id).await?;
    if today {
        return Err(Fail::conflict("You've already used Early Pay today."));
    }
    // The instant fee comes out of the withdrawal: 1%, rounded up to the cent.
    let fee = if kind == "early_instant" {
        (limit * INSTANT_FEE_PERCENT + 99) / 100
    } else {
        0
    };
    let cents = limit - fee;
    if cents < 1 {
        return Err(Fail::conflict(
            "Nothing is available for Early Pay right now.",
        ));
    }
    let id = reserve(&mut tx, &user.id, kind, cents, fee).await?;
    tx.commit().await?;
    execute(&app, &id, &user.id, &account, kind, cents, fee).await?;
    Ok(Json(summary_json(&app, &user.id).await?))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/payouts/summary", get(summary))
        .route("/api/me/payouts/early", post(early))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paydays_every_two_weeks() {
        let first = anchor();
        assert_eq!(last_payday(first - Duration::seconds(1)), None);
        assert_eq!(next_payday(first - Duration::days(3)), first);
        assert_eq!(last_payday(first + Duration::days(13)), Some(first));
        let second = first + Duration::days(14);
        assert_eq!(last_payday(second), Some(second));
        assert_eq!(
            next_payday(second + Duration::hours(1)),
            first + Duration::days(28)
        );
    }
}
