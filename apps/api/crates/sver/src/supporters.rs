//! Platform supporters and Founders (docs/SUPPORTERS.md): Wolfsworn ($5) and Greycloak ($25), a
//! monthly card subscription to S.V.E.R itself. Everything it gives is how a person looks: a Wolf
//! badge, a silver name and player-card frame, "Supporting since", and for the first 500 paid
//! Greycloak accounts a numbered Founder mark kept for good. Nothing here is read by MAGNet, the
//! war, progression, Valor or chat rules. All of the money goes to SVER LLC (`usd:supporters`).
use crate::{
    App, ledger,
    profiles::{self, Fail, Res},
    safety, security as sec, stripe,
    support::{self, post_pairs},
    switches,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// Monthly price in cents: Wolfsworn, Greycloak.
pub const PRICES: [i64; 2] = [500, 2_500];
pub const FOUNDERS: i32 = 500;
const NAMES: [&str; 2] = ["wolfsworn", "greycloak"];

fn tier_named(name: &str) -> Res<i16> {
    NAMES
        .iter()
        .position(|n| *n == name)
        .map(|i| i as i16 + 1)
        .ok_or_else(|| Fail::field("tier", "Choose Wolfsworn or Greycloak."))
}
pub fn tier_name(tier: i16) -> &'static str {
    NAMES[(tier.clamp(1, 2) - 1) as usize]
}
fn price(tier: i16) -> i64 {
    PRICES[(tier.clamp(1, 2) - 1) as usize]
}
/// What the subscription agrees to, beside the button and above Stripe's pay button (the same
/// California renewal terms as creator subscriptions, docs/SUPPORT.md).
pub fn renewal_terms(cents: i64) -> String {
    format!(
        "Renews automatically every month at ${}.{:02} until you cancel. Cancel anytime from your wallet; your supporter look lasts to the end of the paid month.",
        cents / 100,
        cents % 100
    )
}

/// Supporter SQL for one user id expression: the active tier, months and Founder number, or NULL.
/// Chat and cards show these only while subscribed; the Founder mark is added separately where
/// it shows for good.
pub fn active_sql(user: &str) -> String {
    format!(
        "(SELECT jsonb_build_object('tier',CASE p.tier WHEN 2 THEN 'greycloak' ELSE 'wolfsworn' END,'months',p.months,'founder',(SELECT f.number FROM founders f WHERE f.user_id=p.user_id)) FROM platform_subscriptions p WHERE p.user_id={user} AND p.paid_through>now())"
    )
}
/// A person's marks for a profile, user card or player card: the active tier (if any), months,
/// "Supporting since" and the Founder number, which stays after cancelling.
pub async fn marks(db: &mut PgConnection, user: &str) -> Res<Value> {
    let row: Option<(i16, i32, chrono::DateTime<chrono::Utc>, bool)> = sqlx::query_as(
        "SELECT tier,months,since,paid_through>now() FROM platform_subscriptions WHERE user_id=$1",
    )
    .bind(user)
    .fetch_optional(&mut *db)
    .await?;
    let founder: Option<i32> = sqlx::query_scalar("SELECT number FROM founders WHERE user_id=$1")
        .bind(user)
        .fetch_optional(&mut *db)
        .await?;
    let active = row.as_ref().filter(|r| r.3);
    Ok(json!({
        "tier": active.map(|r| tier_name(r.0)),
        "months": active.map(|r| r.1),
        "since": active.map(|r| r.2),
        "founder": founder,
    }))
}

