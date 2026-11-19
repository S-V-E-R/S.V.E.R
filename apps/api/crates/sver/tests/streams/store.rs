//! App Store and Google Play packs against synthetic store APIs: the purchase pre-check, claims,
//! notifications, the hourly sweep, refunds and clawbacks, the under-18 cap, test purchases on
//! production, and a balanced ledger.
use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use sver::{App, Config, security as sec};
use tower::ServiceExt;

const SECRET: &str = "synthetic-notify-secret-0123456789abcdef";

/// What the synthetic stores know.
#[derive(Default)]
struct Stores {
    /// Apple transactions by id (the decoded signedTransactionInfo).
    apple: HashMap<String, Value>,
    /// Signed payloads Apple's notification history returns.
    history: Vec<Value>,
    /// Play productsv2 purchases by token.
    google: HashMap<String, Value>,
    /// Voided Purchases records.
    voided: Vec<Value>,
    /// Tokens consumed, and whether consume fails right now.
    consumed: Vec<String>,
    consume_fails: bool,
    /// Store API calls, to show what a refused request never reached.
    calls: usize,
}
type Fake = Arc<Mutex<Stores>>;

fn jws(payload: &Value) -> String {
    format!(
        "{}.{}.sig",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256"}"#),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}
async fn apple_transaction(
    State(fake): State<Fake>,
    Path((env, id)): Path<(String, String)>,
) -> Response {
    let mut f = fake.lock().unwrap();
    f.calls += 1;
    match f.apple.get(&id) {
        Some(t) if t["environment"].as_str().unwrap().to_lowercase() == env => {
            Json(json!({"signedTransactionInfo": jws(t)})).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn apple_history(State(fake): State<Fake>) -> Json<Value> {
    let f = fake.lock().unwrap();
    Json(json!({"notificationHistory": f.history, "hasMore": false}))
}
async fn google_purchase(State(fake): State<Fake>, Path(token): Path<String>) -> Response {
    let mut f = fake.lock().unwrap();
    f.calls += 1;
    match f.google.get(&token) {
        Some(p) => Json(p.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn google_consume(
    State(fake): State<Fake>,
    Path((_, call)): Path<(String, String)>,
) -> Response {
    let mut f = fake.lock().unwrap();
    if f.consume_fails {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let token = call.strip_suffix(":consume").unwrap().to_string();
    f.consumed.push(token);
    Json(json!({})).into_response()
}
async fn google_voided(State(fake): State<Fake>) -> Json<Value> {
    Json(json!({"voidedPurchases": fake.lock().unwrap().voided}))
}

async fn serve(fake: Fake) -> String {
    let purchases = "/androidpublisher/v3/applications/tv.sver.app/purchases";
    let router = Router::new()
        .route("/{env}/inApps/v1/transactions/{id}", get(apple_transaction))
        .route(
            "/{env}/inApps/v1/notifications/history",
            post(apple_history),
        )
        .route(
            "/token",
            post(|| async { Json(json!({"access_token": "synthetic"})) }),
        )
        .route(
            &format!("{purchases}/productsv2/tokens/{{token}}"),
            get(google_purchase),
        )
        .route(
            &format!("{purchases}/products/{{product}}/tokens/{{call}}"),
            post(google_consume),
        )
        .route(&format!("{purchases}/voidedpurchases"), get(google_voided))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{address}")
}

/// Store keys made for this run only (nothing committed), pointed at the synthetic stores.
fn configured(base: &str, production: bool) -> Config {
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};
    use web_push_native::jwt_simple::prelude::ES256KeyPair;
    let mut config = Config::from_env().unwrap();
    config.resend_key.clear();
    config.production = production;
    let rsa = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
    config.store = sver::store::Config {
        apple: Some(sver::store::apple::Key {
            issuer: "synthetic-issuer".into(),
            key_id: "SYNTHETIC1".into(),
            pem: ES256KeyPair::generate().to_pem().unwrap(),
        }),
        google: Some(sver::store::google::Account {
            email: "synthetic@example.test".into(),
            pem: rsa.to_pkcs8_pem(LineEnding::LF).unwrap().to_string(),
        }),
        test_users: vec!["st-tester".into()],
        test_monthly_valor: 2_000,
        notify_secret: SECRET.into(),
        google_us_link: false,
        apple_url: format!("{base}/production"),
        apple_sandbox_url: format!("{base}/sandbox"),
        google_api_url: base.into(),
        google_token_url: format!("{base}/token"),
    };
    config
}

async fn send(
    app: &App,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
    origin: bool,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
    if origin {
        request = request.header("origin", &app.config.origin);
    }
    if let Some(token) = token {
        request = request.header("cookie", format!("{}={token}", app.config.cookie_name()));
    }
    let response = sver::router(app.clone())
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A signed-in, verified account born `years` ago; returns its session cookie and store token.
async fn person(app: &App, id: &str, years: i32) -> (String, String) {
    sqlx::query("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES($1,$2,$3,true,current_date-make_interval(years=>$4))")
        .bind(id).bind(format!("{id}@example.test")).bind(id.replace('-', "")).bind(years)
        .execute(&app.db).await.unwrap();
    let cookie = sec::token();
    sqlx::query("INSERT INTO sessions(id,user_id,token_hash,auth_version,mfa_verified,user_agent) SELECT $1,id,$2,auth_version,true,'synthetic' FROM users WHERE id=$3")
        .bind(format!("{id}-session")).bind(sec::digest(&cookie)).bind(id).execute(&app.db).await.unwrap();
    let token = sqlx::query_scalar("SELECT store_token::text FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    (cookie, token)
}
async fn balance(app: &App, account: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(amount),0)::bigint FROM ledger_entries WHERE account=$1",
    )
    .bind(account)
    .fetch_one(&app.db)
    .await
    .unwrap()
}
async fn count(app: &App, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&app.db).await.unwrap()
}

fn apple(f: &Fake, id: &str, env: &str, product: &str, token: Option<&str>) {
    let mut t = json!({"transactionId": id, "bundleId": "tv.sver.app", "productId": product,
        "type": "Consumable", "quantity": 1, "environment": env});
    if let Some(token) = token {
        t["appAccountToken"] = json!(token);
    }
    f.lock().unwrap().apple.insert(id.into(), t);
}
fn revoke(f: &Fake, id: &str, milli: Option<i64>) {
    let mut f = f.lock().unwrap();
    let t = f.apple.get_mut(id).unwrap();
    match milli {
        Some(0) => {
            t.as_object_mut().unwrap().remove("revocationDate");
            t.as_object_mut().unwrap().remove("revocationPercentage");
        }
        Some(m) => {
            t["revocationDate"] = json!(1_700_000_000_000_i64);
            t["revocationPercentage"] = json!(m);
        }
        None => t["revocationDate"] = json!(1_700_000_000_000_i64),
    }
}
fn google(
    f: &Fake,
    token: &str,
    product: &str,
    quantity: i64,
    state: &str,
    owner: &str,
    test: bool,
) {
    let mut p = json!({"productLineItem": [{"productId": product,
        "productOfferDetails": {"quantity": quantity, "refundableQuantity": quantity}}],
        "purchaseStateContext": {"purchaseState": state}, "obfuscatedExternalAccountId": owner});
    if test {
        p["testPurchaseContext"] = json!({"fopType": "TEST"});
    }
    f.lock().unwrap().google.insert(token.into(), p);
}
fn refundable(f: &Fake, token: &str, left: i64) {
    f.lock().unwrap().google.get_mut(token).unwrap()["productLineItem"][0]["productOfferDetails"]
        ["refundableQuantity"] = json!(left);
}
fn apple_notice(id: &str, env: &str, kind: &str) -> Value {
    let transaction = jws(&json!({"transactionId": id}));
    json!({"signedPayload": jws(&json!({"notificationType": kind,
        "data": {"environment": env, "signedTransactionInfo": transaction}}))})
}
fn google_notice(token: &str, voided: bool) -> Value {
    let n = if voided {
        json!({"packageName": "tv.sver.app", "voidedPurchaseNotification": {"purchaseToken": token, "productType": 2, "refundType": 2}})
    } else {
        json!({"packageName": "tv.sver.app", "oneTimeProductNotification": {"notificationType": 1, "purchaseToken": token}})
    };
    json!({"message": {"data": STANDARD.encode(n.to_string())}})
}
async fn claim_apple(app: &App, cookie: &str, id: &str, env: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/api/me/wallet/apple",
        Some(cookie),
        json!({"transaction_id": id, "environment": env}),
        true,
    )
    .await
}
async fn claim_google(app: &App, cookie: &str, token: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/api/me/wallet/google",
        Some(cookie),
        json!({"token": token}),
        true,
    )
    .await
}
async fn precheck(app: &App, cookie: &str, product: &str, consent: bool) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/api/me/wallet/store",
        Some(cookie),
        json!({"store": "apple", "product": product, "guardian_consent": consent}),
        true,
    )
    .await
}
async fn notify(app: &App, store: &str, secret: &str, body: Value) -> StatusCode {
    let path = format!("/api/store/{store}/notifications/{secret}");
    send(app, "POST", &path, None, body, false).await.0
}
/// Runs the hourly sweep now.
async fn sweep(app: &App) {
    sqlx::query("DELETE FROM rate_limits WHERE key='store-tick'")
        .execute(&app.db)
        .await
        .unwrap();
    sver::store::tick(app).await.unwrap();
}
async fn tribute(app: &App, cookie: &str, valor: i64) -> (StatusCode, Value) {
    let body = json!({"id": uuid::Uuid::new_v4().to_string(), "body": "For the stream!", "tribute": valor});
    send(
        app,
        "POST",
        "/api/channels/stowner/chat",
        Some(cookie),
        body,
        true,
    )
    .await
}

pub async fn exercise(db: sqlx::PgPool) {
    let fake: Fake = Arc::default();
    let base = serve(fake.clone()).await;
    let dev = App::new(db.clone(), configured(&base, false))
        .await
        .unwrap();
    let prod = App::new(db.clone(), configured(&base, true)).await.unwrap();
    let mut bare_config = Config::from_env().unwrap();
    bare_config.resend_key.clear();
    bare_config.store = Default::default();
    let bare = App::new(db.clone(), bare_config).await.unwrap();

    let (owner, _) = person(&dev, "st-owner", 30).await;
    // The streamer can earn: authenticator 2FA and finished payout setup.
    sqlx::query("UPDATE users SET mfa_enabled=true,mfa_secret=$1 WHERE id='st-owner'")
        .bind(sec::seal(&dev, "totp:st-owner", "JBSWY3DPEHPK3PXP").unwrap())
        .execute(&dev.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO payout_accounts(user_id,stripe_account,details_submitted,payouts_enabled) VALUES('st-owner','acct_st_owner',true,true)")
        .execute(&dev.db).await.unwrap();
    let (buyer, buyer_token) = person(&dev, "st-buyer", 30).await;
    let (_, other_token) = person(&dev, "st-other", 30).await;
    let (_, stranger_token) = person(&dev, "st-stranger", 30).await;

    // Unconfigured: the wallet says so, and every endpoint refuses or doesn't exist.
    let (status, wallet) = send(
        &bare,
        "GET",
        "/api/me/wallet",
        Some(&buyer),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{wallet}");
    assert_eq!(wallet["store"]["apple"], false);
    assert_eq!(wallet["store"]["google"], false);
    assert_eq!(wallet["store"]["google_us_link"], false);
    assert_eq!(wallet["store"]["packs"].as_array().unwrap().len(), 7);
    assert_eq!(
        precheck(&bare, &buyer, "tv.sver.app.valor.100", false)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        claim_apple(&bare, &buyer, "1", "Sandbox").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        claim_google(&bare, &buyer, "tok").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        notify(&bare, "apple", SECRET, json!({})).await,
        StatusCode::NOT_FOUND,
        "no secret set: no endpoint"
    );
    let (_, wallet) = send(
        &dev,
        "GET",
        "/api/me/wallet",
        Some(&buyer),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(wallet["store"]["apple"], true);
    assert_eq!(wallet["store"]["google"], true);

    // The pre-check hands out the account's store token and records the intent.
    let (status, refused) = precheck(&dev, &buyer, "tv.sver.app.valor.123", false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["field"], "product");
    let (status, checked) = precheck(&dev, &buyer, "tv.sver.app.valor.500", false).await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["token"], buyer_token.as_str());
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_intents WHERE user_id='st-buyer'"
        )
        .await,
        1
    );

    // Ids are checked before any store call: 400, and the store never hears of them.
    let calls = fake.lock().unwrap().calls;
    for (id, env) in [
        ("12a", "Sandbox"),
        ("../1", "Sandbox"),
        ("1?x", "Sandbox"),
        ("1", "Xcode"),
        ("1", "sandbox"),
        ("", "Sandbox"),
    ] {
        assert_eq!(
            claim_apple(&dev, &buyer, id, env).await.0,
            StatusCode::BAD_REQUEST,
            "{id} {env}"
        );
    }
    for token in ["..", "a/b", "a?b", "a#b", "a%2Fb", ""] {
        assert_eq!(
            claim_google(&dev, &buyer, token).await.0,
            StatusCode::BAD_REQUEST,
            "{token}"
        );
    }
    assert_eq!(
        claim_google(&dev, &buyer, &"a".repeat(513)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(fake.lock().unwrap().calls, calls);
    // Claims need the app's Origin like every other write.
    let (status, _) = send(
        &dev,
        "POST",
        "/api/me/wallet/apple",
        Some(&buyer),
        json!({"transaction_id": "1001", "environment": "Sandbox"}),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A claim credits exactly once: replays and concurrent claims too.
    apple(
        &fake,
        "1001",
        "Sandbox",
        "tv.sver.app.valor.500",
        Some(&buyer_token),
    );
    let (status, credited) = claim_apple(&dev, &buyer, "1001", "Sandbox").await;
    assert_eq!(status, StatusCode::OK, "{credited}");
    assert_eq!(
        credited,
        json!({"status": "credited", "valor": 500, "mine": true})
    );
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_intents WHERE user_id='st-buyer'"
        )
        .await,
        0,
        "the intent is removed on credit"
    );
    assert_eq!(
        claim_apple(&dev, &buyer, "1001", "Sandbox").await.1["valor"],
        500
    );
    apple(
        &fake,
        "1002",
        "Sandbox",
        "tv.sver.app.valor.100",
        Some(&buyer_token),
    );
    let (a, b) = tokio::join!(
        claim_apple(&dev, &buyer, "1002", "Sandbox"),
        claim_apple(&dev, &buyer, "1002", "Sandbox")
    );
    assert_eq!(
        (a.0, b.0),
        (StatusCode::OK, StatusCode::OK),
        "{} {}",
        a.1,
        b.1
    );
    assert_eq!(balance(&dev, "valor:st-buyer").await, 600);
    // A development server only asks the sandbox, whatever the app says.
    apple(
        &fake,
        "1003",
        "Sandbox",
        "tv.sver.app.valor.100",
        Some(&buyer_token),
    );
    assert_eq!(
        claim_apple(&dev, &buyer, "1003", "Production").await.0,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-buyer").await, 700);
    // Apple doesn't know it yet: retry later.
    assert_eq!(
        claim_apple(&dev, &buyer, "1999", "Sandbox").await.0,
        StatusCode::CONFLICT
    );

    // The token decides the owner: another account's purchase goes to it, and the caller sees only
    // their own balance. No token, or an unknown one, is refused.
    apple(
        &fake,
        "1004",
        "Sandbox",
        "tv.sver.app.valor.200",
        Some(&other_token),
    );
    let (_, theirs) = claim_apple(&dev, &buyer, "1004", "Sandbox").await;
    assert_eq!(
        theirs,
        json!({"status": "credited", "valor": 700, "mine": false})
    );
    assert_eq!(balance(&dev, "valor:st-other").await, 200);
    apple(&fake, "1005", "Sandbox", "tv.sver.app.valor.100", None);
    assert_eq!(
        claim_apple(&dev, &buyer, "1005", "Sandbox").await.0,
        StatusCode::FORBIDDEN
    );
    apple(
        &fake,
        "1006",
        "Sandbox",
        "tv.sver.app.valor.100",
        Some(&uuid::Uuid::new_v4().to_string()),
    );
    assert_eq!(
        claim_apple(&dev, &buyer, "1006", "Sandbox").await.0,
        StatusCode::FORBIDDEN
    );
    // A product that isn't a pack stays unfinished and credits nothing.
    apple(
        &fake,
        "1007",
        "Sandbox",
        "tv.sver.app.valor.123",
        Some(&buyer_token),
    );
    assert_eq!(
        claim_apple(&dev, &buyer, "1007", "Sandbox").await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(balance(&dev, "valor:st-buyer").await, 700);
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_purchases WHERE store_id IN ('1005','1006','1007')"
        )
        .await,
        0
    );

    // Notifications: no Origin needed, a secret path, and a body that only names the purchase.
    assert_eq!(
        notify(
            &dev,
            "apple",
            "wrong-secret-0123456789abcdef-wrong",
            apple_notice("1001", "Sandbox", "REFUND")
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        notify(&dev, "google", "x", json!({})).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        notify(&dev, "apple", SECRET, json!({"signedPayload": "garbage"})).await,
        StatusCode::OK
    );
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("1001", "Sandbox", "ONE_TIME_CHARGE")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        balance(&dev, "valor:st-buyer").await,
        700,
        "already claimed"
    );
    // A notification that arrives before the claim credits the token's owner; the claim then
    // finds it credited.
    apple(
        &fake,
        "1008",
        "Sandbox",
        "tv.sver.app.valor.100",
        Some(&buyer_token),
    );
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("1008", "Sandbox", "ONE_TIME_CHARGE")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-buyer").await, 800);
    assert_eq!(
        claim_apple(&dev, &buyer, "1008", "Sandbox").await.1["valor"],
        800
    );
    // A refusal that will never change is acknowledged; a store outage is retried (503).
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("1005", "Sandbox", "ONE_TIME_CHARGE")
        )
        .await,
        StatusCode::OK
    );
    // A production notification on a development server is ignored.
    apple(
        &fake,
        "1009",
        "Production",
        "tv.sver.app.valor.100",
        Some(&buyer_token),
    );
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("1009", "Production", "ONE_TIME_CHARGE")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-buyer").await, 800);

    // Google: pending credits nothing; purchased credits, then consumes after the commit; a
    // failed consume is retried by the sweep. A production purchase is refused in development.
    google(
        &fake,
        "gp-pending",
        "tv.sver.app.valor.100",
        1,
        "PENDING",
        &buyer_token,
        true,
    );
    assert_eq!(
        claim_google(&dev, &buyer, "gp-pending").await.1,
        json!({"status": "pending"})
    );
    google(
        &fake,
        "gp-real",
        "tv.sver.app.valor.100",
        1,
        "PURCHASED",
        &buyer_token,
        false,
    );
    assert_eq!(
        claim_google(&dev, &buyer, "gp-real").await.0,
        StatusCode::FORBIDDEN
    );
    google(
        &fake,
        "gp-cancelled",
        "tv.sver.app.valor.100",
        1,
        "CANCELLED",
        &buyer_token,
        true,
    );
    assert_eq!(
        claim_google(&dev, &buyer, "gp-cancelled").await.1,
        json!({"status": "void"})
    );
    assert_eq!(balance(&dev, "valor:st-buyer").await, 800);
    google(
        &fake,
        "gp-1",
        "tv.sver.app.valor.100",
        1,
        "PURCHASED",
        &buyer_token,
        true,
    );
    assert_eq!(claim_google(&dev, &buyer, "gp-1").await.1["valor"], 900);
    assert_eq!(fake.lock().unwrap().consumed, vec!["gp-1".to_string()]);
    fake.lock().unwrap().consume_fails = true;
    google(
        &fake,
        "gp-2",
        "tv.sver.app.valor.100",
        1,
        "PURCHASED",
        &buyer_token,
        true,
    );
    let (status, credited) = claim_google(&dev, &buyer, "gp-2").await;
    assert_eq!(
        (status, &credited["status"]),
        (StatusCode::OK, &json!("credited")),
        "a failed consume still answers credited"
    );
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_purchases WHERE store_id='gp-2' AND NOT consumed"
        )
        .await,
        1
    );
    fake.lock().unwrap().consume_fails = false;
    sweep(&dev).await;
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_purchases WHERE store='google' AND NOT consumed"
        )
        .await,
        0
    );
    // A Play notification for a new purchase credits and consumes it.
    google(
        &fake,
        "gp-3",
        "tv.sver.app.valor.100",
        1,
        "PURCHASED",
        &buyer_token,
        true,
    );
    assert_eq!(
        notify(&dev, "google", SECRET, google_notice("gp-3", false)).await,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-buyer").await, 1_100);
    assert!(fake.lock().unwrap().consumed.contains(&"gp-3".to_string()));

    // Refunds. A full refund after a tribute: the balance goes negative, spending locks and the
    // streamer's earnings are clawed back; REFUND_REVERSED restores both.
    let (spender, spender_token) = person(&dev, "st-spender", 30).await;
    apple(
        &fake,
        "2001",
        "Sandbox",
        "tv.sver.app.valor.500",
        Some(&spender_token),
    );
    assert_eq!(
        claim_apple(&dev, &spender, "2001", "Sandbox").await.1["valor"],
        500
    );
    let (status, sent) = tribute(&dev, &spender, 400).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(balance(&dev, "usd:earnings:st-owner").await, 3_200);
    revoke(&fake, "2001", None);
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("2001", "Sandbox", "REFUND")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("2001", "Sandbox", "REFUND")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-spender").await, -400);
    assert_eq!(balance(&dev, "usd:earnings:st-owner").await, 0);
    let (status, locked) = tribute(&dev, &spender, 10).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        locked["error"].as_str().unwrap().contains("locked"),
        "{locked}"
    );
    // A claim that read Apple before the refund can't undo it: claims never change refunds.
    revoke(&fake, "2001", Some(0));
    assert_eq!(
        claim_apple(&dev, &spender, "2001", "Sandbox").await.1,
        json!({"status": "void"}),
        "refunded in full: nothing left to credit"
    );
    assert_eq!(balance(&dev, "valor:st-spender").await, -400);
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("2001", "Sandbox", "REFUND_REVERSED")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-spender").await, 100);
    assert_eq!(balance(&dev, "usd:earnings:st-owner").await, 3_200);
    // A partial refund in milliunits: 40% of 500 Valor.
    apple(
        &fake,
        "2002",
        "Sandbox",
        "tv.sver.app.valor.500",
        Some(&spender_token),
    );
    claim_apple(&dev, &spender, "2002", "Sandbox").await;
    revoke(&fake, "2002", Some(40_000));
    assert_eq!(
        notify(
            &dev,
            "apple",
            SECRET,
            apple_notice("2002", "Sandbox", "REFUND")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-spender").await, 100 + 300);
    // Refunded before it was ever credited: the late claim nets to zero and answers void.
    apple(
        &fake,
        "2003",
        "Sandbox",
        "tv.sver.app.valor.1000",
        Some(&spender_token),
    );
    revoke(&fake, "2003", None);
    assert_eq!(
        claim_apple(&dev, &spender, "2003", "Sandbox").await.1,
        json!({"status": "void"})
    );
    assert_eq!(balance(&dev, "valor:st-spender").await, 400);
    // The hourly backstop reads Apple's notification history and settles what it names.
    revoke(&fake, "2002", Some(100_000));
    fake.lock().unwrap().history = vec![apple_notice("2002", "Sandbox", "REFUND")];
    sweep(&dev).await;
    assert_eq!(balance(&dev, "valor:st-spender").await, 100);

    // Google refunds by quantity come from refundableQuantity, only ever grow, and a repeated
    // poll posts nothing new. A voided notification reads its own purchase.
    google(
        &fake,
        "gq-1",
        "tv.sver.app.valor.100",
        4,
        "PURCHASED",
        &spender_token,
        true,
    );
    assert_eq!(claim_google(&dev, &spender, "gq-1").await.1["valor"], 500);
    refundable(&fake, "gq-1", 3);
    fake.lock().unwrap().voided = vec![json!({"purchaseToken": "gq-1", "voidedQuantity": 1})];
    sweep(&dev).await;
    assert_eq!(balance(&dev, "valor:st-spender").await, 400);
    let posted = count(&dev, "SELECT count(*) FROM ledger_transactions").await;
    sweep(&dev).await;
    assert_eq!(
        count(&dev, "SELECT count(*) FROM ledger_transactions").await,
        posted
    );
    refundable(&fake, "gq-1", 4);
    sweep(&dev).await;
    assert_eq!(
        balance(&dev, "valor:st-spender").await,
        400,
        "a Google refund never shrinks"
    );
    refundable(&fake, "gq-1", 0);
    assert_eq!(
        notify(&dev, "google", SECRET, google_notice("gq-1", true)).await,
        StatusCode::OK
    );
    assert_eq!(balance(&dev, "valor:st-spender").await, 100);
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_purchases WHERE store_id='gq-1' AND refunded_milli=100000"
        )
        .await,
        1
    );
    // A full refund whose productsv2 leaves refundableQuantity out is still taken back: the
    // voided list's full-refund record is the floor.
    google(
        &fake,
        "gq-2",
        "tv.sver.app.valor.100",
        1,
        "PURCHASED",
        &spender_token,
        true,
    );
    assert_eq!(claim_google(&dev, &spender, "gq-2").await.1["valor"], 200);
    fake.lock().unwrap().google.get_mut("gq-2").unwrap()["productLineItem"][0]
        ["productOfferDetails"]
        .as_object_mut()
        .unwrap()
        .remove("refundableQuantity");
    fake.lock().unwrap().voided = vec![json!({"purchaseToken": "gq-2"})];
    sweep(&dev).await;
    assert_eq!(balance(&dev, "valor:st-spender").await, 100);

    // Under 18: the pre-check needs a guardian; store packs and open intents share the $50 cap
    // with card checkouts; a purchase completed over the cap is credited anyway and flagged.
    let (teen, teen_token) = person(&dev, "st-teen", 15).await;
    let (status, needs) = precheck(&dev, &teen, "tv.sver.app.valor.100", false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(needs["field"], "guardian_consent");
    assert_eq!(
        precheck(&dev, &teen, "tv.sver.app.valor.5000", true)
            .await
            .0,
        StatusCode::OK
    );
    let (status, capped) = precheck(&dev, &teen, "tv.sver.app.valor.100", false).await;
    assert_eq!(
        (status, &capped["field"]),
        (StatusCode::BAD_REQUEST, &json!("cents")),
        "the open intent counts"
    );
    apple(
        &fake,
        "3001",
        "Sandbox",
        "tv.sver.app.valor.5000",
        Some(&teen_token),
    );
    assert_eq!(
        claim_apple(&dev, &teen, "3001", "Sandbox").await.1["valor"],
        5_000
    );
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_intents WHERE user_id='st-teen'"
        )
        .await,
        0
    );
    assert_eq!(
        precheck(&dev, &teen, "tv.sver.app.valor.100", false)
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "the purchase counts"
    );
    let (status, card) = send(
        &dev,
        "POST",
        "/api/me/wallet/checkout",
        Some(&teen),
        json!({"cents": 99}),
        true,
    )
    .await;
    assert_eq!(
        (status, &card["field"]),
        (StatusCode::BAD_REQUEST, &json!("cents")),
        "card checkouts share the cap"
    );
    let (teen2, _) = person(&dev, "st-teen2", 16).await;
    sqlx::query("INSERT INTO checkout_sessions(id,user_id,kind,amount_cents,valor) VALUES('cs_st_open','st-teen2','valor',4500,4500)")
        .execute(&dev.db).await.unwrap();
    assert_eq!(
        precheck(&dev, &teen2, "tv.sver.app.valor.1000", true)
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "an open card checkout counts"
    );
    apple(
        &fake,
        "3002",
        "Sandbox",
        "tv.sver.app.valor.200",
        Some(&teen_token),
    );
    assert_eq!(
        claim_apple(&dev, &teen, "3002", "Sandbox").await.1["valor"],
        5_200
    );
    let flagged: bool = sqlx::query_scalar("SELECT (detail->>'over_cap')::boolean FROM ledger_transactions WHERE reference='store:apple:3002'")
        .fetch_one(&dev.db).await.unwrap();
    assert!(flagged);
    // Staff see store purchases (with the flag) and store refunds on the Money page.
    sqlx::query("INSERT INTO staff_roles(user_id,role) VALUES('st-owner','admin')")
        .execute(&dev.db)
        .await
        .unwrap();
    let (status, money) = send(
        &dev,
        "GET",
        "/api/admin/money",
        Some(&owner),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{money}");
    let payments = money["payments"].as_array().unwrap();
    assert!(
        payments.iter().any(|p| p["purchase"] == "apple:3002"
            && p["over_cap"] == true
            && p["status"]
                .as_str()
                .unwrap()
                .contains("over the under-18 cap")),
        "{money}"
    );
    let kinds: Vec<&str> = money["reversals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"store_refund") && kinds.contains(&"store_refund_reversed"),
        "{kinds:?}"
    );

    // Production: sandbox only for test accounts, against valor:test, capped monthly; real
    // purchases book cash at the list price.
    assert_eq!(
        claim_apple(&prod, &buyer, "4001", "Sandbox").await.0,
        StatusCode::FORBIDDEN
    );
    let (tester, tester_token) = person(&prod, "st-tester", 30).await;
    let test_valor = balance(&prod, "valor:test").await;
    // Two claims at once that each fit the allowance but not together: only one is credited.
    for id in ["4002", "4005"] {
        apple(
            &fake,
            id,
            "Sandbox",
            "tv.sver.app.valor.2000",
            Some(&tester_token),
        );
    }
    let (a, b) = tokio::join!(
        claim_apple(&prod, &tester, "4002", "Sandbox"),
        claim_apple(&prod, &tester, "4005", "Sandbox")
    );
    let mut codes = [a.0, b.0];
    codes.sort();
    assert_eq!(
        codes,
        [StatusCode::OK, StatusCode::FORBIDDEN],
        "{} {}",
        a.1,
        b.1
    );
    assert_eq!(balance(&prod, "valor:test").await, test_valor - 2_000);
    assert_eq!(
        balance(&prod, "usd:apple").await,
        0,
        "test purchases book no cash"
    );
    apple(
        &fake,
        "4003",
        "Sandbox",
        "tv.sver.app.valor.100",
        Some(&tester_token),
    );
    assert_eq!(
        claim_apple(&prod, &tester, "4003", "Sandbox").await.0,
        StatusCode::FORBIDDEN,
        "over the monthly test allowance"
    );
    google(
        &fake,
        "gp-test",
        "tv.sver.app.valor.100",
        1,
        "PURCHASED",
        &stranger_token,
        true,
    );
    assert_eq!(
        notify(&prod, "google", SECRET, google_notice("gp-test", false)).await,
        StatusCode::OK
    );
    assert_eq!(
        balance(&prod, "valor:st-stranger").await,
        0,
        "a test purchase for a regular account"
    );
    apple(
        &fake,
        "4004",
        "Production",
        "tv.sver.app.valor.1000",
        Some(&buyer_token),
    );
    let before = balance(&prod, "valor:st-buyer").await;
    assert_eq!(
        claim_apple(&prod, &buyer, "4004", "Production").await.1["valor"],
        before + 1_000
    );
    assert_eq!(balance(&prod, "usd:apple").await, 9_990);
    google(
        &fake,
        "gp-prod",
        "tv.sver.app.valor.200",
        1,
        "PURCHASED",
        &buyer_token,
        false,
    );
    assert_eq!(
        claim_google(&prod, &buyer, "gp-prod").await.1["valor"],
        before + 1_200
    );
    assert_eq!(balance(&prod, "usd:google").await, 1_990);

    // Lapsed intents are pruned by the sweep.
    sqlx::query("INSERT INTO store_intents(user_id,cents,created_at) VALUES('st-buyer',99,now()-interval '31 minutes')")
        .execute(&dev.db).await.unwrap();
    sweep(&dev).await;
    assert_eq!(
        count(
            &dev,
            "SELECT count(*) FROM store_intents WHERE user_id='st-buyer'"
        )
        .await,
        0
    );

    // The ledger balances in every unit.
    assert_eq!(
        count(&dev, "SELECT count(*) FROM (SELECT unit FROM ledger_entries GROUP BY unit HAVING sum(amount)<>0) u").await,
        0
    );
}
