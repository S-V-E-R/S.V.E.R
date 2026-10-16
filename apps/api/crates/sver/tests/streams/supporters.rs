//! Platform supporters and Founders (docs/SUPPORTERS.md) against the synthetic Stripe: checkout
//! with renewal terms, invoice webhooks (new, renewal, upgrade, downgrade at renewal, cancel, end),
//! server-decided badges, Founder numbers under concurrent payments, revocation on refund and lost
//! disputes, the under-18 rules, the emergency switch and that supporting changes nothing else.
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
/// A paid platform invoice; the fake Stripe links it to payment intent `pi_{id}`.
fn invoice(id: &str, user: &str, tier: u8, cents: i64, reason: &str, days: i64) -> Value {
    let end = chrono::Utc::now().timestamp() + days * 86_400;
    json!({"id": format!("evt_{id}"), "type": "invoice.paid", "data": {"object": {
        "id": id, "object": "invoice", "amount_paid": cents, "billing_reason": reason,
        "lines": {"data": [{"period": {"start": end - 30 * 86_400, "end": end}}]},
        "parent": {"type": "subscription_details", "subscription_details": {
            "subscription": format!("sub_{user}"),
            "metadata": {"sver_platform": "1", "sver_user": user, "sver_platform_tier": tier.to_string()}}}}}})
}
async fn founder(e: &Env, user: &str) -> Option<i32> {
    sqlx::query_scalar("SELECT number FROM founders WHERE user_id=$1")
        .bind(user)
        .fetch_optional(&e.app.db)
        .await
        .unwrap()
}
async fn plan(e: &Env, user: &str) -> (i16, Option<i16>, i32, bool) {
    sqlx::query_as("SELECT tier,pending_tier,months,paid_through>now() FROM platform_subscriptions WHERE user_id=$1")
        .bind(user)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}
fn last_stripe(e: &Env) -> (String, String) {
    e.fake.lock().unwrap().stripe.last().unwrap().clone()
}

