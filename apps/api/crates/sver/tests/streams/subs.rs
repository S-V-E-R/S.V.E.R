//! Module 6 Support, part 2 against a synthetic Stripe: Valor months, card subscriptions through
//! invoice webhooks (renewal, upgrade proration, cancel, end), badges, subscriber-only chat,
//! subscriber emotes, gifts (random chatters by Valor, a named viewer by card), refunds and a
//! balanced ledger. Runs after the support test, so the streamer can already earn.
use super::Env;
use super::chat::{call, id, person};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::net::SocketAddr;
use tower::ServiceExt;

const CHANNEL: &str = "/api/channels/streamer";

async fn event(e: &Env, event: Value) -> StatusCode {
    let body = event.to_string();
    let signature = sver::stripe::sign(
        "whsec_synthetic",
        body.as_bytes(),
        chrono::Utc::now().timestamp(),
    );
    let request = Request::builder()
        .method("POST")
        .uri("/api/stripe/webhook")
        .header("content-type", "application/json")
        .header("stripe-signature", signature)
        .extension(ConnectInfo("127.0.0.1:1".parse::<SocketAddr>().unwrap()))
        .body(Body::from(body))
        .unwrap();
    sver::router(e.app.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}
/// A paid subscription invoice in the API version 2025-03-31.basil shape.
fn invoice(id: &str, user: &str, tier: u8, cents: i64, reason: &str, days: i64) -> Value {
    let end = chrono::Utc::now().timestamp() + days * 86_400;
    json!({"id": format!("evt_{id}"), "type": "invoice.paid", "data": {"object": {
        "id": id, "object": "invoice", "amount_paid": cents, "billing_reason": reason,
        "lines": {"data": [{"period": {"start": end - 30 * 86_400, "end": end}}]},
        "parent": {"type": "subscription_details", "subscription_details": {
            "subscription": "sub_sb_1",
            "metadata": {"sver_channel": "stream-owner", "sver_user": user, "sver_tier": tier.to_string()}}}}}})
}
/// Synthetic Purchased Valor, posted as a balanced ledger transaction.
async fn fund(e: &Env, user: &str, valor: i64) {
    let mut tx = e.app.db.begin().await.unwrap();
    sqlx::query("INSERT INTO ledger_transactions(id,kind,reference) VALUES($1,'test',$1)")
        .bind(format!("fund-{user}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ledger_entries(transaction_id,account,unit,amount) VALUES($1,$2,'valor',$3),($1,'valor:issued','valor',-$3)")
        .bind(format!("fund-{user}")).bind(format!("valor:{user}")).bind(valor)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn balance(e: &Env, account: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(amount),0)::bigint FROM ledger_entries WHERE account=$1",
    )
    .bind(account)
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}
async fn mine(e: &Env, token: &str) -> Value {
    let (status, body) = call(
        e,
        "GET",
        &format!("{CHANNEL}/subscription"),
        Some(token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body
}
async fn post(e: &Env, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
    call(e, "POST", &format!("{CHANNEL}{path}"), Some(token), body).await
}
async fn say(e: &Env, token: &str, body: &str) -> (StatusCode, Value) {
    post(e, token, "/chat", json!({"id": id(), "body": body})).await
}
async fn months(e: &Env, user: &str) -> i32 {
    sqlx::query_scalar("SELECT coalesce((SELECT months FROM channel_subs WHERE channel_id='stream-owner' AND user_id=$1),0)")
        .bind(user).fetch_one(&e.app.db).await.unwrap()
}

pub async fn exercise(e: &Env) {
    let fan = person(e, "sb-fan", "SbFan", true).await;
    let gifter = person(e, "sb-gifter", "SbGifter", true).await;
    let outsider = person(e, "sb-out", "SbOutsider", true).await;
    for n in 1..=5 {
        person(e, &format!("sb-c{n}"), &format!("SbChatter{n}"), true).await;
    }
    person(e, "sb-picky", "SbPicky", true).await;
    e.sql("UPDATE users SET allow_gifts=false WHERE id='sb-picky'")
        .await;
    e.sql("INSERT INTO chat_messages(id,channel_id,author_id,body) SELECT 'sb-msg-'||id,'stream-owner',id,'hello' FROM users WHERE id LIKE 'sb-c_' OR id='sb-picky'").await;
    fund(e, "sb-fan", 5_000).await;
    fund(e, "sb-gifter", 20_000).await;
    let share = |cents: i64| cents * 10 * 65 / 100;
    let start = balance(e, "usd:earnings:stream-owner").await;

    // Prices and status; the owner can't subscribe to themself.
    let status = mine(e, &fan).await;
    assert_eq!(status["can_subscribe"], true);
    assert_eq!(status["mine"], Value::Null);
    assert_eq!(status["tiers"][2]["cents"], 2_499);
    let own = e
        .call("GET", &format!("{CHANNEL}/subscription"), Value::Null)
        .await;
    assert_eq!(own["can_subscribe"], false);

    // One Valor month: 499 Valor at 0.8¢ each to the streamer; a retry changes nothing.
    let body = json!({"tier": 1, "pay": "valor", "id": id()});
    assert_eq!(
        post(e, &fan, "/subscription", body.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(post(e, &fan, "/subscription", body).await.0, StatusCode::OK);
    assert_eq!(balance(e, "valor:sb-fan").await, 5_000 - 499);
    assert_eq!(
        balance(e, "usd:earnings:stream-owner").await - start,
        499 * 8
    );
    let status = mine(e, &fan).await;
    assert_eq!(status["mine"]["tier"], 1);
    assert_eq!(status["mine"]["months"], 1);
    assert_eq!(status["mine"]["card"], false);
    let (status, said) = say(e, &fan, "Badge check").await;
    assert_eq!(status, StatusCode::OK, "{said}");
    assert_eq!(said["message"]["sub"], json!({"tier": 1, "months": 1}));

    // Subscriber-only chat: subscribers and channel roles only.
    e.call(
        "PUT",
        &format!("{CHANNEL}/chat/subs-only"),
        json!({"on": true}),
    )
    .await;
    assert_eq!(
        say(e, &outsider, "Let me in").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(say(e, &fan, "Subscribers only").await.0, StatusCode::OK);
    e.call(
        "PUT",
        &format!("{CHANNEL}/chat/subs-only"),
        json!({"on": false}),
    )
    .await;
    assert_eq!(say(e, &outsider, "Open again").await.0, StatusCode::OK);
    // A Tier 2 emote needs Tier 2.
    e.sql("INSERT INTO channel_emotes(id,channel_id,code,image_key,tier) VALUES('sb-emote','stream-owner','SbHype','emotes/sb-hype',2)").await;
    let (status, refused) = say(e, &fan, "SbHype").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(refused["error"].as_str().unwrap().contains("Tier 2"));
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat-%:sb-fan'")
        .await;
    assert_eq!(say(e, &fan, "SbHypeless is fine").await.0, StatusCode::OK);

    // A card subscription starts at Stripe Checkout and is credited by its invoices.
    let (status, started) = post(
        e,
        &gifter,
        "/subscription",
        json!({"tier": 2, "pay": "card"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let form = e.fake.lock().unwrap().stripe.last().unwrap().1.clone();
    assert!(
        form.contains("mode=subscription")
            && form.contains("unit_amount%5D=999")
            && form.contains("sver_tier%5D=2")
            && form.contains("custom_text%5Bsubmit%5D%5Bmessage%5D=Renews"),
        "the renewal terms sit above Checkout's pay button: {form}"
    );
    let mails = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mail_jobs WHERE user_id='sb-gifter'")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    let mails_before = mails().await;
    let before = balance(e, "usd:earnings:stream-owner").await;
    let first = invoice("in_sb_1", "sb-gifter", 2, 999, "subscription_create", 30);
    assert_eq!(event(e, first.clone()).await, StatusCode::OK);
    assert_eq!(event(e, first).await, StatusCode::OK);
    assert_eq!(
        mails().await - mails_before,
        1,
        "one acknowledgment with the renewal terms and how to cancel"
    );
    assert_eq!(
        balance(e, "usd:earnings:stream-owner").await - before,
        share(999)
    );
    let status = mine(e, &gifter).await;
    assert_eq!(status["mine"]["tier"], 2);
    assert_eq!(status["mine"]["auto_renew"], true);
    assert_eq!(status["mine"]["months"], 1);
    assert_eq!(
        post(
            e,
            &gifter,
            "/subscription",
            json!({"tier": 1, "pay": "valor", "id": id()})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "no second subscription alongside a renewing one"
    );
    // Renewal adds a month; an upgrade's proration doesn't, but its share is credited.
    let renewal = invoice("in_sb_2", "sb-gifter", 2, 999, "subscription_cycle", 60);
    assert_eq!(event(e, renewal).await, StatusCode::OK);
    assert_eq!(
        mails().await - mails_before,
        1,
        "renewals aren't acknowledged again"
    );
    assert_eq!(months(e, "sb-gifter").await, 2);
    assert_eq!(
        post(e, &gifter, "/subscription/upgrade", json!({"tier": 1}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, upgraded) = post(e, &gifter, "/subscription/upgrade", json!({"tier": 3})).await;
    assert_eq!(status, StatusCode::OK, "{upgraded}");
    let form = e.fake.lock().unwrap().stripe.last().unwrap().1.clone();
    assert!(
        form.contains("proration_behavior=always_invoice")
            && form.contains("unit_amount%5D=2499")
            && form.contains("product%5D=prod_test"),
        "{form}"
    );
    let before = balance(e, "usd:earnings:stream-owner").await;
    let proration = invoice("in_sb_3", "sb-gifter", 3, 1_500, "subscription_update", 60);
    assert_eq!(event(e, proration).await, StatusCode::OK);
    assert_eq!(
        balance(e, "usd:earnings:stream-owner").await - before,
        share(1_500)
    );
    assert_eq!(months(e, "sb-gifter").await, 2);
    assert_eq!(mine(e, &gifter).await["mine"]["tier"], 3);
    // Cancel stops renewal; Stripe's updates and the end of the subscription follow.
    assert_eq!(
        post(e, &gifter, "/subscription/cancel", Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(mine(e, &gifter).await["mine"]["auto_renew"], false);
    let updated = |kind: &str| {
        json!({"id": format!("evt_sb_{kind}"), "type": kind, "data": {"object": {
            "id": "sub_sb_1", "status": "active", "cancel_at_period_end": false}}})
    };
    assert_eq!(
        event(e, updated("customer.subscription.updated")).await,
        StatusCode::OK
    );
    assert_eq!(mine(e, &gifter).await["mine"]["auto_renew"], true);
    assert_eq!(
        event(e, updated("customer.subscription.deleted")).await,
        StatusCode::OK
    );
    let status = mine(e, &gifter).await;
    assert_eq!(status["mine"]["card"], false);
    assert_eq!(
        status["mine"]["tier"], 3,
        "benefits run to the end of the paid period"
    );

    // Gifts to random recent chatters who allow gifts (never the gifter or the owner).
    e.sql("UPDATE users SET allow_gifts=false WHERE id IN ('sb-fan','sb-out')")
        .await;
    let (status, short) = post(
        e,
        &gifter,
        "/gifts",
        json!({"tier": 1, "count": 20, "pay": "valor", "id": id()}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(short["field"], "count");
    let subscribed = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM channel_subs WHERE channel_id='stream-owner' AND paid_through>now()")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    let before = subscribed().await;
    let body = json!({"tier": 1, "count": 5, "pay": "valor", "id": id()});
    let (status, gifted) = post(e, &gifter, "/gifts", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{gifted}");
    assert_eq!(post(e, &gifter, "/gifts", body).await.0, StatusCode::OK);
    assert_eq!(balance(e, "valor:sb-gifter").await, 20_000 - 5 * 499);
    assert_eq!(subscribed().await - before, 5, "five new subscribers, once");
    assert_eq!(months(e, "sb-picky").await, 0);
    e.sql("UPDATE users SET allow_gifts=true WHERE id IN ('sb-fan','sb-out')")
        .await;
    // A named gift respects the setting; by card, the recipient is granted when Stripe confirms.
    let (status, refused) = post(
        e,
        &gifter,
        "/gifts",
        json!({"tier": 2, "count": 1, "recipient": "SbPicky", "pay": "card"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["field"], "recipient");
    let (status, started) = post(
        e,
        &gifter,
        "/gifts",
        json!({"tier": 2, "count": 1, "recipient": "@SbOutsider", "pay": "card"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let session = format!("cs_test_{}", e.fake.lock().unwrap().stripe.len());
    let before = balance(e, "usd:earnings:stream-owner").await;
    let paid = json!({"id": "evt_sb_gift", "type": "checkout.session.completed", "data": {"object": {
        "id": session, "payment_status": "paid", "amount_total": 999, "payment_intent": "pi_sb_gift"}}});
    assert_eq!(event(e, paid.clone()).await, StatusCode::OK);
    let again = json!({"id": "evt_sb_gift_again", "type": "checkout.session.completed", "data": paid["data"]});
    assert_eq!(event(e, again).await, StatusCode::OK);
    assert_eq!(months(e, "sb-out").await, 1, "granted once");
    assert_eq!(
        balance(e, "usd:earnings:stream-owner").await - before,
        share(999)
    );
    assert_eq!(mine(e, &outsider).await["mine"]["tier"], 2);

    // A full refund of a subscription payment reverses the streamer's share and ends benefits.
    let before = balance(e, "usd:earnings:stream-owner").await;
    let refund = json!({"id": "evt_sb_refund", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_in_sb_1", "amount": 999, "amount_refunded": 999}}});
    assert_eq!(event(e, refund).await, StatusCode::OK);
    assert_eq!(
        balance(e, "usd:earnings:stream-owner").await - before,
        -share(999)
    );
    assert_eq!(mine(e, &gifter).await["mine"], Value::Null);

    let unbalanced: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT unit FROM ledger_entries GROUP BY unit HAVING sum(amount)<>0) u",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(unbalanced, 0);
    e.sql("DELETE FROM channel_emotes WHERE id='sb-emote'")
        .await;
}
