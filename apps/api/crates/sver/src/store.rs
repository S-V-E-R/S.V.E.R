//! Valor bought through the App Store and Google Play (docs/SUPPORT.md "Store packs"). The store's
//! own server API is the only authority: every credit and refund is read from it over TLS, and
//! notifications are only a signal to read it again. Everything settles through `settle`, which is
//! idempotent per purchase and posts through the ledger like a card payment.
pub mod apple;
pub mod google;

use crate::{
    App, ledger,
    profiles::{self, Fail, Res},
    security as sec, support,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State as Ax},
    routing::post,
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// (product id on both stores, list price in US cents, Valor): a flat 100 Valor per $0.99 tier,
/// no bonus (bonus Valor is a web perk). docs/SUPPORT.md "Store packs".
pub const PACKS: [(&str, i64, i64); 7] = [
    ("tv.sver.app.valor.100", 99, 100),
    ("tv.sver.app.valor.200", 199, 200),
    ("tv.sver.app.valor.500", 499, 500),
    ("tv.sver.app.valor.1000", 999, 1_000),
    ("tv.sver.app.valor.2000", 1_999, 2_000),
    ("tv.sver.app.valor.5000", 4_999, 5_000),
    ("tv.sver.app.valor.10000", 9_999, 10_000),
];
/// The iOS bundle id and the Play package (public; app.config.ts).
pub const APP_ID: &str = "tv.sver.app";

const UNAVAILABLE: &str = "Buying Valor in the app isn't available yet.";
const TEST_REFUSED: &str = "Test purchases aren't accepted on this account.";
const NOT_YET: &str = "We couldn't find this purchase yet. It's added once it goes through.";

