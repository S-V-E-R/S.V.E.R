//! Module 6 Support, part 1 (docs/SUPPORT.md): payout setup on Stripe Connect Express, Purchased
//! Valor by Stripe Checkout, and the Stripe webhook that turns payments, refunds and disputes into
//! ledger entries. Tributes are chat messages (chat::send_from) that spend Valor.
use crate::{
    App, ledger,
    profiles::{self, Fail, Res},
    security as sec, stripe,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::HeaderMap,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// (price in cents, Valor): docs/SUPPORT.md "Purchased Valor".
pub const PACKS: [(i64, i64); 7] = [
    (99, 100),
    (199, 200),
    (499, 525),
    (999, 1_075),
    (1_999, 2_200),
    (4_999, 5_600),
    (9_999, 11_500),
];
/// Purchases by an account aged 13 to 17 are capped at $50 a calendar month.
const MINOR_CAP_CENTS: i64 = 5_000;

/// Age in whole years from the date of birth; None when it isn't on file.
async fn age(db: &mut PgConnection, user: &str) -> Res<Option<i32>> {
    Ok(sqlx::query_scalar(
        "SELECT date_part('year', age(date_of_birth))::int FROM users WHERE id=$1",
    )
    .bind(user)
    .fetch_one(db)
    .await?)
}

/// Who can earn: verified, authenticator 2FA, and Stripe onboarding (with the tax form) finished.
pub async fn can_earn(db: &mut PgConnection, owner: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_accounts p JOIN users u ON u.id=p.user_id WHERE p.user_id=$1 AND p.details_submitted AND u.email_verified AND u.mfa_enabled AND u.deleted_at IS NULL)")
        .bind(owner).fetch_one(db).await?)
}

// ---- Wallet and Valor packs ----

async fn month_cents(db: &mut PgConnection, user: &str) -> Res<i64> {
    // Open sessions count too (they expire after 30 minutes), so two tabs can't pass the cap.
    Ok(sqlx::query_scalar("SELECT COALESCE(sum(amount_cents),0)::bigint FROM checkout_sessions WHERE user_id=$1 AND status IN ('open','paid') AND created_at>=date_trunc('month', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'")
        .bind(user).fetch_one(db).await?)
}

/// Credits the buyer's paid checkouts by asking Stripe directly, so a late or failed webhook
/// never leaves a buyer waiting. Posting is idempotent by session, so this and the webhook can't
/// both credit. Stripe errors are ignored here; the webhook remains the other path.
async fn reconcile(app: &App, user: &str) -> Res<()> {
    let open: Vec<String> = sqlx::query_scalar("SELECT id FROM checkout_sessions WHERE user_id=$1 AND status='open' AND created_at>now()-interval '1 day' ORDER BY created_at DESC LIMIT 5")
        .bind(user).fetch_all(&app.db).await?;
    for id in open {
        let Ok(session) = stripe::get(
            &app.http,
            &app.config.stripe,
            &format!("checkout/sessions/{id}"),
        )
        .await
        else {
            continue;
        };
        let mut tx = app.db.begin().await?;
        match (
            session["status"].as_str(),
            session["payment_status"].as_str(),
        ) {
            (_, Some("paid")) => paid(&mut tx, &session).await?,
            (Some("expired"), _) => process(&mut tx, "checkout.session.expired", &session).await?,
            _ => continue,
        }
        tx.commit().await?;
    }
    Ok(())
}

pub async fn wallet(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    reconcile(&app, &user.id).await?;
    let mut db = app.db.acquire().await?;
    let valor = ledger::balance(&mut db, &format!("valor:{}", user.id), "valor").await?;
    let minor = age(&mut db, &user.id).await?.is_some_and(|a| a < 18);
    let guardian: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guardian_consents WHERE user_id=$1)")
            .bind(&user.id)
            .fetch_one(&mut *db)
            .await?;
    let spent = month_cents(&mut db, &user.id).await?;
    Ok(Json(json!({
        "valor": valor,
        "locked": valor < 0,
        "available": app.config.stripe.available(),
        "packs": PACKS.iter().map(|(cents, valor)| json!({"cents":cents,"valor":valor})).collect::<Vec<_>>(),
        "minor": minor,
        "guardian_confirmed": guardian,
        "month_cents": if minor { Some(spent) } else { None },
        "cap_cents": if minor { Some(MINOR_CAP_CENTS) } else { None },
    })))
}

