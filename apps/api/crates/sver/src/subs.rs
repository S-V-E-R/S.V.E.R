//! Module 6 Support, part 2 (docs/SUPPORT.md "Subscriptions"): three tiers paid monthly by card
//! (Stripe Billing, auto-renewing) or one month at a time with Purchased Valor, gift subs, badges,
//! subscriber emotes and subscriber-only chat. Every payment posts to the ledger.
use crate::{
    App, ledger,
    profiles::{self, Fail, Res},
    security as sec, stripe,
    support::{self, card_pairs, post_pairs},
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

/// Monthly price in cents per tier; a Valor month costs the same number of Valor.
pub const PRICES: [i64; 3] = [499, 999, 2_499];
/// The streamer's share of card subscription and gift revenue.
// ponytail: every creator is Scout (65%) until weekly creator-tier checks ship; read the owner's
// tier here then.
pub const SPLIT_PERCENT: i64 = 65;
const GIFT_COUNTS: [i64; 4] = [1, 5, 10, 20];

fn tier(value: i16) -> Res<i16> {
    if (1..=3).contains(&value) {
        Ok(value)
    } else {
        Err(Fail::field("tier", "Choose tier 1, 2 or 3."))
    }
}
fn price(tier: i16) -> i64 {
    PRICES[(tier - 1) as usize]
}

/// The viewer's active tier in a channel, if subscribed.
pub async fn active_tier(app: &App, channel: &str, user: &str) -> Res<Option<i16>> {
    Ok(sqlx::query_scalar(
        "SELECT tier FROM channel_subs WHERE channel_id=$1 AND user_id=$2 AND paid_through>now()",
    )
    .bind(channel)
    .bind(user)
    .fetch_optional(&app.db)
    .await?)
}

/// The channel's owner id and username (same 404 as profiles for unknown or restricted channels).
async fn channel(app: &App, name: &str) -> Res<(String, String)> {
    let mut conn = app.db.acquire().await?;
    let owner = profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    Ok((owner.id, owner.username))
}
/// A signed-in, verified viewer who may pay this channel: not its owner, not banned or blocked,
/// and the channel can earn.
async fn payer(app: &App, jar: &CookieJar, name: &str) -> Res<(crate::auth::User, String, String)> {
    let user = profiles::signed_in(app, jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to subscribe.")?;
    let (channel, username) = channel(app, name).await?;
    if channel == user.id {
        return Err(Fail::bad("You can't subscribe to your own channel."));
    }
    let mut db = app.db.acquire().await?;
    let refused: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1)) OR EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=$1 AND user_id=$2 AND kind='ban')")
        .bind(&channel).bind(&user.id).fetch_one(&mut *db).await?;
    if refused {
        return Err(Fail::denied("You can't support this channel."));
    }
    if !support::can_earn(&mut db, &channel).await? {
        return Err(Fail::denied("This channel can't take subscriptions yet."));
    }
    Ok((user, channel, username))
}

