//! Google Play Developer API client: purchases.productsv2 (state), purchases.products consume and
//! Voided Purchases. Called with our service-account token over TLS, so responses are trusted;
//! Pub/Sub notification bodies are only decoded to learn which purchase token to fetch.
use super::apple::{call, code, down};
use super::{APP_ID, Config, Env, Found, State};
use crate::profiles::{Fail, Res};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use rsa::{
    RsaPrivateKey,
    pkcs1v15::SigningKey,
    pkcs8::DecodePrivateKey,
    sha2::Sha256,
    signature::{SignatureEncoding, Signer},
};
use serde_json::{Value, json};

/// The service account that calls the Play Developer API.
#[derive(Clone)]
pub struct Account {
    pub email: String,
    /// `private_key` from the service-account JSON (PKCS#8 PEM).
    pub pem: String,
}

/// `GOOGLE_PLAY_SERVICE_ACCOUNT`: the service-account JSON. Only `client_email` and `private_key`
/// are read; when it is set, both must be present and the key must parse.
pub fn account_from_env() -> Result<Option<Account>, String> {
    let Some(json) = std::env::var("GOOGLE_PLAY_SERVICE_ACCOUNT")
        .ok()
        .filter(|v| !v.trim().is_empty())
    else {
        return Ok(None);
    };
    let bad = || "GOOGLE_PLAY_SERVICE_ACCOUNT is not a valid service-account key".to_string();
    let v: Value = serde_json::from_str(&json).map_err(|_| bad())?;
    let (Some(email), Some(pem)) = (v["client_email"].as_str(), v["private_key"].as_str()) else {
        return Err(bad());
    };
    RsaPrivateKey::from_pkcs8_pem(pem).map_err(|_| bad())?;
    Ok(Some(Account {
        email: email.into(),
        pem: pem.into(),
    }))
}

/// Play purchase tokens (and product ids) as they may go into a signed URL: no `/`, `?`, `#` or
/// `%`, and not only dots.
pub fn valid_token(token: &str) -> bool {
    (1..=512).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && token.bytes().any(|b| b != b'.')
}

/// The RS256 assertion exchanged for an access token (Google's JWT bearer flow), five minutes.
fn assertion(account: &Account, audience: &str) -> Res<String> {
    let key = RsaPrivateKey::from_pkcs8_pem(&account.pem).map_err(|_| Fail::internal())?;
    let now = chrono::Utc::now().timestamp();
    let claims = json!({
        "iss": account.email,
        "scope": "https://www.googleapis.com/auth/androidpublisher",
        "aud": audience,
        "iat": now,
        "exp": now + 300,
    });
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let signature = SigningKey::<Sha256>::new(key).sign(input.as_bytes());
    Ok(format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

/// A fresh access token. ponytail: one token per call; cache it if call volume ever matters.
async fn access(http: &reqwest::Client, c: &Config) -> Res<String> {
    let Some(account) = &c.google else {
        return Err(Fail::unavailable(
            "Buying Valor in the app isn't available yet.",
        ));
    };
    let form = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
        ("assertion", &assertion(account, &c.google_token_url)?),
    ];
    let request = http.post(&c.google_token_url).form(&form);
    let (status, body) = call(request, "Google token").await?;
    match body["access_token"].as_str() {
        Some(token) if (200..=299).contains(&status) => Ok(token.into()),
        _ => {
            eprintln!(
                "store_api=credentials_refused call=google_token status={status} code={} fix=check_GOOGLE_PLAY_SERVICE_ACCOUNT",
                code(&body)
            );
            Err(down())
        }
    }
}

fn app_url(c: &Config) -> String {
    format!(
        "{}/androidpublisher/v3/applications/{APP_ID}/purchases",
        c.google_api_url
    )
}

/// Google writes int64 values as JSON strings and int32 values as numbers.
fn int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}

/// The raw productsv2 purchase, or None when Google doesn't know the token.
async fn get(http: &reqwest::Client, c: &Config, token: &str) -> Res<Option<Value>> {
    if !valid_token(token) {
        return Err(Fail::field("token", "This isn't a Google Play purchase."));
    }
    let url = format!("{}/productsv2/tokens/{token}", app_url(c));
    let request = http.get(url).bearer_auth(access(http, c).await?);
    match call(request, "Google purchase").await? {
        (200..=299, body) => Ok(Some(body)),
        (status @ (400 | 404 | 410), body) => {
            eprintln!(
                "store_api=not_found call=google_purchase status={status} code={}",
                code(&body)
            );
            Ok(None)
        }
        (status, body) => {
            eprintln!(
                "store_api=unexpected call=google_purchase status={status} code={}",
                code(&body)
            );
            Err(down())
        }
    }
}