/// GET /api/supporters: the tiers, the Founder count and list, and the viewer's own plan.
async fn overview(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let founders: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('number',f.number,'username',CASE WHEN f.hidden OR u.deleted_at IS NOT NULL THEN NULL ELSE u.username END,'faction',CASE WHEN f.hidden THEN NULL ELSE (SELECT m.faction FROM faction_members m WHERE m.user_id=f.user_id) END) FROM founders f JOIN users u ON u.id=f.user_id ORDER BY f.number")
        .fetch_all(&mut *db)
        .await?;
    let mine = match &viewer {
        Some(v) => Some(mine(&mut db, &v.id).await?),
        None => None,
    };
    let open = !switches::off(&mut db, "supporter_sales").await?
        && !switches::off(&mut db, "purchases").await?;
    Ok(Json(json!({
        "available": app.config.stripe.available(),
        "sales_open": open,
        "tiers": NAMES.iter().zip(PRICES).map(|(n, c)| json!({"tier": n, "cents": c, "terms": renewal_terms(c)})).collect::<Vec<_>>(),
        "founders_claimed": founders.len(),
        "founders_total": FOUNDERS,
        "founders": founders,
        "signed_in": viewer.is_some(),
        "mine": mine,
    })))
}
async fn mine(db: &mut PgConnection, user: &str) -> Res<Value> {
    let plan: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('tier',CASE tier WHEN 2 THEN 'greycloak' ELSE 'wolfsworn' END,'pending_tier',CASE pending_tier WHEN 2 THEN 'greycloak' WHEN 1 THEN 'wolfsworn' END,'paid_through',paid_through,'auto_renew',stripe_subscription IS NOT NULL AND NOT cancel_at_period_end,'card',stripe_subscription IS NOT NULL,'months',months,'since',since,'next_cents',CASE coalesce(pending_tier,tier) WHEN 2 THEN 2500 ELSE 500 END) FROM platform_subscriptions WHERE user_id=$1 AND paid_through>now()")
        .bind(user).fetch_optional(&mut *db).await?;
    let founder: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('number',number,'hidden',hidden) FROM founders WHERE user_id=$1",
    )
    .bind(user)
    .fetch_optional(db)
    .await?;
    Ok(json!({"plan": plan, "founder": founder}))
}
/// GET /api/me/supporter: the signed-in person's own marks, for their player card.
async fn my_marks(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    Ok(Json(marks(&mut *app.db.acquire().await?, &user.id).await?))
}

#[derive(Deserialize)]
struct Subscribe {
    tier: String,
    #[serde(default)]
    guardian_consent: bool,
}
/// POST /api/supporters/subscribe: a card subscription by Stripe Checkout (renews monthly).
async fn subscribe(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Subscribe>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to support S.V.E.R.")?;
    let tier = tier_named(&input.tier)?;
    {
        let mut db = app.db.acquire().await?;
        switches::guard(&mut db, "purchases").await?;
        switches::guard(&mut db, "supporter_sales").await?;
    }
    let renewing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM platform_subscriptions WHERE user_id=$1 AND stripe_subscription IS NOT NULL AND paid_through>now())")
        .bind(&user.id).fetch_one(&app.db).await?;
    if renewing {
        return Err(Fail::conflict(
            "You already support S.V.E.R. Change your tier or cancel from your wallet.",
        ));
    }
    sec::reserve(
        &app,
        vec![format!("supporter-checkout:{}", user.id)],
        10,
        3600,
    )
    .await?;
    let cents = price(tier);
    let mut tx = app.db.begin().await?;
    // One checkout at a time per buyer, so concurrent tabs can't slip past the under-18 cap.
    ledger::lock(&mut tx, &format!("checkout:{}", user.id)).await?;
    // One supporter checkout at a time: a new one first closes the last, so two tabs can't start
    // two renewing subscriptions. A session Stripe can't close was paid or is finishing.
    let previous: Vec<(String, String)> = sqlx::query_as("SELECT c.id,c.status FROM checkout_sessions c WHERE c.user_id=$1 AND c.kind='platform' AND c.detail->>'upgrade' IS NULL AND c.created_at>now()-interval '1 hour' AND (c.status='open' OR c.status='paid' AND NOT EXISTS(SELECT 1 FROM platform_invoices i WHERE i.user_id=c.user_id AND i.created_at>c.created_at))")
        .bind(&user.id).fetch_all(&mut *tx).await?;
    for (id, status) in previous {
        let closed = status == "open"
            && stripe::post(
                &app.http,
                &app.config.stripe,
                &format!("checkout/sessions/{id}/expire"),
                &[],
                None,
            )
            .await
            .is_ok();
        let gone = closed
            || status == "open"
                && stripe::get(
                    &app.http,
                    &app.config.stripe,
                    &format!("checkout/sessions/{id}"),
                )
                .await
                .is_ok_and(|s| s["status"] == "expired");
        if !gone {
            return Err(Fail::conflict(
                "A supporter payment is already being processed. Give it a minute, then check your wallet.",
            ));
        }
        sqlx::query("UPDATE checkout_sessions SET status='expired' WHERE id=$1 AND status='open'")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    support::card_gate(&mut tx, &user.id, cents, input.guardian_consent).await?;
    let origin = &app.config.origin;
    let name = if tier == 2 { "Greycloak" } else { "Wolfsworn" };
    let session = stripe::post(
        &app.http,
        &app.config.stripe,
        "checkout/sessions",
        &[
            ("mode", "subscription".into()),
            ("line_items[0][quantity]", "1".into()),
            ("line_items[0][price_data][currency]", "usd".into()),
            ("line_items[0][price_data][unit_amount]", cents.to_string()),
            (
                "line_items[0][price_data][recurring][interval]",
                "month".into(),
            ),
            (
                "line_items[0][price_data][product_data][name]",
                format!("S.V.E.R {name} supporter"),
            ),
            ("subscription_data[metadata][sver_platform]", "1".into()),
            ("subscription_data[metadata][sver_user]", user.id.clone()),
            (
                "subscription_data[metadata][sver_platform_tier]",
                tier.to_string(),
            ),
            ("client_reference_id", user.id.clone()),
            ("metadata[sver_kind]", "platform".into()),
            ("custom_text[submit][message]", renewal_terms(cents)),
            (
                "expires_at",
                (chrono::Utc::now().timestamp() + 31 * 60).to_string(),
            ),
            ("success_url", format!("{origin}/supporters?subscribed=1")),
            ("cancel_url", format!("{origin}/supporters")),
        ],
        Some(&format!("sver-platform-checkout-{}", profiles::new_id())),
    )
    .await?;
    let (Some(id), Some(url)) = (session["id"].as_str(), session["url"].as_str()) else {
        return Err(Fail::unavailable("Stripe sent an unexpected response."));
    };
    sqlx::query("INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,detail) VALUES($1,$2,'platform',$3,$4)")
        .bind(id).bind(&user.id).bind(cents as i32).bind(json!({"tier": tier}))
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"url": url})))
}