/// Adds one month (a Valor month or a gift). Gifts keep a higher tier the viewer already has.
async fn grant(
    tx: &mut PgConnection,
    channel: &str,
    user: &str,
    tier: i16,
    keep_higher: bool,
) -> Res<()> {
    sqlx::query("INSERT INTO channel_subs(channel_id,user_id,tier,paid_through,months) VALUES($1,$2,$3,now()+interval '1 month',1)
        ON CONFLICT(channel_id,user_id) DO UPDATE SET
            tier=CASE WHEN $4 AND channel_subs.paid_through>now() THEN greatest(channel_subs.tier,EXCLUDED.tier) ELSE EXCLUDED.tier END,
            paid_through=greatest(channel_subs.paid_through,now())+interval '1 month',
            months=channel_subs.months+1, updated_at=now()")
        .bind(channel).bind(user).bind(tier).bind(keep_higher)
        .execute(tx).await?;
    Ok(())
}

/// Spends the viewer's Purchased Valor on a channel inside `tx` (0.8¢ per Valor to the streamer).
/// Returns false when `reference` was already spent, so a retried request changes nothing.
async fn spend(
    tx: &mut PgConnection,
    user: &str,
    channel: &str,
    valor: i64,
    kind: &str,
    reference: &str,
) -> Res<bool> {
    let wallet = format!("valor:{user}");
    ledger::lock(tx, &wallet).await?;
    let balance = ledger::balance(tx, &wallet, "valor").await?;
    if balance < 0 {
        return Err(Fail::field(
            "pay",
            "Your Valor is locked until a payment problem is settled.",
        ));
    }
    if balance < valor {
        return Err(Fail::field("pay", "You don't have enough Valor."));
    }
    let earned = valor * ledger::EARN_PER_VALOR;
    ledger::post(
        tx,
        kind,
        reference,
        json!({"channel": channel, "from": user, "valor": valor}),
        &[
            (&wallet, "valor", -valor),
            ("valor:spent", "valor", valor),
            (&format!("usd:earnings:{channel}"), "usd", earned),
            ("usd:platform", "usd", -earned),
        ],
    )
    .await
}
fn request_id(id: &Option<String>) -> Res<&str> {
    id.as_deref()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or_else(|| Fail::bad("Invalid request ID."))
}

/// Starts a Stripe Checkout; the checkout lock and the under-18 rules are the caller's.
async fn start_checkout(
    app: &App,
    tx: &mut PgConnection,
    user: &str,
    kind: &str,
    cents: i64,
    mut form: Vec<(&str, String)>,
    detail: Value,
) -> Res<String> {
    form.extend([
        ("client_reference_id", user.to_string()),
        ("metadata[sver_kind]", kind.to_string()),
        (
            "expires_at",
            (chrono::Utc::now().timestamp() + 31 * 60).to_string(),
        ),
    ]);
    let session = stripe::post(
        &app.http,
        &app.config.stripe,
        "checkout/sessions",
        &form,
        Some(&format!("sver-{kind}-checkout-{}", profiles::new_id())),
    )
    .await?;
    let (Some(id), Some(url)) = (session["id"].as_str(), session["url"].as_str()) else {
        return Err(Fail::unavailable("Stripe sent an unexpected response."));
    };
    sqlx::query(
        "INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,detail) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(user)
    .bind(kind)
    .bind(cents as i32)
    .bind(detail)
    .execute(tx)
    .await?;
    Ok(url.to_string())
}

/// GET /api/channels/{username}/subscription: prices, whether the channel takes subscriptions,
/// and the viewer's own subscription.
async fn status(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let (channel, _) = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let can_earn = support::can_earn(&mut db, &channel).await?;
    let mine: Option<Value> = match &viewer {
        Some(v) => sqlx::query_scalar("SELECT jsonb_build_object('tier',tier,'paid_through',paid_through,'months',months,'auto_renew',stripe_subscription IS NOT NULL AND NOT cancel_at_period_end,'card',stripe_subscription IS NOT NULL) FROM channel_subs WHERE channel_id=$1 AND user_id=$2 AND paid_through>now()")
            .bind(&channel).bind(&v.id).fetch_optional(&mut *db).await?,
        None => None,
    };
    let own = viewer.as_ref().is_some_and(|v| v.id == channel);
    Ok(Json(json!({
        "available": app.config.stripe.available(),
        "can_subscribe": can_earn && !own,
        "own": own,
        "tiers": (1..=3).map(|t| json!({"tier": t, "cents": price(t), "valor": price(t)})).collect::<Vec<_>>(),
        "gift_counts": GIFT_COUNTS,
        "mine": mine,
    })))
}

#[derive(Deserialize)]
pub struct Subscribe {
    tier: i16,
    pay: String,
    id: Option<String>,
    #[serde(default)]
    guardian_consent: bool,
}
/// POST /api/channels/{username}/subscription: a card subscription (Stripe Checkout, renews
/// monthly) or one Valor month.
async fn subscribe(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Subscribe>,
) -> Res<Json<Value>> {
    let (user, channel, username) = payer(&app, &jar, &name).await?;
    let tier = tier(input.tier)?;
    let renewing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_subs WHERE channel_id=$1 AND user_id=$2 AND stripe_subscription IS NOT NULL AND paid_through>now())")
        .bind(&channel).bind(&user.id).fetch_one(&app.db).await?;
    if renewing {
        return Err(Fail::conflict(
            "You already have an auto-renewing subscription here. Upgrade or cancel it instead.",
        ));
    }
    sec::reserve(&app, vec![format!("subscribe:{}", user.id)], 20, 3600).await?;
    let mut tx = app.db.begin().await?;
    ledger::lock(&mut tx, &format!("checkout:{}", user.id)).await?;
    match input.pay.as_str() {
        "valor" => {
            let id = request_id(&input.id)?;
            if spend(
                &mut tx,
                &user.id,
                &channel,
                price(tier),
                "valor_sub",
                &format!("valor-sub:{id}"),
            )
            .await?
            {
                grant(&mut tx, &channel, &user.id, tier, false).await?;
            }
            tx.commit().await?;
            Ok(Json(json!({"subscribed": true})))
        }
        "card" => {
            let cents = price(tier);
            support::card_gate(&mut tx, &user.id, cents, input.guardian_consent).await?;
            let origin = &app.config.origin;
            let url = start_checkout(
                &app,
                &mut tx,
                &user.id,
                "sub",
                cents,
                vec![
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
                        format!("{username} subscription, Tier {tier}"),
                    ),
                    ("subscription_data[metadata][sver_channel]", channel.clone()),
                    ("subscription_data[metadata][sver_user]", user.id.clone()),
                    ("subscription_data[metadata][sver_tier]", tier.to_string()),
                    ("success_url", format!("{origin}/{username}?subscribed=1")),
                    ("cancel_url", format!("{origin}/{username}")),
                ],
                json!({"channel": channel, "tier": tier}),
            )
            .await?;
            tx.commit().await?;
            Ok(Json(json!({"url": url})))
        }
        _ => Err(Fail::field("pay", "Pay by card or with Valor.")),
    }
}

