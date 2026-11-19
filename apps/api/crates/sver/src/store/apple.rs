//! App Store Server API client: Get Transaction Info and Get Notification History. Responses are
//! read from Apple's API host over TLS, so they are trusted without checking the JWS signature
//! (docs/SUPPORT.md "Store packs"); notification bodies are only decoded to learn which
//! transaction to fetch.
use super::{APP_ID, Config, Env, Found, State};
use crate::profiles::{Fail, Res};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use serde_json::{Value, json};
use web_push_native::jwt_simple::prelude::*;

/// The In-App Purchase key from App Store Connect.
#[derive(Clone)]
pub struct Key {
    pub issuer: String,
    pub key_id: String,
    /// The `.p8` contents (PKCS#8 PEM).
    pub pem: String,
}

/// `APPLE_IAP_ISSUER_ID`, `APPLE_IAP_KEY_ID`, `APPLE_IAP_PRIVATE_KEY`: all three or none, and the
/// key must parse. A PEM written on one line with literal `\n` is accepted.
pub fn key_from_env() -> Result<Option<Key>, String> {
    let var = |name| {
        std::env::var(name)
            .ok()
            .filter(|v: &String| !v.trim().is_empty())
    };
    match (
        var("APPLE_IAP_ISSUER_ID"),
        var("APPLE_IAP_KEY_ID"),
        var("APPLE_IAP_PRIVATE_KEY"),
    ) {
        (None, None, None) => Ok(None),
        (Some(issuer), Some(key_id), Some(pem)) => {
            let key = Key {
                issuer: issuer.trim().into(),
                key_id: key_id.trim().into(),
                pem: pem.replace("\\n", "\n"),
            };
            ES256KeyPair::from_pem(&key.pem)
                .map_err(|_| "APPLE_IAP_PRIVATE_KEY is not a valid .p8 key")?;
            Ok(Some(key))
        }
        _ => Err(
            "Set all of APPLE_IAP_ISSUER_ID, APPLE_IAP_KEY_ID and APPLE_IAP_PRIVATE_KEY, or none"
                .into(),
        ),
    }
}

/// Apple transaction ids are decimal numbers. Checked before any id goes into a signed URL.
pub fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// The app's `environmentIOS` hint: exactly `Production` or `Sandbox` (Xcode and LocalTesting
/// purchases can't be checked with Apple's server, so they are refused).
pub fn env(hint: &str) -> Option<Env> {
    match hint {
        "Production" => Some(Env::Production),
        "Sandbox" => Some(Env::Sandbox),
        _ => None,
    }
}

#[derive(Serialize, Deserialize)]
struct Bid {
    bid: String,
}

/// A fresh API token per request, as Apple asks: ES256, `kid` header, five minutes.
fn token(key: &Key) -> Res<String> {
    let pair = ES256KeyPair::from_pem(&key.pem)
        .map_err(|_| Fail::internal())?
        .with_key_id(&key.key_id);
    let mut claims =
        Claims::with_custom_claims(Bid { bid: APP_ID.into() }, Duration::from_secs(300))
            .with_issuer(&key.issuer)
            .with_audience("appstoreconnect-v1");
    claims.invalid_before = None;
    pair.sign(claims).map_err(|_| Fail::internal())
}

fn base(c: &Config, env: Env) -> &str {
    match env {
        Env::Production => &c.apple_url,
        Env::Sandbox => &c.apple_sandbox_url,
    }
}

/// The store couldn't answer now; the caller leaves the purchase unfinished and tries again.
pub(super) fn down() -> Fail {
    Fail::unavailable("The store couldn't be reached. Try again in a moment.")
}

/// Sends one store API request. Transport errors, 429 and 5xx are retryable 503s; 401 and 403 mean
/// our credentials or permissions are wrong, so they are logged as errors and also answer 503. Any
/// other status comes back with its body (null when it isn't JSON) for the caller to judge.
pub(super) async fn call(request: reqwest::RequestBuilder, what: &str) -> Res<(u16, Value)> {
    let response = request.send().await.map_err(|e| {
        eprintln!("store_api=unreachable call={what} error={e}");
        down()
    })?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        eprintln!("store_api=credentials_refused call={what} status={status} fix=check_store_keys");
        return Err(down());
    }
    if status.as_u16() == 429 || status.is_server_error() {
        eprintln!("store_api=retry call={what} status={status}");
        return Err(down());
    }
    let body = response.json::<Value>().await;
    if status.is_success() {
        return body.map(|b| (status.as_u16(), b)).map_err(|e| {
            eprintln!("store_api=unreadable call={what} error={e}");
            down()
        });
    }
    Ok((status.as_u16(), body.unwrap_or(Value::Null)))
}