/// The viewer's auto-renewing platform subscription: Stripe id, tier.
async fn renewing(app: &App, user: &str) -> Res<(String, i16)> {
    sqlx::query_as("SELECT stripe_subscription,tier FROM platform_subscriptions WHERE user_id=$1 AND stripe_subscription IS NOT NULL AND paid_through>now()")
        .bind(user).fetch_optional(&app.db).await?
        .ok_or_else(|| Fail::conflict("You don't have a supporter subscription that renews."))
}

#[derive(Deserialize)]
struct Change {
    tier: String,
    #[serde(default)]
    guardian_consent: bool,
}
/// POST /api/supporters/change: up to Greycloak now, with Stripe's prorated charge for the rest of
/// the month; down to Wolfsworn at the next renewal (the paid Greycloak month runs out first).
async fn change(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Change>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let tier = tier_named(&input.tier)?;
    let (subscription, current) = renewing(&app, &user.id).await?;
    // The row's last change: the same request sent twice reuses one Stripe idempotency key.
    let (pending, version): (Option<i16>, String) = sqlx::query_as(
        "SELECT pending_tier,(extract(epoch FROM updated_at)*1000000)::bigint::text FROM platform_subscriptions WHERE user_id=$1",
    )
    .bind(&user.id)
    .fetch_one(&app.db)
    .await?;
    if tier == pending.unwrap_or(current) {
        return Err(Fail::field("tier", "That's already your plan."));
    }
    let up = tier > current;
    let mut tx = app.db.begin().await?;
    let gate = format!("platform-upgrade:{subscription}:{version}");
    if up {
        switches::guard(&mut tx, "purchases").await?;
        // The prorated charge is at most the difference for a full month; it counts toward the
        // under-18 cap like any other purchase.
        let cents = price(tier) - price(current);
        ledger::lock(&mut tx, &format!("checkout:{}", user.id)).await?;
        support::card_gate(&mut tx, &user.id, cents, input.guardian_consent).await?;
        sqlx::query("INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,detail,status) VALUES($1,$2,'platform',$3,$4,'paid') ON CONFLICT(id) DO NOTHING")
            .bind(&gate).bind(&user.id).bind(cents as i32).bind(json!({"tier": tier, "upgrade": true}))
            .execute(&mut *tx).await?;
    }
    let sub = stripe::get(
        &app.http,
        &app.config.stripe,
        &format!("subscriptions/{subscription}"),
    )
    .await?;
    let item = &sub["items"]["data"][0];
    let (Some(item_id), Some(product)) = (item["id"].as_str(), item["price"]["product"].as_str())
    else {
        return Err(Fail::unavailable("Stripe sent an unexpected response."));
    };
    stripe::post(
        &app.http,
        &app.config.stripe,
        &format!("subscriptions/{subscription}"),
        &[
            ("items[0][id]", item_id.to_string()),
            ("items[0][price_data][currency]", "usd".into()),
            ("items[0][price_data][product]", product.to_string()),
            ("items[0][price_data][unit_amount]", price(tier).to_string()),
            ("items[0][price_data][recurring][interval]", "month".into()),
            // Up: charge the difference now. Down: nothing now; the next renewal bills the lower price.
            (
                "proration_behavior",
                if up { "always_invoice" } else { "none" }.into(),
            ),
            // Up: the change applies only if the prorated charge is paid now.
            (
                "payment_behavior",
                if up {
                    "error_if_incomplete"
                } else {
                    "allow_incomplete"
                }
                .into(),
            ),
            ("metadata[sver_platform_tier]", tier.to_string()),
        ],
        Some(&format!("sver-{gate}-{tier}")),
    )
    .await?;
    if up {
        sqlx::query("UPDATE platform_subscriptions SET tier=$2,pending_tier=NULL,updated_at=now() WHERE user_id=$1")
            .bind(&user.id).bind(tier).execute(&mut *tx).await?;
    } else if tier == current {
        // Undoing a downgrade that hadn't happened yet.
        sqlx::query(
            "UPDATE platform_subscriptions SET pending_tier=NULL,updated_at=now() WHERE user_id=$1",
        )
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE platform_subscriptions SET pending_tier=$2,updated_at=now() WHERE user_id=$1",
        )
        .bind(&user.id)
        .bind(tier)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(mine(&mut *app.db.acquire().await?, &user.id).await?))
}