#[derive(Deserialize)]
pub struct Buy {
    cents: i64,
    #[serde(default)]
    guardian_consent: bool,
}
/// Starts a Stripe Checkout for one Valor pack. Valor is credited only by the webhook.
pub async fn checkout(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Buy>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to buy Valor.")?;
    let Some(&(cents, valor)) = PACKS.iter().find(|p| p.0 == input.cents) else {
        return Err(Fail::field("cents", "Choose one of the Valor packs."));
    };
    sec::reserve(&app, vec![format!("valor-checkout:{}", user.id)], 10, 3600).await?;
    let mut tx = app.db.begin().await?;
    // One checkout at a time per buyer, so concurrent tabs can't slip past the monthly cap.
    ledger::lock(&mut tx, &format!("checkout:{}", user.id)).await?;
    match age(&mut tx, &user.id).await? {
        Some(a) if a < 13 => return Err(Fail::denied("You can't buy Valor on this account.")),
        Some(a) if a < 18 => {
            let confirmed: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM guardian_consents WHERE user_id=$1)",
            )
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
            if !confirmed && !input.guardian_consent {
                return Err(Fail::field(
                    "guardian_consent",
                    "A parent or guardian must confirm they are the cardholder and consent to this purchase.",
                ));
            }
            if month_cents(&mut tx, &user.id).await? + cents > MINOR_CAP_CENTS {
                return Err(Fail::field(
                    "cents",
                    "Accounts under 18 can spend up to $50 a month. Choose a smaller pack or wait until next month.",
                ));
            }
            sqlx::query("INSERT INTO guardian_consents(user_id) VALUES($1) ON CONFLICT DO NOTHING")
                .bind(&user.id)
                .execute(&mut *tx)
                .await?;
        }
        _ => {}
    }
    let origin = &app.config.origin;
    let expires = chrono::Utc::now().timestamp() + 31 * 60;
    let session = stripe::post(
        &app.http,
        &app.config.stripe,
        "checkout/sessions",
        &[
            ("mode", "payment".into()),
            ("line_items[0][quantity]", "1".into()),
            ("line_items[0][price_data][currency]", "usd".into()),
            ("line_items[0][price_data][unit_amount]", cents.to_string()),
            (
                "line_items[0][price_data][product_data][name]",
                format!("{valor} Valor"),
            ),
            ("client_reference_id", user.id.clone()),
            ("metadata[sver_kind]", "valor".into()),
            ("payment_intent_data[metadata][sver_user]", user.id.clone()),
            ("expires_at", expires.to_string()),
            ("success_url", format!("{origin}/wallet?checkout=done")),
            ("cancel_url", format!("{origin}/wallet")),
        ],
        Some(&format!("valor-checkout-{}", profiles::new_id())),
    )
    .await?;
    let (Some(id), Some(url)) = (session["id"].as_str(), session["url"].as_str()) else {
        return Err(Fail::unavailable("Stripe sent an unexpected response."));
    };
    sqlx::query("INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,valor) VALUES($1,$2,'valor',$3,$4)")
        .bind(id).bind(&user.id).bind(cents as i32).bind(valor as i32)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"url": url})))
}

// ---- Payout setup (Stripe Connect Express) ----

#[derive(sqlx::FromRow)]
struct Account {
    stripe_account: String,
    guardian: bool,
    details_submitted: bool,
    payouts_enabled: bool,
    requirements: Value,
}
async fn account(db: &mut PgConnection, user: &str) -> Res<Option<Account>> {
    Ok(sqlx::query_as("SELECT stripe_account,guardian,details_submitted,payouts_enabled,requirements FROM payout_accounts WHERE user_id=$1")
        .bind(user).fetch_optional(db).await?)
}
/// Copies a Stripe Account object's onboarding state (from a webhook or a direct read).
async fn sync_account(db: &mut PgConnection, object: &Value) -> Res<()> {
    sqlx::query("UPDATE payout_accounts SET details_submitted=$2,payouts_enabled=$3,requirements=$4,updated_at=now() WHERE stripe_account=$1")
        .bind(object["id"].as_str().unwrap_or_default())
        .bind(object["details_submitted"].as_bool().unwrap_or(false))
        .bind(object["payouts_enabled"].as_bool().unwrap_or(false))
        .bind(match &object["requirements"]["currently_due"] {
            Value::Array(due) => Value::Array(due.clone()),
            _ => json!([]),
        })
        .execute(db).await?;
    Ok(())
}

