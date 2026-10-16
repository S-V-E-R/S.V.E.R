//! S.V.E.R's own ad network, build step 1 (docs/ADVERTISERS.md) against the synthetic Stripe:
//! sales stay off behind the switch, advertisers are verified adults with an authenticator, ads
//! wait for review, quotes never sell more than the forecast allows (held checkouts count),
//! payment lands in the campaign's prepaid account, and cancellation, withdrawal after the end
//! date and staff refunds return exactly what wasn't delivered.
use super::Env;
use super::chat::{call, person};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::net::SocketAddr;
use tower::ServiceExt;

fn picture(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x * 5 % 251) as u8,
            (y * 7 % 253) as u8,
            ((x ^ y) % 255) as u8,
        ])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
async fn upload(e: &Env, token: &str, format: &str, file: Vec<u8>) -> (StatusCode, Value) {
    let mut body = Vec::new();
    for (name, value) in [
        ("format", format),
        ("title", "Play the beta"),
        ("link", "https://example.com/game"),
    ] {
        body.extend(
            format!("--ad\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    body.extend(b"--ad\r\nContent-Disposition: form-data; name=\"file\"; filename=\"ad.png\"\r\nContent-Type: image/png\r\n\r\n");
    body.extend(file);
    body.extend(b"\r\n--ad--\r\n");
    let request = Request::builder()
        .method("POST")
        .uri("/api/advertise/creatives")
        .header("content-type", "multipart/form-data; boundary=ad")
        .header("cookie", format!("sver_dev={token}"))
        .header("origin", &e.app.config.origin)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    let response = sver::router(e.app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}
async fn webhook(e: &Env, id: &str, session: &str, cents: i64, intent: &str) -> StatusCode {
    let body = json!({"id": id, "type": "checkout.session.completed", "data": {"object": {
        "id": session, "payment_status": "paid", "amount_total": cents, "payment_intent": intent}}})
    .to_string();
    let signature = sver::stripe::sign("whsec_synthetic", body.as_bytes(), Utc::now().timestamp());
    let request = Request::builder()
        .method("POST")
        .uri("/api/stripe/webhook")
        .header("content-type", "application/json")
        .header("stripe-signature", signature)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    sver::router(e.app.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}
fn day(offset: i64) -> String {
    (Utc::now().date_naive() + Duration::days(offset)).to_string()
}
fn want(impressions: i64, from: i64, to: i64) -> Value {
    json!({"placement": "site", "impressions": impressions, "starts_on": day(from), "ends_on": day(to)})
}
async fn status(e: &Env, id: &str) -> (String, i32, Option<String>) {
    sqlx::query_as("SELECT status,refund_cents,ending FROM ad_campaigns WHERE id=$1")
        .bind(id)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}
async fn balance(e: &Env, account: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(sum(amount),0)::bigint FROM ledger_entries WHERE account=$1 AND unit='usd'",
    )
    .bind(account)
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}
/// Starts a campaign and pays for it; returns (campaign, cents).
async fn buy(e: &Env, token: &str, creative: &str, body: Value, n: &str) -> (String, i64) {
    let mut body = body;
    body["name"] = json!(format!("Campaign {n}"));
    body["creative_id"] = json!(creative);
    let (code, out) = call(e, "POST", "/api/advertise/campaigns", Some(token), body).await;
    assert_eq!(code, StatusCode::OK, "{out}");
    let id = out["campaign"].as_str().unwrap().to_string();
    let session = format!("cs_test_{}", e.fake.lock().unwrap().stripe.len());
    let cents: i64 =
        sqlx::query_scalar("SELECT amount_cents::bigint FROM ad_campaigns WHERE id=$1")
            .bind(&id)
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        webhook(
            e,
            &format!("evt_ad_{n}"),
            &session,
            cents,
            &format!("pi_ad_{n}")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(status(e, &id).await.0, "paid", "the webhook marks it paid");
    (id, cents)
}

pub async fn exercise(e: &Env) {
    let buyer = person(e, "an-buyer", "NetBuyer", true).await;
    let plain = person(e, "an-plain", "NetPlain", true).await;
    let other = person(e, "an-other", "NetOther", true).await;
    let staff = super::bans::staff(e, "an-staff", "NetStaff").await;
    for statement in [
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id IN ('an-buyer','an-other')",
        "UPDATE sessions SET mfa_verified=true WHERE user_id IN ('an-buyer','an-other')",
    ] {
        e.sql(statement).await;
    }

    // ---- Who can advertise ----
    let account =
        json!({"name": "Beta Games LLC", "website": "https://example.com", "country": "us"});
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/advertise/account",
            Some(&plain),
            account.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "an authenticator is required"
    );
    assert_eq!(
        call(e, "GET", "/api/advertise", Some(&plain), Value::Null)
            .await
            .1["blocked"],
        "Turn on an authenticator to advertise."
    );
    let landscape = picture(1280, 720);
    assert_eq!(
        upload(e, &buyer, "image_landscape", landscape.clone())
            .await
            .0,
        StatusCode::CONFLICT,
        "business details come first"
    );
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/advertise/account",
            Some(&buyer),
            account.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "PUT", "/api/advertise/account", Some(&other), account)
            .await
            .0,
        StatusCode::OK
    );
    let country: String =
        sqlx::query_scalar("SELECT country FROM advertisers WHERE user_id='an-buyer'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(country, "US");

    // ---- Ads: the right shape, then review ----
    assert_eq!(
        upload(e, &buyer, "image_landscape", picture(1280, 1280))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "16:9 only"
    );
    assert_eq!(
        upload(e, &buyer, "image_landscape", picture(640, 360))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "too small"
    );
    let (code, made) = upload(e, &buyer, "image_landscape", landscape.clone()).await;
    assert_eq!(code, StatusCode::OK, "{made}");
    assert_eq!(made["creative"]["status"], "review");
    let creative = made["creative"]["id"].as_str().unwrap().to_string();
    let (_, portrait) = upload(e, &buyer, "image_portrait", picture(720, 1280)).await;
    let portrait = portrait["creative"]["id"].as_str().unwrap().to_string();
    let (_, theirs) = upload(e, &other, "image_landscape", landscape).await;
    let theirs = theirs["creative"]["id"].as_str().unwrap().to_string();

    // ---- The rate card and quotes ----
    assert_eq!(
        call(
            e,
            "POST",
            "/api/advertise/quote",
            Some(&buyer),
            want(1000, 0, 9)
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE,
        "not for sale until staff price it"
    );
    let rates = json!({"placements": {"site": {"cpm_cents": 500, "min_spend_cents": 1000, "daily_estimate": 1000}}, "note": "Launch prices"});
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/admin/adnet/rate-card",
            Some(&buyer),
            rates.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(e, "PUT", "/api/admin/adnet/rate-card", Some(&staff), rates)
            .await
            .0,
        StatusCode::OK
    );
    let (_, quote) = call(
        e,
        "POST",
        "/api/advertise/quote",
        Some(&buyer),
        want(1000, 0, 9),
    )
    .await;
    assert_eq!(quote["fits"], true);
    assert_eq!(
        quote["available"], 7000,
        "1,000 a day for 10 days, 70% sellable"
    );
    assert_eq!(quote["amount_cents"], 1000, "the minimum spend applies");
    assert_eq!(quote["below_minimum"], true);
    let (_, big) = call(
        e,
        "POST",
        "/api/advertise/quote",
        Some(&buyer),
        want(8000, 0, 9),
    )
    .await;
    assert_eq!(big["fits"], false);
    assert!(big["soonest_start"].is_null(), "10 days never hold 8,000");
    assert_eq!(
        call(
            e,
            "POST",
            "/api/advertise/quote",
            Some(&buyer),
            want(500, 0, 9)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/advertise/quote",
            Some(&buyer),
            want(1000, -1, 9)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/advertise/quote",
            Some(&buyer),
            want(1000, 0, 95)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // ---- Sales are off until the switch opens them ----
    let mut first = want(5000, 0, 9);
    first["name"] = json!("Launch week");
    first["creative_id"] = json!(creative);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/advertise/campaigns",
            Some(&buyer),
            first.clone()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    e.sql("UPDATE feature_switches SET off=false WHERE name='ad_sales'")
        .await;
    let mut wrong = first.clone();
    wrong["creative_id"] = json!(portrait);
    assert_eq!(
        call(e, "POST", "/api/advertise/campaigns", Some(&buyer), wrong)
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "a 9:16 ad can't run in the site slot"
    );
    let mut stolen = first.clone();
    stolen["creative_id"] = json!(theirs);
    assert_eq!(
        call(e, "POST", "/api/advertise/campaigns", Some(&buyer), stolen)
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "only your own ads"
    );

    // ---- Admission control: a held checkout takes its inventory ----
    let (code, started) = call(e, "POST", "/api/advertise/campaigns", Some(&buyer), first).await;
    assert_eq!(code, StatusCode::OK, "{started}");
    let held = started["campaign"].as_str().unwrap().to_string();
    let (_, after) = call(
        e,
        "POST",
        "/api/advertise/quote",
        Some(&buyer),
        want(3000, 0, 9),
    )
    .await;
    assert_eq!(after["available"], 2000);
    assert_eq!(after["fits"], false);
    assert!(
        after["soonest_start"].is_string(),
        "a later window fits the full amount"
    );
    let mut too_much = want(3000, 0, 9);
    too_much["name"] = json!("Too much");
    too_much["creative_id"] = json!(theirs);
    let (code, refused) = call(
        e,
        "POST",
        "/api/advertise/campaigns",
        Some(&other),
        too_much,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    // An unpaid checkout lapses and gives the inventory back.
    e.sql("UPDATE ad_campaigns SET hold_until=now()-interval '1 second' WHERE status='checkout'")
        .await;
    sver::adnet::tick(&e.app).await.unwrap();
    assert_eq!(status(e, &held).await.0, "expired");
    let (_, freed) = call(
        e,
        "POST",
        "/api/advertise/quote",
        Some(&buyer),
        want(3000, 0, 9),
    )
    .await;
    assert_eq!(freed["available"], 7000);
    // Paid late: the inventory is still there, so the campaign stands.
    let session: String = sqlx::query_scalar("SELECT checkout_id FROM ad_campaigns WHERE id=$1")
        .bind(&held)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        webhook(e, "evt_ad_late", &session, 2500, "pi_ad_late").await,
        StatusCode::OK
    );
    assert_eq!(status(e, &held).await.0, "paid");
    assert_eq!(
        balance(e, &format!("usd:ads:prepaid:{held}")).await,
        -25_000,
        "$25.00 in tenths of a cent, prepaid"
    );

    // ---- Review ----
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/adnet/creatives/{creative}/review"),
            Some(&staff),
            json!({"action": "reject"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "a rejection needs a reason"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/adnet/creatives/{creative}/review"),
            Some(&staff),
            json!({"action": "approve_teen", "note": ""})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/adnet/creatives/{creative}/review"),
            Some(&staff),
            json!({"action": "approve"})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "reviewed once"
    );
    let (_, admin) = call(e, "GET", "/api/admin/adnet", Some(&staff), Value::Null).await;
    assert_eq!(
        admin["review"].as_array().unwrap().len(),
        2,
        "two ads still waiting"
    );
    let site = admin["inventory"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["placement"] == "site")
        .unwrap();
    assert_eq!(site["days"][0]["promised"], 500, "5,000 over 10 days");

    // ---- Cancel before delivery: a full refund ----
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/advertise/campaigns/{held}/cancel"),
            Some(&other),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/advertise/campaigns/{held}/withdraw"),
            Some(&buyer),
            Value::Null
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "not past its end date"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/advertise/campaigns/{held}/cancel"),
            Some(&buyer),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        status(e, &held).await,
        ("refund_due".into(), 2500, Some("cancelled".into()))
    );
    sver::adnet::tick(&e.app).await.unwrap();
    assert_eq!(status(e, &held).await.0, "cancelled");
    let refund = e
        .fake
        .lock()
        .unwrap()
        .stripe
        .iter()
        .rev()
        .find(|(p, _)| p == "/v1/refunds")
        .unwrap()
        .1
        .clone();
    assert!(
        refund.contains("payment_intent=pi_ad_late") && refund.contains("amount=2500"),
        "{refund}"
    );

    // ---- Withdraw after the end date: only the undelivered part ----
    let (late, cents) = buy(e, &buyer, &creative, want(3000, 0, 9), "past").await;
    assert_eq!(cents, 1500);
    e.sql(sqlx::AssertSqlSafe(format!("UPDATE ad_campaigns SET delivered=1000,starts_on=current_date-3,ends_on=current_date-1 WHERE id='{late}'"))).await;
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/advertise/campaigns/{late}/cancel"),
            Some(&buyer),
            Value::Null
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "started: withdraw instead"
    );
    let (code, out) = call(
        e,
        "POST",
        &format!("/api/advertise/campaigns/{late}/withdraw"),
        Some(&buyer),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{out}");
    assert_eq!(out["refund_cents"], 1000, "2,000 of 3,000 undelivered");
    sver::adnet::tick(&e.app).await.unwrap();
    assert_eq!(
        status(e, &late).await,
        ("withdrawn".into(), 1000, Some("withdrawn".into()))
    );
    let refund = e
        .fake
        .lock()
        .unwrap()
        .stripe
        .iter()
        .rev()
        .find(|(p, _)| p == "/v1/refunds")
        .unwrap()
        .1
        .clone();
    assert!(refund.contains("amount=1000"), "{refund}");

    // ---- Staff refund, and an ad that's never approved ----
    let (kept, _) = buy(e, &other, &theirs, want(2000, 0, 9), "staff").await;
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/adnet/campaigns/{kept}/refund"),
            Some(&staff),
            json!({})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "a note is required"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/adnet/campaigns/{kept}/refund"),
            Some(&staff),
            json!({"note": "Advertiser asked by email"})
        )
        .await
        .0,
        StatusCode::OK
    );
    sver::adnet::tick(&e.app).await.unwrap();
    assert_eq!(status(e, &kept).await.0, "cancelled");
    let (stale, _) = buy(e, &other, &theirs, want(2000, 0, 9), "stale").await;
    e.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE ad_campaigns SET paid_at=now()-interval '31 days' WHERE id='{stale}'"
    )))
    .await;
    sver::adnet::tick(&e.app).await.unwrap();
    assert_eq!(
        status(e, &stale).await.0,
        "cancelled",
        "no approved ad within 30 days: refunded in full"
    );

    // ---- The advertiser's view ----
    let (_, mine) = call(e, "GET", "/api/advertise", Some(&buyer), Value::Null).await;
    assert!(mine["blocked"].is_null());
    assert_eq!(mine["account"]["name"], "Beta Games LLC");
    assert_eq!(mine["campaigns"].as_array().unwrap().len(), 2);
    assert_eq!(mine["creatives"].as_array().unwrap().len(), 2);
    e.sql("UPDATE feature_switches SET off=true WHERE name='ad_sales'")
        .await;
}