/// The viewer's auto-renewing Stripe subscription in a channel.
async fn renewing(app: &App, channel: &str, user: &str) -> Res<(String, i16)> {
    sqlx::query_as("SELECT stripe_subscription,tier FROM channel_subs WHERE channel_id=$1 AND user_id=$2 AND stripe_subscription IS NOT NULL AND paid_through>now()")
        .bind(channel).bind(user).fetch_optional(&app.db).await?
        .ok_or_else(|| Fail::conflict("You don't have an auto-renewing subscription here."))
}

#[derive(Deserialize)]
pub struct Change {
    tier: i16,
}
/// POST /api/channels/{username}/subscription/upgrade: a higher tier now, with Stripe's prorated
/// charge for the rest of the month (credited by its invoice).
async fn upgrade(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Change>,
) -> Res<Json<Value>> {
    let (user, channel, _) = payer(&app, &jar, &name).await?;
    let tier = tier(input.tier)?;
    let (subscription, current) = renewing(&app, &channel, &user.id).await?;
    if tier <= current {
        return Err(Fail::field(
            "tier",
            "Choose a higher tier. To move down, cancel and subscribe again when this month ends.",
        ));
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
            ("proration_behavior", "always_invoice".into()),
            ("metadata[sver_tier]", tier.to_string()),
        ],
        Some(&format!("sver-upgrade-{subscription}-{tier}")),
    )
    .await?;
    sqlx::query(
        "UPDATE channel_subs SET tier=$3,updated_at=now() WHERE channel_id=$1 AND user_id=$2",
    )
    .bind(&channel)
    .bind(&user.id)
    .bind(tier)
    .execute(&app.db)
    .await?;
    Ok(Json(json!({"tier": tier})))
}

