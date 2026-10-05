//! Module 6 Support, part 1 against a synthetic Stripe: payout setup, Valor checkout, signed and
//! replayed webhooks, tributes, refunds, disputes, the under-18 rules and a balanced ledger.
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

async fn event(e: &Env, event: Value, signed: bool) -> StatusCode {
    let body = event.to_string();
    let signature = if signed {
        sver::stripe::sign(
            "whsec_synthetic",
            body.as_bytes(),
            chrono::Utc::now().timestamp(),
        )
    } else {
        "t=1,v1=00".into()
    };
    // Stripe sends no Origin and no cookie.
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
fn completed(evt: &str, session: &str, cents: i64, intent: &str) -> Value {
    json!({"id": evt, "type": "checkout.session.completed", "data": {"object": {
        "id": session, "payment_status": "paid", "amount_total": cents, "payment_intent": intent}}})
}
async fn valor(e: &Env, token: &str) -> i64 {
    let (status, wallet) = call(e, "GET", "/api/me/wallet", Some(token), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    wallet["valor"].as_i64().unwrap()
}
async fn buy(e: &Env, token: &str, cents: i64, consent: bool) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        "/api/me/wallet/checkout",
        Some(token),
        json!({"cents": cents, "guardian_consent": consent}),
    )
    .await
}
fn last_session(e: &Env) -> String {
    let fake = e.fake.lock().unwrap();
    let (path, _) = fake.stripe.last().unwrap();
    assert_eq!(path, "/v1/checkout/sessions");
    format!("cs_test_{}", fake.stripe.len())
}
async fn tribute(e: &Env, token: &str, message: &str, valor: i64) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        "/api/channels/streamer/chat",
        Some(token),
        json!({"id": message, "body": "For the stream!", "tribute": valor}),
    )
    .await
}

