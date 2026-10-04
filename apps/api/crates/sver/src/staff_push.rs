//! Required staff alerts. Uses the existing HTTP client and Postgres queue; payloads carry no case details.
use crate::{
    App,
    profiles::{Fail, Res, new_id},
    safety, security as sec,
};
use axum::{Json, Router, extract::State, routing::get};
use axum_extra::extract::cookie::CookieJar;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;
use web_push_native::{
    Auth, WebPushBuilder, jwt_simple::algorithms::ES256KeyPair, p256::PublicKey,
};

#[derive(Clone, Default)]
pub struct Config {
    pub public_key: String,
    pub private_key: String,
    pub subject: String,
}
impl Config {
    pub fn from_env() -> Self {
        Self {
            public_key: std::env::var("VAPID_PUBLIC_KEY").unwrap_or_default(),
            private_key: std::env::var("VAPID_PRIVATE_KEY").unwrap_or_default(),
            subject: std::env::var("VAPID_SUBJECT")
                .unwrap_or_else(|_| "mailto:safety@sver.tv".into()),
        }
    }
}
#[derive(Deserialize, Serialize)]
struct Keys {
    p256dh: String,
    auth: String,
}
#[derive(Deserialize, Serialize)]
struct Subscription {
    endpoint: String,
    keys: Keys,
}
fn validate(input: &Subscription) -> Res<(PublicKey, Auth)> {
    let url =
        url::Url::parse(&input.endpoint).map_err(|_| Fail::bad("Invalid push subscription."))?;
    let host = url.host_str().unwrap_or("");
    // The API never fetches arbitrary subscriber URLs (including redirects or private networks).
    let allowed = host == "fcm.googleapis.com"
        || host == "updates.push.services.mozilla.com"
        || host == "web.push.apple.com"
        || host == "wns.windows.com"
        || host.ends_with(".notify.windows.com");
    if input.endpoint.len() > 2048
        || url.scheme() != "https"
        || url.port().is_some_and(|p| p != 443)
        || !url.username().is_empty()
        || url.password().is_some()
        || !allowed
    {
        return Err(Fail::bad("This browser's push service is not supported."));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(&input.keys.p256dh)
        .map_err(|_| Fail::bad("Invalid push key."))?;
    let public = PublicKey::from_sec1_bytes(&bytes).map_err(|_| Fail::bad("Invalid push key."))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(&input.keys.auth)
        .map_err(|_| Fail::bad("Invalid push secret."))?;
    if bytes.len() != 16 {
        return Err(Fail::bad("Invalid push secret."));
    }
    Ok((public, Auth::clone_from_slice(&bytes)))
}
async fn config(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    Ok(Json(json!({"public_key":app.config.staff_push.public_key})))
}
async fn subscribe(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Subscription>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    validate(&input)?;
    if app.config.staff_push.private_key.is_empty() {
        return Err(Fail::unavailable("Staff push is not configured."));
    }
    let id = sec::digest(&input.endpoint);
    let mut tx = app.db.begin().await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM staff_push_subscriptions WHERE user_id=$1 AND id<>$2",
    )
    .bind(&staff.id)
    .bind(&id)
    .fetch_one(&mut *tx)
    .await?;
    if count >= 10 {
        return Err(Fail::bad(
            "This account already has ten push subscriptions.",
        ));
    }
    sqlx::query("INSERT INTO staff_push_subscriptions(id,user_id,subscription) VALUES($1,$2,$3) ON CONFLICT(id) DO UPDATE SET user_id=EXCLUDED.user_id,subscription=EXCLUDED.subscription")
        .bind(&id).bind(&staff.id).bind(sec::seal(&app,"staff-push",&serde_json::to_string(&input).map_err(|_|Fail::internal())?)?).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO staff_push_jobs(id,subscription_id) VALUES($1,$2)")
        .bind(new_id())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub async fn enqueue(db: &mut PgConnection, user: &str) -> Res<Vec<String>> {
    Ok(sqlx::query_scalar("INSERT INTO staff_push_jobs(id,subscription_id) SELECT $1||id,id FROM staff_push_subscriptions WHERE user_id=$2 RETURNING id")
        .bind(new_id()).bind(user).fetch_all(db).await?)
}
pub async fn tick(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let expired: Vec<String> = sqlx::query_scalar("DELETE FROM staff_push_jobs j WHERE j.delivered_at IS NULL AND (j.created_at<=now()-interval '2 days' OR NOT EXISTS(SELECT 1 FROM staff_push_subscriptions s JOIN staff_roles r ON r.user_id=s.user_id AND r.role='admin' JOIN users u ON u.id=s.user_id AND u.deleted_at IS NULL WHERE s.id=j.subscription_id)) RETURNING j.id")
        .fetch_all(&mut *tx).await?;
    for id in expired {
        crate::take_down::notice_result(&mut tx, &id, "expired", false).await?;
    }
    tx.commit().await?;
    let config = &app.config.staff_push;
    if config.private_key.is_empty() {
        return Ok(());
    }
    let key = URL_SAFE_NO_PAD
        .decode(&config.private_key)
        .map_err(|_| Fail::internal())?;
    let key = ES256KeyPair::from_bytes(&key).map_err(|_| Fail::internal())?;
    for _ in 0..20 {
        let mut tx = app.db.begin().await?;
        let row:Option<(String,String,String,i32)> = sqlx::query_as("SELECT j.id,s.id,s.subscription,j.attempts FROM staff_push_jobs j JOIN staff_push_subscriptions s ON s.id=j.subscription_id JOIN staff_roles r ON r.user_id=s.user_id AND r.role='admin' WHERE j.delivered_at IS NULL AND j.available_at<=now() AND j.created_at>now()-interval '2 days' ORDER BY j.available_at FOR UPDATE OF j SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some((id, subscription, encrypted, attempts)) = row else {
            break;
        };
        let input: Subscription =
            serde_json::from_str(&sec::unseal(app, "staff-push", &encrypted)?)
                .map_err(|_| Fail::internal())?;
        let (public, auth) = validate(&input)?;
        let request=WebPushBuilder::new(input.endpoint.parse().map_err(|_|Fail::internal())?,public,auth).with_vapid(&key,&config.subject)
            .build(json!({"id":id,"title":"S.V.E.R staff alert","body":"Check urgent removal requests in the staff console.","url":"/admin/take-it-down"}).to_string()).map_err(|_|Fail::internal())?;
        let (parts, body) = request.into_parts();
        let response = app
            .http
            .post(parts.uri.to_string())
            .headers(parts.headers)
            .header("Topic", &sec::digest(&id)[..32])
            .body(body)
            .send()
            .await;
        match response.map(|r| r.status()) {
            Ok(status) if status.is_success() => {
                crate::take_down::notice_result(&mut tx, &id, "accepted", true).await?;
                sqlx::query("UPDATE staff_push_jobs SET delivered_at=now() WHERE id=$1")
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
            }
            Ok(axum::http::StatusCode::NOT_FOUND | axum::http::StatusCode::GONE) => {
                let failed: Vec<String> = sqlx::query_scalar("SELECT id FROM staff_push_jobs WHERE subscription_id=$1 AND delivered_at IS NULL")
                    .bind(&subscription).fetch_all(&mut *tx).await?;
                for job in failed {
                    crate::take_down::notice_result(&mut tx, &job, "failed", job == id).await?;
                }
                sqlx::query("DELETE FROM staff_push_subscriptions WHERE id=$1")
                    .bind(subscription)
                    .execute(&mut *tx)
                    .await?;
            }
            _ => {
                crate::take_down::notice_result(&mut tx, &id, "retrying", true).await?;
                sqlx::query("UPDATE staff_push_jobs SET attempts=attempts+1,available_at=now()+make_interval(secs=>$2) WHERE id=$1").bind(id).bind((30_i32 * 2_i32.pow(attempts.min(6) as u32)).min(1800) as f64).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
    }
    sqlx::query("DELETE FROM staff_push_jobs WHERE created_at<now()-interval '30 days'")
        .execute(&app.db)
        .await?;
    Ok(())
}
pub fn routes() -> Router<App> {
    Router::new().route("/api/admin/push", get(config).post(subscribe))
}

#[cfg(test)]
mod tests {
    use super::*;
    use web_push_native::p256::{SecretKey, elliptic_curve::sec1::ToEncodedPoint};

    #[test]
    fn push_rejects_arbitrary_endpoints_and_encrypts_alerts() {
        // Deterministic keys belong only to this synthetic test; no network request is sent.
        let secret = SecretKey::from_slice(&[7; 32]).unwrap();
        let mut subscription = Subscription {
            endpoint: "https://fcm.googleapis.com/synthetic-subscription".into(),
            keys: Keys {
                p256dh: URL_SAFE_NO_PAD.encode(secret.public_key().to_encoded_point(false)),
                auth: URL_SAFE_NO_PAD.encode([9; 16]),
            },
        };
        let (public, auth) = validate(&subscription).unwrap();
        let signing = ES256KeyPair::from_bytes(&[8; 32]).unwrap();
        let request = WebPushBuilder::new(subscription.endpoint.parse().unwrap(), public, auth)
            .with_vapid(&signing, "mailto:test@example.invalid")
            .build("Synthetic private notification")
            .unwrap();
        assert_eq!(request.headers()["content-encoding"], "aes128gcm");
        assert!(request.headers().contains_key("authorization"));
        assert!(!request.body().windows(9).any(|w| w == b"Synthetic"));
        for endpoint in [
            "http://fcm.googleapis.com/x",
            "https://127.0.0.1/x",
            "https://fcm.googleapis.com.evil.invalid/x",
            "https://fcm.googleapis.com:8443/x",
            "https://user@fcm.googleapis.com/x",
        ] {
            subscription.endpoint = endpoint.into();
            assert!(validate(&subscription).is_err(), "{endpoint}");
        }
        subscription.endpoint = "https://updates.push.services.mozilla.com/wpush/test".into();
        assert!(validate(&subscription).is_ok());
        subscription.keys.auth = URL_SAFE_NO_PAD.encode([9; 15]);
        assert!(validate(&subscription).is_err());
        subscription.keys.auth = URL_SAFE_NO_PAD.encode([9; 16]);
        subscription.keys.p256dh = URL_SAFE_NO_PAD.encode([0; 65]);
        assert!(validate(&subscription).is_err());
    }
}