/// Store credentials and API base URLs (tests point the URLs at local fakes).
#[derive(Clone, Default)]
pub struct Config {
    pub apple: Option<apple::Key>,
    pub google: Option<google::Account>,
    /// S.V.E.R user ids that may credit sandbox and license-tester purchases on production.
    pub test_users: Vec<String>,
    /// Sandbox Valor a test user may credit per calendar month on production.
    pub test_monthly_valor: i64,
    /// The secret path segment of the notification URLs; empty turns them off (404).
    pub notify_secret: String,
    /// Whether the Android app may show the US link to sver.tv (Google's US program).
    pub google_us_link: bool,
    pub apple_url: String,
    pub apple_sandbox_url: String,
    pub google_api_url: String,
    pub google_token_url: String,
}
impl Config {
    /// A store key that is set but unusable, or a malformed setting, stops startup.
    pub fn from_env() -> Result<Self, String> {
        let env = |key: &str| std::env::var(key).unwrap_or_default().trim().to_string();
        let notify_secret = env("STORE_NOTIFY_SECRET");
        if !notify_secret.is_empty()
            && (notify_secret.len() < 32
                || !notify_secret
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        {
            return Err("STORE_NOTIFY_SECRET must be 32 or more letters, digits, - or _".into());
        }
        let test_monthly_valor = match env("STORE_TEST_MONTHLY_VALOR") {
            v if v.is_empty() => 2_000,
            v => v
                .parse()
                .ok()
                .filter(|n: &i64| *n >= 0)
                .ok_or("STORE_TEST_MONTHLY_VALOR must be a whole number of Valor")?,
        };
        let google_us_link = match env("STORE_GOOGLE_US_LINK").as_str() {
            "" | "false" | "0" => false,
            "true" | "1" => true,
            _ => return Err("STORE_GOOGLE_US_LINK must be true or false".into()),
        };
        Ok(Self {
            apple: apple::key_from_env()?,
            google: google::account_from_env()?,
            test_users: env("STORE_TEST_USERS")
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(String::from)
                .collect(),
            test_monthly_valor,
            notify_secret,
            google_us_link,
            apple_url: "https://api.storekit.apple.com".into(),
            apple_sandbox_url: "https://api.storekit-sandbox.apple.com".into(),
            google_api_url: "https://androidpublisher.googleapis.com".into(),
            google_token_url: "https://oauth2.googleapis.com/token".into(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Env {
    Production,
    Sandbox,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Pending,
    Purchased,
    /// Cancelled before it was paid; nothing to credit.
    Void,
}

/// One purchase as the store's API reports it.
pub struct Found {
    pub store: &'static str,
    /// Apple transactionId, Play purchaseToken.
    pub id: String,
    pub product: String,
    pub quantity: i64,
    /// Apple appAccountToken, Google obfuscatedExternalAccountId.
    pub token: Option<uuid::Uuid>,
    pub env: Env,
    pub state: State,
    /// Share refunded in milliunits, 0..=100000. Apple: revocationPercentage (100000 when revoked
    /// without one). Google: (quantity - refundableQuantity) * 100000 / quantity.
    pub refunded_milli: Option<i64>,
    /// The store API's response, kept on the purchase row.
    pub raw: serde_json::Value,
}

/// What `settle` did. `Credited` names the owner (None once that account is erased).
pub enum Outcome {
    Credited(Option<String>),
    Pending,
    Void,
}

/// The `store` object of `GET /api/me/wallet`: which stores sell packs, and the packs.
pub fn catalog(c: &Config) -> Value {
    json!({
        "apple": c.apple.is_some(),
        "google": c.google.is_some(),
        "google_us_link": c.google_us_link,
        "packs": PACKS.iter().map(|(product, _, valor)| json!({"product": product, "valor": valor})).collect::<Vec<_>>(),
    })
}

/// Credits a purchase once and, when `refund` is set, moves its refund to what the store reports.
/// Claims pass `refund = false`: their fetch happened before this lock, so it may be stale. Only
/// notifications and the poll, which fetch while holding the lock, change a credited purchase's
/// refund. A first credit always applies the refund fetched with it, so a purchase refunded before
/// it was credited nets to zero.
pub(crate) async fn settle(
    app: &App,
    tx: &mut PgConnection,
    found: &Found,
    refund: bool,
) -> Res<Outcome> {
    let (store, id) = (found.store, found.id.as_str());
    let reference = format!("store:{store}:{id}");
    ledger::lock(tx, &reference).await?;
    let row: Option<(Option<String>, i32, i32)> = sqlx::query_as(
        "SELECT user_id,refunded_milli,refund_seq FROM store_purchases WHERE store=$1 AND store_id=$2 FOR UPDATE",
    )
    .bind(store)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((owner, old, seq)) = row {
        let mut refunded = i64::from(old);
        if let (true, Some(new)) = (refund, found.refunded_milli) {
            // Google has no refund reversal, and a refund record can age out of its lists.
            let new = if store == "google" {
                new.max(refunded)
            } else {
                new
            };
            refund_change(tx, store, id, refunded, new, seq, true).await?;
            refunded = new.clamp(0, 100_000);
        }
        // Refunded in full: nothing of it is left to credit.
        return Ok(if refunded == 100_000 {
            Outcome::Void
        } else {
            Outcome::Credited(owner)
        });
    }
    match found.state {
        State::Pending => return Ok(Outcome::Pending),
        State::Void => return Ok(Outcome::Void),
        State::Purchased => {}
    }
    let Some(&(product, pack_cents, pack_valor)) = PACKS.iter().find(|p| p.0 == found.product)
    else {
        // Left unfinished and visible; an unconsumed Play purchase refunds itself after 3 days.
        eprintln!("store_event=unknown_product store={store} id={id}");
        return Err(Fail::internal());
    };
    let Some(quantity) = i32::try_from(found.quantity)
        .ok()
        .filter(|q| (1..=1000).contains(q))
    else {
        eprintln!("store_event=bad_quantity store={store} id={id}");
        return Err(Fail::internal());
    };
    let owner: Option<String> = match found.token {
        Some(token) => {
            sqlx::query_scalar("SELECT id FROM users WHERE store_token=$1::uuid")
                .bind(token.to_string())
                .fetch_optional(&mut *tx)
                .await?
        }
        None => None,
    };
    let Some(owner) = owner else {
        // Only our app's pre-check hands out the token; without a known one we can't tell whose
        // purchase this is (and it skipped the under-18 gate).
        eprintln!("store_event=refused reason=unknown_account store={store} id={id}");
        return Err(Fail::denied(
            "This purchase isn't linked to a S.V.E.R account.",
        ));
    };
    let (valor, cents) = (
        pack_valor * i64::from(quantity),
        pack_cents * i64::from(quantity),
    );
    let sandbox = found.env == Env::Sandbox;
    let c = &app.config.store;
    if !sandbox && !app.config.production {
        // Development never moves real money (as with live Stripe keys).
        return Err(Fail::denied(
            "Real purchases aren't accepted on a development server.",
        ));
    }
    if sandbox && app.config.production {
        if !c.test_users.contains(&owner) {
            eprintln!("store_event=refused reason=sandbox store={store} id={id}");
            return Err(Fail::denied(TEST_REFUSED));
        }
        // Sandbox Valor spends like any other, so testers get a small monthly allowance. The owner
        // lock stops parallel claims for different purchases from all passing the same SUM.
        ledger::lock(tx, &format!("store-test:{owner}")).await?;
        let month: i64 = sqlx::query_scalar("SELECT COALESCE(sum(valor),0)::bigint FROM store_purchases WHERE user_id=$1 AND environment='sandbox' AND created_at>=date_trunc('month', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'")
            .bind(&owner).fetch_one(&mut *tx).await?;
        if month + valor > c.test_monthly_valor {
            return Err(Fail::denied(
                "This account has used its test purchases for this month.",
            ));
        }
    }
    sqlx::query("INSERT INTO store_purchases(store,store_id,user_id,product_id,quantity,valor,cents,environment,consumed,payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(store).bind(id).bind(&owner).bind(product).bind(quantity).bind(valor).bind(cents)
        .bind(if sandbox { "sandbox" } else { "production" }).bind(store == "apple").bind(&found.raw)
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM store_intents WHERE id=(SELECT id FROM store_intents WHERE user_id=$1 AND cents=$2 ORDER BY created_at LIMIT 1)")
        .bind(&owner).bind(pack_cents).execute(&mut *tx).await?;
    // The store already took the money (Ask to Buy can be approved days later), so a purchase that
    // ends up over an under-18 account's cap is credited anyway and flagged for staff.
    let over_cap = support::age(tx, &owner).await?.is_none_or(|a| a < 18)
        && support::month_cents(tx, &owner).await? > support::MINOR_CAP_CENTS;
    if over_cap {
        eprintln!("store_event=over_cap store={store} id={id}");
    }
    let wallet = format!("valor:{owner}");
    let cash = format!("usd:{store}");
    let mut entries = vec![
        (wallet.as_str(), "valor", valor),
        (
            if sandbox {
                "valor:test"
            } else {
                "valor:issued"
            },
            "valor",
            -valor,
        ),
    ];
    if !sandbox {
        // Booked at the US list price; the store's commission and taxes reconcile against its
        // financial reports. ponytail: post the commission when payouts reconcile.
        entries.extend([
            (cash.as_str(), "usd", cents * 10),
            ("usd:sales", "usd", -cents * 10),
        ]);
    }
    ledger::post(
        tx,
        "store_purchase",
        &reference,
        json!({"store": store, "id": id, "payment_intent": format!("{store}:{id}"), "user": owner,
            "product": product, "quantity": quantity, "valor": valor, "cents": cents,
            "environment": if sandbox { "sandbox" } else { "production" }, "over_cap": over_cap}),
        &entries,
    )
    .await?;
    match found.refunded_milli.unwrap_or(0).clamp(0, 100_000) {
        0 => Ok(Outcome::Credited(Some(owner))),
        refunded => {
            // Credit and refund land together, so nothing was spent in between: no clawback.
            refund_change(tx, store, id, 0, refunded, 0, false).await?;
            Ok(if refunded == 100_000 {
                Outcome::Void
            } else {
                Outcome::Credited(Some(owner))
            })
        }
    }
}

/// Moves a credited purchase's refund from `old` to `new` milliunits: the credit's own entries,
/// negated in proportion (a lower `new` posts them back). It reads the entries from the ledger,
/// so it works after the account is erased. A refund that leaves the wallet negative takes the
/// spent Valor back from the creators it paid, as a chargeback does; a reversal posts those back.
async fn refund_change(
    tx: &mut PgConnection,
    store: &str,
    id: &str,
    old: i64,
    new: i64,
    seq: i32,
    claw: bool,
) -> Res<()> {
    let new = new.clamp(0, 100_000);
    if new == old {
        return Ok(());
    }
    let credit: Vec<(String, String, i64)> = sqlx::query_as("SELECT e.account,e.unit,e.amount::bigint FROM ledger_entries e JOIN ledger_transactions t ON t.id=e.transaction_id WHERE t.reference=$1")
        .bind(format!("store:{store}:{id}")).fetch_all(&mut *tx).await?;
    // Each unit is one +a/-a pair and division truncates toward zero, so every posting balances,
    // and the steps telescope to exactly the credit at 100000.
    let share = |amount: i64| amount * new / 100_000 - amount * old / 100_000;
    let wallet = credit
        .iter()
        .find(|(_, unit, amount)| unit == "valor" && *amount > 0);
    if let Some((wallet, _, _)) = wallet {
        ledger::lock(tx, wallet).await?;
    }
    let seq = seq + 1;
    let dispute = format!("store-refund:{store}:{id}:{seq}");
    let cents = credit
        .iter()
        .find(|(account, _, _)| account.starts_with("usd:") && account != "usd:sales")
        .map_or(0, |(_, _, amount)| share(*amount).abs() / 10);
    let entries: Vec<(&str, &str, i64)> = credit
        .iter()
        .map(|(account, unit, amount)| (account.as_str(), unit.as_str(), -share(*amount)))
        .collect();
    ledger::post(
        tx,
        if new > old {
            "store_refund"
        } else {
            "store_refund_reversed"
        },
        &dispute,
        json!({"payment_intent": format!("{store}:{id}"), "store": store, "id": id,
            "refunded_milli": new, "seq": seq, "cents": cents}),
        &entries,
    )
    .await?;
    sqlx::query(
        "UPDATE store_purchases SET refunded_milli=$3,refund_seq=$4 WHERE store=$1 AND store_id=$2",
    )
    .bind(store)
    .bind(id)
    .bind(new as i32)
    .bind(seq)
    .execute(&mut *tx)
    .await?;
    let (true, Some((wallet, _, valor))) = (claw, wallet) else {
        return Ok(());
    };
    let intent = format!("{store}:{id}");
    if new > old {
        support::claw_back_spent(tx, &dispute, &intent, wallet, share(*valor), false).await?;
    } else {
        // Every earlier step's clawback goes back; each is posted back at most once.
        for earlier in 1..seq {
            let step = format!("store-refund:{store}:{id}:{earlier}");
            support::claw_back_spent(tx, &step, &intent, wallet, 0, true).await?;
        }
    }
    Ok(())
}

// ---- The app: pre-check and claims ----

#[derive(Deserialize)]
pub struct Intent {
    store: String,
    product: String,
    #[serde(default)]
    guardian_consent: bool,
}
/// POST /api/me/wallet/store: the checks a card checkout makes, before the store's sheet opens.
/// Answers the account's store token, which the app passes to the store with the purchase.
async fn precheck(Ax(app): Ax<App>, jar: CookieJar, Json(input): Json<Intent>) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to buy Valor.")?;
    crate::switches::guard(&mut *app.db.acquire().await?, "purchases").await?;
    let c = &app.config.store;
    let configured = match input.store.as_str() {
        "apple" => c.apple.is_some(),
        "google" => c.google.is_some(),
        _ => return Err(Fail::field("store", "Choose the App Store or Google Play.")),
    };
    if !configured {
        return Err(Fail::unavailable(UNAVAILABLE));
    }
    let Some(&(_, cents, _)) = PACKS.iter().find(|p| p.0 == input.product) else {
        return Err(Fail::field("product", "Choose one of the Valor packs."));
    };
    sec::reserve(&app, vec![format!("valor-checkout:{}", user.id)], 10, 3600).await?;
    let mut tx = app.db.begin().await?;
    // One purchase check at a time per buyer, shared with card checkouts, so the cap holds.
    ledger::lock(&mut tx, &format!("checkout:{}", user.id)).await?;
    support::card_gate(&mut tx, &user.id, cents, input.guardian_consent).await?;
    sqlx::query("INSERT INTO store_intents(user_id,cents) VALUES($1,$2)")
        .bind(&user.id)
        .bind(cents)
        .execute(&mut *tx)
        .await?;
    let token: String = sqlx::query_scalar("SELECT store_token::text FROM users WHERE id=$1")
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"token": token})))
}

#[derive(Deserialize)]
pub struct AppleClaim {
    transaction_id: String,
    environment: String,
}
/// POST /api/me/wallet/apple: the app reports a finished App Store purchase.
async fn claim_apple(
    Ax(app): Ax<App>,
    jar: CookieJar,
    Json(input): Json<AppleClaim>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let c = &app.config.store;
    if c.apple.is_none() {
        return Err(Fail::unavailable(UNAVAILABLE));
    }
    let Some(hint) = apple::env(&input.environment) else {
        return Err(Fail::field(
            "environment",
            "This purchase can't be checked with the App Store.",
        ));
    };
    if !apple::valid_id(&input.transaction_id) {
        return Err(Fail::field(
            "transaction_id",
            "This isn't an App Store purchase.",
        ));
    }
    // A development server only ever asks the sandbox.
    let env = if app.config.production {
        hint
    } else {
        Env::Sandbox
    };
    if app.config.production && env == Env::Sandbox && !c.test_users.contains(&user.id) {
        return Err(Fail::denied(TEST_REFUSED));
    }
    sec::reserve(&app, vec![format!("store-claim:{}", user.id)], 60, 3600).await?;
    let found = apple::transaction(&app.http, c, env, &input.transaction_id)
        .await?
        .ok_or_else(|| Fail::conflict(NOT_YET))?;
    claimed(&app, &user.id, found).await
}

#[derive(Deserialize)]
pub struct GoogleClaim {
    token: String,
}
/// POST /api/me/wallet/google: the app reports a Google Play purchase.
async fn claim_google(
    Ax(app): Ax<App>,
    jar: CookieJar,
    Json(input): Json<GoogleClaim>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let c = &app.config.store;
    if c.google.is_none() {
        return Err(Fail::unavailable(UNAVAILABLE));
    }
    if !google::valid_token(&input.token) {
        return Err(Fail::field("token", "This isn't a Google Play purchase."));
    }
    sec::reserve(&app, vec![format!("store-claim:{}", user.id)], 60, 3600).await?;
    let found = google::purchase(&app.http, c, &input.token)
        .await?
        .ok_or_else(|| Fail::conflict(NOT_YET))?;
    claimed(&app, &user.id, found).await
}

/// Settles a claimed purchase and answers the app. `valor` is always the caller's own balance, so
/// a purchase that belongs to another account never reveals that account's.
async fn claimed(app: &App, user: &str, found: Found) -> Res<Json<Value>> {
    let mut tx = app.db.begin().await?;
    let outcome = settle(app, &mut tx, &found, false).await?;
    tx.commit().await?;
    let owner = match outcome {
        Outcome::Pending => return Ok(Json(json!({"status": "pending"}))),
        Outcome::Void => return Ok(Json(json!({"status": "void"}))),
        Outcome::Credited(owner) => owner,
    };
    consume(app, &found).await;
    let valor = ledger::balance(
        &mut *app.db.acquire().await?,
        &format!("valor:{user}"),
        "valor",
    )
    .await?;
    Ok(Json(
        json!({"status": "credited", "valor": valor, "mine": owner.as_deref() == Some(user)}),
    ))
}

/// Consumes a credited Play purchase, after the credit committed, so it can be bought again. A
/// failure is left for `tick` to retry; the purchase is credited either way.
async fn consume(app: &App, found: &Found) {
    if found.store != "google" {
        return;
    }
    let product: Option<String> = sqlx::query_scalar(
        "SELECT product_id FROM store_purchases WHERE store='google' AND store_id=$1 AND NOT consumed",
    )
    .bind(&found.id)
    .fetch_optional(&app.db)
    .await
    .unwrap_or(None);
    let Some(product) = product else {
        return;
    };
    match google::consume(&app.http, &app.config.store, &product, &found.id).await {
        Ok(()) => {
            let _ = sqlx::query(
                "UPDATE store_purchases SET consumed=true WHERE store='google' AND store_id=$1",
            )
            .bind(&found.id)
            .execute(&app.db)
            .await;
        }
        Err(_) => eprintln!("store_event=consume outcome=retry"),
    }
}

// ---- The stores: notifications and the hourly sweep ----

/// Re-reads one purchase while holding its lock, so the refund it posts can't be undone by an
/// older read, then settles it.
/// `voided` is the quantity Google's voided list reports for it (`i64::MAX` for a full refund): a
/// floor for the refund, since productsv2 may leave `refundableQuantity` out once it reaches 0.
async fn refresh(app: &App, store: &str, id: &str, env: Env, voided: Option<i64>) -> Res<()> {
    let c = &app.config.store;
    let mut tx = app.db.begin().await?;
    ledger::lock(&mut tx, &format!("store:{store}:{id}")).await?;
    let found = match store {
        "apple" => apple::transaction(&app.http, c, env, id).await?,
        _ => google::purchase(&app.http, c, id).await?,
    };
    let Some(mut found) = found else {
        eprintln!("store_event=missing store={store} id={id}");
        return Ok(());
    };
    if let Some(voided) = voided {
        let floor = voided.clamp(0, found.quantity) * 100_000 / found.quantity;
        found.refunded_milli = Some(found.refunded_milli.unwrap_or(0).max(floor));
    }
    settle(app, &mut tx, &found, true).await?;
    tx.commit().await?;
    consume(app, &found).await;
    Ok(())
}

/// The notification path's last segment must equal `STORE_NOTIFY_SECRET` (compared in constant
/// time); otherwise, or when it is unset, the endpoint doesn't exist.
fn notify_secret(c: &Config, given: &str) -> Res<()> {
    let expected = c.notify_secret.as_bytes();
    let same = expected.len() == given.len()
        && expected
            .iter()
            .zip(given.bytes())
            .fold(0, |d, (a, b)| d | (a ^ b))
            == 0;
    if expected.is_empty() || !same {
        return Err(Fail::missing());
    }
    Ok(())
}

/// What a store hears back. A refusal that will never change (an unknown account, a test purchase
/// on a regular account) is acknowledged so the store stops retrying; anything else that failed
/// answers 503 so it retries.
fn acknowledge(result: Res<()>) -> Res<Json<Value>> {
    match result {
        Ok(()) => Ok(Json(json!({"received": true}))),
        Err(fail) if fail.status.is_client_error() && fail.status.as_u16() != 429 => {
            Ok(Json(json!({"received": true})))
        }
        Err(_) => Err(Fail::unavailable("Not processed yet.")),
    }
}

/// POST /api/store/apple/notifications/{secret}: App Store Server Notifications V2. The body only
/// names the transaction; its state is fetched from Apple.
async fn apple_notification(
    Ax(app): Ax<App>,
    Path(secret): Path<String>,
    body: Bytes,
) -> Res<Json<Value>> {
    let c = &app.config.store;
    notify_secret(c, &secret)?;
    sec::reserve(&app, vec!["store-notify:apple".into()], 600, 60).await?;
    let signal = serde_json::from_slice(&body)
        .ok()
        .and_then(|b: Value| apple::notification(&b));
    let Some((id, env, kind)) = signal.filter(|_| c.apple.is_some()) else {
        return acknowledge(Ok(()));
    };
    // CONSUMPTION_REQUEST gets no consumption data (Apple decides alone); TEST and the rest need
    // nothing. A development server never reads production purchases.
    if !matches!(
        kind.as_str(),
        "ONE_TIME_CHARGE" | "REFUND" | "REFUND_REVERSED" | "REFUND_DECLINED"
    ) || (!app.config.production && env == Env::Production)
    {
        return acknowledge(Ok(()));
    }
    acknowledge(refresh(&app, "apple", &id, env, None).await)
}

/// POST /api/store/google/notifications/{secret}: Real-time developer notifications pushed by
/// Pub/Sub. The body only names the purchase token; its state is fetched from Google. The Pub/Sub
/// OIDC token isn't checked, since the body carries no authority.
async fn google_notification(
    Ax(app): Ax<App>,
    Path(secret): Path<String>,
    body: Bytes,
) -> Res<Json<Value>> {
    let c = &app.config.store;
    notify_secret(c, &secret)?;
    sec::reserve(&app, vec!["store-notify:google".into()], 600, 60).await?;
    let signal = serde_json::from_slice(&body)
        .ok()
        .and_then(|b: Value| google::notification(&b));
    let Some((token, _voided)) = signal.filter(|_| c.google.is_some()) else {
        return acknowledge(Ok(()));
    };
    acknowledge(refresh(&app, "google", &token, Env::Production, None).await)
}

/// Hourly (the rate-limit row is the clock, shared by every API process): drops lapsed purchase
/// checks, retries Play consumes, and reads refunds the stores may not have pushed: Google's
/// voided list (30 days) and Apple's notification history (2 days).
pub async fn tick(app: &App) -> Res<()> {
    if sec::reserve(app, vec!["store-tick".into()], 1, 3600)
        .await
        .is_err()
    {
        return Ok(());
    }
    sqlx::query("DELETE FROM store_intents WHERE created_at<now()-interval '30 minutes'")
        .execute(&app.db)
        .await?;
    let google = sweep_google(app).await;
    let apple = sweep_apple(app).await;
    google.and(apple)
}

async fn sweep_google(app: &App) -> Res<()> {
    let c = &app.config.store;
    if c.google.is_none() {
        return Ok(());
    }
    // Play refunds a purchase that stays unconsumed for 3 days.
    let unconsumed: Vec<String> = sqlx::query_scalar("SELECT store_id FROM store_purchases WHERE store='google' AND NOT consumed AND created_at>now()-interval '3 days'")
        .fetch_all(&app.db).await?;
    for token in unconsumed {
        let product: Option<String> = sqlx::query_scalar(
            "SELECT product_id FROM store_purchases WHERE store='google' AND store_id=$1",
        )
        .bind(&token)
        .fetch_optional(&app.db)
        .await?;
        if let Some(product) = product
            && google::consume(&app.http, c, &product, &token)
                .await
                .is_ok()
        {
            sqlx::query(
                "UPDATE store_purchases SET consumed=true WHERE store='google' AND store_id=$1",
            )
            .bind(&token)
            .execute(&app.db)
            .await?;
        }
    }
    let since = chrono::Utc::now() - chrono::Duration::days(30);
    let voided = google::voided(&app.http, c, since).await?;
    let tokens: Vec<&str> = voided.iter().map(|(token, _)| token.as_str()).collect();
    // Each voided purchase of ours is re-read; the larger of refundableQuantity and the voided
    // quantity wins.
    let ours: Vec<String> = sqlx::query_scalar(
        "SELECT store_id FROM store_purchases WHERE store='google' AND store_id=ANY($1)",
    )
    .bind(&tokens)
    .fetch_all(&app.db)
    .await?;
    for token in ours {
        let quantity = voided
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, q)| q.unwrap_or(i64::MAX));
        if refresh(app, "google", &token, Env::Production, quantity)
            .await
            .is_err()
        {
            eprintln!("store_event=voided outcome=retry");
        }
    }
    Ok(())
}