/// POST /api/channels/{username}/subscription/cancel: stops auto-renewal; benefits run to the end
/// of the paid month.
async fn cancel(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (channel, _) = channel(&app, &name).await?;
    let (subscription, _) = renewing(&app, &channel, &user.id).await?;
    stripe::post(
        &app.http,
        &app.config.stripe,
        &format!("subscriptions/{subscription}"),
        &[("cancel_at_period_end", "true".into())],
        None,
    )
    .await?;
    sqlx::query("UPDATE channel_subs SET cancel_at_period_end=true,updated_at=now() WHERE stripe_subscription=$1")
        .bind(&subscription).execute(&app.db).await?;
    Ok(Json(json!({"auto_renew": false})))
}

/// Who can receive a gift in channel $1 from gifter $2: verified, gifts allowed, not banned or
/// blocked, and without an auto-renewing subscription there already.
const RECEIVES: &str = "u.id<>$1 AND u.id<>$2 AND u.email_verified AND u.allow_gifts AND u.deleted_at IS NULL
    AND EXISTS(SELECT 1 FROM channel_users c WHERE c.id=u.id AND c.eligible)
    AND NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=$1 AND r.user_id=u.id AND r.kind='ban')
    AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE (b.blocker_id IN ($1,$2) AND b.blocked_id=u.id) OR (b.blocker_id=u.id AND b.blocked_id IN ($1,$2)))
    AND NOT EXISTS(SELECT 1 FROM channel_subs s WHERE s.channel_id=$1 AND s.user_id=u.id AND s.stripe_subscription IS NOT NULL AND s.paid_through>now())";