/// A store error body's short code for the log (never the whole body, which may carry tokens).
pub(super) fn code(body: &Value) -> String {
    let code = [&body["errorCode"], &body["error"]["status"], &body["error"]]
        .into_iter()
        .find(|v| v.is_string() || v.is_number())
        .map_or(String::new(), |v| v.to_string());
    code.chars().take(80).collect()
}

/// The decoded (not verified) payload of a compact JWS: `header.payload.signature`, base64url
/// without padding.
fn payload(jws: &str) -> Option<Value> {
    let mut parts = jws.split('.');
    let (_, middle, _, None) = (parts.next()?, parts.next()?, parts.next()?, parts.next()) else {
        return None;
    };
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(middle).ok()?).ok()
}

/// Get Transaction Info: the transaction as Apple's API reports it now, or None when Apple doesn't
/// know the id (404, or 400 for an id it rejects). `env` only picks which Apple host to ask.
pub async fn transaction(
    http: &reqwest::Client,
    c: &Config,
    env: Env,
    id: &str,
) -> Res<Option<Found>> {
    if !valid_id(id) {
        return Err(Fail::field(
            "transaction_id",
            "This isn't an App Store purchase.",
        ));
    }
    let Some(key) = &c.apple else {
        return Err(Fail::unavailable(
            "Buying Valor in the app isn't available yet.",
        ));
    };
    let url = format!("{}/inApps/v1/transactions/{id}", base(c, env));
    let (status, body) = call(http.get(url).bearer_auth(token(key)?), "Apple transaction").await?;
    match status {
        200..=299 => parse(env, id, body).map(Some),
        400 | 404 => {
            eprintln!(
                "store_api=not_found call=apple_transaction id={id} status={status} code={}",
                code(&body)
            );
            Ok(None)
        }
        _ => {
            eprintln!(
                "store_api=unexpected call=apple_transaction id={id} status={status} code={}",
                code(&body)
            );
            Err(down())
        }
    }
}

/// Checks a Get Transaction Info response and maps it. A purchase for another app or of a type
/// other than Consumable is refused (403).
fn parse(env: Env, id: &str, body: Value) -> Res<Found> {
    let Some(t) = body["signedTransactionInfo"].as_str().and_then(payload) else {
        eprintln!("store_api=unreadable call=apple_transaction id={id}");
        return Err(down());
    };
    if t["bundleId"] != APP_ID {
        eprintln!(
            "store_api=refused call=apple_transaction id={id} bundle={}",
            t["bundleId"]
        );
        return Err(Fail::denied("This purchase isn't for this app."));
    }
    if t["type"] != "Consumable" {
        eprintln!(
            "store_api=refused call=apple_transaction id={id} type={}",
            t["type"]
        );
        return Err(Fail::denied("This purchase isn't a Valor pack."));
    }
    let named = match env {
        Env::Production => "Production",
        Env::Sandbox => "Sandbox",
    };
    let quantity = t.get("quantity").map_or(Some(1), Value::as_i64);
    let (Some(product), Some(quantity), true, true) = (
        t["productId"].as_str(),
        quantity.filter(|q| (1..=100).contains(q)),
        t["transactionId"] == id,
        t["environment"] == named,
    ) else {
        eprintln!("store_api=unexpected_fields call=apple_transaction id={id}");
        return Err(Fail::internal());
    };
    let refunded = if t.get("revocationDate").is_some_and(|d| !d.is_null()) {
        match t.get("revocationPercentage").map(Value::as_i64) {
            None => 100_000,
            Some(Some(p)) if (0..=100_000).contains(&p) => p,
            Some(_) => {
                eprintln!(
                    "store_api=unexpected_fields call=apple_transaction id={id} field=revocationPercentage"
                );
                return Err(Fail::internal());
            }
        }
    } else {
        0
    };
    Ok(Found {
        store: "apple",
        id: id.into(),
        product: product.into(),
        quantity,
        token: t["appAccountToken"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok()),
        env,
        state: State::Purchased,
        refunded_milli: Some(refunded),
        raw: t,
    })
}

/// Decodes an App Store Server Notification V2 body `{signedPayload}` (unverified; only a signal
/// to fetch the transaction): its transaction id, environment and notificationType. None when it is
/// malformed or carries no transaction (a TEST notification, for one).
pub fn notification(body: &Value) -> Option<(String, Env, String)> {
    let n = payload(body["signedPayload"].as_str()?)?;
    let kind = n["notificationType"].as_str()?.to_string();
    let t = payload(n["data"]["signedTransactionInfo"].as_str()?)?;
    let id = t["transactionId"].as_str().filter(|id| valid_id(id))?;
    let env = env(n["data"]["environment"].as_str()?)?;
    Some((id.into(), env, kind))
}

