//! The staff console's Money page (docs/ADMIN.md "Money"): read-only views of payments,
//! subscriptions, tributes, payouts, refunds and chargebacks, negative balances, guardian accounts
//! and tax-form status; refunds through Stripe (the refund webhook posts the reversing entries); and
//! Valor adjustments as double-entry ledger rows with a reason. Card and bank details never show.
use crate::{
    App, ledger,
    profiles::{Fail, Res, new_id},
    safety, stripe,
};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};

/// The largest Valor adjustment staff can post at once.
/// ponytail: one cap and one approver; add a second approval above a private cap once someone
/// besides Joe can approve (docs/ADMIN.md).
const MAX_ADJUST: i64 = 100_000;

async fn view(app: &App) -> Res<Json<Value>> {
    let q = |sql: &'static str| sqlx::query_scalar::<_, Value>(sql).fetch_one(&app.db);
    let payments = q("SELECT coalesce(jsonb_agg(x ORDER BY x->>'created_at' DESC),'[]') FROM (SELECT jsonb_build_object('session',s.id,'kind',s.kind,'cents',s.amount_cents,'status',s.status,'payment_intent',s.payment_intent,'created_at',s.created_at,'user',u.username) x FROM checkout_sessions s LEFT JOIN users u ON u.id=s.user_id ORDER BY s.created_at DESC LIMIT 50) t").await?;
    let invoices = q("SELECT coalesce(jsonb_agg(x ORDER BY x->>'created_at' DESC),'[]') FROM (SELECT jsonb_build_object('channel',c.username,'user',u.username,'tier',i.tier,'cents',i.amount_cents,'payment_intent',i.payment_intent,'created_at',i.created_at) x FROM sub_invoices i LEFT JOIN users c ON c.id=i.channel_id LEFT JOIN users u ON u.id=i.user_id ORDER BY i.created_at DESC LIMIT 50) t").await?;
    let subscriptions = q("SELECT jsonb_build_object('active',count(*),'by_card',count(*) FILTER (WHERE stripe_subscription IS NOT NULL),'tier1',count(*) FILTER (WHERE tier=1),'tier2',count(*) FILTER (WHERE tier=2),'tier3',count(*) FILTER (WHERE tier=3)) FROM channel_subs WHERE paid_through>now()").await?;
    let tributes = q("SELECT coalesce(jsonb_agg(x ORDER BY x->>'created_at' DESC),'[]') FROM (SELECT jsonb_build_object('valor',(t.detail->>'valor')::bigint,'from',u.username,'channel',c.username,'created_at',t.created_at) x FROM ledger_transactions t LEFT JOIN users u ON u.id=t.detail->>'from' LEFT JOIN users c ON c.id=t.detail->>'channel' WHERE t.kind='tribute' ORDER BY t.created_at DESC LIMIT 50) t").await?;
    let payouts = q("SELECT coalesce(jsonb_agg(x ORDER BY x->>'created_at' DESC),'[]') FROM (SELECT jsonb_build_object('user',u.username,'kind',p.kind,'cents',p.amount_cents,'fee_cents',p.fee_cents,'status',p.status,'error',p.error,'created_at',p.created_at) x FROM payout_runs p JOIN users u ON u.id=p.user_id ORDER BY p.created_at DESC LIMIT 50) t").await?;
    let reversals = q("SELECT coalesce(jsonb_agg(x ORDER BY x->>'created_at' DESC),'[]') FROM (SELECT jsonb_build_object('kind',kind,'payment_intent',detail->>'payment_intent','cents',coalesce((detail->>'refunded_cents')::bigint,(detail->>'cents')::bigint),'created_at',created_at) x FROM ledger_transactions WHERE kind IN ('refund','valor_refund','dispute','dispute_won') ORDER BY created_at DESC LIMIT 50) t").await?;
    // Valor and earnings accounts are named by user ID; staff see the username.
    let negative = q("SELECT coalesce(jsonb_agg(jsonb_build_object('user',u.username,'unit',b.unit,'balance',b.balance) ORDER BY b.balance),'[]') FROM (SELECT account,unit,sum(amount)::bigint AS balance FROM ledger_entries WHERE account LIKE 'valor:%' OR account LIKE 'usd:earnings:%' GROUP BY account,unit HAVING sum(amount)<0) b JOIN users u ON u.id=regexp_replace(b.account,'^(valor|usd:earnings):','')").await?;
    let accounts = q("SELECT coalesce(jsonb_agg(jsonb_build_object('user',u.username,'guardian',p.guardian,'tax_complete',p.details_submitted,'payouts_enabled',p.payouts_enabled) ORDER BY u.username),'[]') FROM payout_accounts p JOIN users u ON u.id=p.user_id").await?;
    Ok(Json(json!({
        "payments": payments, "invoices": invoices, "subscriptions": subscriptions, "tributes": tributes,
        "payouts": payouts, "reversals": reversals, "negative": negative, "accounts": accounts,
        "max_adjust": MAX_ADJUST,
    })))
}
/// GET /api/admin/money
async fn money(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    view(&app).await
}

#[derive(Deserialize)]
pub struct Refund {
    payment_intent: String,
    note: Option<String>,
}
/// POST /api/admin/money/refund: a full refund through Stripe of a payment S.V.E.R took. The
/// `charge.refunded` webhook posts the reversing ledger entries, as for a refund from Stripe.
async fn refund(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Refund>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let ours: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM checkout_sessions WHERE payment_intent=$1 AND status='paid') OR EXISTS(SELECT 1 FROM sub_invoices WHERE payment_intent=$1)")
        .bind(&input.payment_intent).fetch_one(&app.db).await?;
    if !ours {
        return Err(Fail::missing());
    }
    stripe::post(
        &app.http,
        &app.config.stripe,
        "refunds",
        &[("payment_intent", input.payment_intent.clone())],
        Some(&format!("staff-refund-{}", input.payment_intent)),
    )
    .await?;
    let mut tx = app.db.begin().await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "refund",
        "payment",
        &input.payment_intent,
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    view(&app).await
}

#[derive(Deserialize)]
pub struct Adjust {
    username: String,
    valor: i64,
    note: Option<String>,
}
/// POST /api/admin/money/valor: adds or removes Valor with a reason, as a balanced ledger movement
/// against the adjustments account. Never an edit to a balance.
async fn adjust(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Adjust>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    if input.valor == 0 || input.valor.abs() > MAX_ADJUST {
        return Err(Fail::field(
            "valor",
            "Adjust by 1 to 100,000 Valor, up or down.",
        ));
    }
    let user: String = sqlx::query_scalar(
        "SELECT id FROM users WHERE lower(username)=lower($1) AND deleted_at IS NULL",
    )
    .bind(input.username.trim())
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Fail::missing)?;
    if user == staff.id {
        return Err(Fail::denied("Staff can't adjust their own Valor."));
    }
    let mut tx = app.db.begin().await?;
    ledger::lock(&mut tx, &format!("valor:{user}")).await?;
    let id = new_id();
    ledger::post(
        &mut tx,
        "valor_adjustment",
        &format!("adjust:{id}"),
        json!({"user": user, "valor": input.valor, "by": staff.id}),
        &[
            (&format!("valor:{user}"), "valor", input.valor),
            ("valor:adjustments", "valor", -input.valor),
        ],
    )
    .await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "valor_adjustment",
        "user",
        &user,
        &[],
        &note,
        json!({"valor": input.valor}),
        false,
    )
    .await?;
    tx.commit().await?;
    view(&app).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/admin/money", get(money))
        .route("/api/admin/money/refund", post(refund))
        .route("/api/admin/money/valor", post(adjust))
}
