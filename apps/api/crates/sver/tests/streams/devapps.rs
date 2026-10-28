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
    let stored: String = sqlx::query_scalar("SELECT secret_hash FROM dev_apps WHERE id=$1")
        .bind(&client)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_ne!(stored, secret, "only a digest is kept");
    // An icon goes through the image pipeline and shows wherever the app is named.
    let image = image::RgbImage::from_fn(128, 128, |x, y| image::Rgb([x as u8, y as u8, 200]));
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let (status, apps) = super::overlays::upload(
        e,
        &e.cookie,
        &format!("/api/me/apps/{client}/icon"),
        png.into_inner(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{apps}");
    assert!(apps["apps"][0]["icon"].is_object(), "{apps}");

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
    assert!(consent["app"]["icon"].is_object(), "{consent}");
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
    // What an app needs to change the title (PATCH /api/me/stream) without a session.
    assert!(
        channel["revision"].is_i64() && channel.get("category_id").is_some(),
        "{channel}"
    );
    // Each app call says how much of its per-minute budget is left.
    let (limit, remaining, reset) = {
        let response = sver::router(e.app.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/channels/Streamer")
                    .header("sver-client-id", &client)
                    .extension(ConnectInfo(
                        "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let header = |name: &str| {
            response.headers()[name]
                .to_str()
                .unwrap()
                .parse::<i64>()
                .unwrap()
        };
        (
            header("ratelimit-limit"),
            header("ratelimit-remaining"),
            header("ratelimit-reset"),
        )
    };
    assert_eq!(limit, 600);
    assert!((0..600).contains(&remaining));
    assert!((0..=60).contains(&reset));

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
    let apps: i64 = sqlx::query_scalar("SELECT count(*) FROM dev_apps WHERE owner_id IS NOT NULL")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(apps, 0);

    device(e).await;
    desktop(e).await;
}

/// Device sign-in (sver.tv/go): pending, slow down, approval, tokens once.
async fn device(e: &Env) {
    let created = e
        .call(
            "POST",
            "/api/me/apps",
            json!({"name":"TV App","redirect_uris":["https://example.test/tv"]}),
        )
        .await;
    let client = created["created"]["client_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = raw(
        e,
        "POST",
        "/api/oauth/device",
        Some(&[("client_id", &client), ("scope", "user:read")]),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let device = started["device_code"].as_str().unwrap().to_string();
    let code = started["user_code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 7, "ABC-DEF");
    let poll = || {
        let (client, device) = (client.clone(), device.clone());
        async move {
            raw(
                e,
                "POST",
                "/api/oauth/token",
                Some(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", &client),
                    ("device_code", &device),
                ]),
                &[],
            )
            .await
        }
    };
    assert_eq!(poll().await.1["error"], "authorization_pending");
    assert_eq!(poll().await.1["error"], "slow_down", "polled again at once");
    let (status, _) = e
        .request(
            "GET",
            "/api/oauth/device/ZZZ-ZZZ",
            Value::Null,
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "wrong code");
    let shown = e
        .call(
            "GET",
            &format!("/api/oauth/device/{}", code.to_lowercase()),
            Value::Null,
        )
        .await;
    assert_eq!(shown["app"]["name"], "TV App");
    e.call(
        "POST",
        &format!("/api/oauth/device/{code}"),
        json!({"approve": true}),
    )
    .await;
    e.sql("UPDATE oauth_devices SET last_poll_at=now()-interval '10 seconds'")
        .await;
    let (status, tokens) = poll().await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    let bearer = format!("Bearer {}", tokens["access_token"].as_str().unwrap());
    assert_eq!(
        raw(
            e,
            "GET",
            "/api/v1/me",
            None,
            &[("sver-client-id", &client), ("authorization", &bearer)]
        )
        .await
        .1["username"],
        "Streamer"
    );
    assert_eq!(
        poll().await.1["error"],
        "invalid_grant",
        "a device code works once"
    );
    e.call("DELETE", &format!("/api/me/apps/{client}"), Value::Null)
        .await;
}

/// S.V.E.R Desktop: a built-in app with no owner. Only it may ask for `stream:key` (through
/// sver.tv/go), no one can delete it in Settings, and the key still needs a code on every call.
async fn desktop(e: &Env) {
    let created = e
        .call(
            "POST",
            "/api/me/apps",
            json!({"name":"Not Desktop","redirect_uris":["http://127.0.0.1/cb"]}),
        )
        .await;
    let other = created["created"]["client_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, refused) = raw(
        e,
        "POST",
        "/api/oauth/device",
        Some(&[
            ("client_id", other.as_str()),
            ("scope", "user:read stream:key"),
        ]),
        &[],
    )
    .await;
    assert_eq!(
        (status, refused["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_scope")),
        "only S.V.E.R Desktop"
    );
    e.call("DELETE", &format!("/api/me/apps/{other}"), Value::Null)
        .await;
    let client = sver::devapps::DESKTOP_APP.to_string();
    // Deleting someone else's (or no one's) app does nothing.
    e.call("DELETE", &format!("/api/me/apps/{client}"), Value::Null)
        .await;
    let ask = [
        ("client_id", client.as_str()),
        ("scope", "user:read stream:key viewer:act clips:upload"),
    ];
    let (status, started) = raw(e, "POST", "/api/oauth/device", Some(&ask), &[]).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let code = started["user_code"].as_str().unwrap().to_string();
    let shown = e
        .call("GET", &format!("/api/oauth/device/{code}"), Value::Null)
        .await;
    assert_eq!(shown["app"]["owner"], Value::Null, "Made by S.V.E.R");
    assert!(
        shown["scopes"][1]["text"]
            .as_str()
            .unwrap()
            .contains("stream key"),
        "{shown}"
    );
    e.call(
        "POST",
        &format!("/api/oauth/device/{code}"),
        json!({"approve": true}),
    )
    .await;
    let (status, tokens) = raw(
        e,
        "POST",
        "/api/oauth/token",
        Some(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", &client),
            ("device_code", started["device_code"].as_str().unwrap()),
        ]),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    assert_eq!(
        tokens["scope"],
        "user:read stream:key viewer:act clips:upload"
    );
    let bearer = format!("Bearer {}", tokens["access_token"].as_str().unwrap());
    let post = |path: String, body: Value| {
        let (client, bearer) = (client.clone(), bearer.clone());
        async move {
            let response = sver::router(e.app.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("content-type", "application/json")
                        .header("sver-client-id", &client)
                        .header("authorization", &bearer)
                        .extension(ConnectInfo(
                            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                        ))
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    let (status, _) = post(
        "/api/v1/stream/key".into(),
        json!({"code": "RECOVERY-WRONG"}),
    )
    .await;
    assert!(status.is_client_error(), "a wrong code gets no key");
    let (status, first) = post("/api/v1/stream/key".into(), json!({"code": e.code().await})).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(first["key"].as_str().unwrap().contains("?key="));
    let (_, again) = post("/api/v1/stream/key".into(), json!({"code": e.code().await})).await;
    assert_eq!(again["key"], first["key"], "revealed, not rotated");
    // The viewer actions take the desktop token (viewer:act); the owner's own beat doesn't count.
    let (status, beat) = post(
        "/api/channels/Streamer/live/beat".into(),
        json!({"broadcast_id": "none", "browser_id": "desktop-0123456789ab", "visible": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{beat}");
    assert_eq!(beat["recorded"], false);
    // Clip uploads take the desktop token (clips:upload) with no Origin; here the title is wrong,
    // so a 400 shows the token got past sign-in (tests/streams/clip_uploads.rs covers the rest).
    let (status, _) = post(
        "/api/me/clips".into(),
        json!({"title": "", "request_id": uuid::Uuid::new_v4().to_string()}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Studio's own key tests start from no key.
    e.sql("DELETE FROM stream_credentials WHERE owner_id='stream-owner'")
        .await;
    let listed = e.call("GET", "/api/me/connections", Value::Null).await;
    assert!(
        listed.to_string().contains("S.V.E.R Desktop"),
        "an ownerless app still lists: {listed}"
    );
}
