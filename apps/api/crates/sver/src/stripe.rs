//! Stripe for Module 6 (docs/SUPPORT.md): a small form-encoded API client and webhook signature
//! checks. Card data never reaches S.V.E.R; Checkout and Connect onboarding are Stripe-hosted.
use crate::profiles::{Fail, Res};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2::Sha256;

#[derive(Clone, Default)]
pub struct Config {
    pub secret_key: String,
    pub webhook_secret: String,
    /// `https://api.stripe.com`; tests point this at a local fake.
    pub api_url: String,
}
impl Config {
    pub fn from_env(production: bool) -> std::result::Result<Self, String> {
        let env = |key: &str| std::env::var(key).unwrap_or_default();
        let secret_key = env("STRIPE_SECRET_KEY");
        // Development and staging never move real money.
        if !production && secret_key.starts_with("sk_live_") {
            return Err("STRIPE_SECRET_KEY must be a test key outside production".into());
        }
        let api_url = match env("STRIPE_API_URL") {
            url if url.is_empty() => "https://api.stripe.com".to_string(),
            url => url.trim_end_matches('/').to_string(),
        };
        Ok(Self {
            secret_key,
            webhook_secret: env("STRIPE_WEBHOOK_SECRET"),
            api_url,
        })
    }
    pub fn available(&self) -> bool {
        !self.secret_key.is_empty()
    }
}

/// POSTs a form to the Stripe API. `idempotency` makes a retried request create nothing twice.
pub async fn post(
    http: &reqwest::Client,
    config: &Config,
    path: &str,
    form: &[(&str, String)],
    idempotency: Option<&str>,
) -> Res<Value> {
    post_as(http, config, None, path, form, idempotency).await
}
/// `post` on behalf of a connected account (the `Stripe-Account` header), for example an instant
/// payout from a creator's Express balance.
pub async fn post_as(
    http: &reqwest::Client,
    config: &Config,
    account: Option<&str>,
    path: &str,
    form: &[(&str, String)],
    idempotency: Option<&str>,
) -> Res<Value> {
    match send(http, config, account, path, form, idempotency).await {
        Outcome::Done(body) => Ok(body),
        Outcome::Declined { fail, .. } | Outcome::Unknown(fail) => Err(fail),
    }
}

/// How a Stripe write ended, for callers that must not guess. `Declined`: Stripe answered with a
/// definite refusal and did nothing. `Unknown`: it may or may not have happened (no response, a
/// timeout, a rate limit, a Stripe-side error, a garbled body), so retry with the same idempotency
/// key, or look the object up, before treating it as failed.
pub enum Outcome {
    Done(Value),
    Declined { code: String, fail: Fail },
    Unknown(Fail),
}
/// POSTs a form and reports a definite refusal apart from an unknown result.
pub async fn send(
    http: &reqwest::Client,
    config: &Config,
    account: Option<&str>,
    path: &str,
    form: &[(&str, String)],
    idempotency: Option<&str>,
) -> Outcome {
    if !config.available() {
        return Outcome::Declined {
            code: String::new(),
            fail: Fail::unavailable("Payments aren't available yet."),
        };
    }
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form.iter().map(|(k, v)| (*k, v.as_str())))
        .finish();
    let mut request = http
        .post(format!("{}/v1/{path}", config.api_url))
        .bearer_auth(&config.secret_key)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body);
    if let Some(key) = idempotency {
        request = request.header("Idempotency-Key", key);
    }
    if let Some(account) = account {
        request = request.header("Stripe-Account", account);
    }
    classify(request.send().await).await
}
/// GETs a Stripe object.
pub async fn get(http: &reqwest::Client, config: &Config, path: &str) -> Res<Value> {
    if !config.available() {
        return Err(Fail::unavailable("Payments aren't available yet."));
    }
    let request = http
        .get(format!("{}/v1/{path}", config.api_url))
        .bearer_auth(&config.secret_key);
    match classify(request.send().await).await {
        Outcome::Done(body) => Ok(body),
        Outcome::Declined { fail, .. } | Outcome::Unknown(fail) => Err(fail),
    }
}
/// Issues a refund, idempotent by `key`. True once the money is on its way back, including when the
/// charge was already refunded some other way (Stripe's dashboard, staff tools); false to retry.
pub async fn refund(
    http: &reqwest::Client,
    config: &Config,
    form: &[(&str, String)],
    key: &str,
) -> bool {
    match send(http, config, None, "refunds", form, Some(key)).await {
        Outcome::Done(_) => true,
        Outcome::Declined { code, .. } => code == "charge_already_refunded",
        Outcome::Unknown(_) => false,
    }
}
async fn classify(response: reqwest::Result<reqwest::Response>) -> Outcome {
    let Ok(response) = response else {
        return Outcome::Unknown(Fail::unavailable("Stripe can't be reached. Try again."));
    };
    let status = response.status();
    let Ok(body) = response.json::<Value>().await else {
        return Outcome::Unknown(Fail::unavailable("Stripe sent an unexpected response."));
    };
    if status.is_success() {
        return Outcome::Done(body);
    }
    let code = body["error"]["code"].as_str().unwrap_or("").to_string();
    eprintln!(
        "stripe_event=api_error status={} type={} code={code}",
        status.as_u16(),
        body["error"]["type"].as_str().unwrap_or("")
    );
    let fail = Fail::unavailable(message(&code));
    // 409 is an idempotent request still in flight; 429 and 5xx may or may not have applied.
    if status.is_server_error() || matches!(status.as_u16(), 409 | 429) {
        Outcome::Unknown(fail)
    } else {
        Outcome::Declined { code, fail }
    }
}