#[derive(Deserialize)]
pub struct Gift {
    tier: i16,
    count: i64,
    recipient: Option<String>,
    pay: String,
    id: Option<String>,
    #[serde(default)]
    guardian_consent: bool,
}
/// POST /api/channels/{username}/gifts: one month to a named viewer, or 5, 10 or 20 months to
/// random signed-in chatters from the last day who allow gifts. Gifted months never renew.
async fn gift(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Gift>,
) -> Res<Json<Value>> {
    let (user, channel, username) = payer(&app, &jar, &name).await?;
    let tier = tier(input.tier)?;
    if !GIFT_COUNTS.contains(&input.count) {
        return Err(Fail::field("count", "Gift 1, 5, 10 or 20 subscriptions."));
    }
    sec::reserve(&app, vec![format!("gift:{}", user.id)], 20, 3600).await?;
    let mut tx = app.db.begin().await?;
    ledger::lock(&mut tx, &format!("checkout:{}", user.id)).await?;
    let recipients: Vec<String> = if input.count == 1 {
        let Some(recipient) = input
            .recipient
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
        else {
            return Err(Fail::field("recipient", "Enter who the gift is for."));
        };
        // RECEIVES is fixed SQL; every value is bound.
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT u.id FROM users u WHERE lower(u.username)=lower(ltrim($3,'@')) AND {RECEIVES}"
        )))
        .bind(&channel)
        .bind(&user.id)
        .bind(recipient)
        .fetch_all(&mut *tx)
        .await?
    } else {
        // RECEIVES is fixed SQL; every value is bound.
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT u.id FROM users u WHERE u.id IN (SELECT author_id FROM chat_messages WHERE channel_id=$1 AND squad_id IS NULL AND created_at>now()-interval '1 day') AND {RECEIVES} ORDER BY random() LIMIT $3"
        )))
        .bind(&channel)
        .bind(&user.id)
        .bind(input.count)
        .fetch_all(&mut *tx)
        .await?
    };
    if input.count == 1 && recipients.is_empty() {
        return Err(Fail::field(
            "recipient",
            "That viewer can't receive a gift here. They may have turned gifts off or already have an auto-renewing subscription.",
        ));
    }
    if (recipients.len() as i64) < input.count {
        return Err(Fail::field_owned(
            "count",
            format!(
                "Only {} recent chatters can receive gifts right now.",
                recipients.len()
            ),
        ));
    }
    let count = input.count;
    match input.pay.as_str() {
        "valor" => {
            let id = request_id(&input.id)?;
            if spend(
                &mut tx,
                &user.id,
                &channel,
                count * price(tier),
                "valor_gift",
                &format!("valor-gift:{id}"),
            )
            .await?
            {
                for recipient in &recipients {
                    grant(&mut tx, &channel, recipient, tier, true).await?;
                }
            }
            tx.commit().await?;
            Ok(Json(json!({"gifted": count})))
        }
        "card" => {
            let cents = count * price(tier);
            support::card_gate(&mut tx, &user.id, cents, input.guardian_consent).await?;
            let origin = &app.config.origin;
            let what = if count == 1 {
                "1 gift subscription".to_string()
            } else {
                format!("{count} gift subscriptions")
            };
            // Recipients are chosen now and granted when Stripe confirms the payment.
            let url = start_checkout(
                &app,
                &mut tx,
                &user.id,
                "gift",
                cents,
                vec![
                    ("mode", "payment".into()),
                    ("line_items[0][quantity]", "1".into()),
                    ("line_items[0][price_data][currency]", "usd".into()),
                    ("line_items[0][price_data][unit_amount]", cents.to_string()),
                    (
                        "line_items[0][price_data][product_data][name]",
                        format!("{what} to {username}, Tier {tier}"),
                    ),
                    ("payment_intent_data[metadata][sver_user]", user.id.clone()),
                    ("success_url", format!("{origin}/{username}?gifted=1")),
                    ("cancel_url", format!("{origin}/{username}")),
                ],
                json!({"channel": channel, "tier": tier, "recipients": recipients,
                    "share_tenths": cents * 10 * SPLIT_PERCENT / 100}),
            )
            .await?;
            tx.commit().await?;
            Ok(Json(json!({"url": url})))
        }
        _ => Err(Fail::field("pay", "Pay by card or with Valor.")),
    }
}

/// A paid gift checkout grants its reserved recipients (called once, under the checkout row lock).
pub(crate) async fn grant_gifts(tx: &mut PgConnection, detail: &Value) -> Res<()> {
    let channel = detail["channel"].as_str().unwrap_or_default();
    let tier = detail["tier"].as_i64().unwrap_or(1) as i16;
    for recipient in detail["recipients"].as_array().into_iter().flatten() {
        if let Some(recipient) = recipient.as_str() {
            grant(tx, channel, recipient, tier, true).await?;
        }
    }
    Ok(())
}