pub async fn exercise(e: &Env) {
    let grey = person(e, "su-grey", "SuGrey", true).await;
    let wolf = person(e, "su-wolf", "SuWolf", true).await;
    let teen = person(e, "su-teen", "SuTeen", true).await;
    let staff = person(e, "su-staff", "SuStaff", true).await;
    for statement in [
        "UPDATE users SET date_of_birth=current_date - interval '15 years' WHERE id='su-teen'",
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='su-staff'",
        "UPDATE sessions SET mfa_verified=true WHERE user_id='su-staff'",
        "INSERT INTO staff_roles(user_id,role) VALUES('su-staff','admin')",
    ] {
        e.sql(statement).await;
    }

    // The page: real counts, both tiers with their renewal terms.
    let (code, page) = call(e, "GET", "/api/supporters", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        (
            page["founders_claimed"].as_i64(),
            page["founders_total"].as_i64()
        ),
        (Some(0), Some(500))
    );
    assert_eq!(page["tiers"][1]["cents"], 2500);
    assert!(
        page["tiers"][1]["terms"]
            .as_str()
            .unwrap()
            .contains("until you cancel")
    );

    // Checkout: a monthly card subscription with the renewal terms above Stripe's pay button.
    assert_eq!(
        call(
            e,
            "POST",
            "/api/supporters/subscribe",
            Some(&grey),
            json!({"tier": "gold"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (code, started) = call(
        e,
        "POST",
        "/api/supporters/subscribe",
        Some(&grey),
        json!({"tier": "greycloak"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{started}");
    let (path, form) = last_stripe(e);
    assert_eq!(path, "/v1/checkout/sessions");
    for part in [
        "mode=subscription",
        "unit_amount%5D=2500",
        "sver_platform%5D=1",
        "Renews+automatically",
        "interval%5D=month",
    ] {
        assert!(form.contains(part), "{part} missing from {form}");
    }

    // The first paid Greycloak month starts the plan, claims Founder #1 and posts to the
    // platform only; the acknowledgment email goes out once.
    let mails_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mail_jobs WHERE user_id='su-grey'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        event(
            e,
            invoice("in_su_1", "su-grey", 2, 2500, "subscription_create", 30)
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        event(
            e,
            invoice("in_su_1", "su-grey", 2, 2500, "subscription_create", 30)
        )
        .await,
        StatusCode::OK,
        "replays are safe"
    );
    assert_eq!(plan(e, "su-grey").await, (2, None, 1, true));
    assert_eq!(founder(e, "su-grey").await, Some(1));
    let mails: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_jobs WHERE user_id='su-grey'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(mails, mails_before + 1, "one acknowledgment email");
    assert_eq!(
        call(
            e,
            "POST",
            "/api/supporters/subscribe",
            Some(&grey),
            json!({"tier": "greycloak"})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "one subscription at a time"
    );

    // Badges come from the server: the user card, the profile and every chat message.
    let card = call(e, "GET", "/api/users/SuGrey/card", Some(&wolf), Value::Null)
        .await
        .1;
    assert_eq!(
        (
            card["supporter"]["tier"].as_str(),
            card["supporter"]["founder"].as_i64()
        ),
        (Some("greycloak"), Some(1))
    );
    let profile = call(e, "GET", "/api/channels/SuGrey", None, Value::Null)
        .await
        .1;
    assert!(
        profile["channel"]["supporter"]["since"].is_string(),
        "{profile}"
    );
    let (code, said) = call(
        e,
        "POST",
        "/api/channels/streamer/chat",
        Some(&grey),
        json!({"id": id(), "body": "Wolf on watch"}),
    )
    .await;
    if code == StatusCode::OK {
        let chat = call(
            e,
            "GET",
            "/api/channels/streamer/chat",
            Some(&grey),
            Value::Null,
        )
        .await
        .1;
        let mine = chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["body"] == "Wolf on watch")
            .unwrap()
            .clone();
        assert_eq!(
            (
                mine["supporter"]["tier"].as_str(),
                mine["supporter"]["founder"].as_i64()
            ),
            (Some("greycloak"), Some(1)),
            "the client can't pick its badge"
        );
    } else {
        panic!("chat refused: {said}");
    }

    // From here on, nothing the supporter does earns anything; snapshot what supporting must
    // never change.
    let card_before = call(e, "GET", "/api/users/SuGrey/card", Some(&wolf), Value::Null)
        .await
        .1;
    let progression_before = call(e, "GET", "/api/me/progression", Some(&grey), Value::Null)
        .await
        .1;
    let wallet_before = call(e, "GET", "/api/me/wallet", Some(&grey), Value::Null)
        .await
        .1;

    // Founder numbers are unique under concurrent payments.
    let mut names = vec![];
    for i in 0..12 {
        let id = format!("su-f{i}");
        person(e, &id, &format!("SuFounder{i}"), true).await;
        names.push(id);
    }
    let paid = futures_util::future::join_all(names.iter().map(|u| {
        event(
            e,
            invoice(&format!("in_{u}"), u, 2, 2500, "subscription_create", 30),
        )
    }))
    .await;
    assert!(paid.iter().all(|s| *s == StatusCode::OK), "{paid:?}");
    let numbers: Vec<i32> = sqlx::query_scalar("SELECT number FROM founders ORDER BY number")
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        numbers,
        (1..=13).collect::<Vec<i32>>(),
        "no duplicates, no gaps"
    );

    // Wolfsworn gets the badge but no Founder spot; an upgrade is prorated, and its partial charge
    // doesn't claim one: the first full Greycloak month does.
    assert_eq!(
        event(
            e,
            invoice("in_su_w1", "su-wolf", 1, 500, "subscription_create", 30)
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(founder(e, "su-wolf").await, None);
    let (code, changed) = call(
        e,
        "POST",
        "/api/supporters/change",
        Some(&wolf),
        json!({"tier": "greycloak"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{changed}");
    let (path, form) = last_stripe(e);
    assert!(
        path.starts_with("/v1/subscriptions/")
            && form.contains("always_invoice")
            && form.contains("payment_behavior=error_if_incomplete")
            && form.contains("unit_amount%5D=2500"),
        "{form}"
    );
    assert_eq!(
        plan(e, "su-wolf").await.0,
        2,
        "an upgrade takes effect at once"
    );
    assert_eq!(
        event(
            e,
            invoice("in_su_w2", "su-wolf", 2, 1333, "subscription_update", 30)
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        founder(e, "su-wolf").await,
        None,
        "a prorated upgrade charge isn't a Greycloak month"
    );
    assert_eq!(
        plan(e, "su-wolf").await.2,
        1,
        "a proration isn't a new month"
    );

    // A downgrade waits for the renewal, and months carry over.
    let (code, changed) = call(
        e,
        "POST",
        "/api/supporters/change",
        Some(&grey),
        json!({"tier": "wolfsworn"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{changed}");
    assert!(last_stripe(e).1.contains("proration_behavior=none"));
    assert_eq!(
        plan(e, "su-grey").await,
        (2, Some(1), 1, true),
        "Greycloak until the paid month ends"
    );
    assert_eq!(
        event(
            e,
            invoice("in_su_2", "su-grey", 1, 500, "subscription_cycle", 60)
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(plan(e, "su-grey").await, (1, None, 2, true));
    assert_eq!(
        founder(e, "su-grey").await,
        Some(1),
        "the Founder mark survives a downgrade"
    );

    // Cancelling runs to the end of the period; afterwards the badge goes and the Founder mark stays.
    let (code, cancelled) = call(
        e,
        "POST",
        "/api/supporters/cancel",
        Some(&grey),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["plan"]["auto_renew"], false);
    let (code, resumed) = call(
        e,
        "POST",
        "/api/supporters/resume",
        Some(&grey),
        Value::Null,
    )
    .await;
    assert_eq!(
        (code, resumed["plan"]["auto_renew"].as_bool()),
        (StatusCode::OK, Some(true)),
        "renewal can be turned back on during the paid month"
    );
    assert!(last_stripe(e).1.contains("cancel_at_period_end=false"));
    call(
        e,
        "POST",
        "/api/supporters/cancel",
        Some(&grey),
        Value::Null,
    )
    .await;
    assert_eq!(event(e, json!({"id": "evt_su_del", "type": "customer.subscription.deleted", "data": {"object": {"id": "sub_su-grey", "status": "canceled"}}})).await, StatusCode::OK);
    assert!(plan(e, "su-grey").await.3, "the paid period still runs");
    e.sql("UPDATE platform_subscriptions SET paid_through=now()-interval '1 second' WHERE user_id='su-grey'").await;
    let card = call(e, "GET", "/api/users/SuGrey/card", Some(&wolf), Value::Null)
        .await
        .1;
    assert!(
        card["supporter"]["tier"].is_null() && card["supporter"]["founder"] == 1,
        "{card}"
    );
    let chat = call(
        e,
        "GET",
        "/api/channels/streamer/chat",
        Some(&grey),
        Value::Null,
    )
    .await
    .1;
    let mine = chat["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["body"] == "Wolf on watch")
        .unwrap()
        .clone();
    assert!(
        mine["supporter"].is_null(),
        "in chat the mark shows only while subscribed"
    );

    // A full refund of the claiming payment revokes the mark and frees its number for the next
    // new Greycloak; a lost dispute does the same.
    let refund = json!({"id": "evt_su_refund", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_in_su-f3", "amount": 2500, "amount_refunded": 2500, "metadata": {"sver_user": "su-f3"}}}});
    assert_eq!(event(e, refund).await, StatusCode::OK);
    assert_eq!(founder(e, "su-f3").await, None);
    assert!(!plan(e, "su-f3").await.3, "a refunded month ends now");
    assert_eq!(
        last_stripe(e),
        (
            "/v1/subscriptions/sub_su-f3".to_owned(),
            "cancel_at_period_end=true".to_owned()
        ),
        "nothing more is charged after a refund"
    );
    let lost = json!({"id": "evt_su_lost", "type": "charge.dispute.closed", "data": {"object": {
        "id": "dp_su", "status": "lost", "amount": 2500, "payment_intent": "pi_in_su-f5"}}});
    assert_eq!(event(e, lost).await, StatusCode::OK);
    assert_eq!(founder(e, "su-f5").await, None);
    let freed: i32 = sqlx::query_scalar("SELECT n FROM generate_series(1,500) n WHERE NOT EXISTS(SELECT 1 FROM founders WHERE number=n) LIMIT 1").fetch_one(&e.app.db).await.unwrap();
    let late = person(e, "su-late", "SuLate", true).await;
    assert_eq!(
        event(
            e,
            invoice("in_su_late", "su-late", 2, 2500, "subscription_create", 30)
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        founder(e, "su-late").await,
        Some(freed),
        "the lowest free number"
    );
    let (_, page) = call(e, "GET", "/api/supporters", None, Value::Null).await;
    assert_eq!(
        page["founders_claimed"].as_i64(),
        Some(12),
        "the public count is real (su-wolf's upgrade hasn't claimed one)"
    );
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM moderation_actions WHERE action='founder_revoked'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(audited, 2);

    // A Founder can hide their name; the spot then reads as just the number.
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/supporters/founder",
            Some(&late),
            json!({"hidden": true})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, page) = call(e, "GET", "/api/supporters", None, Value::Null).await;
    let spot = page["founders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["number"] == freed)
        .unwrap()
        .clone();
    assert!(spot["username"].is_null(), "{spot}");
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/supporters/founder",
            Some(&teen),
            json!({"hidden": true})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Under 18: guardian confirmation, and the $50 monthly cap with other purchases.
    let (code, body) = call(
        e,
        "POST",
        "/api/supporters/subscribe",
        Some(&teen),
        json!({"tier": "greycloak"}),
    )
    .await;
    assert_eq!(
        (code, body["field"].as_str()),
        (StatusCode::BAD_REQUEST, Some("guardian_consent"))
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/supporters/subscribe",
            Some(&teen),
            json!({"tier": "greycloak", "guardian_consent": true})
        )
        .await
        .0,
        StatusCode::OK
    );
    // A second checkout closes the first, so two tabs can't start two subscriptions.
    let first = sqlx::query_scalar::<_, String>(
        "SELECT id FROM checkout_sessions WHERE user_id='su-teen' AND status='open'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(
        call(
            e,
            "POST",
            "/api/supporters/subscribe",
            Some(&teen),
            json!({"tier": "greycloak"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        e.fake
            .lock()
            .unwrap()
            .stripe
            .iter()
            .any(|(p, _)| *p == format!("/v1/checkout/sessions/{first}/expire")),
        "the earlier checkout is closed"
    );
    // A checkout that's paid but whose invoice hasn't arrived yet blocks a new one.
    e.sql("UPDATE checkout_sessions SET status='paid' WHERE user_id='su-teen' AND status='open'")
        .await;
    assert_eq!(
        call(
            e,
            "POST",
            "/api/supporters/subscribe",
            Some(&teen),
            json!({"tier": "greycloak"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    e.sql(
        "UPDATE checkout_sessions SET status='expired' WHERE user_id='su-teen' AND kind='platform'",
    )
    .await;
    e.sql("INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,status) VALUES('cs_su_teen_valor','su-teen','valor',4600,'paid')").await;
    let (code, body) = call(
        e,
        "POST",
        "/api/supporters/subscribe",
        Some(&teen),
        json!({"tier": "wolfsworn"}),
    )
    .await;
    assert_eq!(
        (code, body["field"].as_str()),
        (StatusCode::BAD_REQUEST, Some("cents")),
        "{body}"
    );
    // An upgrade's prorated charge counts toward the cap too.
    assert_eq!(
        event(
            e,
            invoice("in_su_t1", "su-teen", 1, 0, "subscription_create", 30)
        )
        .await,
        StatusCode::OK
    );
    let (code, body) = call(
        e,
        "POST",
        "/api/supporters/change",
        Some(&teen),
        json!({"tier": "greycloak"}),
    )
    .await;
    assert_eq!(
        (code, body["field"].as_str()),
        (StatusCode::BAD_REQUEST, Some("cents")),
        "{body}"
    );

    // The emergency switch pauses new subscriptions only.
    e.sql("INSERT INTO feature_switches(name,off) VALUES('supporter_sales',true) ON CONFLICT(name) DO UPDATE SET off=true").await;
    let other = person(e, "su-new", "SuNew", true).await;
    assert_eq!(
        call(
            e,
            "POST",
            "/api/supporters/subscribe",
            Some(&other),
            json!({"tier": "wolfsworn"})
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        event(
            e,
            invoice("in_su_w3", "su-wolf", 2, 2500, "subscription_cycle", 60)
        )
        .await,
        StatusCode::OK,
        "renewals keep running"
    );
    let wolf_number = founder(e, "su-wolf").await;
    assert!(
        wolf_number.is_some(),
        "the first full Greycloak month claims a spot"
    );
    e.sql("DELETE FROM feature_switches WHERE name='supporter_sales'")
        .await;

    // Staff: the list, an audited refund through Stripe and an audited revocation.
    let (code, admin) = call(e, "GET", "/api/admin/supporters", Some(&staff), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert!(
        admin["founders"].as_array().unwrap().len() == 13
            && admin["paid_cents"].as_i64().unwrap() > 0
    );
    assert_eq!(
        call(e, "GET", "/api/admin/supporters", Some(&grey), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/supporters/SuWolf/refund",
            Some(&staff),
            json!({"note": "Asked within a day"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(last_stripe(e).1.contains("pi_in_su_w3"));
    let older = json!({"id": "evt_su_refund_w1", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_in_su_w1", "amount": 500, "amount_refunded": 500}}});
    assert_eq!(event(e, older).await, StatusCode::OK);
    assert!(
        plan(e, "su-wolf").await.3,
        "refunding an older month leaves the later paid month"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!(
                "/api/admin/supporters/founders/{}/revoke",
                wolf_number.unwrap()
            ),
            Some(&staff),
            json!({"note": "Chargeback outside Stripe"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(founder(e, "su-wolf").await, None);
    let latest = json!({"id": "evt_su_refund_w3", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_in_su_w3", "amount": 2500, "amount_refunded": 2500}}});
    assert_eq!(event(e, latest).await, StatusCode::OK);
    assert!(
        !plan(e, "su-wolf").await.3,
        "refunding the current month ends it"
    );

    // Supporting changes nothing else, and its money goes to the platform alone.
    let mut card_after = call(e, "GET", "/api/users/SuGrey/card", Some(&wolf), Value::Null)
        .await
        .1;
    let mut card_was = card_before.clone();
    card_after.as_object_mut().unwrap().remove("supporter");
    card_was.as_object_mut().unwrap().remove("supporter");
    assert_eq!(
        card_was, card_after,
        "level, frame, showcase and the rest are unchanged"
    );
    assert_eq!(
        call(e, "GET", "/api/me/progression", Some(&grey), Value::Null)
            .await
            .1,
        progression_before,
        "no XP"
    );
    assert_eq!(
        call(e, "GET", "/api/me/wallet", Some(&grey), Value::Null)
            .await
            .1["valor"],
        wallet_before["valor"],
        "no Valor"
    );
    let accounts: Vec<String> = sqlx::query_scalar("SELECT DISTINCT e.account FROM ledger_entries e JOIN ledger_transactions t ON t.id=e.transaction_id WHERE t.kind='platform_sub' ORDER BY 1")
        .fetch_all(&e.app.db).await.unwrap();
    assert_eq!(
        accounts,
        ["usd:stripe", "usd:supporters"],
        "nothing reaches a creator ledger"
    );
    let influence: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_transactions WHERE kind='platform_sub' AND detail ? 'channel'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(influence, 0);
}