pub async fn payouts(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let mut current = account(&mut db, &user.id).await?;
    // Returning from onboarding: read the account now instead of waiting for the webhook.
    if let Some(a) = current.as_ref().filter(|a| !a.payouts_enabled)
        && let Ok(object) = stripe::get(
            &app.http,
            &app.config.stripe,
            &format!("accounts/{}", a.stripe_account),
        )
        .await
    {
        sync_account(&mut db, &object).await?;
        current = account(&mut db, &user.id).await?;
    }
    let age = age(&mut db, &user.id).await?;
    let earnings = ledger::balance(&mut db, &format!("usd:earnings:{}", user.id), "usd").await?;
    Ok(Json(json!({
        "available": app.config.stripe.available(),
        "requirements": {
            "email_verified": user.email_verified,
            "mfa_enabled": user.mfa_enabled,
            "age_ok": age.is_some_and(|a| a >= 13),
        },
        "guardian": age.is_some_and(|a| a < 18),
        "account": current.map(|a| json!({
            "guardian": a.guardian,
            "details_submitted": a.details_submitted,
            "payouts_enabled": a.payouts_enabled,
            "requirements": a.requirements,
        })),
        "can_earn": can_earn(&mut db, &user.id).await?,
        // Tenths of a cent; rounded down only at payout.
        "earnings_tenths": earnings,
    })))
}

/// Creates the Express account on first use, then returns a Stripe-hosted link: onboarding until
/// it's finished, the Express dashboard after. A guardian onboards for a creator aged 13 to 17.
pub async fn payout_setup(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to set up payouts.")?;
    if !user.mfa_enabled {
        return Err(Fail::denied(
            "Turn on authenticator two-factor authentication to set up payouts.",
        ));
    }
    sec::reserve(&app, vec![format!("payout-setup:{}", user.id)], 20, 3600).await?;
    let mut db = app.db.acquire().await?;
    let guardian = match age(&mut db, &user.id).await? {
        Some(a) if a >= 18 => false,
        Some(a) if a >= 13 => true,
        _ => {
            return Err(Fail::denied(
                "Payouts need a date of birth showing you're 13 or older.",
            ));
        }
    };
    let existing = account(&mut db, &user.id).await?;
    if let Some(a) = existing.as_ref().filter(|a| a.details_submitted) {
        let link = stripe::post(
            &app.http,
            &app.config.stripe,
            &format!("accounts/{}/login_links", a.stripe_account),
            &[],
            None,
        )
        .await?;
        return Ok(Json(json!({"url": link["url"]})));
    }
    let stripe_account = match existing {
        Some(a) => a.stripe_account,
        None => {
            // The idempotency key makes a double click create one account, not two.
            let created = stripe::post(
                &app.http,
                &app.config.stripe,
                "accounts",
                &[
                    ("type", "express".into()),
                    ("capabilities[transfers][requested]", "true".into()),
                    ("business_type", "individual".into()),
                    ("metadata[sver_user]", user.id.clone()),
                    ("metadata[guardian]", guardian.to_string()),
                    (
                        "business_profile[url]",
                        format!("{}/{}", app.config.origin, user.username),
                    ),
                ],
                Some(&format!("sver-payout-account-{}", user.id)),
            )
            .await?;
            let Some(id) = created["id"].as_str() else {
                return Err(Fail::unavailable("Stripe sent an unexpected response."));
            };
            sqlx::query("INSERT INTO payout_accounts(user_id,stripe_account,guardian) VALUES($1,$2,$3) ON CONFLICT(user_id) DO NOTHING")
                .bind(&user.id).bind(id).bind(guardian).execute(&mut *db).await?;
            account(&mut db, &user.id)
                .await?
                .ok_or_else(Fail::internal)?
                .stripe_account
        }
    };
    let origin = &app.config.origin;
    let link = stripe::post(
        &app.http,
        &app.config.stripe,
        "account_links",
        &[
            ("account", stripe_account),
            ("type", "account_onboarding".into()),
            ("refresh_url", format!("{origin}/studio/payouts")),
            ("return_url", format!("{origin}/studio/payouts?returned=1")),
        ],
        None,
    )
    .await?;
    Ok(Json(json!({"url": link["url"]})))
}

// ---- Webhook ----