async fn sweep_apple(app: &App) -> Res<()> {
    let c = &app.config.store;
    if c.apple.is_none() {
        return Ok(());
    }
    let env = if app.config.production {
        Env::Production
    } else {
        Env::Sandbox
    };
    let since = chrono::Utc::now() - chrono::Duration::days(2);
    let mut ids: Vec<String> = apple::history(&app.http, c, env, since)
        .await?
        .into_iter()
        .filter(|(_, _, kind)| {
            matches!(
                kind.as_str(),
                "ONE_TIME_CHARGE" | "REFUND" | "REFUND_REVERSED" | "REFUND_DECLINED"
            )
        })
        .map(|(id, _, _)| id)
        .collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        if refresh(app, "apple", &id, env, None).await.is_err() {
            eprintln!("store_event=history outcome=retry");
        }
    }
    Ok(())
}

pub fn routes() -> Router<App> {
    // Apple's signed payloads run to several KiB.
    let notices = DefaultBodyLimit::max(64 * 1024);
    Router::new()
        .route("/api/me/wallet/store", post(precheck))
        .route("/api/me/wallet/apple", post(claim_apple))
        .route("/api/me/wallet/google", post(claim_google))
        .route(
            "/api/store/apple/notifications/{secret}",
            post(apple_notification).layer(notices),
        )
        .route(
            "/api/store/google/notifications/{secret}",
            post(google_notification).layer(notices),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_are_flat_and_named_by_their_valor() {
        assert_eq!(PACKS.len(), 7);
        for (product, cents, valor) in PACKS {
            assert_eq!((cents + 1) / 100 * 100, valor, "{product}");
            assert_eq!(product, format!("{APP_ID}.valor.{valor}"));
        }
    }

    #[test]
    fn notification_secret_must_match_exactly() {
        let mut c = Config::default();
        assert!(
            notify_secret(&c, "").is_err(),
            "unset turns the endpoints off"
        );
        c.notify_secret = "s".repeat(32);
        assert!(notify_secret(&c, &"s".repeat(32)).is_ok());
        assert!(notify_secret(&c, &"s".repeat(31)).is_err());
        assert!(notify_secret(&c, &format!("{}t", "s".repeat(31))).is_err());
    }
}