/// POST /api/supporters/cancel: stops renewal; the supporter look lasts to the end of the paid month.
async fn cancel(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (subscription, _) = renewing(&app, &user.id).await?;
    stripe::post(
        &app.http,
        &app.config.stripe,
        &format!("subscriptions/{subscription}"),
        &[("cancel_at_period_end", "true".into())],
        None,
    )
    .await?;
    sqlx::query("UPDATE platform_subscriptions SET cancel_at_period_end=true,updated_at=now() WHERE stripe_subscription=$1")
        .bind(&subscription).execute(&app.db).await?;
    Ok(Json(mine(&mut *app.db.acquire().await?, &user.id).await?))
}

/// POST /api/supporters/resume: turns renewal back on before the paid month ends.
async fn resume(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (subscription, _) = renewing(&app, &user.id).await?;
    switches::guard(&mut *app.db.acquire().await?, "purchases").await?;
    stripe::post(
        &app.http,
        &app.config.stripe,
        &format!("subscriptions/{subscription}"),
        &[("cancel_at_period_end", "false".into())],
        None,
    )
    .await?;
    sqlx::query("UPDATE platform_subscriptions SET cancel_at_period_end=false,updated_at=now() WHERE stripe_subscription=$1")
        .bind(&subscription).execute(&app.db).await?;
    Ok(Json(mine(&mut *app.db.acquire().await?, &user.id).await?))
}

#[derive(Deserialize)]
struct FounderSetting {
    hidden: bool,
}
/// PUT /api/supporters/founder: a Founder hides their name from the public list (the spot then
/// reads "Founder #123").
async fn founder_setting(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<FounderSetting>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let changed = sqlx::query("UPDATE founders SET hidden=$2 WHERE user_id=$1")
        .bind(&user.id)
        .bind(input.hidden)
        .execute(&app.db)
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(Fail::missing());
    }
    Ok(Json(mine(&mut *app.db.acquire().await?, &user.id).await?))
}

/// The acknowledgment after a new subscription: its terms, the next charge and how to cancel.
async fn acknowledge(
    app: &App,
    tx: &mut PgConnection,
    user: &str,
    tier: i16,
    next: i64,
) -> Res<()> {
    let name = if tier == 2 { "Greycloak" } else { "Wolfsworn" };
    let next = chrono::DateTime::from_timestamp(next, 0)
        .map_or_else(String::new, |at| at.format("%B %-d, %Y").to_string());
    let body = format!(
        "Thank you for supporting S.V.E.R as a {name}.\n\n{}\nYour next charge is on {next}.\n\nTo change tier or cancel, open {}/wallet.",
        renewal_terms(price(tier)),
        app.config.origin
    );
    safety::queue_notice(app, tx, user, "You're supporting S.V.E.R", &body).await?;
    Ok(())
}