/// `invoice.paid` for a card subscription: extends it to the end of the billed period, counts a
/// new month (not for an upgrade's proration) and posts the streamer's share.
pub(crate) async fn invoice_paid(app: &App, tx: &mut PgConnection, invoice: &Value) -> Res<()> {
    let details = &invoice["parent"]["subscription_details"];
    let meta = &details["metadata"];
    let (Some(id), Some(channel), Some(user), Some(tier)) = (
        invoice["id"].as_str(),
        meta["sver_channel"].as_str(),
        meta["sver_user"].as_str(),
        meta["sver_tier"]
            .as_str()
            .and_then(|t| t.parse::<i16>().ok())
            .filter(|t| (1..=3).contains(t)),
    ) else {
        // Not one of ours.
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
    // Since API version 2025-03-31.basil the payment is linked through Invoice Payments.
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
    let share = cents * 10 * SPLIT_PERCENT / 100;
    let inserted = sqlx::query("INSERT INTO sub_invoices(id,channel_id,user_id,tier,payment_intent,amount_cents,share_tenths) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(id) DO NOTHING")
        .bind(id).bind(channel).bind(user).bind(tier).bind(&intent).bind(cents as i32).bind(share)
        .execute(&mut *tx).await?.rows_affected();
    if inserted == 0 {
        return Ok(());
    }
    let reason = invoice["billing_reason"].as_str().unwrap_or_default();
    let month = i32::from(matches!(
        reason,
        "subscription_create" | "subscription_cycle"
    ));
    sqlx::query("INSERT INTO channel_subs(channel_id,user_id,tier,paid_through,months,stripe_subscription) VALUES($1,$2,$3,to_timestamp($4),$5,$6)
        ON CONFLICT(channel_id,user_id) DO UPDATE SET tier=EXCLUDED.tier,
            paid_through=greatest(channel_subs.paid_through,EXCLUDED.paid_through),
            months=channel_subs.months+EXCLUDED.months,
            stripe_subscription=coalesce(EXCLUDED.stripe_subscription,channel_subs.stripe_subscription),
            cancel_at_period_end=CASE WHEN $7 THEN false ELSE channel_subs.cancel_at_period_end END,
            updated_at=now()")
        .bind(channel).bind(user).bind(tier).bind(end as f64).bind(month)
        .bind(details["subscription"].as_str()).bind(reason == "subscription_create")
        .execute(&mut *tx).await?;
    if cents > 0 {
        post_pairs(
            tx,
            "sub_payment",
            &format!("invoice:{id}"),
            json!({"invoice": id, "payment_intent": intent, "channel": channel, "user": user, "tier": tier, "cents": cents}),
            &card_pairs(channel, cents, share),
            |amount| amount,
        )
        .await?;
    }
    Ok(())
}

/// `customer.subscription.updated` / `.deleted`: auto-renewal state; an ended subscription keeps
/// its benefits until the paid period runs out.
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
        sqlx::query("UPDATE channel_subs SET stripe_subscription=NULL,cancel_at_period_end=false,updated_at=now() WHERE stripe_subscription=$1")
            .bind(id).execute(tx).await?;
    } else {
        sqlx::query("UPDATE channel_subs SET cancel_at_period_end=$2,updated_at=now() WHERE stripe_subscription=$1")
            .bind(id).bind(sub["cancel_at_period_end"].as_bool().unwrap_or(false))
            .execute(tx).await?;
    }
    Ok(())
}

/// A fully refunded subscription payment ends that subscription's benefits now.
pub(crate) async fn refunded_in_full(tx: &mut PgConnection, intent: &str) -> Res<()> {
    sqlx::query("UPDATE channel_subs s SET paid_through=least(s.paid_through,now()),updated_at=now() FROM sub_invoices i WHERE i.payment_intent=$1 AND s.channel_id=i.channel_id AND s.user_id=i.user_id")
        .bind(intent).execute(tx).await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct GiftSetting {
    allow: bool,
}
/// PUT /api/me/gifts: whether the viewer can receive gift subs.
async fn gift_setting(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<GiftSetting>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query("UPDATE users SET allow_gifts=$2 WHERE id=$1")
        .bind(&user.id)
        .bind(input.allow)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"allow_gifts": input.allow})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/channels/{username}/subscription",
            get(status).post(subscribe),
        )
        .route(
            "/api/channels/{username}/subscription/upgrade",
            post(upgrade),
        )
        .route("/api/channels/{username}/subscription/cancel", post(cancel))
        .route("/api/channels/{username}/gifts", post(gift))
        .route("/api/me/gifts", put(gift_setting))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_and_prices() {
        assert!(tier(0).is_err() && tier(4).is_err());
        assert_eq!((price(1), price(2), price(3)), (499, 999, 2_499));
        // The streamer's 65% of a $4.99 month, in tenths of a cent: $3.2435 → 3243.
        assert_eq!(499 * 10 * SPLIT_PERCENT / 100, 3_243);
    }
}
