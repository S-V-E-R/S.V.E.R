//! Developer platform part 1 (docs/DEVELOPER_PLATFORM.md §1): an app is registered, a person
//! approves it with PKCE, the app calls the API within its scope, refresh tokens rotate, a reused
//! refresh token revokes everything, and revoking in Settings ends access.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

/// A server-to-server call: no cookie, no Origin.
async fn raw(
    e: &Env,
    method: &str,
    path: &str,
    form: Option<&[(&str, &str)]>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
    for (k, v) in headers {
        request = request.header(*k, *v);
    }
    let body = match form {
        Some(pairs) => {
            request = request.header("content-type", "application/x-www-form-urlencoded");
            Body::from(
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(pairs)
                    .finish(),
            )
        }
        None => Body::empty(),
    };
    let response = sver::router(e.app.clone())
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

const VERIFIER: &str = "a-synthetic-pkce-verifier-of-sufficient-length-1234567890";

pub async fn exercise(e: &Env) {
    let (status, _) = e
        .request(
            "POST",
            "/api/me/apps",
            json!({"name":"Bad","redirect_uris":["http://example.test/cb"]}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "plain http only on 127.0.0.1"
    );
    let created = e.call("POST", "/api/me/apps", json!({"name":"Overlay Kit","redirect_uris":["https://example.test/cb"],"confidential":true})).await;
    let client = created["created"]["client_id"]
        .as_str()
        .unwrap()
        .to_string();
    let secret = created["created"]["client_secret"]
        .as_str()
        .unwrap()
        .to_string();
    let stored: String = sqlx::query_scalar("SELECT secret_hash FROM dev_apps")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_ne!(stored, secret, "only a digest is kept");

    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER.as_bytes()));
    let query = format!(
        "client_id={client}&redirect_uri=https%3A%2F%2Fexample.test%2Fcb&response_type=code&scope=user%3Aread&state=xyz&code_challenge={challenge}&code_challenge_method=S256"
    );
    let consent = e
        .call("GET", &format!("/api/oauth/authorize?{query}"), Value::Null)
        .await;
    assert_eq!(
        (&consent["app"]["name"], &consent["scopes"][0]["scope"]),
        (&json!("Overlay Kit"), &json!("user:read"))
    );
    let (status, _) = e
        .request(
            "GET",
            &format!(
                "/api/oauth/authorize?{}",
                query.replace("user%3Aread", "stream%3Akey")
            ),
            Value::Null,
            true,
            true,
            false,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "stream:key is never offered"
    );
    let approve = || async {
        let decided = e.call("POST", "/api/oauth/authorize", json!({"client_id":client,"redirect_uri":"https://example.test/cb","response_type":"code","scope":"user:read","state":"xyz","code_challenge":challenge,"code_challenge_method":"S256","approve":true})).await;
        let redirect = url::Url::parse(decided["redirect"].as_str().unwrap()).unwrap();
        let pairs: std::collections::HashMap<String, String> =
            redirect.query_pairs().into_owned().collect();
        assert_eq!(pairs["state"], "xyz");
        pairs["code"].clone()
    };
    let exchange = |verifier: &'static str, code: String| {
        let (client, secret) = (client.clone(), secret.clone());
        async move {
            raw(
                e,
                "POST",
                "/api/oauth/token",
                Some(&[
                    ("grant_type", "authorization_code"),
                    ("client_id", &client),
                    ("client_secret", &secret),
                    ("code", &code),
                    ("redirect_uri", "https://example.test/cb"),
                    ("code_verifier", verifier),
                ]),
                &[],
            )
            .await
        }
    };
    let (status, bad) = exchange(
        "the-wrong-verifier-the-wrong-verifier-the-wrong-verifier",
        approve().await,
    )
    .await;
    assert_eq!(
        (status, &bad["error"]),
        (StatusCode::BAD_REQUEST, &json!("invalid_grant")),
        "{bad}"
    );
    let (status, tokens) = exchange(VERIFIER, approve().await).await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    let access = tokens["access_token"].as_str().unwrap().to_string();
    let refresh = tokens["refresh_token"].as_str().unwrap().to_string();

    // The API: a client ID always, a token with the scope for private calls.
    let bearer = format!("Bearer {access}");
    let (status, me) = raw(
        e,
        "GET",
        "/api/v1/me",
        None,
        &[("sver-client-id", &client), ("authorization", &bearer)],
    )
    .await;
    assert_eq!(
        (status, &me["username"]),
        (StatusCode::OK, &json!("Streamer"))
    );
    assert_eq!(
        raw(e, "GET", "/api/v1/me", None, &[("authorization", &bearer)])
            .await
            .0,
        StatusCode::UNAUTHORIZED,
        "no client ID"
    );
    let (status, channel) = raw(
        e,
        "GET",
        "/api/v1/channels/Streamer",
        None,
        &[("sver-client-id", &client)],
    )
    .await;
    assert_eq!(
        (status, &channel["username"]),
        (StatusCode::OK, &json!("Streamer"))
    );

    // Refresh rotates; reusing a spent refresh token revokes the grant and its access token.
    let refreshed = raw(
        e,
        "POST",
        "/api/oauth/token",
        Some(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client),
            ("client_secret", &secret),
            ("refresh_token", &refresh),
        ]),
        &[],
    )
    .await;
    assert_eq!(refreshed.0, StatusCode::OK, "{}", refreshed.1);
    let new_access = format!("Bearer {}", refreshed.1["access_token"].as_str().unwrap());
    assert_eq!(
        raw(
            e,
            "GET",
            "/api/v1/me",
            None,
            &[("sver-client-id", &client), ("authorization", &new_access)]
        )
        .await
        .0,
        StatusCode::OK
    );
    let reused = raw(
        e,
        "POST",
        "/api/oauth/token",
        Some(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client),
            ("client_secret", &secret),
            ("refresh_token", &refresh),
        ]),
        &[],
    )
    .await;
    assert_eq!(reused.1["error"], "invalid_grant");
    assert_eq!(
        raw(
            e,
            "GET",
            "/api/v1/me",
            None,
            &[("sver-client-id", &client), ("authorization", &new_access)]
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED,
        "the whole grant was revoked"
    );

    // A new approval, then revoking it in Settings → Connections ends access at once.
    let (_, tokens) = exchange(VERIFIER, approve().await).await;
    let bearer = format!("Bearer {}", tokens["access_token"].as_str().unwrap());
    let listed = e.call("GET", "/api/me/connections", Value::Null).await;
    assert_eq!(listed["connections"][0]["app"], "Overlay Kit");
    let grant = listed["connections"][0]["id"].as_str().unwrap().to_string();
    e.call(
        "DELETE",
        &format!("/api/me/connections/{grant}"),
        Value::Null,
    )
    .await;
    assert_eq!(
        raw(
            e,
            "GET",
            "/api/v1/me",
            None,
            &[("sver-client-id", &client), ("authorization", &bearer)]
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    e.call("DELETE", &format!("/api/me/apps/{client}"), Value::Null)
        .await;
    let apps: i64 = sqlx::query_scalar("SELECT count(*) FROM dev_apps")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(apps, 0);
}