/// `invoice.paid` for a platform subscription: extends the paid period, counts a month, claims a
/// Founder spot for a first paid Greycloak invoice, and posts the money to `usd:supporters`.
pub(crate) async fn invoice_paid(app: &App, tx: &mut PgConnection, invoice: &Value) -> Res<()> {
    let details = &invoice["parent"]["subscription_details"];
    let meta = &details["metadata"];
    let (Some(id), Some("1"), Some(user), Some(tier)) = (
        invoice["id"].as_str(),
        meta["sver_platform"].as_str(),
        meta["sver_user"].as_str(),
        meta["sver_platform_tier"]
            .as_str()
            .and_then(|t| t.parse::<i16>().ok())
            .filter(|t| (1..=2).contains(t)),
    ) else {
        return Ok(());
    };
    let cents = invoice["amount_paid"].as_i64().unwrap_or(0);
    let Some(end) = invoice["lines"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| line["period"]["end"].as_i64())
        .max()
    else {
        return Err(Fail::bad("Invoice has no billing period."));
    };
    let intent = if cents > 0 {
        let payments = stripe::get(
            &app.http,
            &app.config.stripe,
            &format!("invoice_payments?invoice={id}"),
        )
        .await?;
        payments["data"][0]["payment"]["payment_intent"]
            .as_str()
            .map(str::to_owned)
    } else {
        None
    };
    let inserted = sqlx::query("INSERT INTO platform_invoices(id,user_id,tier,payment_intent,amount_cents) VALUES($1,$2,$3,$4,$5) ON CONFLICT(id) DO NOTHING")
        .bind(id).bind(user).bind(tier).bind(&intent).bind(cents as i32)
        .execute(&mut *tx).await?.rows_affected();
    if inserted == 0 {
        return Ok(());
    }
    let reason = invoice["billing_reason"].as_str().unwrap_or_default();
    let renewal = matches!(reason, "subscription_create" | "subscription_cycle");
    let month = i32::from(renewal && cents > 0);
    sqlx::query("INSERT INTO platform_subscriptions(user_id,tier,stripe_subscription,paid_through,months) VALUES($1,$2,$3,to_timestamp($4),$5)
        ON CONFLICT(user_id) DO UPDATE SET tier=EXCLUDED.tier,
            pending_tier=CASE WHEN $6 THEN NULL ELSE platform_subscriptions.pending_tier END,
            paid_through=greatest(platform_subscriptions.paid_through,EXCLUDED.paid_through),
            months=platform_subscriptions.months+EXCLUDED.months,
            stripe_subscription=CASE WHEN $7 THEN EXCLUDED.stripe_subscription ELSE platform_subscriptions.stripe_subscription END,
            cancel_at_period_end=CASE WHEN $7 THEN false ELSE platform_subscriptions.cancel_at_period_end END,
            since=CASE WHEN platform_subscriptions.months=0 THEN now() ELSE platform_subscriptions.since END,
            updated_at=now()")
        .bind(user).bind(tier).bind(details["subscription"].as_str()).bind(end as f64).bind(month)
        .bind(renewal).bind(reason == "subscription_create")
        .execute(&mut *tx).await?;
    // A Founder spot takes a paid Greycloak month (a new subscription or a renewal); an upgrade's
    // prorated charge waits for the first full Greycloak renewal.
    if tier == 2 && cents > 0 && renewal {
        claim_founder(tx, user, id).await?;
    }
    if reason == "subscription_create" {
        acknowledge(app, tx, user, tier, end).await?;
    }
    if cents > 0 {
        post_pairs(
            tx,
            "platform_sub",
            &format!("invoice:{id}"),
            json!({"invoice": id, "payment_intent": intent, "user": user, "tier": tier, "cents": cents}),
            &pairs(cents),
            |amount| amount,
        )
        .await?;
    }
    Ok(())
}
/// A platform payment: cash in, all of it to the platform (no creator share).
pub(crate) fn pairs(cents: i64) -> Vec<support::Pair> {
    vec![(
        "usd:stripe".into(),
        "usd:supporters".into(),
        "usd",
        cents * 10,
    )]
}
/// The lowest free Founder number, under a lock shared by every claim, so two payments settling at
/// once never get the same number. An account claims once; a full house claims nothing.
async fn claim_founder(tx: &mut PgConnection, user: &str, invoice: &str) -> Res<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(1179602516)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO founders(number,user_id,invoice_id) SELECT n,$1,$2 FROM generate_series(1,$3) n WHERE NOT EXISTS(SELECT 1 FROM founders WHERE number=n) AND NOT EXISTS(SELECT 1 FROM founders WHERE user_id=$1) ORDER BY n LIMIT 1")
        .bind(user).bind(invoice).bind(FOUNDERS)
        .execute(tx).await?;
    Ok(())
}

