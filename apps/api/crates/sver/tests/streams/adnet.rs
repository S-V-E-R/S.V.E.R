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
    stripe_event(e, json!({"id": id, "type": "checkout.session.completed", "data": {"object": {
        "id": session, "payment_status": "paid", "amount_total": cents, "payment_intent": intent}}}))
    .await
}
async fn stripe_event(e: &Env, body: Value) -> StatusCode {
    let body = body.to_string();
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

    // ==== Step 2: serving, verification, pacing and the make-good ====
    let viewer = person(e, "an-viewer", "NetViewer", true).await;
    let streamer = person(e, "an-streamer", "NetStreamer", true).await;
    let guest_browser = "guest-browser-0000000001";
    let browser = "viewer-browser-000000001";
    let serve = |token: Option<String>, body: Value| async move {
        call(e, "POST", "/api/ads/serve", token.as_deref(), body).await
    };
    let report = |token: Option<String>,
                  ticket: Value,
                  browser: &'static str,
                  ms: i64,
                  visible: bool| async move {
        call(
            e,
            "POST",
            "/api/ads/impressions",
            token.as_deref(),
            json!({"ticket": ticket, "browser_id": browser, "visible_ms": ms, "visible": visible}),
        )
        .await
        .1
    };
    // A site campaign starts once paid, approved and its start date has come.
    let (site, site_cents) = buy(e, &buyer, &creative, want(2000, 0, 9), "site").await;
    sver::adserve::tick(&e.app).await.unwrap();
    assert_eq!(status(e, &site).await.0, "running");
    assert!(
        serve(
            None,
            json!({"placement": "site", "browser_id": guest_browser})
        )
        .await
        .1["ad"]
            .is_null(),
        "guests get no site ads"
    );
    assert!(
        serve(
            Some(viewer.clone()),
            json!({"placement": "site", "browser_id": browser})
        )
        .await
        .1["ad"]
            .is_null(),
        "an account that hasn't watched anything recently gets no feed or site ads"
    );
    for statement in [
        "INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state,ended_at,end_reason) VALUES('an-w','an-streamer','an-w',1,'ENDED','s','v','c',now()-interval '1 hour',now()-interval '1 hour',now(),'skipped',now(),'test')",
        "INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level,last_beat_at) VALUES('an-w','u:an-viewer',now()-interval '1 minute','counted',now()-interval '1 minute'),('an-w','u:an-streamer',now()-interval '1 minute','counted',now()-interval '1 minute')",
    ] {
        e.sql(statement).await;
    }
    let (code, served) = serve(
        Some(viewer.clone()),
        json!({"placement": "site", "browser_id": browser}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{served}");
    // A viewer who turns ads off gets none, in any placement (docs/ADS.md "Turning ads off").
    let quiet = person(e, "an-quiet", "NetQuiet", true).await;
    let (code, prefs) = call(
        e,
        "PUT",
        "/api/me/preferences",
        Some(&quiet),
        json!({"ads_off": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{prefs}");
    assert_eq!(prefs["ads_off"], true);
    assert!(
        serve(
            Some(quiet.clone()),
            json!({"placement": "site", "browser_id": "quiet-browser-00000001"})
        )
        .await
        .1["ad"]
            .is_null(),
        "ads off means no ads"
    );
    let ticket = served["ad"]["ticket"].clone();
    assert_eq!(served["ad"]["kind"], "image");
    assert_eq!(served["ad"]["advertiser"], "Beta Games LLC");
    let (_, again) = serve(
        Some(viewer.clone()),
        json!({"placement": "site", "browser_id": browser}),
    )
    .await;
    assert!(
        again["ad"].is_null(),
        "the same ad at most once per 10 minutes"
    );
    assert_eq!(
        report(Some(viewer.clone()), ticket.clone(), browser, 1500, true).await["reason"],
        "too_short",
        "the time has to really pass"
    );
    let (_, hidden) = serve(
        Some(streamer.clone()),
        json!({"placement": "site", "browser_id": browser}),
    )
    .await;
    let hidden_ticket = hidden["ad"]["ticket"].clone();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(
        report(Some(streamer.clone()), hidden_ticket, browser, 1500, false).await["reason"],
        "hidden"
    );
    // A fresh ticket, really seen: counted once.
    e.sql("DELETE FROM ad_serves WHERE viewer_key='u:an-viewer'")
        .await;
    let (_, served) = serve(
        Some(viewer.clone()),
        json!({"placement": "site", "browser_id": browser}),
    )
    .await;
    let ticket = served["ad"]["ticket"].clone();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(
        report(None, ticket.clone(), browser, 1500, true).await,
        json!({"error": "Not found."}),
        "someone else\'s ticket is unknown"
    );
    assert_eq!(
        report(Some(viewer.clone()), ticket.clone(), browser, 1500, true).await["counted"],
        true
    );
    assert_eq!(
        report(Some(viewer.clone()), ticket.clone(), browser, 1500, true).await["reason"],
        "duplicate"
    );
    let delivered: i32 = sqlx::query_scalar("SELECT delivered FROM ad_campaigns WHERE id=$1")
        .bind(&site)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(delivered, 1);
    let posted: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_transactions WHERE kind='ad_impression' AND detail->>'campaign'=$1").bind(&site).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(posted, 1, "each verified impression settles once");
    let unbalanced: i64 = sqlx::query_scalar("SELECT count(*) FROM (SELECT t.id FROM ledger_transactions t JOIN ledger_entries x ON x.transaction_id=t.id WHERE t.kind='ad_impression' GROUP BY t.id HAVING sum(x.amount)<>0) u").fetch_one(&e.app.db).await.unwrap();
    assert_eq!(unbalanced, 0);

    // Breaks: only a streamer who can earn and turned ads on, at most one per 10 minutes.
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/me/ads",
            Some(&streamer),
            json!({"enabled": true})
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "payouts first"
    );
    for statement in [
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='an-streamer'",
        "UPDATE sessions SET mfa_verified=true WHERE user_id='an-streamer'",
        "INSERT INTO payout_accounts(user_id,stripe_account,details_submitted) VALUES('an-streamer','acct_test_an',true)",
        "INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('an-b','an-streamer','an-b',1,'LIVE','s','v','c',now()-interval '3 minutes',now(),now(),'skipped')",
        "INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('an-b','u:an-viewer',now()+interval '1 hour','counted')",
        "INSERT INTO ad_creatives(id,advertiser_id,format,stored,duration_ms,title,link,status,teen_safe) VALUES('an-video','an-buyer','video_landscape','ads/synthetic',15000,'Trailer','https://example.com/trailer','approved',true)",
    ] {
        e.sql(statement).await;
    }
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/ads/breaks",
            Some(&streamer),
            json!({"seconds": 30})
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "ads are off until turned on"
    );
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/me/ads",
            Some(&streamer),
            json!({"enabled": true})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/ads/breaks",
            Some(&streamer),
            json!({"seconds": 45})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/ads/breaks",
            Some(&streamer),
            json!({"seconds": 30})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/ads/breaks",
            Some(&streamer),
            json!({"seconds": 30})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "10 minutes between breaks"
    );
    let (_, live) = call(
        e,
        "GET",
        "/api/channels/NetStreamer/live",
        Some(&viewer),
        Value::Null,
    )
    .await;
    assert!(live["ad_break"]["ends_at"].is_string(), "{live}");
    let (_, own) = call(
        e,
        "GET",
        "/api/channels/NetStreamer/live",
        Some(&streamer),
        Value::Null,
    )
    .await;
    assert!(
        own["ad_break"].is_null(),
        "the streamer never sees their own ads"
    );
    e.sql("INSERT INTO ad_campaigns(id,advertiser_id,creative_id,name,placement,impressions,delivered,cpm_cents,amount_cents,starts_on,ends_on,status) VALUES('an-break','an-buyer','an-video','Break','break',1000,999,500,500,current_date-9,current_date-1,'running')").await;
    assert!(
        serve(
            None,
            json!({"placement": "break", "channel": "NetStreamer", "browser_id": guest_browser})
        )
        .await
        .1["ad"]
            .is_null(),
        "no counted session, no ad"
    );
    let (_, served) = serve(
        Some(viewer.clone()),
        json!({"placement": "break", "channel": "NetStreamer", "browser_id": browser}),
    )
    .await;
    assert_eq!(
        served["ad"]["kind"], "video",
        "past its end date and unfinished, it still runs (the make-good)"
    );
    let ticket = served["ad"]["ticket"].clone();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    assert_eq!(
        report(Some(viewer.clone()), ticket, browser, 2100, true).await["counted"],
        true
    );
    assert_eq!(
        status(e, "an-break").await.0,
        "delivered",
        "the last impression completes it"
    );
    let (_, studio) = call(e, "GET", "/api/me/ads", Some(&streamer), Value::Null).await;
    assert_eq!(studio["breaks"][0]["impressions"], 1);
    // A Scout streamer earns 65% of the break impression (5 tenths of a cent: 3); the viewer's
    // lease is Counted, not Trusted, so no Ad Valor; the rest stays with S.V.E.R.
    assert_eq!(balance(e, "usd:earnings:an-streamer").await, 3);
    assert_eq!(studio["earnings"]["total_tenths"], 3);
    assert_eq!(balance(e, "usd:adv:an-viewer").await, 0);
    let (_, valor) = call(e, "GET", "/api/me/ad-valor", Some(&viewer), Value::Null).await;
    assert_eq!(valor["balance_tenths"], 0);
    let (_, admin) = call(e, "GET", "/api/admin/adnet", Some(&staff), Value::Null).await;
    let placements = admin["revenue"]["placements"].as_array().unwrap();
    assert!(
        placements
            .iter()
            .any(|p| p["placement"] == "break" && p["streamers"] == 3),
        "{placements:?}"
    );
    assert!(
        admin["revenue"]["rejected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["reason"] == "hidden")
    );
    let clicked = call(
        e,
        "GET",
        &format!(
            "/api/ads/click/{}",
            served["ad"]["ticket"].as_str().unwrap()
        ),
        None,
        Value::Null,
    )
    .await
    .0;
    assert_eq!(clicked, StatusCode::SEE_OTHER);
    // Parallel reports from one guest browser count once: the caps hold under concurrency.
    let guest = format!("b:{}", sver::security::digest(guest_browser));
    let break_id: String = sqlx::query_scalar(
        "SELECT id FROM ad_breaks WHERE channel_id='an-streamer' ORDER BY starts_at DESC LIMIT 1",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    for statement in [
        "INSERT INTO ad_campaigns(id,advertiser_id,creative_id,name,placement,impressions,delivered,cpm_cents,amount_cents,starts_on,ends_on,status) VALUES('an-race','an-buyer','an-video','Race','break',1000,0,500,500,current_date,current_date+9,'running')".to_string(),
        format!("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('an-b','{guest}',now()+interval '1 hour','counted')"),
        format!("INSERT INTO ad_serves(id,campaign_id,creative_id,placement,viewer_key,channel_id,break_id,served_at) SELECT 'an-p'||n,'an-race','an-video','break','{guest}','an-streamer','{break_id}',now()-interval '3 seconds' FROM generate_series(1,3) n"),
    ] {
        e.sql(sqlx::AssertSqlSafe(statement)).await;
    }
    // Holding the campaign row makes all three reach their cap checks before any of them counts.
    let mut blocker = e.app.db.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM ad_campaigns WHERE id='an-race' FOR UPDATE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (a, b, c, _) = tokio::join!(
        report(None, json!("an-p1"), guest_browser, 2100, true),
        report(None, json!("an-p2"), guest_browser, 2100, true),
        report(None, json!("an-p3"), guest_browser, 2100, true),
        async move {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            blocker.commit().await.unwrap();
        },
    );
    assert_eq!(
        [&a, &b, &c].iter().filter(|r| r["counted"] == true).count(),
        1,
        "the same ad once per 10 minutes, however the reports race: {a} {b} {c}"
    );
    let posted: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_transactions WHERE kind='ad_impression' AND reference LIKE 'ad:an-p%'").fetch_one(&e.app.db).await.unwrap();
    assert_eq!(posted, 1);

    // A partial refund from Stripe's dashboard: the campaign stops, the job refunds the rest of
    // the undelivered part, and the prepaid account ends at zero.
    let undelivered = sver::adnet::undelivered_refund(site_cents, 2000, 1);
    let half = site_cents / 2;
    let refund = json!({"id": "evt_ad_site_refund", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_ad_site", "amount": site_cents, "amount_refunded": half}}});
    assert_eq!(stripe_event(e, refund).await, StatusCode::OK);
    assert_eq!(
        status(e, &site).await,
        (
            "refund_due".into(),
            undelivered as i32,
            Some("cancelled".into())
        )
    );
    sver::adnet::tick(&e.app).await.unwrap();
    assert_eq!(status(e, &site).await.0, "cancelled");
    let sent = e
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
        sent.contains(&format!("amount={}", undelivered - half)),
        "only the rest: {sent}"
    );
    let ours = json!({"id": "evt_ad_site_refund_2", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_ad_site", "amount": site_cents, "amount_refunded": undelivered}}});
    assert_eq!(stripe_event(e, ours).await, StatusCode::OK);
    assert_eq!(status(e, &site).await.0, "cancelled");
    assert_eq!(balance(e, &format!("usd:ads:prepaid:{site}")).await, 0);

    // A chargeback pauses the campaign; winning it resumes; losing ends it and zeroes prepaid.
    let (disp, disp_cents) = buy(e, &buyer, &creative, want(2000, 0, 9), "disp").await;
    sver::adserve::tick(&e.app).await.unwrap();
    let dispute = |evt: &str, kind: &str, id: &str, status: &str| {
        json!({"id": evt, "type": kind, "data": {"object": {
            "id": id, "payment_intent": "pi_ad_disp", "amount": disp_cents, "status": status}}})
    };
    assert_eq!(
        stripe_event(
            e,
            dispute(
                "evt_ad_d1",
                "charge.dispute.created",
                "dp_ad_1",
                "needs_response"
            )
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(status(e, &disp).await.0, "disputed");
    assert_eq!(
        stripe_event(
            e,
            dispute("evt_ad_d2", "charge.dispute.closed", "dp_ad_1", "won")
        )
        .await,
        StatusCode::OK
    );
    sver::adserve::tick(&e.app).await.unwrap();
    assert_eq!(status(e, &disp).await.0, "running", "won: back on");
    for (evt, kind, s) in [
        ("evt_ad_d3", "charge.dispute.created", "needs_response"),
        ("evt_ad_d4", "charge.dispute.closed", "lost"),
    ] {
        assert_eq!(
            stripe_event(e, dispute(evt, kind, "dp_ad_2", s)).await,
            StatusCode::OK
        );
    }
    assert_eq!(status(e, &disp).await.0, "cancelled", "lost: ended");
    assert_eq!(balance(e, &format!("usd:ads:prepaid:{disp}")).await, 0);
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test' WHERE id='an-b'")
        .await;
    e.sql("UPDATE feature_switches SET off=true WHERE name='ad_sales'")
        .await;
}
