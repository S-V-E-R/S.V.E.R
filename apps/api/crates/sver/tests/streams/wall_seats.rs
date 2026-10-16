//! The Wall (docs/WALL.md) against the synthetic Stripe: sales stay closed until staff open them,
//! adults only, no overlapping spots, size limits, price stages at checkout, hold expiry and late
//! payments, review, rejection and resubmission, refunds, edits, removal, reports, the emergency
//! switch, and that a seat changes nothing else about its buyer.
use super::Env;
use super::chat::{call, person};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::net::SocketAddr;
use tower::ServiceExt;

/// A detailed square PNG of `side` pixels.
fn picture(side: u32, seed: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(side, side, |x, y| {
        image::Rgb([
            (x * seed % 251) as u8,
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
/// A multipart tile form (picture optional) as the person with `token`.
async fn form(
    e: &Env,
    method: &str,
    path: &str,
    token: &str,
    file: Option<Vec<u8>>,
    fields: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend(
            format!("--tile\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    if let Some(file) = file {
        body.extend(b"--tile\r\nContent-Disposition: form-data; name=\"file\"; filename=\"tile.png\"\r\nContent-Type: image/png\r\n\r\n");
        body.extend(file);
        body.extend(b"\r\n");
    }
    body.extend(b"--tile--\r\n");
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "multipart/form-data; boundary=tile")
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
/// Any signed Stripe event.
async fn signed(e: &Env, event: Value) -> StatusCode {
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
async fn hold(e: &Env, token: &str, x: i32, y: i32, size: &str) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        "/api/wall/holds",
        Some(token),
        json!({"x": x, "y": y, "size": size}),
    )
    .await
}
/// Holds, fills in and pays for a spot; returns the tile number and the Stripe form sent.
async fn buy(e: &Env, token: &str, x: i32, y: i32, size: &str, name: &str) -> (i64, String) {
    let (status, held) = hold(e, token, x, y, size).await;
    assert_eq!(status, StatusCode::OK, "{held}");
    let n = held["tile"]["n"].as_i64().unwrap();
    let side = held["tile"]["min_picture"].as_u64().unwrap() as u32;
    let (status, out) = form(
        e,
        "POST",
        &format!("/api/wall/{n}/checkout"),
        token,
        Some(picture(side, n as u32 + 3)),
        &[
            ("name", name),
            ("link", "https://example.com/build"),
            ("message", "Building it together."),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{out}");
    let (session, sent) = {
        let fake = e.fake.lock().unwrap();
        (
            format!("cs_test_{}", fake.stripe.len()),
            fake.stripe.last().unwrap().1.clone(),
        )
    };
    let cents: i64 = sqlx::query_scalar("SELECT price_cents::bigint FROM wall_tiles WHERE n=$1")
        .bind(n)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        webhook(
            e,
            &format!("evt_wall_{n}"),
            &session,
            cents,
            &format!("pi_wall_{n}")
        )
        .await,
        StatusCode::OK
    );
    (n, sent)
}
async fn status(e: &Env, n: i64) -> String {
    sqlx::query_scalar("SELECT status FROM wall_tiles WHERE n=$1")
        .bind(n)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}

pub async fn exercise(e: &Env) {
    let buyer = person(e, "wl-buyer", "WallBuyer", true).await;
    let other = person(e, "wl-other", "WallOther", true).await;
    let third = person(e, "wl-third", "WallThird", true).await;
    let teen = person(e, "wl-teen", "WallTeen", true).await;
    let viewer = person(e, "wl-viewer", "WallViewer", true).await;
    let staff = person(e, "wl-staff", "WallStaff", true).await;
    for statement in [
        "UPDATE users SET date_of_birth=current_date - interval '15 years' WHERE id='wl-teen'",
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='wl-staff'",
        "UPDATE sessions SET mfa_verified=true WHERE user_id='wl-staff'",
        "INSERT INTO staff_roles(user_id,role) VALUES('wl-staff','admin')",
    ] {
        e.sql(statement).await;
    }
    let profile_before = call(
        e,
        "GET",
        "/api/channels/WallBuyer",
        Some(&viewer),
        Value::Null,
    )
    .await
    .1;

    // Nothing sells until staff open sales; the grid is public.
    let (code, wall) = call(e, "GET", "/api/wall", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        (wall["columns"].as_i64(), wall["rows"].as_i64()),
        (Some(40), Some(25))
    );
    assert_eq!(
        (wall["taken"].as_i64(), wall["sales_open"].as_bool()),
        (Some(0), Some(false))
    );
    assert_eq!(
        hold(e, &buyer, 0, 0, "seat").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // Before sales open, prices and limits can change; one hall is allowed for this test.
    let prices =
        json!({"seat":[2500,3000,3500],"table":[9000,11000,12500],"hall":[30000,36000,42000]});
    let (code, body) = call(
        e,
        "PUT",
        "/api/admin/wall/settings",
        Some(&staff),
        json!({"prices": prices, "limits": {"table": 60, "hall": 1}, "note": "Test limits"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/admin/wall/settings",
            Some(&buyer),
            json!({"prices": prices, "limits": {"table": 60, "hall": 1}, "note": "x"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (code, body) = call(
        e,
        "POST",
        "/api/admin/wall/open",
        Some(&staff),
        json!({"note": "Terms, tax and prices settled"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    // After opening, the current stage's prices and the limits are fixed.
    assert_eq!(call(e, "PUT", "/api/admin/wall/settings", Some(&staff), json!({"prices": {"seat":[2000,3000,3500],"table":[9000,11000,12500],"hall":[30000,36000,42000]}, "limits": {"table": 60, "hall": 1}, "note": "Cheaper"})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(call(e, "PUT", "/api/admin/wall/settings", Some(&staff), json!({"prices": {"seat":[2500,3100,3500],"table":[9000,11000,12500],"hall":[30000,36000,42000]}, "limits": {"table": 60, "hall": 1}, "note": "Next stage"})).await.0, StatusCode::OK);

    // Adults only; spots must fit and never overlap; one open hold at a time.
    assert_eq!(hold(e, &teen, 0, 0, "seat").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        hold(e, &buyer, 39, 0, "table").await.0,
        StatusCode::BAD_REQUEST
    );
    let (n1, sent) = buy(e, &buyer, 0, 0, "seat", "Ember Forge").await;
    assert!(
        sent.contains("unit_amount%5D=2500") && sent.contains("mode=payment"),
        "{sent}"
    );
    assert_eq!(status(e, n1).await, "in_review");
    let (code, _) = hold(e, &other, 0, 0, "table").await;
    assert_eq!(code, StatusCode::CONFLICT, "overlapping spots are refused");
    let (code, held) = hold(e, &other, 1, 0, "table").await;
    assert_eq!(code, StatusCode::OK, "{held}");
    let n2 = held["tile"]["n"].as_i64().unwrap();
    assert_eq!(
        hold(e, &other, 10, 10, "seat").await.0,
        StatusCode::CONFLICT,
        "one hold at a time"
    );
    let (_, wall) = call(e, "GET", "/api/wall", None, Value::Null).await;
    let claiming = wall["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["n"] == n2)
        .unwrap()
        .clone();
    assert_eq!(claiming["state"], "claiming");
    assert!(claiming.get("name").is_none());

    // Size limits hold.
    let (n3, _) = buy(e, &third, 20, 10, "hall", "Hall of Glint").await;
    assert_eq!(status(e, n3).await, "in_review");
    assert_eq!(
        hold(e, &viewer, 30, 10, "hall").await.0,
        StatusCode::BAD_REQUEST
    );

    // A hold lapses after 15 minutes and frees its cells; a late payment for a spot taken
    // meanwhile is refunded automatically.
    let (code, out) = form(
        e,
        "POST",
        &format!("/api/wall/{n2}/checkout"),
        &other,
        Some(picture(200, 9)),
        &[("name", "Late table")],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{out}");
    let late_session = format!("cs_test_{}", e.fake.lock().unwrap().stripe.len());
    e.sql("UPDATE wall_tiles SET hold_until=now()-interval '1 second' WHERE status='held'")
        .await;
    sver::seats::tick(&e.app).await.unwrap();
    assert_eq!(status(e, n2).await, "expired");
    let (code, taken) = hold(e, &viewer, 2, 1, "seat").await;
    assert_eq!(code, StatusCode::OK, "{taken}");
    assert_eq!(
        webhook(e, "evt_wall_late", &late_session, 9000, "pi_wall_late").await,
        StatusCode::OK
    );
    assert_eq!(status(e, n2).await, "refund_due");
    sver::seats::tick(&e.app).await.unwrap();
    assert_eq!(status(e, n2).await, "refunded");
    assert!(
        e.fake
            .lock()
            .unwrap()
            .stripe
            .iter()
            .any(|(p, b)| p == "/v1/refunds" && b.contains("pi_wall_late")),
        "the late payment is refunded"
    );
    // A late payment whose spot is still free gets it back.
    let viewer_n = taken["tile"]["n"].as_i64().unwrap();
    let (code, out) = form(
        e,
        "POST",
        &format!("/api/wall/{viewer_n}/checkout"),
        &viewer,
        Some(picture(100, 5)),
        &[("name", "Viewer seat")],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{out}");
    let session = format!("cs_test_{}", e.fake.lock().unwrap().stripe.len());
    e.sql("UPDATE wall_tiles SET hold_until=now()-interval '1 second' WHERE status='held'")
        .await;
    sver::seats::tick(&e.app).await.unwrap();
    assert_eq!(status(e, viewer_n).await, "expired");
    assert_eq!(
        webhook(e, "evt_wall_restored", &session, 2500, "pi_wall_restored").await,
        StatusCode::OK
    );
    assert_eq!(status(e, viewer_n).await, "in_review");

    // A second checkout for the same hold closes the first; a payment that still arrives for
    // the first is refunded in full and changes nothing.
    let (code, held) = hold(e, &third, 35, 20, "seat").await;
    assert_eq!(code, StatusCode::OK, "{held}");
    let twice = held["tile"]["n"].as_i64().unwrap();
    let path = format!("/api/wall/{twice}/checkout");
    assert_eq!(
        form(
            e,
            "POST",
            &path,
            &third,
            Some(picture(100, 7)),
            &[("name", "Twice")]
        )
        .await
        .0,
        StatusCode::OK
    );
    let first = format!("cs_test_{}", e.fake.lock().unwrap().stripe.len());
    assert_eq!(
        form(
            e,
            "POST",
            &path,
            &third,
            Some(picture(100, 7)),
            &[("name", "Twice")]
        )
        .await
        .0,
        StatusCode::OK
    );
    let second = format!("cs_test_{}", e.fake.lock().unwrap().stripe.len());
    assert!(
        e.fake
            .lock()
            .unwrap()
            .stripe
            .iter()
            .any(|(p, _)| *p == format!("/v1/checkout/sessions/{first}/expire")),
        "the earlier checkout is closed"
    );
    assert_eq!(
        webhook(e, "evt_wall_first", &first, 2500, "pi_wall_first").await,
        StatusCode::OK
    );
    assert_eq!(
        status(e, twice).await,
        "held",
        "a superseded checkout doesn't buy the tile"
    );
    assert_eq!(
        webhook(e, "evt_wall_second", &second, 2500, "pi_wall_second").await,
        StatusCode::OK
    );
    assert_eq!(status(e, twice).await, "in_review");
    assert_eq!(
        webhook(e, "evt_wall_first_again", &first, 2500, "pi_wall_first").await,
        StatusCode::OK
    );
    sver::seats::tick(&e.app).await.unwrap();
    let refunds = e
        .fake
        .lock()
        .unwrap()
        .stripe
        .iter()
        .filter(|(p, b)| p == "/v1/refunds" && b.contains("pi_wall_first"))
        .count();
    assert_eq!(refunds, 1, "the extra payment is refunded once");
    assert!(
        !e.fake
            .lock()
            .unwrap()
            .stripe
            .iter()
            .any(|(p, b)| p == "/v1/refunds" && b.contains("pi_wall_second"))
    );

    // Prices follow the stage at checkout start.
    e.sql("UPDATE wall_settings SET stages='{1,600}'").await;
    let (n4, sent) = buy(e, &other, 30, 0, "seat", "Second stage").await;
    assert!(sent.contains("unit_amount%5D=3100"), "{sent}");

    // Review: approval puts a tile live; rejection tells the buyer, who can resubmit or get a refund.
    let (_, queue) = call(
        e,
        "GET",
        "/api/admin/wall/review",
        Some(&staff),
        Value::Null,
    )
    .await;
    assert!(
        queue["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["n"] == n1),
        "{queue}"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/wall/{n1}/review"),
            Some(&staff),
            json!({"action": "approve"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(status(e, n1).await, "live");
    let (code, page) = call(e, "GET", &format!("/api/wall/{n1}"), None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        (
            page["tile"]["name"].as_str(),
            page["tile"]["owner"].as_str()
        ),
        (Some("Ember Forge"), Some("WallBuyer"))
    );
    assert!(page["tile"]["image"].as_str().unwrap().contains("/wall/"));
    assert_eq!(
        call(e, "GET", &format!("/api/wall/{n4}"), None, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND,
        "tiles in review aren't public"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/wall/{n4}/review"),
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
            &format!("/api/admin/wall/{n4}/review"),
            Some(&staff),
            json!({"action": "reject", "note": "The picture shows a betting site."})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(status(e, n4).await, "rejected");
    let (_, mine) = call(
        e,
        "GET",
        &format!("/api/wall/{n4}"),
        Some(&other),
        Value::Null,
    )
    .await;
    assert_eq!(
        mine["mine"]["review_note"],
        "The picture shows a betting site."
    );
    let (code, out) = form(
        e,
        "PUT",
        &format!("/api/wall/{n4}"),
        &other,
        Some(picture(100, 11)),
        &[("name", "Second stage, fixed")],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{out}");
    assert_eq!(status(e, n4).await, "in_review");
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/wall/{n4}/refund"),
            Some(&buyer),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "only the owner"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/wall/{n4}/refund"),
            Some(&other),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    sver::seats::tick(&e.app).await.unwrap();
    assert_eq!(status(e, n4).await, "refunded");
    assert_eq!(
        hold(e, &other, 30, 0, "seat").await.0,
        StatusCode::OK,
        "a refunded spot is free again"
    );
    e.sql("UPDATE wall_tiles SET hold_until=now()-interval '1 second' WHERE status='held'")
        .await;
    sver::seats::tick(&e.app).await.unwrap();
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/wall/{n1}/refund"),
            Some(&buyer),
            Value::Null
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "no refund after approval"
    );

    // A change to a live tile is reviewed; the approved version stays up meanwhile.
    let (code, out) = form(
        e,
        "PUT",
        &format!("/api/wall/{n1}"),
        &buyer,
        None,
        &[
            ("name", "Ember Forge Guild"),
            ("link", "https://example.com/guild"),
        ],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{out}");
    let (_, page) = call(e, "GET", &format!("/api/wall/{n1}"), None, Value::Null).await;
    assert_eq!(page["tile"]["name"], "Ember Forge");
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/wall/{n1}/review"),
            Some(&staff),
            json!({"action": "approve", "note": "Fine"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, page) = call(e, "GET", &format!("/api/wall/{n1}"), None, Value::Null).await;
    assert_eq!(
        (page["tile"]["name"].as_str(), page["tile"]["link"].as_str()),
        (Some("Ember Forge Guild"), Some("https://example.com/guild"))
    );
    assert!(
        page["tile"]["image"].is_string(),
        "a change without a picture keeps the picture"
    );

    // Reports reach the queue; a removal keeps the spot empty, and every action is audited.
    let (code, body) = call(
        e,
        "POST",
        "/api/reports",
        Some(&viewer),
        json!({"target_type": "wall_tile", "target_id": n1.to_string(), "reason": "spam"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (_, reports) = call(
        e,
        "GET",
        "/api/admin/reports?type=wall_tile",
        Some(&staff),
        Value::Null,
    )
    .await;
    assert!(
        reports
            .to_string()
            .contains(&format!("\"target_id\":\"{n1}\"")),
        "{reports}"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/wall/{n3}/review"),
            Some(&staff),
            json!({"action": "approve"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/wall/{n3}/unlink"),
            Some(&staff),
            json!({"note": "The link now leads to a banned site"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, page) = call(e, "GET", &format!("/api/wall/{n3}"), None, Value::Null).await;
    assert!(page["tile"]["link"].is_null() && page["tile"]["name"] == "Hall of Glint");
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/wall/{n3}/remove"),
            Some(&staff),
            json!({"note": "Broke the Wall terms"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "GET", &format!("/api/wall/{n3}"), None, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        hold(e, &viewer, 21, 11, "seat").await.0,
        StatusCode::CONFLICT,
        "a removed tile's spot stays empty"
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE target_type IN ('wall_tile','wall') AND action LIKE 'wall_%'")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(audited >= 8, "{audited} audited Wall actions");

    // The emergency switch pauses holds and checkouts; live tiles stay up.
    e.sql("INSERT INTO feature_switches(name,off) VALUES('wall_sales',true) ON CONFLICT(name) DO UPDATE SET off=true").await;
    assert_eq!(
        hold(e, &viewer, 15, 15, "seat").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let (_, wall) = call(e, "GET", "/api/wall", None, Value::Null).await;
    assert_eq!(wall["sales_open"], false);
    assert!(
        wall["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["n"] == n1 && t["state"] == "live")
    );
    e.sql("DELETE FROM feature_switches WHERE name='wall_sales'")
        .await;

    // Wall money changes nothing else about the buyer, and posts only to the Wall's own account.
    let profile_after = call(
        e,
        "GET",
        "/api/channels/WallBuyer",
        Some(&viewer),
        Value::Null,
    )
    .await
    .1;
    assert_eq!(
        profile_before, profile_after,
        "a seat buys nothing outside /wall"
    );
    let accounts: Vec<String> = sqlx::query_scalar("SELECT DISTINCT e.account FROM ledger_entries e JOIN ledger_transactions t ON t.id=e.transaction_id WHERE t.kind='wall_purchase' ORDER BY 1")
        .fetch_all(&e.app.db).await.unwrap();
    assert_eq!(accounts, ["usd:stripe", "usd:wall"]);
    let (_, admin) = call(e, "GET", "/api/admin/wall", Some(&staff), Value::Null).await;
    assert!(
        admin["sales_cents"].as_i64().unwrap() >= 2500 + 30000,
        "{admin}"
    );

    // A refund made outside the Wall's own flow (Stripe's dashboard, staff money tools) takes the
    // tile down and frees its spot; the Wall's own refund job then has nothing left to send.
    let cents: i64 = sqlx::query_scalar("SELECT price_cents::bigint FROM wall_tiles WHERE n=$1")
        .bind(n1)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        signed(
            e,
            json!({"id": format!("evt_refund_{n1}"), "type": "charge.refunded", "data": {"object": {
            "id": format!("ch_wall_{n1}"), "payment_intent": format!("pi_wall_{n1}"),
            "amount": cents, "amount_refunded": cents}}})
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(status(e, n1).await, "refunded");
    let cells: i64 = sqlx::query_scalar("SELECT count(*) FROM wall_cells WHERE tile_n=$1")
        .bind(n1)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(cells, 0, "the spot is free again");
    let refunds_before = e
        .fake
        .lock()
        .unwrap()
        .stripe
        .iter()
        .filter(|c| c.0 == "/v1/refunds")
        .count();
    sver::seats::tick(&e.app).await.unwrap();
    let refunds_after = e
        .fake
        .lock()
        .unwrap()
        .stripe
        .iter()
        .filter(|c| c.0 == "/v1/refunds")
        .count();
    assert_eq!(refunds_before, refunds_after, "nothing is refunded twice");
}