/// What to tell the user for a Stripe error code (Stripe's own text is logged, not shown).
fn message(code: &str) -> &'static str {
    match code {
        "balance_insufficient" => {
            "S.V.E.R's Stripe balance doesn't have enough available funds for this payout yet. Try again later."
        }
        "instant_payouts_unsupported" | "instant_payouts_limit_exceeded" => {
            "Instant payout isn't available for this account right now."
        }
        _ => "Payments are having trouble. Try again.",
    }
}

/// Stripe's webhook signature: `t=<unix>,v1=<hex hmac>` over `<t>.<raw body>`, within 5 minutes.
pub fn verify(secret: &str, header: &str, body: &[u8], now: i64) -> bool {
    if secret.is_empty() {
        return false;
    }
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.split_once('=') {
            Some(("t", t)) => timestamp = t.parse::<i64>().ok(),
            Some(("v1", sig)) => signatures.push(sig),
            _ => {}
        }
    }
    let Some(t) = timestamp else {
        return false;
    };
    if (now - t).abs() > 300 {
        return false;
    }
    signatures.iter().any(|sig| {
        let Some(expected) = hex_decode(sig) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(format!("{t}.").as_bytes());
        mac.update(body);
        // verify_slice compares in constant time.
        mac.verify_slice(&expected).is_ok()
    })
}
pub(crate) fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}
/// A signature header for `body` at `t` (tests and the local fake).
pub fn sign(secret: &str, body: &[u8], t: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(format!("{t}.").as_bytes());
    mac.update(body);
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("t={t},v1={sig}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_signatures_are_checked() {
        let body = br#"{"id":"evt_1"}"#;
        let header = sign("whsec_test", body, 1_000);
        assert!(verify("whsec_test", &header, body, 1_100));
        assert!(!verify("whsec_other", &header, body, 1_100), "wrong secret");
        assert!(
            !verify("whsec_test", &header, br#"{"id":"evt_2"}"#, 1_100),
            "tampered body"
        );
        assert!(!verify("whsec_test", &header, body, 1_400), "too old");
        assert!(!verify("", &header, body, 1_100), "no secret configured");
        assert!(!verify("whsec_test", "t=1000,v1=zz", body, 1_100));
        // During secret rotation Stripe sends one v1 per active secret.
        let other = sign("whsec_new", body, 1_000);
        let both = format!("{header},{}", other.split(',').nth(1).unwrap());
        assert!(verify("whsec_new", &both, body, 1_000));
    }
}