/// The amount of a platform payment by its intent, for refunds and disputes (support.rs).
pub(crate) async fn payment(tx: &mut PgConnection, intent: &str) -> Res<Option<i64>> {
    Ok(sqlx::query_scalar::<_, i32>(
        "SELECT amount_cents FROM platform_invoices WHERE payment_intent=$1",
    )
    .bind(intent)
    .fetch_optional(tx)
    .await?
    .map(i64::from))
}
/// A fully refunded or lost platform payment revokes the Founder spot it claimed, which frees the
/// number for the next new Greycloak. When it paid for the current period, that period ends now
/// and the subscription stops renewing; an older month's refund leaves a later paid month alone.
pub(crate) async fn reversed(app: &App, tx: &mut PgConnection, intent: &str, why: &str) -> Res<()> {
    let Some((invoice, user, latest)): Option<(String, String, bool)> =
        sqlx::query_as("SELECT i.id,i.user_id,NOT EXISTS(SELECT 1 FROM platform_invoices l WHERE l.user_id=i.user_id AND l.amount_cents>0 AND l.created_at>i.created_at) FROM platform_invoices i WHERE i.payment_intent=$1")
            .bind(intent)
            .fetch_optional(&mut *tx)
            .await?
    else {
        return Ok(());
    };
    if latest {
        let renewing: Option<String> = sqlx::query_scalar("UPDATE platform_subscriptions SET paid_through=least(paid_through,now()),cancel_at_period_end=true,pending_tier=NULL,updated_at=now() WHERE user_id=$1 RETURNING stripe_subscription")
            .bind(&user).fetch_optional(&mut *tx).await?.flatten();
        // Nothing more is charged for a payment taken back. A Stripe error fails the webhook, which
        // Stripe then retries.
        if let Some(subscription) = renewing {
            stripe::post(
                &app.http,
                &app.config.stripe,
                &format!("subscriptions/{subscription}"),
                &[("cancel_at_period_end", "true".into())],
                None,
            )
            .await?;
        }
    }
    let revoked: Option<i32> =
        sqlx::query_scalar("DELETE FROM founders WHERE invoice_id=$1 RETURNING number")
            .bind(&invoice)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(number) = revoked {
        safety::audit(
            tx,
            None,
            "founder_revoked",
            "user",
            &user,
            &[],
            why,
            json!({"number": number, "invoice": invoice}),
            false,
        )
        .await?;
    }
    Ok(())
}
/// `customer.subscription.updated` / `.deleted`: renewal state; an ended subscription keeps the
/// look until the paid period runs out.
pub(crate) async fn subscription_changed(
    tx: &mut PgConnection,
    kind: &str,
    sub: &Value,
) -> Res<()> {
    let id = sub["id"].as_str().unwrap_or_default();
    let ended = kind == "customer.subscription.deleted"
        || matches!(
            sub["status"].as_str(),
            Some("canceled" | "incomplete_expired" | "unpaid")
        );
    if ended {
        sqlx::query("UPDATE platform_subscriptions SET stripe_subscription=NULL,cancel_at_period_end=false,pending_tier=NULL,updated_at=now() WHERE stripe_subscription=$1")
            .bind(id).execute(tx).await?;
    } else {
        sqlx::query("UPDATE platform_subscriptions SET cancel_at_period_end=$2,updated_at=now() WHERE stripe_subscription=$1")
            .bind(id).bind(sub["cancel_at_period_end"].as_bool().unwrap_or(false))
            .execute(tx).await?;
    }
    Ok(())
}