pub async fn exercise(e: &Env) {
    let buyer = person(e, "sp-buyer", "SpBuyer", true).await;
    let teen = person(e, "sp-teen", "SpTeen", true).await;
    e.sql("UPDATE users SET date_of_birth=current_date - interval '15 years' WHERE id='sp-teen'")
        .await;

    // Valor packs: only the listed prices; Valor arrives only from a signed webhook.
    let (status, wallet) = call(e, "GET", "/api/me/wallet", Some(&buyer), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(wallet["packs"].as_array().unwrap().len(), 7);
    assert_eq!(wallet["valor"], 0);
    assert_eq!(buy(e, &buyer, 123, false).await.0, StatusCode::BAD_REQUEST);
    let (status, started) = buy(e, &buyer, 499, false).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert!(
        started["url"]
            .as_str()
            .unwrap()
            .starts_with("https://checkout.stripe.test/")
    );
    let session = last_session(e);
    let form = e.fake.lock().unwrap().stripe.last().unwrap().1.clone();
    assert!(
        form.contains("unit_amount%5D=499") && form.contains("525+Valor"),
        "{form}"
    );
    let paid = completed("evt_sp_1", &session, 499, "pi_sp_1");
    assert_eq!(event(e, paid.clone(), false).await, StatusCode::BAD_REQUEST);
    assert_eq!(valor(e, &buyer).await, 0, "unsigned events move nothing");
    assert_eq!(event(e, paid.clone(), true).await, StatusCode::OK);
    assert_eq!(valor(e, &buyer).await, 525);
    // Replays, and a second event for the same session, never credit twice.
    assert_eq!(event(e, paid, true).await, StatusCode::OK);
    let mut again = completed("evt_sp_2", &session, 499, "pi_sp_1");
    again["type"] = json!("checkout.session.async_payment_succeeded");
    assert_eq!(event(e, again, true).await, StatusCode::OK);
    assert_eq!(valor(e, &buyer).await, 525);
    // A tampered amount is not credited and Stripe is asked to retry.
    buy(e, &buyer, 99, false).await;
    let cheap = last_session(e);
    assert_eq!(
        event(e, completed("evt_sp_3", &cheap, 1, "pi_sp_3"), true).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(valor(e, &buyer).await, 525);

    // Tributes need a channel that can earn.
    let (status, refused) = tribute(e, &buyer, &id(), 100).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["field"], "tribute");
    // Payout setup: an Express account and an onboarding link, then the Express dashboard.
    let setup = e.call("POST", "/api/me/payouts/setup", Value::Null).await;
    assert_eq!(setup["url"], "https://connect.stripe.test/onboarding");
    assert_eq!(
        e.call("GET", "/api/me/payouts", Value::Null).await["can_earn"],
        false
    );
    e.fake.lock().unwrap().account = json!({"id": "acct_test_owner", "details_submitted": true,
        "payouts_enabled": true, "requirements": {"currently_due": []}});
    let updated = json!({"id": "evt_sp_acct", "type": "account.updated",
        "data": {"object": e.fake.lock().unwrap().account.clone()}});
    assert_eq!(event(e, updated, true).await, StatusCode::OK);
    let payouts = e.call("GET", "/api/me/payouts", Value::Null).await;
    assert_eq!(payouts["can_earn"], true);
    assert_eq!(payouts["account"]["payouts_enabled"], true);
    let setup = e.call("POST", "/api/me/payouts/setup", Value::Null).await;
    assert_eq!(setup["url"], "https://connect.stripe.test/express");
    let accounts = e
        .fake
        .lock()
        .unwrap()
        .stripe
        .iter()
        .filter(|c| c.0 == "/v1/accounts")
        .count();
    assert_eq!(accounts, 1, "one Express account per creator");

    // Paying tribute: at least 10 Valor, 0.8¢ per Valor to the streamer, idempotent by message.
    assert_eq!(
        tribute(e, &buyer, &id(), 5).await.0,
        StatusCode::BAD_REQUEST
    );
    let message = id();
    let (status, sent) = tribute(e, &buyer, &message, 100).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["message"]["tribute"], 100);
    assert_eq!(tribute(e, &buyer, &message, 100).await.0, StatusCode::OK);
    assert_eq!(valor(e, &buyer).await, 425);
    let payouts = e.call("GET", "/api/me/payouts", Value::Null).await;
    assert_eq!(payouts["earnings_tenths"], 800);
    assert_eq!(
        tribute(e, &buyer, &id(), 1_000).await.0,
        StatusCode::BAD_REQUEST
    );
    // A channel that blocked the viewer refuses the tribute before any Valor moves.
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('stream-owner','sp-buyer')")
        .await;
    assert_eq!(tribute(e, &buyer, &id(), 10).await.0, StatusCode::FORBIDDEN);
    e.sql("DELETE FROM user_blocks WHERE blocker_id='stream-owner' AND blocked_id='sp-buyer'")
        .await;
    assert_eq!(valor(e, &buyer).await, 425);

    // A refund before its checkout is credited waits (503) for Stripe's retry; one that isn't
    // ours is acknowledged and ignored.
    let early = json!({"id": "evt_sp_early", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_sp_later", "amount": 99, "amount_refunded": 99,
        "metadata": {"sver_user": "sp-buyer"}}}});
    assert_eq!(event(e, early, true).await, StatusCode::SERVICE_UNAVAILABLE);
    let foreign = json!({"id": "evt_sp_foreign", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_other", "amount": 99, "amount_refunded": 99, "metadata": {}}}});
    assert_eq!(event(e, foreign, true).await, StatusCode::OK);
    // Partial then full refund: cumulative amounts, each replay-safe; the balance goes negative
    // (the Valor was already spent) and spending locks.
    let refund = |evt: &str, refunded: i64| {
        json!({"id": evt, "type": "charge.refunded", "data": {"object": {
            "payment_intent": "pi_sp_1", "amount": 499, "amount_refunded": refunded,
            "metadata": {"sver_user": "sp-buyer"}}}})
    };
    assert_eq!(
        event(e, refund("evt_sp_r1", 100), true).await,
        StatusCode::OK
    );
    assert_eq!(
        event(e, refund("evt_sp_r1", 100), true).await,
        StatusCode::OK
    );
    assert_eq!(valor(e, &buyer).await, 425 - 105);
    assert_eq!(
        event(e, refund("evt_sp_r2", 499), true).await,
        StatusCode::OK
    );
    assert_eq!(valor(e, &buyer).await, -100);
    let (_, wallet) = call(e, "GET", "/api/me/wallet", Some(&buyer), Value::Null).await;
    assert_eq!(wallet["locked"], true);
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat-%:sp-buyer'")
        .await;
    let (status, locked) = tribute(e, &buyer, &id(), 10).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(locked["error"].as_str().unwrap().contains("locked"));

    // A chargeback reverses the Valor; winning it puts it back.
    buy(e, &buyer, 999, false).await;
    let big = last_session(e);
    assert_eq!(
        event(e, completed("evt_sp_4", &big, 999, "pi_sp_4"), true).await,
        StatusCode::OK
    );
    assert_eq!(valor(e, &buyer).await, 975);
    let dispute = |evt: &str, kind: &str, status: &str| {
        json!({"id": evt, "type": kind, "data": {"object": {
            "id": "dp_sp_1", "payment_intent": "pi_sp_4", "amount": 999, "status": status}}})
    };
    assert_eq!(
        event(
            e,
            dispute("evt_sp_d1", "charge.dispute.created", "needs_response"),
            true
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(valor(e, &buyer).await, -100);
    assert_eq!(
        event(
            e,
            dispute("evt_sp_d2", "charge.dispute.closed", "won"),
            true
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(valor(e, &buyer).await, 975);

    // Under 18: a guardian confirms once, and purchases are capped at $50 a month.
    let (status, needs) = buy(e, &teen, 499, false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(needs["field"], "guardian_consent");
    assert_eq!(buy(e, &teen, 4_999, true).await.0, StatusCode::OK);
    let (status, capped) = buy(e, &teen, 99, false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{capped}");
    assert_eq!(capped["field"], "cents");
    let (_, wallet) = call(e, "GET", "/api/me/wallet", Some(&teen), Value::Null).await;
    assert_eq!(wallet["minor"], true);
    assert_eq!(wallet["guardian_confirmed"], true);
    let (_, payouts) = call(e, "GET", "/api/me/payouts", Some(&teen), Value::Null).await;
    assert_eq!(payouts["guardian"], true);

    // The ledger balances in every unit and can't be rewritten.
    let unbalanced: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT unit FROM ledger_entries GROUP BY unit HAVING sum(amount)<>0) u",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(unbalanced, 0);
    assert!(
        sqlx::query("UPDATE ledger_entries SET amount=amount+1")
            .execute(&e.app.db)
            .await
            .is_err()
    );
    assert!(
        sqlx::raw_sql("INSERT INTO ledger_transactions(id,kind,reference) VALUES('sp-bad','test','sp-bad'); INSERT INTO ledger_entries(transaction_id,account,unit,amount) VALUES('sp-bad','valor:x','valor',5)")
            .execute(&e.app.db)
            .await
            .is_err(),
        "an unbalanced transaction is rejected at commit"
    );
    let waiting: i64 =
        sqlx::query_scalar("SELECT count(*) FROM stripe_events WHERE processed_at IS NULL")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        waiting, 2,
        "the tampered checkout and the early refund wait for a retry"
    );
}
