//! Module 6 Support, part 2 (docs/SUPPORT.md "Subscriptions"): three tiers paid monthly by card
//! (Stripe Billing, auto-renewing) or one month at a time with Purchased Valor, gift subs, badges,
//! subscriber emotes and subscriber-only chat. Every payment posts to the ledger.
use crate::{
    App, ledger,
    profiles::{self, Fail, Res},
    security as sec, stripe,
    support::{self, post_pairs, share_pairs, split_equally},
    tiers,
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

/// What an auto-renewing card subscription agrees to, shown beside every place it's started.
fn renewal_terms(cents: i64) -> String {
    format!(
        "Renews automatically every month at ${}.{:02} until you cancel. Cancel anytime from the channel's Subscribe panel; benefits last to the end of the paid month.",
        cents / 100,
        cents % 100
    )
}

/// The acknowledgment after a new card subscription: its terms, the next charge and how to cancel.
async fn acknowledge(
    app: &App,
    tx: &mut PgConnection,
    channel: &str,
    user: &str,
    tier: i16,
    paid_through: i64,
) -> Res<()> {
    let names: Option<(String, String)> =
        sqlx::query_as("SELECT username,display_name FROM channel_users WHERE id=$1")
            .bind(channel)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((username, display_name)) = names else {
        return Ok(());
    };
    let next = chrono::DateTime::from_timestamp(paid_through, 0)
        .map_or_else(String::new, |at| at.format("%B %-d, %Y").to_string());
    let subject = format!("Your subscription to {display_name} on S.V.E.R");
    let body = format!(
        "You subscribed to {display_name} at Tier {tier}.\n\n{}\nYour next charge is on {next}.\n\nTo cancel, open {}/{username}, choose Subscribed, then Cancel renewal.",
        renewal_terms(price(tier)),
        app.config.origin
    );
    crate::safety::queue_notice(app, tx, user, &subject, &body).await?;
    Ok(())
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
        .execute(&mut *tx).await?;
    // Subscribing counts toward Surge for a viewer who is watching (docs/CROWDSYNC.md).
    crate::surge::participated(tx, channel, user).await?;
    Ok(())
}

/// Card revenue shares (tenths of a cent): each streamer at their own tier split, the money split
/// equally among a merged co-stream's members.
async fn card_shares(
    db: &mut PgConnection,
    members: &[String],
    cents: i64,
) -> Res<Vec<(String, i64)>> {
    let mut shares = Vec::new();
    for (member, part) in split_equally(members, cents * 10) {
        let split = tiers::split(db, &member).await?;
        shares.push((member, part * split / 100));
    }
    Ok(shares)
}
fn shares_json(shares: &[(String, i64)]) -> Value {
    shares.iter().map(|(m, s)| json!([m, s])).collect()
}
/// Who shares money spent on `channel`: the channel alone, or, through a merged co-stream's page,
/// its members live now (docs/SUPPORT.md "Co-streams"). Members can't pay their own squad.
async fn pool(app: &App, squad: &Option<String>, channel: &str, payer: &str) -> Res<Vec<String>> {
    let Some(squad) = squad else {
        return Ok(vec![channel.to_string()]);
    };
    let members = crate::squads::chat_context(app, squad, None).await?;
    if !members.iter().any(|m| m == channel) {
        return Err(Fail::conflict("That channel isn't in this co-stream."));
    }
    if members.iter().any(|m| m == payer) {
        return Err(Fail::bad("You can't pay a co-stream you're part of."));
    }
    Ok(members)
}

/// Spends the viewer's Purchased Valor inside `tx`: 0.8¢ per Valor, split equally among
/// `earners` (one channel, or a merged co-stream's members). Returns false when `reference` was
/// already spent, so a retried request changes nothing.
async fn spend(
    tx: &mut PgConnection,
    user: &str,
    earners: &[String],
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
    let shares: Vec<(String, i64)> = split_equally(earners, earned)
        .into_iter()
        .map(|(m, s)| (format!("usd:earnings:{m}"), s))
        .collect();
    let mut entries = vec![
        (wallet.as_str(), "valor", -valor),
        ("valor:spent", "valor", valor),
        ("usd:platform", "usd", -earned),
    ];
    entries.extend(shares.iter().map(|(a, s)| (a.as_str(), "usd", *s)));
    ledger::post(
        tx,
        kind,
        reference,
        json!({"earners": earners, "from": user, "valor": valor}),
        &entries,
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
    /// Bought through a merged co-stream's page: the first month is pooled.
    squad: Option<String>,
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
    let pool = pool(&app, &input.squad, &channel, &user.id).await?;
    let mut tx = app.db.begin().await?;
    ledger::lock(&mut tx, &format!("checkout:{}", user.id)).await?;
    match input.pay.as_str() {
        "valor" => {
            let id = request_id(&input.id)?;
            if spend(
                &mut tx,
                &user.id,
                &pool,
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
            let mut form = vec![
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
                // The renewal terms sit right above Checkout's pay button.
                ("custom_text[submit][message]", renewal_terms(cents)),
            ];
            if pool.len() > 1 {
                form.push(("subscription_data[metadata][sver_pool]", pool.join(",")));
            }
            let url = start_checkout(
                &app,
                &mut tx,
                &user.id,
                "sub",
                cents,
                form,
                json!({"channel": channel, "tier": tier, "pool": pool}),
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
    /// Bought through a merged co-stream's page: the money is pooled.
    squad: Option<String>,
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
    let pool = pool(&app, &input.squad, &channel, &user.id).await?;
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
                &pool,
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
            let shares = card_shares(&mut tx, &pool, cents).await?;
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
                    "shares": shares_json(&shares), "share_tenths": shares.iter().map(|s| s.1).sum::<i64>()}),
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
    let reason = invoice["billing_reason"].as_str().unwrap_or_default();
    // A merged co-stream's first month is pooled among its members then; renewals go to the
    // channel the viewer picked.
    let pooled: Vec<String> = match meta["sver_pool"].as_str() {
        Some(pool) if reason == "subscription_create" => {
            pool.split(',').map(str::to_owned).collect()
        }
        _ => vec![channel.to_string()],
    };
    let shares = card_shares(tx, &pooled, cents).await?;
    let share: i64 = shares.iter().map(|s| s.1).sum();
    let stored = (pooled.len() > 1).then(|| shares_json(&shares));
    let inserted = sqlx::query("INSERT INTO sub_invoices(id,channel_id,user_id,tier,payment_intent,amount_cents,share_tenths,shares) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(id) DO NOTHING")
        .bind(id).bind(channel).bind(user).bind(tier).bind(&intent).bind(cents as i32).bind(share).bind(&stored)
        .execute(&mut *tx).await?.rows_affected();
    if inserted == 0 {
        return Ok(());
    }
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
    if reason == "subscription_create" {
        acknowledge(app, tx, channel, user, tier, end).await?;
    }
    if cents > 0 {
        post_pairs(
            tx,
            "sub_payment",
            &format!("invoice:{id}"),
            json!({"invoice": id, "payment_intent": intent, "channel": channel, "user": user, "tier": tier, "cents": cents}),
            &share_pairs(cents, &shares),
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
        // A Scout's 65% of a $4.99 month, in tenths of a cent: $3.2435 → 3243.
        assert_eq!(499 * 10 * tiers::SPLITS[0] / 100, 3_243);
        // Pooled money splits exactly.
        let members = ["a".to_string(), "b".to_string(), "c".to_string()];
        let parts = split_equally(&members, 100);
        assert_eq!(
            parts.iter().map(|p| p.1).collect::<Vec<_>>(),
            vec![34, 33, 33]
        );
    }
}