// ---- Staff ----

/// GET /api/admin/supporters: subscribers, Founders and the money total.
async fn admin(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let subscribers: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('username',u.username,'tier',CASE p.tier WHEN 2 THEN 'greycloak' ELSE 'wolfsworn' END,'pending_tier',p.pending_tier,'paid_through',p.paid_through,'auto_renew',p.stripe_subscription IS NOT NULL AND NOT p.cancel_at_period_end,'months',p.months,'since',p.since,'founder',(SELECT f.number FROM founders f WHERE f.user_id=p.user_id)) FROM platform_subscriptions p JOIN users u ON u.id=p.user_id ORDER BY p.paid_through>now() DESC,p.updated_at DESC LIMIT 500")
        .fetch_all(&mut *db).await?;
    let founders: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('number',f.number,'username',u.username,'hidden',f.hidden,'claimed_at',f.claimed_at) FROM founders f JOIN users u ON u.id=f.user_id ORDER BY f.number")
        .fetch_all(&mut *db).await?;
    let total: i64 =
        sqlx::query_scalar("SELECT coalesce(sum(amount_cents),0)::bigint FROM platform_invoices")
            .fetch_one(&mut *db)
            .await?;
    Ok(Json(
        json!({"subscribers": subscribers, "founders": founders, "paid_cents": total}),
    ))
}

#[derive(Deserialize)]
struct StaffAction {
    note: Option<String>,
}
/// POST /api/admin/supporters/{username}/refund: refunds the latest paid platform invoice in full.
/// The `charge.refunded` webhook then reverses the ledger, ends the period and frees a Founder
/// number it claimed.
async fn staff_refund(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<StaffAction>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let intent: Option<String> = sqlx::query_scalar("SELECT i.payment_intent FROM platform_invoices i JOIN users u ON u.id=i.user_id WHERE u.username=$1 AND i.payment_intent IS NOT NULL ORDER BY i.created_at DESC LIMIT 1")
        .bind(&name).fetch_optional(&app.db).await?.flatten();
    let intent = intent.ok_or_else(|| Fail::conflict("No payment to refund."))?;
    stripe::post(
        &app.http,
        &app.config.stripe,
        "refunds",
        &[("payment_intent", intent.clone())],
        Some(&format!("staff-refund-{intent}")),
    )
    .await?;
    let mut tx = app.db.begin().await?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "supporter_refund",
        "payment",
        &intent,
        &[],
        &note,
        json!({"username": name}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
/// POST /api/admin/supporters/founders/{number}/revoke: takes a Founder mark away (a payment
/// problem Stripe didn't report). Staff can never grant one.
async fn staff_revoke(
    State(app): State<App>,
    jar: CookieJar,
    Path(number): Path<i32>,
    Json(input): Json<StaffAction>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let user: Option<String> =
        sqlx::query_scalar("DELETE FROM founders WHERE number=$1 RETURNING user_id")
            .bind(number)
            .fetch_optional(&mut *tx)
            .await?;
    let user = user.ok_or_else(Fail::missing)?;
    safety::audit(
        &mut tx,
        Some(&staff.id),
        "founder_revoked",
        "user",
        &user,
        &[],
        &note,
        json!({"number": number}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/supporters", get(overview))
        .route("/api/supporters/subscribe", post(subscribe))
        .route("/api/supporters/change", post(change))
        .route("/api/supporters/cancel", post(cancel))
        .route("/api/supporters/resume", post(resume))
        .route("/api/supporters/founder", put(founder_setting))
        .route("/api/me/supporter", get(my_marks))
        .route("/api/admin/supporters", get(admin))
        .route(
            "/api/admin/supporters/{username}/refund",
            post(staff_refund),
        )
        .route(
            "/api/admin/supporters/founders/{number}/revoke",
            post(staff_revoke),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiers_and_terms() {
        assert_eq!(
            (
                tier_named("wolfsworn").unwrap(),
                tier_named("greycloak").unwrap()
            ),
            (1, 2)
        );
        assert!(tier_named("gold").is_err());
        assert_eq!((price(1), price(2)), (500, 2_500));
        assert!(
            renewal_terms(2_500).contains("$25.00")
                && renewal_terms(500).contains("until you cancel")
        );
    }
}