/// purchases.productsv2.get: the purchase as Google reports it now, or None when Google doesn't
/// know the token (400, 404 or 410).
pub async fn purchase(http: &reqwest::Client, c: &Config, token: &str) -> Res<Option<Found>> {
    match get(http, c, token).await? {
        Some(body) => parse(token, body).map(Some),
        None => Ok(None),
    }
}

/// Maps a productsv2 response. It must hold exactly one line item. `refunded_milli` comes from
/// `refundableQuantity` (what is left unrefunded) and is None when Google leaves it out.
fn parse(token: &str, body: Value) -> Res<Found> {
    let items = body["productLineItem"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let [item] = items else {
        eprintln!(
            "store_api=unexpected_fields call=google_purchase line_items={}",
            items.len()
        );
        return Err(Fail::internal());
    };
    let offer = &item["productOfferDetails"];
    let quantity = offer.get("quantity").map_or(Some(1), int);
    let (Some(product), Some(quantity)) = (
        item["productId"].as_str(),
        quantity.filter(|q| (1..=1000).contains(q)),
    ) else {
        eprintln!("store_api=unexpected_fields call=google_purchase");
        return Err(Fail::internal());
    };
    let refunded = match offer.get("refundableQuantity").map(int) {
        None => None,
        Some(Some(left)) if (0..=quantity).contains(&left) => {
            Some((quantity - left) * 100_000 / quantity)
        }
        Some(_) => {
            eprintln!("store_api=unexpected_fields call=google_purchase field=refundableQuantity");
            return Err(Fail::internal());
        }
    };
    Ok(Found {
        store: "google",
        id: token.into(),
        product: product.into(),
        quantity,
        token: body["obfuscatedExternalAccountId"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok()),
        env: if body.get("testPurchaseContext").is_some() {
            Env::Sandbox
        } else {
            Env::Production
        },
        state: match body["purchaseStateContext"]["purchaseState"].as_str() {
            Some("PURCHASED") => State::Purchased,
            Some("PENDING") => State::Pending,
            _ => State::Void,
        },
        refunded_milli: refunded,
        raw: body,
    })
}

/// purchases.products.consume, so the pack can be bought again. Ok when it was already consumed.
pub async fn consume(http: &reqwest::Client, c: &Config, product: &str, token: &str) -> Res<()> {
    if !valid_token(product) || !valid_token(token) {
        return Err(Fail::bad("This isn't a Google Play purchase."));
    }
    let url = format!("{}/products/{product}/tokens/{token}:consume", app_url(c));
    let request = http
        .post(url)
        .bearer_auth(access(http, c).await?)
        .header(reqwest::header::CONTENT_LENGTH, "0");
    let (status, body) = call(request, "Google consume").await?;
    if (200..=299).contains(&status) {
        return Ok(());
    }
    // Google answers an error for a token that was already consumed; check the purchase itself.
    let consumed = get(http, c, token).await?.is_some_and(|p| {
        p["productLineItem"][0]["productOfferDetails"]["consumptionState"]
            == "CONSUMPTION_STATE_CONSUMED"
    });
    if consumed {
        return Ok(());
    }
    eprintln!(
        "store_api=unexpected call=google_consume status={status} code={}",
        code(&body)
    );
    Err(down())
}

/// Voided Purchases (one-time products, quantity-based partial refunds included) voided since
/// `since`, clamped to the API's 30-day window. One entry per token: the voided quantity summed
/// over its records, or None when any record is a full refund.
pub async fn voided(
    http: &reqwest::Client,
    c: &Config,
    since: chrono::DateTime<chrono::Utc>,
) -> Res<Vec<(String, Option<i64>)>> {
    let oldest = chrono::Utc::now() - chrono::Duration::days(30) + chrono::Duration::minutes(5);
    let start = since.max(oldest).timestamp_millis().to_string();
    let url = format!("{}/voidedpurchases", app_url(c));
    let bearer = access(http, c).await?;
    let mut found: Vec<(String, Option<i64>)> = Vec::new();
    let mut page: Option<String> = None;
    // ponytail: 50 pages of 1000 is far beyond our refund volume; Google allows 30 calls per 30 s.
    for _ in 0..50 {
        let mut query = vec![
            ("startTime", start.as_str()),
            ("type", "0"),
            ("includeQuantityBasedPartialRefund", "true"),
            ("maxResults", "1000"),
        ];
        if let Some(page) = &page {
            query.push(("token", page));
        }
        let request = http.get(&url).bearer_auth(&bearer).query(&query);
        let (status, body) = call(request, "Google voided purchases").await?;
        if !(200..=299).contains(&status) {
            eprintln!(
                "store_api=unexpected call=google_voided status={status} code={}",
                code(&body)
            );
            return Err(down());
        }
        for record in body["voidedPurchases"].as_array().into_iter().flatten() {
            let Some(token) = record["purchaseToken"].as_str().filter(|t| valid_token(t)) else {
                eprintln!("store_api=skipped call=google_voided reason=token");
                continue;
            };
            let quantity = record.get("voidedQuantity").and_then(int);
            match found.iter_mut().find(|(t, _)| t == token) {
                Some((_, total)) => *total = total.zip(quantity).map(|(a, b)| a + b),
                None => found.push((token.into(), quantity)),
            }
        }
        page = body["tokenPagination"]["nextPageToken"]
            .as_str()
            .map(String::from);
        if page.is_none() {
            return Ok(found);
        }
    }
    eprintln!("store_api=page_limit call=google_voided");
    Ok(found)
}

/// Decodes a Pub/Sub push body (unverified; only a signal to fetch the purchase): the purchase
/// token and whether it was voided. Only purchased one-time products (type 1) and voided purchases
/// for our package count; everything else, and anything malformed, is None.
pub fn notification(body: &Value) -> Option<(String, bool)> {
    let data = STANDARD.decode(body["message"]["data"].as_str()?).ok()?;
    let n: Value = serde_json::from_slice(&data).ok()?;
    if n["packageName"] != APP_ID {
        return None;
    }
    let (token, voided) = if let Some(v) = n.get("voidedPurchaseNotification") {
        (v["purchaseToken"].as_str()?, true)
    } else {
        let p = n.get("oneTimeProductNotification")?;
        if int(&p["notificationType"])? != 1 {
            return None;
        }
        (p["purchaseToken"].as_str()?, false)
    };
    valid_token(token).then(|| (token.into(), voided))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::{
        RsaPublicKey,
        pkcs1v15::{Signature, VerifyingKey},
        pkcs8::{EncodePrivateKey, LineEnding},
        signature::Verifier,
    };

    fn status<T>(r: Res<T>) -> u16 {
        r.err().map_or(200, |e| e.status.as_u16())
    }
    fn purchase_json(state: &str) -> Value {
        json!({
            "productLineItem": [{
                "productId": "tv.sver.app.valor.1000",
                "productOfferDetails": {"quantity": 3, "refundableQuantity": 3,
                    "consumptionState": "CONSUMPTION_STATE_YET_TO_BE_CONSUMED"},
            }],
            "purchaseStateContext": {"purchaseState": state},
            "obfuscatedExternalAccountId": "6f1c1f43-2c1a-4c4e-9d8e-0d5f9b8a7c21",
            "orderId": "GPA.0000-0000-0000-00000",
        })
    }

    #[test]
    fn assertion_is_rs256_with_the_claims() {
        // Generated at test time: no key is ever committed.
        let key = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
        let account = Account {
            email: "valor@example.iam.gserviceaccount.com".into(),
            pem: key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string(),
        };
        let jwt = assertion(&account, "https://token.example/token").unwrap();
        let (input, signature) = jwt.rsplit_once('.').unwrap();
        let signature = Signature::try_from(URL_SAFE_NO_PAD.decode(signature).unwrap().as_slice());
        VerifyingKey::<Sha256>::new(RsaPublicKey::from(&key))
            .verify(input.as_bytes(), &signature.unwrap())
            .unwrap();
        let (header, claims) = input.split_once('.').unwrap();
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
        assert_eq!(header["alg"], "RS256");
        let claims: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap();
        assert_eq!(claims["iss"], "valor@example.iam.gserviceaccount.com");
        assert_eq!(claims["aud"], "https://token.example/token");
        assert_eq!(
            claims["scope"],
            "https://www.googleapis.com/auth/androidpublisher"
        );
        let life = claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap();
        assert!((60..=3600).contains(&life));
    }

    #[test]
    fn maps_purchase_states() {
        let found = parse("tok", purchase_json("PURCHASED")).unwrap();
        assert_eq!(found.store, "google");
        assert_eq!(found.id, "tok");
        assert_eq!(found.product, "tv.sver.app.valor.1000");
        assert_eq!(found.quantity, 3);
        assert_eq!(found.state, State::Purchased);
        assert_eq!(found.env, Env::Production);
        assert_eq!(found.refunded_milli, Some(0));
        assert_eq!(
            found.token.unwrap().to_string(),
            "6f1c1f43-2c1a-4c4e-9d8e-0d5f9b8a7c21"
        );
        assert_eq!(
            parse("t", purchase_json("PENDING")).unwrap().state,
            State::Pending
        );
        assert_eq!(
            parse("t", purchase_json("CANCELLED")).unwrap().state,
            State::Void
        );
        let mut p = purchase_json("PURCHASED");
        p["testPurchaseContext"] = json!({"fopType": "TEST"});
        p["obfuscatedExternalAccountId"] = json!("someone");
        let found = parse("t", p).unwrap();
        assert_eq!(found.env, Env::Sandbox);
        assert!(found.token.is_none());
    }

    #[test]
    fn refunds_and_line_items() {
        let mut p = purchase_json("PURCHASED");
        p["productLineItem"][0]["productOfferDetails"]["refundableQuantity"] = json!(1);
        assert_eq!(parse("t", p.clone()).unwrap().refunded_milli, Some(66_666));
        p["productLineItem"][0]["productOfferDetails"]["refundableQuantity"] = json!(0);
        assert_eq!(parse("t", p.clone()).unwrap().refunded_milli, Some(100_000));
        p["productLineItem"][0]["productOfferDetails"]["refundableQuantity"] = json!(4);
        assert_eq!(status(parse("t", p.clone())), 500);
        p["productLineItem"][0]["productOfferDetails"] = json!({});
        let found = parse("t", p.clone()).unwrap();
        assert_eq!((found.quantity, found.refunded_milli), (1, None));
        let line = p["productLineItem"][0].clone();
        p["productLineItem"] = json!([line.clone(), line]);
        assert_eq!(status(parse("t", p.clone())), 500);
        p["productLineItem"] = json!([]);
        assert_eq!(status(parse("t", p)), 500);
    }

    #[test]
    fn validates_tokens() {
        assert!(valid_token("abcDEF123.-_xyz"));
        assert!(valid_token("tv.sver.app.valor.100"));
        assert!(valid_token(&"a".repeat(512)));
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a?b",
            "a#b",
            "a%2Fb",
            "a b",
            &"a".repeat(513),
        ] {
            assert!(!valid_token(bad), "{bad}");
        }
    }

    #[test]
    fn decodes_notifications() {
        let push = |n: Value| json!({"message": {"data": STANDARD.encode(n.to_string())}});
        let bought = push(json!({"packageName": APP_ID,
            "oneTimeProductNotification": {"notificationType": 1, "purchaseToken": "tok.1", "sku": "x"}}));
        assert_eq!(notification(&bought), Some(("tok.1".into(), false)));
        let voided = push(json!({"packageName": APP_ID,
            "voidedPurchaseNotification": {"purchaseToken": "tok.2", "productType": 2, "refundType": 2}}));
        assert_eq!(notification(&voided), Some(("tok.2".into(), true)));
        let malformed = [
            json!({}),
            json!({"message": {"data": "%%%"}}),
            json!({"message": {"data": STANDARD.encode("not json")}}),
            push(json!({"packageName": APP_ID, "testNotification": {"version": "1.0"}})),
            push(json!({"packageName": "com.example.other",
                "oneTimeProductNotification": {"notificationType": 1, "purchaseToken": "tok"}})),
            push(json!({"packageName": APP_ID,
                "oneTimeProductNotification": {"notificationType": 2, "purchaseToken": "tok"}})),
            push(json!({"packageName": APP_ID,
                "voidedPurchaseNotification": {"purchaseToken": "../x"}})),
        ];
        for body in malformed {
            assert_eq!(notification(&body), None, "{body}");
        }
    }
}