/// Stripe's webhook endpoint (exempt from the Origin check; the signature is the authority).
/// Events are stored first, then processed once, in the same transaction that marks them done.
/// A failure returns 503 so Stripe retries, which also covers out-of-order events.
pub async fn webhook(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Res<Json<Value>> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    // Comma-separated: the platform endpoint and the Connect endpoint have separate secrets.
    if !app
        .config
        .stripe
        .webhook_secret
        .split(',')
        .any(|secret| stripe::verify(secret.trim(), signature, &body, now))
    {
        return Err(Fail::bad("Invalid signature."));
    }
    let event: Value = serde_json::from_slice(&body).map_err(|_| Fail::bad("Invalid event."))?;
    let (Some(id), Some(kind)) = (event["id"].as_str(), event["type"].as_str()) else {
        return Err(Fail::bad("Invalid event."));
    };
    sqlx::query(
        "INSERT INTO stripe_events(id,type,payload) VALUES($1,$2,$3) ON CONFLICT(id) DO NOTHING",
    )
    .bind(id)
    .bind(kind)
    .bind(&event)
    .execute(&app.db)
    .await?;
    let mut tx = app.db.begin().await?;
    let done: Option<bool> = sqlx::query_scalar(
        "SELECT processed_at IS NOT NULL FROM stripe_events WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if done != Some(false) {
        return Ok(Json(json!({"received": true})));
    }
    match process(&mut tx, kind, &event["data"]["object"]).await {
        Ok(()) => {
            sqlx::query("UPDATE stripe_events SET processed_at=now(),error=NULL WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(Json(json!({"received": true})))
        }
        Err(error) => {
            drop(tx);
            sqlx::query("UPDATE stripe_events SET error=$2 WHERE id=$1")
                .bind(id)
                .bind(error.message.as_ref())
                .execute(&app.db)
                .await?;
            eprintln!("stripe_event=failed type={kind}");
            Err(Fail::unavailable("Event not processed yet."))
        }
    }
}

async fn process(tx: &mut PgConnection, kind: &str, object: &Value) -> Res<()> {
    match kind {
        "checkout.session.completed" | "checkout.session.async_payment_succeeded"
            if object["payment_status"] == "paid" =>
        {
            paid(tx, object).await
        }
        "checkout.session.expired" => {
            sqlx::query(
                "UPDATE checkout_sessions SET status='expired' WHERE id=$1 AND status='open'",
            )
            .bind(object["id"].as_str().unwrap_or_default())
            .execute(tx)
            .await?;
            Ok(())
        }
        "charge.refunded" => refunded(tx, object).await,
        "charge.dispute.created" => disputed(tx, object, false).await,
        "charge.dispute.closed" if object["status"] == "won" => disputed(tx, object, true).await,
        "account.updated" => sync_account(tx, object).await,
        _ => Ok(()),
    }
}

/// A paid Valor checkout credits the buyer.
async fn paid(tx: &mut PgConnection, object: &Value) -> Res<()> {
    let id = object["id"].as_str().unwrap_or_default();
    let row: Option<(Option<String>, i32, Option<i32>)> = sqlx::query_as(
        "SELECT user_id,amount_cents,valor FROM checkout_sessions WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    // Not one of ours (another integration on the same Stripe account).
    let Some((user, cents, valor)) = row else {
        return Ok(());
    };
    if object["amount_total"].as_i64() != Some(cents.into()) {
        return Err(Fail::bad("Checkout amount doesn't match the pack."));
    }
    let intent = object["payment_intent"].as_str();
    sqlx::query("UPDATE checkout_sessions SET status='paid',payment_intent=$2 WHERE id=$1")
        .bind(id)
        .bind(intent)
        .execute(&mut *tx)
        .await?;
    let buyer = format!("valor:{}", user.as_deref().unwrap_or("unclaimed"));
    let (cents, valor) = (i64::from(cents), i64::from(valor.unwrap_or(0)));
    ledger::post(
        tx,
        "valor_purchase",
        &format!("checkout:{id}"),
        json!({"session": id, "payment_intent": intent, "user": user, "cents": cents}),
        &[
            (&buyer, "valor", valor),
            ("valor:issued", "valor", -valor),
            ("usd:stripe", "usd", cents * 10),
            ("usd:sales", "usd", -cents * 10),
        ],
    )
    .await?;
    Ok(())
}

/// The credited purchase (buyer account, cents, Valor) a payment intent belongs to.
async fn purchase(tx: &mut PgConnection, intent: &str) -> Res<Option<(String, i64, i64)>> {
    // The intent is stored only when its checkout is credited; purchase_of tells "not ours" from
    // "not credited yet" by the charge's metadata.
    let row: Option<(Option<String>, i32, Option<i32>)> = sqlx::query_as(
        "SELECT user_id,amount_cents,valor FROM checkout_sessions WHERE payment_intent=$1",
    )
    .bind(intent)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(row.map(|(user, cents, valor)| {
        (
            format!("valor:{}", user.as_deref().unwrap_or("unclaimed")),
            cents.into(),
            valor.unwrap_or(0).into(),
        )
    }))
}
/// `purchase`, but an unknown payment of ours (by its metadata) waits for the checkout event.
async fn purchase_of(
    tx: &mut PgConnection,
    object: &Value,
) -> Res<Option<(String, String, i64, i64)>> {
    let Some(intent) = object["payment_intent"].as_str() else {
        return Ok(None);
    };
    match purchase(tx, intent).await? {
        Some((buyer, cents, valor)) => Ok(Some((intent.to_string(), buyer, cents, valor))),
        None if object["metadata"]["sver_user"].is_string() => {
            Err(Fail::conflict("Purchase not credited yet."))
        }
        None => Ok(None),
    }
}

/// Refunds reverse Valor in proportion. `amount_refunded` is cumulative, so each event posts only
/// the increase over what earlier refund events already reversed.
async fn refunded(tx: &mut PgConnection, object: &Value) -> Res<()> {
    let Some((intent, buyer, cents, valor)) = purchase_of(tx, object).await? else {
        return Ok(());
    };
    let refunded = object["amount_refunded"].as_i64().unwrap_or(0).min(cents);
    ledger::lock(tx, &format!("refund:{intent}")).await?;
    let before: i64 = sqlx::query_scalar("SELECT COALESCE(max((detail->>'refunded_cents')::bigint),0) FROM ledger_transactions WHERE kind='valor_refund' AND detail->>'payment_intent'=$1")
        .bind(&intent).fetch_one(&mut *tx).await?;
    if refunded <= before {
        return Ok(());
    }
    let reversed = valor * refunded / cents - valor * before / cents;
    let cash = (refunded - before) * 10;
    ledger::post(
        tx,
        "valor_refund",
        &format!("refund:{intent}:{refunded}"),
        json!({"payment_intent": intent, "refunded_cents": refunded}),
        &[
            (&buyer, "valor", -reversed),
            ("valor:issued", "valor", reversed),
            ("usd:stripe", "usd", -cash),
            ("usd:sales", "usd", cash),
        ],
    )
    .await?;
    Ok(())
}

/// A chargeback reverses the disputed Valor (the balance may go negative, which locks spending);
/// winning it credits the Valor back.
// ponytail: a dispute on an already-refunded charge reverses twice and Stripe's dispute fee isn't
// posted; handle both when payouts reconcile against Stripe balance transactions.
async fn disputed(tx: &mut PgConnection, object: &Value, won: bool) -> Res<()> {
    let Some(id) = object["id"].as_str() else {
        return Ok(());
    };
    let Some((intent, buyer, cents, valor)) = purchase_of(tx, object).await? else {
        return Ok(());
    };
    let amount = object["amount"].as_i64().unwrap_or(cents).min(cents);
    let reversed = valor * amount / cents;
    let sign = if won { 1 } else { -1 };
    ledger::post(
        tx,
        if won {
            "valor_dispute_won"
        } else {
            "valor_dispute"
        },
        &format!("{}:{id}", if won { "dispute-won" } else { "dispute" }),
        json!({"payment_intent": intent, "dispute": id, "cents": amount}),
        &[
            (&buyer, "valor", sign * reversed),
            ("valor:issued", "valor", -sign * reversed),
            ("usd:stripe", "usd", sign * amount * 10),
            ("usd:sales", "usd", -sign * amount * 10),
        ],
    )
    .await?;
    Ok(())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/wallet", get(wallet))
        .route("/api/me/wallet/checkout", post(checkout))
        .route("/api/me/payouts", get(payouts))
        .route("/api/me/payouts/setup", post(payout_setup))
        // Stripe events (an account.updated can be large) get more room than the 16 KiB default.
        .route(
            "/api/stripe/webhook",
            post(webhook).layer(DefaultBodyLimit::max(512 * 1024)),
        )
}