/// Get Notification History since `since` in one environment: the backstop for notifications
/// Apple gave up delivering. Each entry is decoded as `notification` does; undecodable ones are
/// skipped.
pub async fn history(
    http: &reqwest::Client,
    c: &Config,
    env: Env,
    since: chrono::DateTime<chrono::Utc>,
) -> Res<Vec<(String, Env, String)>> {
    let Some(key) = &c.apple else {
        return Ok(Vec::new());
    };
    let url = format!("{}/inApps/v1/notifications/history", base(c, env));
    let range = json!({
        "startDate": since.timestamp_millis(),
        "endDate": chrono::Utc::now().timestamp_millis(),
    });
    let mut found = Vec::new();
    let mut page: Option<String> = None;
    // ponytail: 100 pages of 20 is far more than two days of notifications at our volume.
    for _ in 0..100 {
        let mut request = http.post(&url).bearer_auth(token(key)?).json(&range);
        if let Some(page) = &page {
            request = request.query(&[("paginationToken", page)]);
        }
        let (status, body) = call(request, "Apple notification history").await?;
        if !(200..=299).contains(&status) {
            eprintln!(
                "store_api=unexpected call=apple_history status={status} code={}",
                code(&body)
            );
            return Err(down());
        }
        for entry in body["notificationHistory"].as_array().into_iter().flatten() {
            found.extend(notification(entry));
        }
        page = body["paginationToken"].as_str().map(String::from);
        if body["hasMore"] != true || page.is_none() {
            return Ok(found);
        }
    }
    eprintln!("store_api=page_limit call=apple_history");
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compact JWS with an empty signature, as the decoder only reads the payload.
    fn jws(payload: &Value) -> String {
        format!(
            "{}.{}.sig",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256"}"#),
            URL_SAFE_NO_PAD.encode(payload.to_string())
        )
    }
    fn transaction_json() -> Value {
        json!({
            "transactionId": "2000000123456789",
            "bundleId": APP_ID,
            "productId": "tv.sver.app.valor.500",
            "type": "Consumable",
            "quantity": 2,
            "environment": "Sandbox",
            "appAccountToken": "6f1c1f43-2c1a-4c4e-9d8e-0d5f9b8a7c21",
        })
    }
    /// The HTTP status a result answers with (Found has no Debug, so no unwrap_err).
    fn status<T>(r: Res<T>) -> u16 {
        r.err().map_or(200, |e| e.status.as_u16())
    }
    fn parsed(t: &Value) -> Res<Found> {
        parse(
            Env::Sandbox,
            "2000000123456789",
            json!({"signedTransactionInfo": jws(t)}),
        )
    }
    fn config(url: &str, key: Option<Key>) -> Config {
        Config {
            apple: key,
            apple_url: url.into(),
            apple_sandbox_url: url.into(),
            ..Config::default()
        }
    }
    fn key() -> (ES256KeyPair, Key) {
        let pair = ES256KeyPair::generate();
        let key = Key {
            issuer: "issuer-uuid".into(),
            key_id: "KEYID12345".into(),
            pem: pair.to_pem().unwrap(),
        };
        (pair, key)
    }

    #[test]
    fn api_token_claims_and_kid() {
        let (pair, key) = key();
        let jwt = token(&key).unwrap();
        let claims = pair.public_key().verify_token::<Bid>(&jwt, None).unwrap();
        assert_eq!(claims.custom.bid, APP_ID);
        assert_eq!(claims.issuer.as_deref(), Some("issuer-uuid"));
        assert_eq!(
            claims.audiences.unwrap().into_string().unwrap(),
            "appstoreconnect-v1"
        );
        let life = claims.expires_at.unwrap() - claims.issued_at.unwrap();
        assert!(life.as_secs() <= 3600 && life.as_secs() >= 60);
        let meta = Token::decode_metadata(&jwt).unwrap();
        assert_eq!(meta.key_id(), Some("KEYID12345"));
        assert_eq!(meta.algorithm(), "ES256");
    }

    #[test]
    fn parses_a_consumable() {
        let found = parsed(&transaction_json()).unwrap();
        assert_eq!(found.store, "apple");
        assert_eq!(found.product, "tv.sver.app.valor.500");
        assert_eq!(found.quantity, 2);
        assert_eq!(found.env, Env::Sandbox);
        assert_eq!(found.state, State::Purchased);
        assert_eq!(found.refunded_milli, Some(0));
        assert_eq!(
            found.token.unwrap().to_string(),
            "6f1c1f43-2c1a-4c4e-9d8e-0d5f9b8a7c21"
        );
        let mut t = transaction_json();
        t["appAccountToken"] = json!("not-a-uuid");
        assert!(parsed(&t).unwrap().token.is_none());
        t.as_object_mut().unwrap().remove("appAccountToken");
        t.as_object_mut().unwrap().remove("quantity");
        let found = parsed(&t).unwrap();
        assert!(found.token.is_none());
        assert_eq!(found.quantity, 1);
    }

    #[test]
    fn refuses_other_apps_types_and_mismatches() {
        let mut t = transaction_json();
        t["bundleId"] = json!("com.example.other");
        assert_eq!(status(parsed(&t)), 403);
        let mut t = transaction_json();
        t["type"] = json!("Non-Consumable");
        assert_eq!(status(parsed(&t)), 403);
        let mut t = transaction_json();
        t["environment"] = json!("Production");
        assert!(parsed(&t).is_err());
        let mut t = transaction_json();
        t["transactionId"] = json!("1");
        assert!(parsed(&t).is_err());
        let mut t = transaction_json();
        t["quantity"] = json!(0);
        assert!(parsed(&t).is_err());
        assert!(parse(Env::Sandbox, "1", json!({"signedTransactionInfo": "x.y"})).is_err());
    }

    #[test]
    fn revocation_is_in_milliunits() {
        let mut t = transaction_json();
        t["revocationDate"] = json!(1_763_000_000_000_i64);
        assert_eq!(parsed(&t).unwrap().refunded_milli, Some(100_000));
        t["revocationPercentage"] = json!(40_000);
        assert_eq!(parsed(&t).unwrap().refunded_milli, Some(40_000));
        t["revocationPercentage"] = json!(100_001);
        assert!(parsed(&t).is_err());
        // A percentage without a revocation date isn't a refund.
        t.as_object_mut().unwrap().remove("revocationDate");
        assert_eq!(parsed(&t).unwrap().refunded_milli, Some(0));
    }

    #[test]
    fn validates_ids_and_environment_hints() {
        assert!(valid_id("2000000123456789"));
        assert!(valid_id(&"9".repeat(32)));
        for bad in ["", "12a", "../1", "1?x", "1#", " 1", &"9".repeat(33)] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert_eq!(env("Production"), Some(Env::Production));
        assert_eq!(env("Sandbox"), Some(Env::Sandbox));
        for bad in ["Xcode", "LocalTesting", "sandbox", ""] {
            assert_eq!(env(bad), None);
        }
    }

    #[test]
    fn decodes_notifications() {
        let signed = |data: Value| json!({"signedPayload": jws(&json!({"notificationType": "REFUND", "data": data}))});
        let body = signed(json!({
            "environment": "Sandbox",
            "signedTransactionInfo": jws(&transaction_json()),
        }));
        assert_eq!(
            notification(&body),
            Some(("2000000123456789".into(), Env::Sandbox, "REFUND".into()))
        );
        let mut t = transaction_json();
        t["transactionId"] = json!("../x");
        let malformed = [
            json!({}),
            json!({"signedPayload": "not a jws"}),
            json!({"signedPayload": "a.!!!.c"}),
            json!({"signedPayload": "a.b.c.d"}),
            signed(json!({"environment": "Sandbox"})),
            signed(
                json!({"environment": "Xcode", "signedTransactionInfo": jws(&transaction_json())}),
            ),
            signed(json!({"environment": "Sandbox", "signedTransactionInfo": jws(&t)})),
        ];
        for body in malformed {
            assert_eq!(notification(&body), None, "{body}");
        }
    }

    #[tokio::test]
    async fn fetches_over_http_and_404_is_none() {
        let info = jws(&transaction_json());
        let fake = axum::Router::new().route(
            "/inApps/v1/transactions/{id}",
            axum::routing::get(
                move |axum::extract::Path(id): axum::extract::Path<String>| {
                    let info = info.clone();
                    async move {
                        match id.as_str() {
                            "2000000123456789" => (
                                axum::http::StatusCode::OK,
                                axum::Json(json!({"signedTransactionInfo": info})),
                            ),
                            "503" => (
                                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                                axum::Json(json!({})),
                            ),
                            _ => (
                                axum::http::StatusCode::NOT_FOUND,
                                axum::Json(json!({"errorCode": 4040010})),
                            ),
                        }
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });
        let http = reqwest::Client::new();
        let c = config(&url, Some(key().1));
        let found = transaction(&http, &c, Env::Sandbox, "2000000123456789")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.product, "tv.sver.app.valor.500");
        assert!(
            transaction(&http, &c, Env::Sandbox, "1")
                .await
                .unwrap()
                .is_none()
        );
        let down = transaction(&http, &c, Env::Sandbox, "503").await;
        assert_eq!(status(down), 503);
        let bad = transaction(&http, &c, Env::Sandbox, "1/../2").await;
        assert_eq!(status(bad), 400);
        let off = transaction(&http, &config(&url, None), Env::Sandbox, "1").await;
        assert_eq!(status(off), 503);
    }
}
