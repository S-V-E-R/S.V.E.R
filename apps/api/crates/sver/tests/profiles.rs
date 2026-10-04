//! Module 2 acceptance checks against real Postgres, the filesystem media adapter and a
//! controlled oEmbed fake (docs/PROFILES.md, "Acceptance"). Synthetic data only.
//! AssertSqlSafe is limited to generated schema names and synthetic fixture IDs, media hashes
//! and integer counters below. Test helpers still require SqlSafeStr at each call site.
use axum::{
    Router,
    body::Body,
    extract::{ConnectInfo, Query},
    http::{HeaderMap, Request, StatusCode},
    response::IntoResponse,
    routing::get,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::Digest;
use sqlx::postgres::PgPoolOptions;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use sver::{App, Config, media, profile_import, security as sec, studio};
use tower::ServiceExt;

#[derive(Clone)]
struct Client {
    app: Router,
    origin: String,
    cookie: Option<String>,
    ip: SocketAddr,
}
impl Client {
    async fn send(&self, req: Request<Body>) -> (StatusCode, Value, HeaderMap) {
        let response = self.app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            headers,
        )
    }
    fn builder(&self, method: &str, path: &str) -> axum::http::request::Builder {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header("origin", &self.origin)
            .header("user-agent", "SVER profile check")
            .extension(ConnectInfo(self.ip));
        if let Some(c) = &self.cookie {
            b = b.header("cookie", format!("sver_dev={c}"));
        }
        b
    }
    async fn call(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let req = self
            .builder(method, path)
            .header("content-type", "application/json")
            .body(if body.is_null() {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap();
        let (s, v, _) = self.send(req).await;
        (s, v)
    }
    async fn ok(&self, method: &str, path: &str, body: Value) -> Value {
        let (s, v) = self.call(method, path, body).await;
        assert_eq!(s, StatusCode::OK, "{method} {path} -> {v}");
        v
    }
    async fn status(&self, method: &str, path: &str, body: Value) -> StatusCode {
        self.call(method, path, body).await.0
    }
    async fn upload(
        &self,
        path: &str,
        fields: &[(&str, &str)],
        file: Option<&[u8]>,
    ) -> (StatusCode, Value) {
        let boundary = "sverprofileboundary";
        let mut body = Vec::new();
        for (k, v) in fields {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"
                )
                .as_bytes(),
            );
        }
        if let Some(bytes) = file {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"upload.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let req = self
            .builder("POST", path)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let (s, v, _) = self.send(req).await;
        (s, v)
    }
}

struct Env {
    app: App,
    anon: Client,
    next_ip: std::sync::atomic::AtomicU8,
}
impl Env {
    /// Creates a verified (or unverified) account with a fresh session, directly in the test schema.
    async fn user(&self, name: &str, verified: bool) -> (String, Client) {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id,email,username,email_verified,created_at,date_of_birth) VALUES($1,$2,$3,$4,now()-interval '30 days','1995-01-01')")
            .bind(&id)
            .bind(format!("{}@example.invalid", name.to_lowercase()))
            .bind(name)
            .bind(verified)
            .execute(&self.app.db)
            .await
            .unwrap();
        let client = self.session(&id, false).await;
        (id, client)
    }
    async fn session(&self, user_id: &str, mfa: bool) -> Client {
        let token = sec::token();
        sqlx::query("INSERT INTO sessions(id,token_hash,user_id,auth_version,user_agent,mfa_verified) SELECT $1,$2,id,auth_version,'synthetic',$3 FROM users WHERE id=$4")
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(sec::digest(&token))
            .bind(mfa)
            .bind(user_id)
            .execute(&self.app.db)
            .await
            .unwrap();
        let octet = self
            .next_ip
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Client {
            cookie: Some(token),
            ip: format!("198.51.100.{octet}:1000").parse().unwrap(),
            ..self.anon.clone()
        }
    }
    async fn id_of(&self, name: &str) -> String {
        sqlx::query_scalar("SELECT id FROM users WHERE username=$1")
            .bind(name)
            .fetch_one(&self.app.db)
            .await
            .unwrap()
    }
    async fn sql(&self, query: impl sqlx::SqlSafeStr) {
        sqlx::query(query).execute(&self.app.db).await.unwrap();
    }
    async fn count(&self, query: impl sqlx::SqlSafeStr) -> i64 {
        sqlx::query_scalar(query)
            .fetch_one(&self.app.db)
            .await
            .unwrap()
    }
    async fn reset_limits(&self) {
        self.sql("DELETE FROM rate_limits").await;
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 255) as u8, (y % 255) as u8, 90])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

async fn oembed(
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let url = q.get("url").cloned().unwrap_or_default();
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    if url.contains("notfound___") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if url.contains("servererror") {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if url.contains("redirect___") {
        return (
            StatusCode::FOUND,
            [("location", format!("http://{host}/oembed?url=ok"))],
        )
            .into_response();
    }
    if url.contains("bigbody____") {
        return format!("{{\"title\":\"{}\"}}", "x".repeat(70 * 1024)).into_response();
    }
    if url.contains("slowreply__") {
        tokio::time::sleep(std::time::Duration::from_secs(7)).await;
    }
    axum::Json(json!({"title": "Synthetic Track", "author_name": "Synthetic Artist", "thumbnail_url": format!("http://{host}/thumb.png"), "html": "<iframe src=\"https://w.soundcloud.com/player/?url=https%3A%2F%2Fapi.soundcloud.com%2Ftracks%2F123456&amp;auto_play=false\"></iframe>"})).into_response()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn profiles_acceptance() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("Use scripts/dev.ps1 test with an isolated local database");
    let parsed = url::Url::parse(&database_url).unwrap();
    assert!(
        matches!(parsed.host_str(), Some("localhost" | "127.0.0.1"))
            && parsed.path() == "/sver_rebuild",
        "Tests require the isolated local sver_rebuild database"
    );
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .unwrap();
    let schema = format!("profiles_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(16)
        .after_connect(move |connection, _| {
            let statement = search_path.clone();
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(statement))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&database_url)
        .await
        .unwrap();
    sqlx::migrate!("../../../../migrations")
        .run(&db)
        .await
        .unwrap();
    let fake = Router::new()
        .route("/oembed", get(oembed))
        .route("/thumb.png", get(|| async { png(480, 360) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fake_task = tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });
    // Media goes to a generated directory outside the workspace and is removed afterwards.
    let media_dir = std::env::temp_dir().join(format!(
        "sver-profile-media-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&media_dir).unwrap();
    let mut config = Config::from_env().unwrap();
    config.turnstile_secret = "test-only-secret".into();
    config.resend_key.clear();
    config.media = media::MediaConfig {
        storage: media::Storage::Filesystem(media_dir.clone()),
        public_base: format!("{}/api/media", config.origin),
    };
    config.youtube_oembed_url = format!("{base}/oembed");
    config.soundcloud_oembed_url = format!("{base}/oembed");
    config.thumbnail_hosts = vec!["127.0.0.1".into()];
    let app = App::new(db.clone(), config).await.unwrap();
    let router = sver::router(app.clone());
    let anon = Client {
        app: router,
        origin: app.config.origin.clone(),
        cookie: None,
        ip: "198.51.100.1:1000".parse().unwrap(),
    };
    let env = Arc::new(Env {
        app: app.clone(),
        anon,
        next_ip: std::sync::atomic::AtomicU8::new(2),
    });
    let result = tokio::spawn({
        let env = env.clone();
        let media_dir = media_dir.clone();
        async move {
            identity_and_channel(&env, &media_dir).await;
            bucket_redirects(&env).await;
            follows_council_and_blocks(&env).await;
            song(&env).await;
            wall(&env).await;
            studio_sections(&env, &media_dir).await;
            parity_additions(&env, &media_dir).await;
            parts_picker(&env).await;
            renames(&env).await;
            safety(&env).await;
            erasure(&env).await;
            legacy_import(&env, &media_dir).await;
            legacy_import_cli().await;
        }
    })
    .await;
    db.close().await;
    // Identifier consists solely of our fixed prefix and a generated UUID; never user input.
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&media_dir);
    fake_task.abort();
    assert!(
        result.is_ok(),
        "Profiles acceptance check failed; isolated test schema and media were removed"
    );
}

/// With bucket storage, URLs issued under the interim filesystem store redirect to the bucket.
async fn bucket_redirects(env: &Env) {
    let mut config = (*env.app.config).clone();
    config.media = media::MediaConfig {
        storage: media::Storage::S3(media::S3 {
            endpoint: "https://bucket.invalid".into(),
            bucket: "sver".into(),
            region: "auto".into(),
            access_key: "unused".into(),
            secret_key: "unused".into(),
            prefix: "v2/".into(),
        }),
        public_base: "https://media.example/v2".into(),
    };
    let app = App::new(env.app.db.clone(), config).await.unwrap();
    let client = Client {
        app: sver::router(app),
        ..env.anon.clone()
    };
    let (status, _, headers) = client
        .send(
            client
                .builder("GET", "/api/media/avatars/0123abcd/64.webp")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        headers["location"],
        "https://media.example/v2/avatars/0123abcd/64.webp"
    );
    assert_eq!(headers["cache-control"], "no-store");
    for bad in [
        "/api/media/avatars/..%2F..%2Fx.webp",
        "/api/media/avatars/a/64.png",
    ] {
        let (status, _, _) = client
            .send(client.builder("GET", bad).body(Body::empty()).unwrap())
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{bad}");
    }
}

async fn identity_and_channel(env: &Env, media_dir: &std::path::Path) {
    let (alice_id, alice) = env.user("Alice_Plays", true).await;
    let (_, bob) = env.user("BobStreams", true).await;
    // Channel resolution: canonical casing, identical 404s for unknown/internal/deleted/restricted.
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/alice_plays/resolve", Value::Null)
            .await["username"],
        "Alice_Plays"
    );
    let channel = env
        .anon
        .ok("GET", "/api/channels/ALICE_PLAYS", Value::Null)
        .await;
    assert_eq!(channel["channel"]["display_name"], "Alice_Plays");
    assert_eq!(channel["viewer"]["signed_in"], false);
    assert!(channel["channel"].get("email").is_none());
    // The Schedule tab always exists: an empty schedule is a 200 with no items, not a 404.
    assert_eq!(channel["tabs"]["schedule"], true);
    let schedule = env
        .anon
        .ok("GET", "/api/channels/Alice_Plays/schedule", Value::Null)
        .await;
    assert_eq!(schedule["items"].as_array().map(Vec::len), Some(0));
    assert_eq!(schedule["owner"]["username"], "Alice_Plays");
    let missing = env
        .anon
        .call("GET", "/api/channels/nobody_here", Value::Null)
        .await;
    assert_eq!(missing.0, StatusCode::NOT_FOUND);
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/nobody_here/schedule", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    env.user("support", true).await;
    let (deleted_id, _) = env.user("GoneUser", true).await;
    sqlx::query("UPDATE users SET deleted_at=now() WHERE id=$1")
        .bind(&deleted_id)
        .execute(&env.app.db)
        .await
        .unwrap();
    let (restricted_id, _) = env.user("TimeOut", true).await;
    sqlx::query("INSERT INTO profiles(user_id,display_name,restricted_until) VALUES($1,'TimeOut',now()+interval '1 hour')").bind(&restricted_id).execute(&env.app.db).await.unwrap();
    for name in ["support", "GoneUser", "TimeOut", "admin", "settings"] {
        assert_eq!(
            env.anon
                .call("GET", &format!("/api/channels/{name}"), Value::Null)
                .await,
            missing,
            "{name} must share the channel 404"
        );
    }
    assert_eq!(
        alice
            .ok("GET", "/api/channels/Alice_Plays", Value::Null)
            .await["viewer"]["is_owner"],
        true
    );
    assert_eq!(
        bob.ok("GET", "/api/channels/Alice_Plays", Value::Null)
            .await["viewer"]["is_owner"],
        false
    );

    // Display name, bio, mood and status boundaries.
    alice
        .ok(
            "PATCH",
            "/api/me/profile",
            json!({"display_name": "N".repeat(32)}),
        )
        .await;
    for bad in [
        "N".repeat(33),
        "Bad\u{202e}Name".into(),
        "Zero\u{200b}Width".into(),
        "SVER Staff".into(),
        "Official Admin".into(),
    ] {
        let (s, v) = alice
            .call("PATCH", "/api/me/profile", json!({"display_name": bad}))
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["field"], "display_name");
    }
    let filtered = sver::reserved::ABUSE[7];
    let (s, v) = alice
        .call(
            "PATCH",
            "/api/me/profile",
            json!({"bio": format!("hello {filtered}")}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(
        !v.to_string().contains(filtered),
        "The filter must not echo the word"
    );
    alice
        .ok(
            "PATCH",
            "/api/me/profile",
            json!({"bio": "b".repeat(300), "status_text": "s".repeat(80), "mood_emoji": "🎮"}),
        )
        .await;
    assert_eq!(
        alice
            .status("PATCH", "/api/me/profile", json!({"bio": "b".repeat(301)}))
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice
            .status(
                "PATCH",
                "/api/me/profile",
                json!({"status_text": "s".repeat(81)})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice
            .status("PATCH", "/api/me/profile", json!({"mood_emoji": "ab"}))
            .await,
        StatusCode::BAD_REQUEST
    );
    alice
        .ok("PATCH", "/api/me/profile", json!({"display_name": "Alice"}))
        .await;
    let revision = alice.ok("GET", "/api/me/profile", Value::Null).await["revisions"]["profile"]
        .as_i64()
        .unwrap();
    alice
        .ok(
            "PATCH",
            "/api/me/profile",
            json!({"bio": "Fresh", "revision": revision}),
        )
        .await;
    assert_eq!(
        alice
            .status(
                "PATCH",
                "/api/me/profile",
                json!({"bio": "Stale", "revision": revision})
            )
            .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        env.anon
            .status("PATCH", "/api/me/profile", json!({"bio": "x"}))
            .await,
        StatusCode::UNAUTHORIZED
    );

    // Links: scheme and host rules, http upgrade, the 5 limit.
    for url in [
        "javascript:alert(1)",
        "https://127.0.0.1/x",
        "https://sver.tv/x",
        "https://evil.example/twitch",
        "ftp://twitch.tv/me",
        "http://twitch.tv/alice",
    ] {
        assert_eq!(
            alice
                .status(
                    "PUT",
                    "/api/me/links",
                    json!({"links": [{"platform": "twitch", "url": url}]})
                )
                .await,
            StatusCode::BAD_REQUEST,
            "{url}"
        );
    }
    alice.ok("PUT", "/api/me/links", json!({"links": [{"platform": "twitch", "url": "https://twitch.tv/alice"}, {"platform": "website", "url": "https://alice.example"}]})).await;
    let links = env
        .anon
        .ok("GET", "/api/channels/Alice_Plays", Value::Null)
        .await["channel"]["links"]
        .clone();
    assert_eq!(links[0]["url"], "https://twitch.tv/alice");
    let six: Vec<Value> = ["twitch", "kick", "tiktok", "instagram", "bluesky", "patreon"].iter().map(|p| json!({"platform": p, "url": format!("https://{}/alice", if *p == "bluesky" { "bsky.app".to_string() } else { format!("{p}.com") })})).collect();
    assert_eq!(
        alice
            .status("PUT", "/api/me/links", json!({"links": six}))
            .await,
        StatusCode::BAD_REQUEST
    );

    // Images: magic bytes, SVG, size limits, three avatar sizes, metadata-free output, removal.
    assert_eq!(
        alice
            .upload("/api/me/avatar", &[], Some(b"just text renamed to png"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice
            .upload(
                "/api/me/avatar",
                &[],
                Some(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>")
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice
            .upload("/api/me/avatar", &[], Some(&png(100, 100)))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "Small avatars are refused"
    );
    let mut huge = png(400, 400);
    huge.resize(6 * 1024 * 1024, 0);
    assert_eq!(
        alice.upload("/api/me/avatar", &[], Some(&huge)).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut oversized = vec![0u8; 12 * 1024 * 1024];
    oversized[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    assert_eq!(
        alice
            .upload("/api/me/banner", &[], Some(&oversized))
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        alice
            .upload(
                "/api/me/avatar",
                &[("crop", "{\"x\":0,\"y\":0,\"width\":300,\"height\":200}")],
                Some(&png(400, 400))
            )
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "Crops must match the aspect ratio"
    );
    let (s, v) = alice
        .upload("/api/me/avatar", &[], Some(&png(500, 400)))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let avatar_key: String = sqlx::query_scalar("SELECT avatar_key FROM profiles WHERE user_id=$1")
        .bind(&alice_id)
        .fetch_one(&env.app.db)
        .await
        .unwrap();
    for size in [64, 160, 400] {
        let bytes = std::fs::read(media_dir.join(format!("{avatar_key}/{size}.webp"))).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert!(!bytes.windows(4).any(|w| w == b"EXIF" || w == b"Exif"));
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (size, size));
    }
    // Only successful media responses are cacheable (keys are content-addressed); every other
    // API response, including a missing media key, stays no-store.
    let header = |path: String| async move {
        let req = env.anon.builder("GET", &path).body(Body::empty()).unwrap();
        let (status, _, headers) = env.anon.send(req).await;
        let get = |k: &str| {
            headers
                .get(k)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        };
        (
            status,
            get("cache-control"),
            get("content-type"),
            get("x-content-type-options"),
        )
    };
    let (status, cache, kind, sniff) = header(format!("/api/media/{avatar_key}/64.webp")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache, "public, max-age=31536000, immutable");
    assert_eq!((kind.as_str(), sniff.as_str()), ("image/webp", "nosniff"));
    for path in [
        "/api/media/avatars/00000000000000000000000000000000/64.webp".to_string(),
        "/api/media/avatars/../secret.webp".to_string(),
        "/api/channels/Alice_Plays".to_string(),
        "/api/health".to_string(),
    ] {
        let (_, cache, _, _) = header(path.clone()).await;
        assert_eq!(cache, "no-store", "{path}");
    }
    let (s, v) = alice
        .upload("/api/me/banner", &[], Some(&png(1600, 600)))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let banner = env
        .anon
        .ok("GET", "/api/channels/Alice_Plays", Value::Null)
        .await["channel"]["banner"]
        .clone();
    assert!(
        banner.get("750").is_some() && banner.get("1500").is_some() && banner.get("3000").is_none(),
        "Banners never upscale: {banner}"
    );
    let (s, _) = alice
        .upload("/api/me/avatar", &[], Some(&png(300, 300)))
        .await;
    assert_eq!(s, StatusCode::OK);
    let queued = env.count(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM media_objects WHERE key LIKE '{avatar_key}/%' AND delete_after IS NOT NULL"))).await;
    assert_eq!(
        queued, 3,
        "Replaced avatar variants are queued for deletion"
    );
    alice.ok("DELETE", "/api/me/avatar", Value::Null).await;
    assert!(
        env.anon
            .ok("GET", "/api/channels/Alice_Plays", Value::Null)
            .await["channel"]["avatar"]
            .is_null()
    );
    env.reset_limits().await;
    println!(
        "Passed: channel resolution and 404 parity, identity field rules, links and the image pipeline."
    );
}

async fn follows_council_and_blocks(env: &Env) {
    let (_, carol) = env.user("CarolCasts", true).await;
    let (dave_id, dave) = env.user("DaveDoes", true).await;
    let alice = env.session(&env.id_of("Alice_Plays").await, false).await;
    assert_eq!(
        carol
            .ok("PUT", "/api/follows/alice_plays", Value::Null)
            .await["follower_count"],
        1
    );
    assert_eq!(
        carol
            .ok("PUT", "/api/follows/Alice_Plays", Value::Null)
            .await["follower_count"],
        1,
        "Follow is idempotent"
    );
    assert_eq!(
        carol
            .status("PUT", "/api/follows/CarolCasts", Value::Null)
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        carol
            .status("PUT", "/api/follows/support", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        env.anon
            .status("GET", "/api/me/following", Value::Null)
            .await,
        StatusCode::UNAUTHORIZED
    );
    // Concurrent follows keep the cached counters exact.
    let mut handles = Vec::new();
    for i in 0..8 {
        let (_, fan) = env.user(&format!("Fan_{i}x"), true).await;
        handles.push(tokio::spawn(async move {
            fan.status("PUT", "/api/follows/DaveDoes", Value::Null)
                .await
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }
    let cached: i32 = sqlx::query_scalar("SELECT follower_count FROM profiles WHERE user_id=$1")
        .bind(&dave_id)
        .fetch_one(&env.app.db)
        .await
        .unwrap();
    assert_eq!(cached, 8);
    let page = env
        .anon
        .ok("GET", "/api/channels/DaveDoes/followers", Value::Null)
        .await;
    assert_eq!(page["items"].as_array().unwrap().len(), 8);
    assert_eq!(
        carol.ok("GET", "/api/me/following", Value::Null).await["items"][0]["user"]["username"],
        "Alice_Plays"
    );
    carol
        .ok("DELETE", "/api/follows/Alice_Plays", Value::Null)
        .await;
    carol
        .ok("DELETE", "/api/follows/Alice_Plays", Value::Null)
        .await;

    // War Council: limit, duplicates, self, ineligible members, order.
    let mut members = Vec::new();
    for i in 0..9 {
        env.user(&format!("Ally_{i}x"), true).await;
        members.push(format!("Ally_{i}x"));
    }
    assert_eq!(
        alice
            .status("PUT", "/api/me/war-council", json!({"members": members}))
            .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        alice
            .status(
                "PUT",
                "/api/me/war-council",
                json!({"members": ["Ally_1x", "ally_1x"]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice
            .status(
                "PUT",
                "/api/me/war-council",
                json!({"members": ["Alice_Plays"]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice
            .status(
                "PUT",
                "/api/me/war-council",
                json!({"members": ["support"]})
            )
            .await,
        StatusCode::NOT_FOUND
    );
    alice
        .ok(
            "PUT",
            "/api/me/war-council",
            json!({"members": ["Ally_3x", "Ally_1x", "DaveDoes"]}),
        )
        .await;
    let council = env
        .anon
        .ok("GET", "/api/channels/Alice_Plays", Value::Null)
        .await["war_council"]
        .clone();
    let names: Vec<&str> = council["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["user"]["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Ally_3x", "Ally_1x", "DaveDoes"]);
    assert!(
        !alice
            .ok("GET", "/api/me/war-council/search?q=ally", Value::Null)
            .await["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        alice
            .ok("GET", "/api/me/war-council/search?q=supp", Value::Null)
            .await["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // SQL-looking search text stays a bound value in the dynamically assembled chip query.
    assert_eq!(
        alice
            .ok(
                "GET",
                "/api/me/war-council/search?q=%27%20OR%20true--",
                Value::Null
            )
            .await["results"],
        json!([])
    );

    // Blocking: silent, removes follows and War Council both ways, prevents new interactions.
    dave.ok("PUT", "/api/follows/Alice_Plays", Value::Null)
        .await;
    dave.ok(
        "PUT",
        "/api/me/war-council",
        json!({"members": ["Alice_Plays"]}),
    )
    .await;
    assert_eq!(
        alice
            .status("PUT", "/api/blocks/Alice_Plays", Value::Null)
            .await,
        StatusCode::BAD_REQUEST
    );
    alice.ok("PUT", "/api/blocks/DaveDoes", Value::Null).await;
    assert_eq!(env.count("SELECT count(*) FROM follows f JOIN users a ON a.id=f.follower_id JOIN users b ON b.id=f.following_id WHERE (a.username,b.username) IN (('DaveDoes','Alice_Plays'),('Alice_Plays','DaveDoes'))").await, 0);
    assert_eq!(env.count("SELECT count(*) FROM war_council w JOIN users m ON m.id=w.member_id JOIN users o ON o.id=w.user_id WHERE (o.username='Alice_Plays' AND m.username='DaveDoes') OR (o.username='DaveDoes' AND m.username='Alice_Plays')").await, 0);
    assert_eq!(env.count("SELECT max(position)::bigint FROM war_council w JOIN users o ON o.id=w.user_id WHERE o.username='Alice_Plays'").await, 2, "Positions are compacted");
    assert_eq!(
        dave.ok("GET", "/api/channels/Alice_Plays", Value::Null)
            .await["viewer"]["blocked"],
        false,
        "The blocked user is never told"
    );
    assert_eq!(
        dave.status("PUT", "/api/follows/Alice_Plays", Value::Null)
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        dave.status(
            "POST",
            "/api/channels/Alice_Plays/wall",
            json!({"body": "hi"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        alice.ok("GET", "/api/me/blocks", Value::Null).await["items"][0]["username"],
        "DaveDoes"
    );
    alice
        .ok("DELETE", "/api/blocks/DaveDoes", Value::Null)
        .await;
    let card = env
        .anon
        .ok("GET", "/api/users/Alice_Plays/card", Value::Null)
        .await;
    assert!(card.to_string().contains("follower_count"), "{card}");
    assert_eq!(
        env.anon
            .status("GET", "/api/users/support/card", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    env.reset_limits().await;
    println!(
        "Passed: follows, exact concurrent counts, lists, War Council rules and block side effects."
    );
}

async fn song(env: &Env) {
    let (id, owner) = env.user("SongOwner", true).await;
    for url in [
        "https://open.spotify.com/track/1",
        "https://youtube.com/playlist?list=abc",
        "https://soundcloud.com/artist/sets/x",
        "https://www.youtube.com/watch?v=short",
        "javascript:alert(1)",
    ] {
        assert_eq!(
            owner
                .status("PUT", "/api/me/song", json!({"url": url}))
                .await,
            StatusCode::BAD_REQUEST,
            "{url}"
        );
    }
    assert_eq!(
        studio::parse_song_url("https://youtu.be/dQw4w9WgXcQ")
            .unwrap()
            .media_id
            .as_deref(),
        Some("dQw4w9WgXcQ")
    );
    assert_eq!(
        studio::parse_song_url("https://m.youtube.com/shorts/dQw4w9WgXcQ")
            .unwrap()
            .provider,
        "youtube"
    );
    assert_eq!(
        studio::parse_song_url("https://soundcloud.com/artist/track-name")
            .unwrap()
            .provider,
        "soundcloud"
    );
    let (s, v) = owner
        .call(
            "PUT",
            "/api/me/song",
            json!({"url": "https://youtu.be/notfound___"}),
        )
        .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (
            StatusCode::BAD_REQUEST,
            Some("That track can't be embedded.")
        )
    );
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/song",
                json!({"url": "https://youtu.be/servererror"})
            )
            .await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/song",
                json!({"url": "https://youtu.be/redirect___"})
            )
            .await,
        StatusCode::SERVICE_UNAVAILABLE,
        "Redirects are never followed"
    );
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/song",
                json!({"url": "https://youtu.be/bigbody____"})
            )
            .await,
        StatusCode::SERVICE_UNAVAILABLE,
        "Bodies over 64 KB are refused"
    );
    let started = std::time::Instant::now();
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/song",
                json!({"url": "https://youtu.be/slowreply__"})
            )
            .await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(7),
        "The oEmbed timeout is 5 seconds"
    );
    env.reset_limits().await;
    owner
        .ok(
            "PUT",
            "/api/me/song",
            json!({"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10", "volume": 40}),
        )
        .await;
    let song = env
        .anon
        .ok("GET", "/api/channels/SongOwner", Value::Null)
        .await["channel"]["song"]
        .clone();
    assert_eq!(song["media_id"], "dQw4w9WgXcQ");
    assert_eq!(song["title"], "Synthetic Track");
    assert!(
        song["thumbnail"]
            .as_str()
            .unwrap()
            .starts_with(&env.app.config.media.public_base),
        "The thumbnail is re-hosted"
    );
    owner
        .ok(
            "PUT",
            "/api/me/song",
            json!({"url": "https://soundcloud.com/artist/track-name"}),
        )
        .await;
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/SongOwner", Value::Null)
            .await["channel"]["song"]["media_id"],
        "123456"
    );
    // A legacy Spotify record: the owner sees the notice, visitors see nothing.
    sqlx::query("UPDATE profiles SET song_provider=NULL,song_media_id=NULL,song_url=NULL,song_notice='spotify' WHERE user_id=$1").bind(&id).execute(&env.app.db).await.unwrap();
    let public = env
        .anon
        .ok("GET", "/api/channels/SongOwner", Value::Null)
        .await;
    assert!(public["channel"]["song"].is_null() && public["channel"]["song_notice"].is_null());
    assert!(
        owner
            .ok("GET", "/api/channels/SongOwner", Value::Null)
            .await["channel"]["song_notice"]
            .is_string()
    );
    owner.ok("DELETE", "/api/me/song", Value::Null).await;
    env.reset_limits().await;
    println!(
        "Passed: song URL forms, oEmbed error mapping, redirect/timeout/size limits, re-hosted thumbnails and the Spotify notice."
    );
}

async fn wall(env: &Env) {
    let (owner_id, owner) = env.user("WallOwner", true).await;
    let (_, fan) = env.user("WallFan", true).await;
    let (_, unverified) = env.user("NoMailYet", false).await;
    assert_eq!(
        env.anon
            .status(
                "POST",
                "/api/channels/WallOwner/wall",
                json!({"body": "hi"})
            )
            .await,
        StatusCode::UNAUTHORIZED
    );
    let (s, v) = unverified
        .call(
            "POST",
            "/api/channels/WallOwner/wall",
            json!({"body": "hi"}),
        )
        .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("Verify your email to post."))
    );
    assert_eq!(
        fan.status(
            "POST",
            "/api/channels/WallOwner/wall",
            json!({"body": "p".repeat(501)})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fan.status(
            "POST",
            "/api/channels/WallOwner/wall",
            json!({"body": "   "})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    let first = fan
        .ok(
            "POST",
            "/api/channels/WallOwner/wall",
            json!({"body": "First!"}),
        )
        .await;
    assert_eq!(first["status"], "APPROVED");
    let post = first["id"].as_str().unwrap().to_string();
    // Likes are idempotent; replies nest one level.
    assert_eq!(
        fan.ok("PUT", &format!("/api/wall/posts/{post}/like"), Value::Null)
            .await["like_count"],
        1
    );
    assert_eq!(
        fan.ok("PUT", &format!("/api/wall/posts/{post}/like"), Value::Null)
            .await["like_count"],
        1
    );
    assert_eq!(
        owner
            .ok("PUT", &format!("/api/wall/posts/{post}/like"), Value::Null)
            .await["like_count"],
        2
    );
    for i in 0..5 {
        owner
            .ok(
                "POST",
                &format!("/api/wall/posts/{post}/replies"),
                json!({"body": format!("reply {i}")}),
            )
            .await;
    }
    assert_eq!(
        fan.status(
            "POST",
            &format!("/api/wall/posts/{post}/replies"),
            json!({"body": "r".repeat(301)})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    let page = env
        .anon
        .ok("GET", "/api/channels/WallOwner/wall", Value::Null)
        .await;
    let item = &page["items"][0];
    assert_eq!(
        (
            item["reply_count"].as_i64(),
            item["replies"].as_array().unwrap().len(),
            item["more_replies"].as_bool()
        ),
        (Some(5), 3, Some(true))
    );
    assert_eq!(
        env.anon
            .ok(
                "GET",
                &format!("/api/wall/posts/{post}/replies"),
                Value::Null
            )
            .await["items"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(page["viewer"]["reason"], "Log in to sign the Wall.");

    // Approval rules: require approval, held links, new accounts; the author sees their pending post.
    owner.ok("PUT", "/api/me/wall/settings", json!({"who_can_post": "ANYONE", "require_approval": false, "hold_links": true, "hold_new_accounts": true})).await;
    let linked = fan
        .ok(
            "POST",
            "/api/channels/WallOwner/wall",
            json!({"body": "see https://example.com"}),
        )
        .await;
    assert_eq!(linked["status_label"], "Waiting for approval");
    let (newbie_id, newbie) = env.user("FreshFace", true).await;
    sqlx::query("UPDATE users SET created_at=now() WHERE id=$1")
        .bind(&newbie_id)
        .execute(&env.app.db)
        .await
        .unwrap();
    assert_eq!(
        newbie
            .ok(
                "POST",
                "/api/channels/WallOwner/wall",
                json!({"body": "hello"})
            )
            .await["status"],
        "PENDING"
    );
    assert_eq!(
        owner
            .ok(
                "POST",
                "/api/channels/WallOwner/wall",
                json!({"body": "owner link https://example.com"})
            )
            .await["status"],
        "APPROVED",
        "The owner is never held"
    );
    let anon_ids: Vec<String> = env
        .anon
        .ok("GET", "/api/channels/WallOwner/wall", Value::Null)
        .await["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    let linked_id = linked["id"].as_str().unwrap().to_string();
    assert!(
        !anon_ids.contains(&linked_id),
        "Pending posts are hidden from visitors"
    );
    assert!(
        fan.ok("GET", "/api/channels/WallOwner/wall", Value::Null)
            .await
            .to_string()
            .contains(&linked_id)
    );
    let pending = owner.ok("GET", "/api/me/wall/pending", Value::Null).await;
    assert_eq!(pending["pending_count"], 2);
    owner
        .ok(
            "POST",
            &format!("/api/wall/posts/{linked_id}/approve"),
            Value::Null,
        )
        .await;
    assert_eq!(
        owner
            .status(
                "POST",
                &format!("/api/wall/posts/{linked_id}/reject"),
                Value::Null
            )
            .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        fan.status(
            "POST",
            &format!("/api/wall/posts/{linked_id}/approve"),
            Value::Null
        )
        .await,
        StatusCode::NOT_FOUND
    );
    let newbie_post = pending["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["author"]["username"] == "FreshFace")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    owner
        .ok(
            "POST",
            &format!("/api/wall/posts/{newbie_post}/block-author"),
            Value::Null,
        )
        .await;
    assert_eq!(env.count(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM user_blocks WHERE blocker_id='{owner_id}' AND blocked_id='{newbie_id}'"))).await, 1);
    assert_eq!(
        newbie
            .status(
                "POST",
                "/api/channels/WallOwner/wall",
                json!({"body": "again"})
            )
            .await,
        StatusCode::FORBIDDEN
    );

    // Who can post.
    owner.ok("PUT", "/api/me/wall/settings", json!({"who_can_post": "FOLLOWING", "require_approval": false, "hold_links": false, "hold_new_accounts": false})).await;
    assert_eq!(
        fan.status("POST", "/api/channels/WallOwner/wall", json!({"body": "x"}))
            .await,
        StatusCode::FORBIDDEN
    );
    owner.ok("PUT", "/api/follows/WallFan", Value::Null).await;
    assert_eq!(
        fan.status("POST", "/api/channels/WallOwner/wall", json!({"body": "x"}))
            .await,
        StatusCode::OK
    );
    owner.ok("PUT", "/api/me/wall/settings", json!({"who_can_post": "MUTUAL", "require_approval": false, "hold_links": false, "hold_new_accounts": false})).await;
    assert_eq!(
        fan.status("POST", "/api/channels/WallOwner/wall", json!({"body": "x"}))
            .await,
        StatusCode::FORBIDDEN
    );
    fan.ok("PUT", "/api/follows/WallOwner", Value::Null).await;
    assert_eq!(
        fan.status("POST", "/api/channels/WallOwner/wall", json!({"body": "x"}))
            .await,
        StatusCode::OK
    );
    owner.ok("PUT", "/api/me/wall/settings", json!({"who_can_post": "NONE", "require_approval": false, "hold_links": false, "hold_new_accounts": false})).await;
    assert_eq!(
        fan.status("POST", "/api/channels/WallOwner/wall", json!({"body": "x"}))
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(owner.status("PUT", "/api/me/wall/settings", json!({"who_can_post": "SUBSCRIBERS", "require_approval": false, "hold_links": false, "hold_new_accounts": false})).await, StatusCode::BAD_REQUEST);
    owner.ok("PUT", "/api/me/wall/settings", json!({"who_can_post": "ANYONE", "require_approval": false, "hold_links": false, "hold_new_accounts": false})).await;

    // Pins: up to 3, approved posts on the owner's wall, shown first.
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/wall/pins",
                json!({"post_ids": ["a", "b", "c", "d"]})
            )
            .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/wall/pins",
                json!({"post_ids": [newbie_post.clone()]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    owner
        .ok(
            "PUT",
            "/api/me/wall/pins",
            json!({"post_ids": [post.clone()]}),
        )
        .await;
    let preview = env
        .anon
        .ok("GET", "/api/channels/WallOwner", Value::Null)
        .await["wall_preview"]
        .clone();
    assert_eq!(preview["pinned"][0]["id"], post.as_str());
    assert!(preview["latest"].as_array().unwrap().len() <= 3);
    // Deletion by the author or owner only.
    assert_eq!(
        newbie
            .status("DELETE", &format!("/api/wall/posts/{post}"), Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    fan.ok("DELETE", &format!("/api/wall/posts/{post}"), Value::Null)
        .await;
    assert!(
        env.anon
            .ok("GET", "/api/channels/WallOwner", Value::Null)
            .await["wall_preview"]["pinned"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        env.anon
            .status(
                "GET",
                &format!("/api/wall/posts/{post}/replies"),
                Value::Null
            )
            .await,
        StatusCode::NOT_FOUND
    );
    env.reset_limits().await;
    println!(
        "Passed: Wall posting rules, likes, replies, approval holds, review actions, who-can-post, pins and deletion."
    );
}
async fn studio_sections(env: &Env, media_dir: &std::path::Path) {
    let (owner_id, owner) = env.user("StudioOwner", true).await;
    let (_, artist) = env.user("ArtistFan", true).await;
    let (_, unverified) = env.user("ArtNoMail", false).await;
    // Schedule: named zones only, block rules, events, Home shows the next occurrences.
    for tz in ["UTC+2", "+02:00", "Etc/GMT-2", "Not/AZone"] {
        assert_eq!(
            owner
                .status("PUT", "/api/me/schedule", json!({"timezone": tz}))
                .await,
            StatusCode::BAD_REQUEST,
            "{tz}"
        );
    }
    let overlap = json!({"timezone": "America/New_York", "blocks": [{"weekday": 1, "start": "18:00", "end": "20:00"}, {"weekday": 1, "start": "19:00", "end": "21:00"}]});
    assert_eq!(
        owner.status("PUT", "/api/me/schedule", overlap).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(owner.status("PUT", "/api/me/schedule", json!({"timezone": "America/New_York", "blocks": [{"weekday": 1, "start": "18:00", "end": "18:00"}]})).await, StatusCode::BAD_REQUEST);
    let blocks: Vec<Value> = (1..=7)
        .map(|d| json!({"weekday": d, "start": "22:00", "end": "01:00", "label": "Late show"}))
        .collect();
    let now = chrono::Utc::now();
    let event = json!({"title": "Special", "start_at": now + chrono::Duration::days(2), "end_at": now + chrono::Duration::days(2) + chrono::Duration::hours(2)});
    let too_long = json!({"title": "Marathon", "start_at": now + chrono::Duration::days(2), "end_at": now + chrono::Duration::days(3) + chrono::Duration::hours(1)});
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/schedule",
                json!({"timezone": "America/New_York", "events": [too_long]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    owner
        .ok(
            "PUT",
            "/api/me/schedule",
            json!({"timezone": "America/New_York", "blocks": blocks, "events": [event]}),
        )
        .await;
    let schedule = env
        .anon
        .ok("GET", "/api/channels/StudioOwner/schedule", Value::Null)
        .await;
    assert_eq!(schedule["timezone"], "America/New_York");
    assert!(
        schedule["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "event")
    );
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/StudioOwner", Value::Null)
            .await["schedule_next"]["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/StudioOwner", Value::Null)
            .await["tabs"]["schedule"],
        true
    );

    // Sponsors, setup and page blocks: limits, strict validation and the About payload.
    let sponsor = |n: usize| json!({"name": format!("Sponsor {n}"), "link": "https://sponsor.example", "category": "HARDWARE", "discount_code": "SVER10"});
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/sponsors",
                json!({"items": (0..11).map(sponsor).collect::<Vec<_>>()})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(owner.status("PUT", "/api/me/sponsors", json!({"items": [{"name": "X", "link": "http://insecure.example", "category": "HARDWARE"}]})).await, StatusCode::BAD_REQUEST);
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/sponsors",
                json!({"items": [{"name": "X", "link": "https://ok.example", "category": "NOPE"}]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    let saved = owner.ok("PUT", "/api/me/sponsors", json!({"items": [sponsor(1), {"name": "Hidden", "link": "https://h.example", "category": "OTHER", "active": false}]})).await;
    let sponsor_id = saved["ids"][0].as_str().unwrap().to_string();
    let (s, v) = owner
        .upload(
            &format!("/api/me/sponsors/{sponsor_id}/logo"),
            &[],
            Some(&png(300, 300)),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let items: Vec<Value> = (0..21)
        .map(|i| json!({"category": "CPU", "name": format!("Part {i}")}))
        .collect();
    assert_eq!(
        owner
            .status("PUT", "/api/me/setup", json!({"items": items}))
            .await,
        StatusCode::BAD_REQUEST
    );
    owner.ok("PUT", "/api/me/setup", json!({"items": [{"category": "MIC", "name": "Synthetic Mic", "note": "Cardioid", "link": "https://mic.example"}]})).await;
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/blocks",
                json!({"items": [{"type": "ABOUT", "config": {"body": "x", "html": "<b>"}}]})
            )
            .await,
        StatusCode::BAD_REQUEST,
        "Unknown fields are rejected"
    );
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/blocks",
                json!({"items": [{"type": "SCRIPT", "config": {}}]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/blocks",
                json!({"items": [{"type": "QUOTES", "config": {"quotes": []}}]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    let many: Vec<Value> = (0..11)
        .map(|_| json!({"type": "ABOUT", "config": {"body": "x"}}))
        .collect();
    assert_eq!(
        owner
            .status("PUT", "/api/me/blocks", json!({"items": many}))
            .await,
        StatusCode::BAD_REQUEST
    );
    owner
        .ok(
            "PUT",
            "/api/me/blocks",
            json!({"items": [
                {"type": "ABOUT", "config": {"body": "**Hello** there"}},
                {"type": "PANEL", "enabled": false, "config": {"title": "Draft", "body": "Hidden"}},
                {"type": "GAME_SHELF", "config": {"games": ["Game One", "Game Two"]}}
            ]}),
        )
        .await;
    assert_eq!(
        owner.ok("GET", "/api/me/page-blocks", Value::Null).await["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let about = env
        .anon
        .ok("GET", "/api/channels/StudioOwner/about", Value::Null)
        .await;
    assert_eq!(
        about["blocks"].as_array().unwrap().len(),
        2,
        "Disabled blocks stay private"
    );
    assert_eq!(
        about["sponsors"].as_array().unwrap().len(),
        1,
        "Inactive sponsors stay private"
    );
    assert!(about["sponsors"][0]["logo"].is_string());
    assert_eq!(about["setup"][0]["name"], "Synthetic Mic");
    let rev = owner.ok("GET", "/api/me/setup", Value::Null).await["revision"]
        .as_i64()
        .unwrap();
    owner
        .ok(
            "PUT",
            "/api/me/setup",
            json!({"items": [], "revision": rev}),
        )
        .await;
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/setup",
                json!({"items": [], "revision": rev})
            )
            .await,
        StatusCode::CONFLICT
    );

    // Fan art: off by default, verified submitters, attestation, review, the public gallery.
    let art = png(800, 600);
    assert_eq!(
        artist
            .upload(
                "/api/channels/StudioOwner/fan-art",
                &[("attest", "true")],
                Some(&art)
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/StudioOwner/fan-art", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    owner
        .ok("PUT", "/api/me/fan-art/settings", json!({"enabled": true}))
        .await;
    assert_eq!(
        unverified
            .upload(
                "/api/channels/StudioOwner/fan-art",
                &[("attest", "true")],
                Some(&art)
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        artist
            .upload("/api/channels/StudioOwner/fan-art", &[], Some(&art))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "Attestation is required"
    );
    let (s, v) = artist
        .upload(
            "/api/channels/StudioOwner/fan-art",
            &[("attest", "true"), ("caption", "For the stream")],
            Some(&art),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let art_id = v["id"].as_str().unwrap().to_string();
    assert!(
        env.anon
            .ok("GET", "/api/channels/StudioOwner/fan-art", Value::Null)
            .await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        artist
            .ok("GET", "/api/channels/StudioOwner/fan-art", Value::Null)
            .await["items"][0]["status"],
        "PENDING"
    );
    assert_eq!(
        artist
            .status(
                "POST",
                &format!("/api/fan-art/{art_id}/approve"),
                Value::Null
            )
            .await,
        StatusCode::NOT_FOUND
    );
    owner
        .ok(
            "POST",
            &format!("/api/fan-art/{art_id}/approve"),
            Value::Null,
        )
        .await;
    assert_eq!(
        owner
            .status(
                "POST",
                &format!("/api/fan-art/{art_id}/reject"),
                Value::Null
            )
            .await,
        StatusCode::CONFLICT
    );
    let gallery = env
        .anon
        .ok("GET", "/api/channels/StudioOwner/fan-art", Value::Null)
        .await;
    assert_eq!(gallery["items"][0]["artist_name"], "ArtistFan");
    let stored: String = sqlx::query_scalar("SELECT image_key FROM fan_art WHERE id=$1")
        .bind(&art_id)
        .fetch_one(&env.app.db)
        .await
        .unwrap();
    assert!(
        media_dir.join(format!("{stored}/1600.webp")).exists()
            || media_dir.join(format!("{stored}/400.webp")).exists()
    );
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/StudioOwner", Value::Null)
            .await["tabs"]["fan_art"],
        true
    );
    let _ = owner_id;
    env.reset_limits().await;
    println!(
        "Passed: schedule rules and occurrences, sponsors with logos, setup, typed page blocks, stale-edit conflicts and the fan art flow."
    );
}
/// Parity additions P1-P9 (docs/PROFILES.md, "Parity additions"). P1 and P8 are web-only and
/// covered by the browser parity check.
async fn parity_additions(env: &Env, media_dir: &std::path::Path) {
    env.reset_limits().await;
    let (owner_id, owner) = env.user("ParityHost", true).await;
    let (fan_id, fan) = env.user("ParityFan", true).await;
    let (_, other) = env.user("ParityOther", true).await;

    // P2: every mood preset passes the server's one-emoji rule and saves.
    let me = owner.ok("GET", "/api/me/profile", Value::Null).await;
    let presets = me["mood_presets"].as_array().unwrap();
    assert_eq!(presets.len(), 12);
    for p in presets {
        assert!(sver::text::valid_mood(p.as_str().unwrap()), "{p}");
    }
    assert_eq!(me["show_linked_accounts"], false);
    owner
        .ok(
            "PATCH",
            "/api/me/profile",
            json!({"mood_emoji": "⚔️", "revision": me["revisions"]["profile"]}),
        )
        .await;
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/ParityHost", Value::Null)
            .await["channel"]["mood_emoji"],
        "⚔️"
    );

    // P9: header copy defaults, edits, toggle, limits, stale revision and clearing.
    let header = env
        .anon
        .ok("GET", "/api/channels/ParityHost", Value::Null)
        .await["header"]
        .clone();
    assert_eq!(
        header,
        json!({"label": "Creator Page", "welcome": "Welcome to my page", "intro_title": "", "intro_body": "", "vibe": ""})
    );
    let mine = owner.ok("GET", "/api/me/header", Value::Null).await;
    let rev = mine["revision"].as_i64().unwrap();
    let saved = owner.ok("PUT", "/api/me/header", json!({"page_label": "Strategy Hub", "welcome_line": "Pull up a chair", "intro_title": "Hi there", "intro_body": "Line one\nLine two", "page_vibe": "Chill nights", "enabled": true, "revision": rev})).await;
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/header",
                json!({"page_label": "x", "revision": rev})
            )
            .await,
        StatusCode::CONFLICT
    );
    let rev = saved["revision"].as_i64().unwrap();
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/header",
                json!({"page_label": "x".repeat(25), "revision": rev})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    let header = env
        .anon
        .ok("GET", "/api/channels/ParityHost", Value::Null)
        .await["header"]
        .clone();
    assert_eq!(
        header,
        json!({"label": "Strategy Hub", "welcome": "Pull up a chair", "intro_title": "Hi there", "intro_body": "Line one\nLine two", "vibe": "Chill nights"})
    );
    let saved = owner
        .ok(
            "PUT",
            "/api/me/header",
            json!({"intro_title": "Orphan title", "enabled": false, "revision": rev}),
        )
        .await;
    let header = env
        .anon
        .ok("GET", "/api/channels/ParityHost", Value::Null)
        .await["header"]
        .clone();
    assert_eq!(
        header,
        json!({"label": null, "welcome": null, "intro_title": "", "intro_body": "", "vibe": ""}),
        "toggle off hides label and welcome; no intro without a body"
    );
    owner
        .ok(
            "PUT",
            "/api/me/header",
            json!({"page_label": "  ", "intro_body": "Kept", "revision": saved["revision"]}),
        )
        .await;
    let header = env
        .anon
        .ok("GET", "/api/channels/ParityHost", Value::Null)
        .await["header"]
        .clone();
    assert_eq!(header["label"], "Creator Page", "blank means the default");
    assert_eq!(header["intro_body"], "Kept");

    // P3: readiness steps, dismiss and restore, owner only.
    assert_eq!(
        env.anon
            .status("GET", "/api/me/readiness", Value::Null)
            .await,
        StatusCode::UNAUTHORIZED
    );
    let ready = owner.ok("GET", "/api/me/readiness", Value::Null).await;
    assert_eq!(
        (ready["total"].as_i64(), ready["done"].as_i64()),
        (Some(7), Some(0))
    );
    assert_eq!(ready["dismissed"], false);
    let keys: Vec<&str> = ready["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "avatar", "banner", "bio", "links", "song", "council", "schedule"
        ]
    );
    let me = owner.ok("GET", "/api/me/profile", Value::Null).await;
    owner
        .ok(
            "PATCH",
            "/api/me/profile",
            json!({"bio": "Parity bio", "revision": me["revisions"]["profile"]}),
        )
        .await;
    owner
        .ok(
            "PUT",
            "/api/me/schedule",
            json!({"timezone": "America/New_York", "blocks": [{"weekday": 2, "start": "19:00", "end": "21:00", "label": "Raid night"}]}),
        )
        .await;
    let ready = owner.ok("GET", "/api/me/readiness", Value::Null).await;
    assert_eq!(ready["done"], 2);
    assert!(
        ready["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["done"] == (s["key"] == "bio" || s["key"] == "schedule"))
    );
    owner
        .ok("PUT", "/api/me/readiness", json!({"dismissed": true}))
        .await;
    let again = env.session(&owner_id, false).await;
    assert_eq!(
        again.ok("GET", "/api/me/readiness", Value::Null).await["dismissed"],
        true,
        "dismissal persists across sessions"
    );
    owner
        .ok("PUT", "/api/me/readiness", json!({"dismissed": false}))
        .await;
    assert_eq!(
        owner.ok("GET", "/api/me/readiness", Value::Null).await["dismissed"],
        false
    );

    // P4: setup title and description, photos, limits, order, delete, moderation.
    let setup = owner.ok("GET", "/api/me/setup", Value::Null).await;
    assert_eq!(setup["max_photos"], 3);
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/setup",
                json!({"items": [], "title": "x".repeat(81), "revision": setup["revision"]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    owner
        .ok(
            "PUT",
            "/api/me/setup",
            json!({"items": [{"category": "MIC", "name": "Desk mic"}], "title": "My desk", "description": "Two monitors\nOne mic", "revision": setup["revision"]}),
        )
        .await;
    let mut photo_ids = Vec::new();
    for (i, w) in [800u32, 801, 802].iter().enumerate() {
        let (s, v) = owner
            .upload(
                "/api/me/setup/photos",
                &[("alt", &format!("Angle {i}"))],
                Some(&png(*w, 600)),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let url = v["image"]["400"].as_str().unwrap();
        let key = url.split("/api/media/").nth(1).unwrap();
        assert!(key.starts_with("setup/"), "{key}");
        assert!(media_dir.join(key).exists());
        assert!(
            media_dir
                .join(key.replace("400.webp", "1600.webp"))
                .exists()
        );
        photo_ids.push(v["id"].as_str().unwrap().to_string());
        if i == 0 {
            let (s, v) = owner
                .upload("/api/me/setup/photos", &[], Some(&png(800, 600)))
                .await;
            assert_eq!(
                (s, v["error"].as_str()),
                (StatusCode::CONFLICT, Some("You already added that photo."))
            );
            let (s, _) = owner
                .upload("/api/me/setup/photos", &[], Some(&png(32, 32)))
                .await;
            assert_eq!(s, StatusCode::BAD_REQUEST);
            let (s, _) = owner
                .upload("/api/me/setup/photos", &[], Some(b"not an image"))
                .await;
            assert_eq!(s, StatusCode::BAD_REQUEST);
        }
    }
    let (s, v) = owner
        .upload("/api/me/setup/photos", &[], Some(&png(803, 600)))
        .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (
            StatusCode::CONFLICT,
            Some("You can add up to 3 setup photos.")
        )
    );
    let about = env
        .anon
        .ok("GET", "/api/channels/ParityHost/about", Value::Null)
        .await;
    assert_eq!(about["setup_title"], "My desk");
    assert_eq!(about["setup_description"], "Two monitors\nOne mic");
    assert_eq!(about["setup_photos"].as_array().unwrap().len(), 3);
    assert_eq!(about["setup_photos"][0]["alt"], "Angle 0");
    assert_eq!(about["viewer"]["can_report"], false);
    assert_eq!(
        fan.ok("GET", "/api/channels/ParityHost/about", Value::Null)
            .await["viewer"]["can_report"],
        true
    );
    assert_eq!(
        owner
            .status(
                "PUT",
                "/api/me/setup/photos",
                json!({"photos": [{"id": photo_ids[0]}]})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    let reversed: Vec<Value> = photo_ids
        .iter()
        .rev()
        .map(|id| json!({"id": id, "alt": format!("Alt {id}")}))
        .collect();
    owner
        .ok("PUT", "/api/me/setup/photos", json!({"photos": reversed}))
        .await;
    let about = env
        .anon
        .ok("GET", "/api/channels/ParityHost/about", Value::Null)
        .await;
    assert_eq!(about["setup_photos"][0]["id"], photo_ids[2].as_str());
    // Deleting a photo queues its media; the slot frees up.
    let gone_key: String = sqlx::query_scalar("SELECT image_key FROM setup_photos WHERE id=$1")
        .bind(&photo_ids[2])
        .fetch_one(&env.app.db)
        .await
        .unwrap();
    owner
        .ok(
            "DELETE",
            &format!("/api/me/setup/photos/{}", photo_ids[2]),
            Value::Null,
        )
        .await;
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM media_objects WHERE key LIKE '{gone_key}/%' AND delete_after IS NOT NULL"))).await,
        2
    );
    assert_eq!(
        other
            .status(
                "DELETE",
                &format!("/api/me/setup/photos/{}", photo_ids[1]),
                Value::Null
            )
            .await,
        StatusCode::NOT_FOUND,
        "only the owner deletes"
    );
    // Restricted owners can't upload or edit.
    env.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE profiles SET restricted_until=now()+interval '1 hour' WHERE user_id='{owner_id}'"
    )))
    .await;
    let (s, _) = owner
        .upload("/api/me/setup/photos", &[], Some(&png(804, 600)))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    env.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE profiles SET restricted_until=NULL WHERE user_id='{owner_id}'"
    )))
    .await;
    // Report, remove with a strike, then an overturned appeal restores the photo.
    let reported = photo_ids[1].clone();
    fan.ok(
        "POST",
        "/api/reports",
        json!({"target_type": "setup_photo", "target_id": reported, "reason": "spam"}),
    )
    .await;
    let (_, _, mod_a) = staff(env, "ParityModA").await;
    let (_, _, mod_b) = staff(env, "ParityModB").await;
    let queue = mod_a.ok("GET", "/api/admin/reports", Value::Null).await;
    assert!(
        queue["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["target_type"] == "setup_photo" && g["target_id"] == reported.as_str())
    );
    let result = mod_a.ok("POST", &format!("/api/admin/reports/setup_photo/{reported}/actions"), json!({"action": "remove_content", "note": "Synthetic removal", "strike": {"reason": "spam", "severity": "STANDARD"}})).await;
    let strike = result["strike_id"].as_str().unwrap().to_string();
    let about = env
        .anon
        .ok("GET", "/api/channels/ParityHost/about", Value::Null)
        .await;
    assert_eq!(about["setup_photos"].as_array().unwrap().len(), 1);
    let studio = owner.ok("GET", "/api/me/setup", Value::Null).await;
    assert!(
        studio["photos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == reported.as_str() && p["status"] == "REMOVED")
    );
    owner
        .ok(
            "POST",
            &format!("/api/me/strikes/{strike}/appeal"),
            json!({"body": "That was my desk."}),
        )
        .await;
    let appeals = mod_b.ok("GET", "/api/admin/appeals", Value::Null).await;
    let appeal = appeals["appeals"][0]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{appeals}"))
        .to_string();
    mod_b
        .ok(
            "POST",
            &format!("/api/admin/appeals/{appeal}/decision"),
            json!({"outcome": "overturned", "staff_note": "Fine"}),
        )
        .await;
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/ParityHost/about", Value::Null)
            .await["setup_photos"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "overturn restores the photo"
    );
    // Header text is a reportable channel field with a reset.
    fan.ok("POST", "/api/reports", json!({"target_type": "profile", "target_id": "ParityHost", "field": "header", "reason": "spam"})).await;
    mod_a
        .ok(
            "POST",
            &format!("/api/admin/reports/profile/{owner_id}/actions"),
            json!({"action": "reset_field", "field": "header", "note": "Synthetic reset"}),
        )
        .await;
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/ParityHost", Value::Null)
            .await["header"]["intro_body"],
        ""
    );
    // The setup field reset clears items, title, description and photos.
    mod_a
        .ok(
            "POST",
            &format!("/api/admin/reports/profile/{owner_id}/actions"),
            json!({"action": "reset_field", "field": "setup", "note": "Synthetic reset"}),
        )
        .await;
    let about = env
        .anon
        .ok("GET", "/api/channels/ParityHost/about", Value::Null)
        .await;
    assert_eq!(
        (
            about["setup_title"].as_str(),
            about["setup"].as_array().unwrap().len(),
            about["setup_photos"].as_array().unwrap().len()
        ),
        (Some(""), 0, 0)
    );
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM setup_photos WHERE user_id='{owner_id}'"
        )))
        .await,
        0
    );

    // P5 and P6: handles on identities, suggestions, the Discord users link and the opt-in card.
    env.sql(sqlx::AssertSqlSafe(format!("INSERT INTO identities(provider,subject,user_id,handle) VALUES('twitch','tw-parity','{owner_id}','parityhost_tv'),('discord','123456789012345678','{owner_id}','parity.host'),('google','g-parity','{owner_id}',NULL)"))).await; // gitleaks:allow -- synthetic Discord user ID for this test.
    let s = owner
        .ok("GET", "/api/me/link-suggestions", Value::Null)
        .await;
    assert_eq!(
        s["suggestions"],
        json!([{"platform": "twitch", "url": "https://twitch.tv/parityhost_tv", "label": "parityhost_tv"}, {"platform": "discord", "url": "https://discord.com/users/123456789012345678", "label": "parity.host"}])
    );
    assert_eq!(
        owner.ok("GET", "/api/me/profile", Value::Null).await["links"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "suggestions never save themselves"
    );
    let me = owner.ok("GET", "/api/me/profile", Value::Null).await;
    let links =
        json!([{"platform": "discord", "url": "https://discord.com/users/123456789012345678"}]);
    owner
        .ok(
            "PUT",
            "/api/me/links",
            json!({"links": links, "revision": me["revisions"]["links"]}),
        )
        .await;
    let s = owner
        .ok("GET", "/api/me/link-suggestions", Value::Null)
        .await;
    assert_eq!(
        s["suggestions"].as_array().unwrap().len(),
        1,
        "an existing platform is not suggested"
    );
    assert!(sver::text::social_link("discord", "https://discord.com/users/12345").is_err());
    assert_eq!(
        sver::text::provider_handle("twitch", "Parity_Host"),
        Some("parity_host".into())
    );
    assert_eq!(sver::text::provider_handle("twitch", "bad name"), None);
    let card = fan
        .ok("GET", "/api/users/ParityHost/card", Value::Null)
        .await;
    assert_eq!(card["also_known_as"], json!([]), "off by default");
    owner
        .ok(
            "PUT",
            "/api/me/card-settings",
            json!({"show_linked_accounts": true}),
        )
        .await;
    let card = fan
        .ok("GET", "/api/users/ParityHost/card", Value::Null)
        .await;
    assert_eq!(
        card["also_known_as"],
        json!([{"platform": "twitch", "handle": "parityhost_tv", "url": "https://twitch.tv/parityhost_tv"}, {"platform": "discord", "handle": "parity.host", "url": null}])
    );
    fan.ok("PUT", "/api/blocks/ParityHost", Value::Null).await;
    assert_eq!(
        fan.ok("GET", "/api/users/ParityHost/card", Value::Null)
            .await["also_known_as"],
        json!([]),
        "hidden across a block"
    );
    fan.ok("DELETE", "/api/blocks/ParityHost", Value::Null)
        .await;
    env.sql(sqlx::AssertSqlSafe(format!(
        "DELETE FROM identities WHERE user_id='{owner_id}' AND provider='twitch'"
    )))
    .await;
    assert_eq!(
        env.anon
            .ok("GET", "/api/users/ParityHost/card", Value::Null)
            .await["also_known_as"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "unlinking removes it"
    );
    owner
        .ok(
            "PUT",
            "/api/me/card-settings",
            json!({"show_linked_accounts": false}),
        )
        .await;
    assert_eq!(
        env.anon
            .ok("GET", "/api/users/ParityHost/card", Value::Null)
            .await["also_known_as"],
        json!([])
    );

    // P7: activity feed recording, coalescing and read-time filters.
    let (_, actor) = env.user("FeedActor", true).await;
    let (target_id, target) = env.user("FeedTarget", true).await;
    let (_, _) = env.user("FeedOther", true).await;
    let feed = |c: &Client| {
        let c = c.clone();
        async move {
            c.ok("GET", "/api/channels/FeedActor/activity", Value::Null)
                .await
        }
    };
    assert_eq!(feed(&env.anon).await["items"], json!([]));
    actor
        .ok("PUT", "/api/follows/FeedTarget", Value::Null)
        .await;
    let items = feed(&env.anon).await["items"].clone();
    assert_eq!(items[0]["kind"], "follow");
    assert_eq!(items[0]["subject"]["username"], "FeedTarget");
    actor
        .ok("DELETE", "/api/follows/FeedTarget", Value::Null)
        .await;
    assert_eq!(
        feed(&env.anon).await["items"],
        json!([]),
        "unfollow hides it"
    );
    actor
        .ok("PUT", "/api/follows/FeedTarget", Value::Null)
        .await;
    assert_eq!(
        env.count("SELECT count(*) FROM activity_events e JOIN users u ON u.id=e.actor_id WHERE u.username='FeedActor' AND kind='follow'").await,
        1,
        "one row per pair"
    );
    let post = actor
        .ok(
            "POST",
            "/api/channels/FeedTarget/wall",
            json!({"body": "Hello from the feed"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    for members in [json!(["FeedTarget"]), json!(["FeedTarget", "FeedOther"])] {
        actor
            .ok("PUT", "/api/me/war-council", json!({"members": members}))
            .await;
    }
    actor
        .ok(
            "PUT",
            "/api/me/schedule",
            json!({"timezone": "America/New_York", "blocks": [{"weekday": 3, "start": "20:00", "end": "22:00"}]}),
        )
        .await;
    actor
        .ok(
            "PUT",
            "/api/me/song",
            json!({"url": "https://youtu.be/feedsong123"}),
        )
        .await;
    let items = feed(&env.anon).await["items"].clone();
    let kinds: Vec<&str> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["song", "schedule", "war_council", "wall_post", "follow"]
    );
    assert_eq!(items[2]["data"]["count"], 2, "council saves coalesce");
    assert_eq!(items[0]["data"]["title"], "Synthetic Track");
    // Removing the song removes its event; a deleted wall post hides its event.
    actor.ok("DELETE", "/api/me/song", Value::Null).await;
    actor
        .ok("DELETE", &format!("/api/wall/posts/{post}"), Value::Null)
        .await;
    let kinds: Vec<String> = feed(&env.anon).await["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(kinds, ["schedule", "war_council", "follow"]);
    // A pending post records nothing until approved.
    let me_t = target.ok("GET", "/api/me/wall", Value::Null).await;
    target
        .ok(
            "PUT",
            "/api/me/wall/settings",
            json!({"who_can_post": "ANYONE", "require_approval": true, "hold_links": false, "hold_new_accounts": false, "revision": me_t["revision"]}),
        )
        .await;
    let pending = actor
        .ok(
            "POST",
            "/api/channels/FeedTarget/wall",
            json!({"body": "Needs approval"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(feed(&env.anon).await["items"].as_array().unwrap().len(), 3);
    target
        .ok(
            "POST",
            &format!("/api/wall/posts/{pending}/approve"),
            Value::Null,
        )
        .await;
    assert_eq!(feed(&env.anon).await["items"][0]["kind"], "wall_post");
    // A block between the viewer and the subject hides subject events for that viewer only.
    let (_, watcher) = env.user("FeedWatcher", true).await;
    watcher
        .ok("PUT", "/api/blocks/FeedTarget", Value::Null)
        .await;
    let seen: Vec<String> = feed(&watcher).await["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(seen, ["schedule", "war_council"]);
    assert_eq!(feed(&env.anon).await["items"].as_array().unwrap().len(), 4);
    // A restricted subject is hidden; an unknown kind is skipped; internal actors record nothing.
    env.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE profiles SET restricted_until=now()+interval '1 hour' WHERE user_id='{target_id}'"
    )))
    .await;
    assert_eq!(feed(&env.anon).await["items"].as_array().unwrap().len(), 2);
    env.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE profiles SET restricted_until=NULL WHERE user_id='{target_id}'"
    )))
    .await;
    env.sql("INSERT INTO activity_events(id,actor_id,kind) SELECT 'future-kind',id,'stream_started' FROM users WHERE username='FeedActor'").await;
    assert!(!feed(&env.anon).await.to_string().contains("stream_started"));
    let (internal_id, internal) = env.user("FeedInternal", true).await;
    env.sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO profiles(user_id,display_name,internal) VALUES('{internal_id}','FeedInternal',true) ON CONFLICT (user_id) DO UPDATE SET internal=true"
    )))
    .await;
    internal
        .ok("PUT", "/api/follows/FeedTarget", Value::Null)
        .await;
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM activity_events WHERE actor_id='{internal_id}'"
        )))
        .await,
        0
    );
    // Pagination: 20 per page with a cursor.
    for i in 0..22 {
        env.sql(sqlx::AssertSqlSafe(format!("INSERT INTO activity_events(id,actor_id,kind,data,created_at) SELECT 'bulk-{i:02}',id,'schedule','{{}}',now()-interval '1 day'-make_interval(mins=>{i}) FROM users WHERE username='FeedActor'"))).await;
    }
    let page = feed(&env.anon).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 20);
    let cursor = page["next_cursor"].as_str().unwrap();
    let next = env
        .anon
        .ok(
            "GET",
            &format!("/api/channels/FeedActor/activity?cursor={cursor}"),
            Value::Null,
        )
        .await;
    assert_eq!(next["items"].as_array().unwrap().len(), 6);
    assert!(next["next_cursor"].is_null());
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/NoSuchFeed/activity", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    // Erasure deletes events as actor or subject, and the user's setup photos with queued media.
    let (s, v) = other
        .upload("/api/me/setup/photos", &[], Some(&png(805, 600)))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let other_id = env.id_of("ParityOther").await;
    for id in [&target_id, &other_id] {
        let mut tx = env.app.db.begin().await.unwrap();
        sver::profile_jobs::erase(&mut tx, id).await.unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM activity_events WHERE subject_id='{target_id}' OR actor_id='{target_id}'"))).await,
        0
    );
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM setup_photos WHERE user_id='{other_id}'"
        )))
        .await,
        0
    );
    assert_eq!(
        env.count("SELECT count(*) FROM media_objects WHERE kind='setup_photo' AND key LIKE 'setup/%' AND delete_after IS NULL AND owner_id IS NULL").await,
        0,
        "erased setup photos are queued for deletion"
    );
    let _ = fan_id;
    eprintln!(
        "Passed: parity additions (mood presets, header copy, readiness, setup photos with moderation, link suggestions, opt-in card, activity feed)."
    );
}

/// Setup parts picker (docs/PROFILES.md, "Setup parts picker"): the seed list, search, picked and
/// custom entries, the staff review queue, entries kept from before the picker, and erasure.
async fn parts_picker(env: &Env) {
    let db = &env.app.db;
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category,count(*) FROM parts WHERE source='SEED' GROUP BY 1 ORDER BY 1",
    )
    .fetch_all(db)
    .await
    .unwrap();
    assert_eq!(counts.len(), 7, "{counts:?}");
    // Mic also lists audio interfaces and mixers (0012, Joe's decision of 5:52 PM ET); everything else is 30-80.
    let interfaces = env
        .count("SELECT count(*) FROM parts WHERE source='SEED' AND kind='AUDIO_INTERFACE'")
        .await;
    assert!(interfaces >= 30, "{interfaces} interfaces");
    assert_eq!(
        env.count("SELECT count(*) FROM parts WHERE kind IS NOT NULL AND category<>'MIC'")
            .await,
        0
    );
    for (category, n) in &counts {
        assert!(sver::parts::CATEGORIES.contains(&category.as_str()));
        let n = if category == "MIC" {
            n - interfaces
        } else {
            *n
        };
        assert!((30..=80).contains(&n), "{category}: {n} seed parts");
    }
    let stored: Vec<(String, String, String)> =
        sqlx::query_as("SELECT brand,model,norm FROM parts")
            .fetch_all(db)
            .await
            .unwrap();
    for (brand, model, norm) in &stored {
        assert_eq!(&sver::text::part_norm(&format!("{brand} {model}")), norm);
    }
    assert_eq!(sver::text::part_norm("  Elgato Wave:3 "), "elgato wave 3");
    assert_eq!(sver::text::part_norm("Shure MV7+"), "shure mv7 plus");

    // Search: signed in only, every typed word, compact forms, popular parts for an empty box.
    let (owner_id, owner) = env.user("PartsOwner", true).await;
    assert_eq!(
        env.anon
            .status("GET", "/api/parts?category=GPU&q=4070", Value::Null)
            .await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        owner
            .status("GET", "/api/parts?category=CASE&q=", Value::Null)
            .await,
        StatusCode::BAD_REQUEST
    );
    let names = |v: &Value| -> Vec<String> {
        v["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_string())
            .collect()
    };
    let gpus = owner
        .ok("GET", "/api/parts?category=GPU&q=rtx%204070", Value::Null)
        .await;
    assert_eq!(names(&gpus)[0], "NVIDIA GeForce RTX 4070", "{gpus}");
    assert!(names(&gpus).iter().all(|n| n.contains("4070")));
    let compact = owner
        .ok("GET", "/api/parts?category=GPU&q=RTX4070", Value::Null)
        .await;
    assert_eq!(names(&compact)[0], "NVIDIA GeForce RTX 4070");
    let popular = owner
        .ok("GET", "/api/parts?category=MIC&q=", Value::Null)
        .await;
    assert_eq!(names(&popular).len(), 10);
    assert_eq!(names(&popular)[0], "Shure SM7B");
    let plus = owner
        .ok("GET", "/api/parts?category=MIC&q=mv7%2B", Value::Null)
        .await;
    assert_eq!(names(&plus)[0], "Shure MV7+");
    let goxlr = owner
        .ok("GET", "/api/parts?category=MIC&q=goxlr", Value::Null)
        .await;
    assert_eq!(
        names(&goxlr)[..2],
        ["TC-Helicon GoXLR", "TC-Helicon GoXLR Mini"],
        "{goxlr}"
    );
    assert_eq!(goxlr["parts"][0]["kind"], "AUDIO_INTERFACE");
    assert_eq!(
        popular["parts"][0]["kind"],
        Value::Null,
        "mics have no kind"
    );
    assert_eq!(
        names(
            &owner
                .ok(
                    "GET",
                    "/api/parts?category=MIC&q=scarlett%202i2",
                    Value::Null
                )
                .await
        )[0],
        "Focusrite Scarlett 2i2 (4th Gen)"
    );
    let none = owner
        .ok("GET", "/api/parts?category=CPU&q=zzzz", Value::Null)
        .await;
    assert!(names(&none).is_empty());
    let gpu_id = gpus["parts"][0]["id"].as_str().unwrap().to_string();

    // Saving: a picked part takes the list's name; a typed exact match links itself; anything
    // else is a custom entry, saved as typed and queued for review.
    let setup = owner.ok("GET", "/api/me/setup", Value::Null).await;
    assert_eq!(
        setup["categories"],
        json!([
            "CPU",
            "GPU",
            "RAM",
            "MOTHERBOARD",
            "CAMERA",
            "MIC",
            "PERIPHERALS"
        ])
    );
    let bad = |items: Value| json!({"items": items});
    for items in [
        json!([{"category": "CPU", "name": "x", "part_id": gpu_id}]),
        json!([{"category": "GPU", "name": "x", "part_id": "part_missing"}]),
        json!([{"category": "OTHER", "name": "Ring light"}]),
        json!([{"category": "CASE", "name": "Big case"}]),
    ] {
        assert_eq!(
            owner.status("PUT", "/api/me/setup", bad(items)).await,
            StatusCode::BAD_REQUEST
        );
    }
    owner
        .ok(
            "PUT",
            "/api/me/setup",
            json!({"items": [
                {"category": "GPU", "name": "my card", "part_id": gpu_id},
                {"category": "MIC", "name": "shure  sm7b"},
                {"category": "CAMERA", "name": "My Custom Cam 9000", "note": "On a tripod"},
            ], "revision": setup["revision"]}),
        )
        .await;
    let mine = owner.ok("GET", "/api/me/setup", Value::Null).await;
    let items = mine["items"].as_array().unwrap();
    assert_eq!(items[0]["name"], "NVIDIA GeForce RTX 4070");
    assert_eq!(items[0]["part_id"], gpu_id.as_str());
    assert_eq!(items[0]["review"], Value::Null);
    assert_eq!(items[1]["name"], "Shure SM7B");
    assert!(items[1]["part_id"].is_string());
    assert_eq!(items[2]["name"], "My Custom Cam 9000");
    assert_eq!(items[2]["part_id"], Value::Null);
    assert_eq!(items[2]["review"], "PENDING");
    let about = env
        .anon
        .ok("GET", "/api/channels/PartsOwner/about", Value::Null)
        .await;
    assert_eq!(about["setup"].as_array().unwrap().len(), 3);
    assert_eq!(about["setup"][2]["name"], "My Custom Cam 9000");
    assert_eq!(about["setup"][2]["note"], "On a tripod");
    assert!(
        about["setup"][2].get("review").is_none(),
        "Review state is private"
    );

    // Another user typing the same name (any spacing or case) shares the one queue entry.
    let (other_id, other) = env.user("PartsOther", true).await;
    other
        .ok(
            "PUT",
            "/api/me/setup",
            json!({"items": [{"category": "CAMERA", "name": "my custom  cam-9000"}]}),
        )
        .await;
    assert_eq!(
        env.count("SELECT count(*) FROM part_submissions WHERE norm='my custom cam 9000'")
            .await,
        1
    );

    // The queue: staff with MFA only (404 otherwise), step-up for decisions.
    assert_eq!(
        owner.status("GET", "/api/admin/parts", Value::Null).await,
        StatusCode::NOT_FOUND
    );
    let (_, stale, staff_client) = staff(env, "PartsStaff").await;
    let queue = staff_client
        .ok("GET", "/api/admin/parts", Value::Null)
        .await;
    let entry = queue["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "My Custom Cam 9000")
        .cloned()
        .expect("custom entry is queued");
    assert_eq!(entry["uses"], 2);
    assert_eq!(entry["submitted_by"], "PartsOwner");
    assert_eq!(entry["category"], "CAMERA");
    let id = entry["id"].as_str().unwrap().to_string();
    let decide = format!("/api/admin/parts/{id}/decision");
    assert_eq!(
        stale
            .status(
                "POST",
                &decide,
                json!({"decision": "approve", "brand": "Custom", "model": "Cam 9000"})
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        staff_client
            .status(
                "POST",
                &decide,
                json!({"decision": "approve", "brand": "", "model": "Cam 9000"})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    let approved = staff_client
        .ok(
            "POST",
            &decide,
            json!({"decision": "approve", "brand": "Custom", "model": "Cam 9000"}),
        )
        .await;
    assert_eq!(approved["linked"], 2);
    assert_eq!(approved["added"], true);
    assert_eq!(
        staff_client
            .status("POST", &decide, json!({"decision": "dismiss"}))
            .await,
        StatusCode::CONFLICT
    );
    let mine = owner.ok("GET", "/api/me/setup", Value::Null).await;
    assert_eq!(mine["items"][2]["name"], "Custom Cam 9000");
    assert!(mine["items"][2]["part_id"].is_string());
    assert_eq!(mine["items"][2]["review"], Value::Null);
    assert_eq!(mine["items"][2]["note"], "On a tripod");
    let found = owner
        .ok(
            "GET",
            "/api/parts?category=CAMERA&q=cam%209000",
            Value::Null,
        )
        .await;
    assert_eq!(names(&found)[0], "Custom Cam 9000");
    assert_eq!(
        env.count("SELECT count(*) FROM moderation_actions WHERE action='part_approved' AND target_type='part_submission'").await,
        1
    );
    let approved_list = staff_client
        .ok("GET", "/api/admin/parts?status=APPROVED", Value::Null)
        .await;
    assert_eq!(approved_list["items"][0]["part"], "Custom Cam 9000");
    assert_eq!(approved_list["items"][0]["reviewed_by"], "PartsStaff");

    // Dismissed: the entry stays on the setup as typed and the name is never queued again.
    let keep: Vec<Value> = mine["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| json!({"category": i["category"], "name": i["name"], "note": i["note"], "link": i["link"], "part_id": i["part_id"]}))
        .collect();
    let mut with_socks = keep.clone();
    with_socks.push(json!({"category": "PERIPHERALS", "name": "Lucky Socks"}));
    owner
        .ok("PUT", "/api/me/setup", json!({"items": with_socks}))
        .await;
    let queue = staff_client
        .ok("GET", "/api/admin/parts", Value::Null)
        .await;
    let socks = queue["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "Lucky Socks")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    staff_client
        .ok(
            "POST",
            &format!("/api/admin/parts/{socks}/decision"),
            json!({"decision": "dismiss"}),
        )
        .await;
    let mine = owner.ok("GET", "/api/me/setup", Value::Null).await;
    assert_eq!(mine["items"][3]["name"], "Lucky Socks");
    assert_eq!(mine["items"][3]["review"], "DISMISSED");
    other
        .ok(
            "PUT",
            "/api/me/setup",
            json!({"items": [{"category": "PERIPHERALS", "name": "lucky socks"}]}),
        )
        .await;
    assert_eq!(
        env.count("SELECT count(*) FROM part_submissions WHERE norm='lucky socks'")
            .await,
        1
    );
    assert_eq!(
        other.ok("GET", "/api/me/setup", Value::Null).await["items"][0]["review"],
        "DISMISSED"
    );

    // Entries from before the picker: kept, editable and removable; "Other" can't grow.
    let (legacy_id, legacy) = env.user("PartsLegacy", true).await;
    env.sql(sqlx::AssertSqlSafe(format!("INSERT INTO setup_items(id,user_id,position,category,name,note,link,legacy_category) VALUES('{}','{legacy_id}',0,'OTHER','Ring light','Left side',NULL,'LIGHTING'),('{}','{legacy_id}',1,'PERIPHERALS','Old Headset','',NULL,'HEADPHONES'),('{}','{legacy_id}',2,'MIC','Blue Yeti','','https://legacy.example/mic','MICROPHONE'),('{}','{legacy_id}',3,'MIC','Old Interface Box','',NULL,'AUDIO_INTERFACE')", uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4()))).await;
    let before = legacy.ok("GET", "/api/me/setup", Value::Null).await;
    assert_eq!(before["items"][0]["category"], "OTHER");
    assert_eq!(before["items"][0]["legacy_category"], "LIGHTING");
    let back: Vec<Value> = before["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| json!({"category": i["category"], "name": i["name"], "note": i["note"], "link": i["link"]}))
        .collect();
    legacy
        .ok("PUT", "/api/me/setup", json!({"items": back.clone()}))
        .await;
    let after = legacy.ok("GET", "/api/me/setup", Value::Null).await;
    assert_eq!(after["items"][0]["name"], "Ring light");
    assert_eq!(after["items"][0]["note"], "Left side");
    assert_eq!(after["items"][0]["legacy_category"], "LIGHTING");
    assert_eq!(after["items"][1]["legacy_category"], "HEADPHONES");
    assert_eq!(
        after["items"][2]["name"], "Blue Yeti",
        "An exact list match links"
    );
    assert!(after["items"][2]["part_id"].is_string());
    assert_eq!(after["items"][2]["link"], "https://legacy.example/mic");
    // Entries from the old AUDIO_INTERFACE category show as interfaces under Mic, before and after a save;
    // a picked interface takes its kind from the list.
    assert_eq!(before["items"][3]["kind"], "AUDIO_INTERFACE");
    assert_eq!(after["items"][3]["category"], "MIC");
    assert_eq!(after["items"][3]["legacy_category"], "AUDIO_INTERFACE");
    assert_eq!(after["items"][3]["kind"], "AUDIO_INTERFACE");
    assert_eq!(after["items"][2]["kind"], Value::Null);
    let legacy_name: String = sqlx::query_scalar("SELECT username FROM users WHERE id=$1")
        .bind(&legacy_id)
        .fetch_one(db)
        .await
        .unwrap();
    let public = env
        .anon
        .ok(
            "GET",
            &format!("/api/channels/{legacy_name}/about"),
            Value::Null,
        )
        .await;
    let kinds: Vec<&Value> = public["setup"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["kind"])
        .collect();
    assert_eq!(kinds[3], "AUDIO_INTERFACE", "{public}");
    let goxlr_id = goxlr["parts"][0]["id"].as_str().unwrap();
    let mut with_mixer = back.clone();
    with_mixer.push(json!({"category": "MIC", "name": "ignored", "part_id": goxlr_id}));
    legacy
        .ok("PUT", "/api/me/setup", json!({"items": with_mixer}))
        .await;
    let mixed = legacy.ok("GET", "/api/me/setup", Value::Null).await;
    let mixer = mixed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["part_id"] == goxlr_id)
        .unwrap();
    assert_eq!(mixer["name"], "TC-Helicon GoXLR");
    assert_eq!(mixer["kind"], "AUDIO_INTERFACE");
    assert_eq!(
        after["items"][1]["review"], "PENDING",
        "Kept custom entries are queued on save"
    );
    let mut renamed = back.clone();
    renamed[0]["name"] = json!("Ring light 2");
    assert_eq!(
        legacy
            .status("PUT", "/api/me/setup", json!({"items": renamed}))
            .await,
        StatusCode::BAD_REQUEST
    );
    legacy
        .ok("PUT", "/api/me/setup", json!({"items": back[1..].to_vec()}))
        .await;
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM setup_items WHERE user_id='{legacy_id}' AND category='OTHER'"
        )))
        .await,
        0
    );

    // Flood limit: past 30 new entries a day, custom entries still save but aren't queued.
    env.sql(sqlx::AssertSqlSafe(format!("INSERT INTO part_submissions(id,category,name,norm,submitted_by) SELECT 'flood'||g,'CPU','Flood '||g,'flood '||g,'{other_id}' FROM generate_series(1,30) g"))).await;
    other
        .ok(
            "PUT",
            "/api/me/setup",
            json!({"items": [{"category": "CPU", "name": "Prototype Chip X"}]}),
        )
        .await;
    assert_eq!(
        other.ok("GET", "/api/me/setup", Value::Null).await["items"][0]["review"],
        Value::Null
    );
    assert_eq!(
        env.count("SELECT count(*) FROM part_submissions WHERE norm='prototype chip x'")
            .await,
        0
    );

    // Erasure: a user's queue entries go with the account; catalog parts and others' entries stay.
    env.sql(sqlx::AssertSqlSafe(format!(
        "DELETE FROM users WHERE id='{owner_id}'"
    )))
    .await;
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM part_submissions WHERE submitted_by='{owner_id}'"
        )))
        .await,
        0
    );
    assert_eq!(
        env.count("SELECT count(*) FROM parts WHERE norm='custom cam 9000'")
            .await,
        1
    );
    assert_eq!(
        other.ok("GET", "/api/me/setup", Value::Null).await["items"][0]["name"],
        "Prototype Chip X"
    );
    env.sql("DELETE FROM part_submissions WHERE id LIKE 'flood%'")
        .await;
    println!(
        "Passed: parts picker (seed list, search, picked and custom entries, review queue, kept entries, flood limit, erasure)."
    );
}

/// Migration 0011 keeps every setup entry made before the parts picker: name, note and link
/// unchanged, mapped to the new categories, with the old category kept in `legacy_category`.
#[tokio::test]
async fn setup_parts_migration_keeps_entries() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("Use scripts/dev.ps1 test with an isolated local database");
    let parsed = url::Url::parse(&database_url).unwrap();
    assert!(
        matches!(parsed.host_str(), Some("localhost" | "127.0.0.1"))
            && parsed.path() == "/sver_rebuild",
        "Tests require the isolated local sver_rebuild database"
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .unwrap();
    let schema = format!("parts_migration_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(move |connection, _| {
            let statement = search_path.clone();
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(statement))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&database_url)
        .await
        .unwrap();
    let pool = db.clone();
    let result = tokio::spawn(async move {
        let db = pool;
        let mut before = sqlx::migrate!("../../../../migrations");
        before.migrations = before
            .migrations
            .iter()
            .filter(|m| m.version < 11)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        before.run(&db).await.unwrap();
        let user = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id,email,username,email_verified,created_at,date_of_birth) VALUES($1,'parts-migration@example.invalid','PartsMigration',true,now(),'1995-01-01')")
            .bind(&user)
            .execute(&db)
            .await
            .unwrap();
        let old = [
            ("CAMERA", "CAMERA"),
            ("MICROPHONE", "MIC"),
            ("AUDIO_INTERFACE", "MIC"),
            ("HEADPHONES", "PERIPHERALS"),
            ("PC", "OTHER"),
            ("CPU", "CPU"),
            ("GPU", "GPU"),
            ("CAPTURE", "PERIPHERALS"),
            ("LIGHTING", "OTHER"),
            ("MONITOR", "PERIPHERALS"),
            ("KEYBOARD", "PERIPHERALS"),
            ("MOUSE", "PERIPHERALS"),
            ("CONTROLLER", "PERIPHERALS"),
            ("OTHER", "OTHER"),
        ];
        for (i, (category, _)) in old.iter().enumerate() {
            sqlx::query("INSERT INTO setup_items(id,user_id,position,category,name,note,link) VALUES($1,$2,$3,$4,$5,'note','https://example.com/x')")
                .bind(format!("item{i}"))
                .bind(&user)
                .bind(i as i32)
                .bind(category)
                .bind(format!("Thing {category}"))
                .execute(&db)
                .await
                .unwrap();
        }
        sqlx::migrate!("../../../../migrations").run(&db).await.unwrap();
        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT category,name,note,link,legacy_category FROM setup_items ORDER BY position",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(rows.len(), old.len());
        for ((was, now), (category, name, note, link, legacy)) in old.iter().zip(&rows) {
            assert_eq!(category, now, "{was}");
            assert_eq!(name, &format!("Thing {was}"));
            assert_eq!(note, "note");
            assert_eq!(link.as_deref(), Some("https://example.com/x"));
            assert_eq!(
                legacy.as_deref(),
                (was != now).then_some(*was),
                "{was} keeps its old category"
            );
        }
    })
    .await;
    db.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    assert!(
        result.is_ok(),
        "Setup migration check failed; its isolated schema was removed"
    );
}

async fn renames(env: &Env) {
    let (id, renamer) = env.user("OldHandle", true).await;
    let (other_id, other) = env.user("Bystander", true).await;
    for (name, field_error) in [
        ("ab", true),
        ("_edge", true),
        ("12345", true),
        ("has space", true),
        ("admin", false),
        ("Bystander", false),
    ] {
        let (s, v) = renamer
            .call("POST", "/api/me/username", json!({"username": name}))
            .await;
        if field_error {
            assert_eq!(
                (s, v["field"].as_str()),
                (StatusCode::BAD_REQUEST, Some("username")),
                "{name}: {v}"
            );
        } else {
            assert_eq!(
                (s, v["error"].as_str()),
                (StatusCode::CONFLICT, Some("That username isn't available.")),
                "{name}"
            );
        }
    }
    // Case-only changes are free and leave no hold.
    assert_eq!(
        renamer
            .ok("POST", "/api/me/username", json!({"username": "oldhandle"}))
            .await["hold"],
        false
    );
    renamer
        .ok("POST", "/api/me/username", json!({"username": "OldHandle"}))
        .await;
    let renamed = renamer
        .ok("POST", "/api/me/username", json!({"username": "NewHandle"}))
        .await;
    assert_eq!(renamed["hold"], true);
    // The old name redirects for 30 days, is unavailable to others and at signup.
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/oldhandle/resolve", Value::Null)
            .await["redirect_to"],
        "NewHandle"
    );
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/OldHandle", Value::Null)
            .await["redirect_to"],
        "NewHandle"
    );
    assert_eq!(
        other
            .call("POST", "/api/me/username", json!({"username": "OldHandle"}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let availability = env
        .anon
        .ok(
            "GET",
            "/api/auth/username-availability?username=OldHandle",
            Value::Null,
        )
        .await;
    assert_eq!(availability["available"], false, "{availability}");
    // The 60-day interval, with the date in the requested zone.
    let (s, v) = renamer
        .call(
            "POST",
            "/api/me/username",
            json!({"username": "ThirdHandle", "timezone": "America/New_York"}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .starts_with("You can change your username again on "),
        "{v}"
    );
    // The one-time switch back to the held name ends the hold.
    assert_eq!(
        renamer
            .ok("POST", "/api/me/username", json!({"username": "OldHandle"}))
            .await["username"],
        "OldHandle"
    );
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM username_holds WHERE user_id='{id}'"
        )))
        .await,
        0
    );
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM username_history WHERE user_id='{id}' AND reason='revert'"
        )))
        .await,
        1
    );
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/NewHandle", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    // After the interval the rename works; once the hold expires the name is free again.
    sqlx::query("UPDATE username_history SET changed_at=now()-interval '61 days' WHERE user_id=$1")
        .bind(&id)
        .execute(&env.app.db)
        .await
        .unwrap();
    renamer
        .ok(
            "POST",
            "/api/me/username",
            json!({"username": "FinalHandle"}),
        )
        .await;
    sqlx::query("UPDATE username_holds SET released_at=now()-interval '1 minute' WHERE user_id=$1")
        .bind(&id)
        .execute(&env.app.db)
        .await
        .unwrap();
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/OldHandle", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other
            .ok("POST", "/api/me/username", json!({"username": "OldHandle"}))
            .await["username"],
        "OldHandle"
    );
    // Stale sign-in blocks the change.
    sqlx::query(
        "UPDATE sessions SET authenticated_at=now()-interval '10 minutes' WHERE user_id=$1",
    )
    .bind(&other_id)
    .execute(&env.app.db)
    .await
    .unwrap();
    sqlx::query("UPDATE username_history SET changed_at=now()-interval '61 days' WHERE user_id=$1")
        .bind(&other_id)
        .execute(&env.app.db)
        .await
        .unwrap();
    assert_eq!(
        other
            .status(
                "POST",
                "/api/me/username",
                json!({"username": "AnotherOne"})
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert!(
        renamer
            .ok("GET", "/api/me/rename-status", Value::Null)
            .await
            .is_object()
    );
    env.reset_limits().await;
    println!(
        "Passed: rename format rules, free case changes, 30-day holds and redirects, the 60-day interval, the one-time revert and hold expiry."
    );
}
async fn staff(env: &Env, name: &str) -> (String, Client, Client) {
    let (id, plain) = env.user(name, true).await;
    sqlx::query(
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic-sealed-secret' WHERE id=$1",
    )
    .bind(&id)
    .execute(&env.app.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO staff_roles(user_id,role) VALUES($1,'admin')")
        .bind(&id)
        .execute(&env.app.db)
        .await
        .unwrap();
    let _ = plain;
    // A verified session whose primary sign-in is older than the 5-minute step-up window.
    let stale = env.session(&id, true).await;
    sqlx::query(
        "UPDATE sessions SET authenticated_at=now()-interval '10 minutes' WHERE token_hash=$1",
    )
    .bind(sec::digest(stale.cookie.as_ref().unwrap()))
    .execute(&env.app.db)
    .await
    .unwrap();
    let verified = env.session(&id, true).await;
    (id, stale, verified)
}

async fn safety(env: &Env) {
    let (offender_id, offender) = env.user("Offender", true).await;
    let (_, reporter) = env.user("Reporter_One", true).await;
    let (_, host) = env.user("WallHost", true).await;
    let (_, regular) = env.user("RegularUser", true).await;
    offender
        .ok(
            "PATCH",
            "/api/me/profile",
            json!({"bio": "Synthetic reportable bio"}),
        )
        .await;
    let post = offender
        .ok(
            "POST",
            "/api/channels/WallHost/wall",
            json!({"body": "Synthetic reportable post"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    // Reports: validation, own content, repeat reports collapse.
    assert_eq!(reporter.status("POST", "/api/reports", json!({"target_type": "profile", "target_id": "Offender", "field": "bio", "reason": "nope"})).await, StatusCode::BAD_REQUEST);
    assert_eq!(reporter.status("POST", "/api/reports", json!({"target_type": "profile", "target_id": "Offender", "field": "email", "reason": "spam"})).await, StatusCode::BAD_REQUEST);
    assert_eq!(
        offender
            .status(
                "POST",
                "/api/reports",
                json!({"target_type": "wall_post", "target_id": post, "reason": "spam"})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        reporter
            .status(
                "POST",
                "/api/reports",
                json!({"target_type": "wall_post", "target_id": "missing", "reason": "spam"})
            )
            .await,
        StatusCode::NOT_FOUND
    );
    reporter.ok("POST", "/api/reports", json!({"target_type": "profile", "target_id": "Offender", "field": "bio", "reason": "harassment", "note": "first"})).await;
    reporter.ok("POST", "/api/reports", json!({"target_type": "profile", "target_id": "Offender", "field": "bio", "reason": "harassment", "note": "second"})).await;
    reporter
        .ok(
            "POST",
            "/api/reports",
            json!({"target_type": "wall_post", "target_id": post, "reason": "spam"}),
        )
        .await;
    host.ok(
        "POST",
        "/api/reports",
        json!({"target_type": "wall_post", "target_id": post, "reason": "harassment"}),
    )
    .await;
    assert_eq!(
        env.count("SELECT count(*) FROM reports WHERE status='OPEN'")
            .await,
        3
    );
    assert_eq!(
        reporter.ok("GET", "/api/me/reports", Value::Null).await["reports"][0]["status"],
        "under_review"
    );

    // Admin access: identical 404 for everyone but staff with MFA; writes need a verified fresh session.
    let missing = env
        .anon
        .call("GET", "/api/admin/reports", Value::Null)
        .await;
    assert_eq!(missing.0, StatusCode::NOT_FOUND);
    assert_eq!(
        regular.call("GET", "/api/admin/reports", Value::Null).await,
        missing
    );
    assert_eq!(
        regular
            .call("GET", "/api/admin/not-a-route", Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (no_mfa_id, no_mfa) = env.user("StaffNoMfa", true).await;
    sqlx::query("INSERT INTO staff_roles(user_id,role) VALUES($1,'admin')")
        .bind(&no_mfa_id)
        .execute(&env.app.db)
        .await
        .unwrap();
    assert_eq!(
        no_mfa.call("GET", "/api/admin/reports", Value::Null).await,
        missing,
        "Staff without MFA get the same 404"
    );
    let (_, mod_a_plain, mod_a) = staff(env, "ModAlpha").await;
    let (_, _, mod_b) = staff(env, "ModBravo").await;
    let queue = mod_a_plain
        .ok("GET", "/api/admin/reports", Value::Null)
        .await;
    let groups = queue["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let post_group = groups
        .iter()
        .find(|g| g["target_type"] == "wall_post")
        .unwrap();
    assert_eq!(post_group["count"], 2);
    assert_eq!(
        mod_a_plain
            .status(
                "POST",
                &format!("/api/admin/reports/wall_post/{post}/actions"),
                json!({"action": "dismiss", "note": "x"})
            )
            .await,
        StatusCode::FORBIDDEN,
        "Writes need a fresh step-up"
    );
    assert_eq!(
        mod_a
            .status(
                "POST",
                &format!("/api/admin/reports/wall_post/{post}/actions"),
                json!({"action": "dismiss"})
            )
            .await,
        StatusCode::BAD_REQUEST,
        "A note is required"
    );
    assert_eq!(mod_a.status("POST", &format!("/api/admin/reports/wall_post/{post}/actions"), json!({"action": "dismiss", "note": "x", "strike": {"reason": "spam", "severity": "STANDARD"}})).await, StatusCode::BAD_REQUEST);

    // Remove the post with a strike: level 1 warning, reports closed with reporter feedback.
    let result = mod_a.ok("POST", &format!("/api/admin/reports/wall_post/{post}/actions"), json!({"action": "remove_content", "note": "Synthetic removal", "strike": {"reason": "spam", "severity": "STANDARD", "message_to_user": "Please keep it friendly."}})).await;
    assert_eq!(result["closed_reports"], 2);
    let strike_one = result["strike_id"].as_str().unwrap().to_string();
    let removed_body = env
        .anon
        .ok("GET", "/api/channels/WallHost/wall", Value::Null)
        .await;
    assert!(
        !removed_body
            .to_string()
            .contains("Synthetic reportable post")
    );
    let feedback = reporter.ok("GET", "/api/me/reports", Value::Null).await;
    assert!(
        feedback["reports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["status"] == "action_taken" && r["unread"] == true)
    );
    assert!(
        !feedback.to_string().contains("ModAlpha"),
        "Reporters never learn who acted"
    );
    assert!(
        reporter.ok("GET", "/api/me/alerts", Value::Null).await["unread_reports"]
            .as_i64()
            .unwrap()
            >= 1
    );
    reporter
        .ok("POST", "/api/me/reports/seen", Value::Null)
        .await;
    assert_eq!(
        reporter.ok("GET", "/api/me/alerts", Value::Null).await["unread_reports"],
        0
    );
    let standing = offender.ok("GET", "/api/me/standing", Value::Null).await;
    assert_eq!(standing["level"], 1);
    assert!(standing["restriction"].is_null());
    assert!(!standing.to_string().contains("ModAlpha"));
    assert_eq!(
        offender.ok("GET", "/api/me/alerts", Value::Null).await["new_strikes"],
        1
    );
    offender
        .ok(
            "POST",
            &format!("/api/me/strikes/{strike_one}/acknowledge"),
            Value::Null,
        )
        .await;
    assert_eq!(
        offender.ok("GET", "/api/me/alerts", Value::Null).await["new_strikes"],
        0
    );
    assert_eq!(
        offender
            .status(
                "POST",
                &format!("/api/me/strikes/{strike_one}/appeal"),
                json!({"body": ""})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    offender
        .ok(
            "POST",
            &format!("/api/me/strikes/{strike_one}/appeal"),
            json!({"body": "I think this was a mistake."}),
        )
        .await;
    let (s, v) = offender
        .call(
            "POST",
            &format!("/api/me/strikes/{strike_one}/appeal"),
            json!({"body": "Again"}),
        )
        .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (
            StatusCode::CONFLICT,
            Some("You've already appealed this strike.")
        )
    );
    assert_eq!(
        reporter
            .status(
                "POST",
                &format!("/api/me/strikes/{strike_one}/appeal"),
                json!({"body": "Not mine"})
            )
            .await,
        StatusCode::NOT_FOUND
    );

    // Reset a reported field; a second standard strike reaches level 2 (72 hours, channel hidden).
    let profile_target = offender_id.clone();
    mod_a
        .ok(
            "POST",
            &format!("/api/admin/reports/profile/{profile_target}/actions"),
            json!({"action": "reset_field", "note": "Synthetic reset"}),
        )
        .await;
    assert_eq!(
        env.anon
            .ok("GET", "/api/channels/Offender", Value::Null)
            .await["channel"]["bio"],
        ""
    );
    let second = mod_b
        .ok(
            "POST",
            "/api/admin/users/Offender/strikes",
            json!({"reason": "harassment", "severity": "STANDARD", "note": "Direct strike"}),
        )
        .await;
    assert_eq!(second["level"], 2);
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/Offender", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        offender
            .status("PATCH", "/api/me/profile", json!({"bio": "x"}))
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        offender
            .status("POST", "/api/channels/WallHost/wall", json!({"body": "x"}))
            .await,
        StatusCode::FORBIDDEN
    );
    assert!(
        offender.ok("GET", "/api/me/standing", Value::Null).await["restriction"]["until"]
            .is_string()
    );
    let admin_view = mod_a_plain
        .ok("GET", "/api/admin/users/Offender/standing", Value::Null)
        .await;
    assert_eq!(admin_view["strikes"].as_array().unwrap().len(), 2);
    assert_eq!(
        mod_a_plain
            .status("GET", "/api/admin/users/Nobody_Here/standing", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mod_a
            .status(
                "POST",
                "/api/admin/users/support/strikes",
                json!({"reason": "spam", "severity": "STANDARD", "note": "x"})
            )
            .await,
        StatusCode::BAD_REQUEST
    );

    // Appeals: the issuer can't decide while another moderator exists; overturn restores content.
    let appeals = mod_a_plain
        .ok("GET", "/api/admin/appeals", Value::Null)
        .await;
    let appeal_id = appeals["appeals"][0]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{appeals}"))
        .to_string();
    assert_eq!(
        mod_a
            .status(
                "POST",
                &format!("/api/admin/appeals/{appeal_id}/decision"),
                json!({"outcome": "overturned", "staff_note": "self"})
            )
            .await,
        StatusCode::FORBIDDEN
    );
    mod_b.ok("POST", &format!("/api/admin/appeals/{appeal_id}/decision"), json!({"outcome": "overturned", "staff_note": "Second look", "message_to_user": "We reversed this."})).await;
    assert_eq!(
        mod_b
            .status(
                "POST",
                &format!("/api/admin/appeals/{appeal_id}/decision"),
                json!({"outcome": "upheld", "staff_note": "again"})
            )
            .await,
        StatusCode::CONFLICT
    );
    assert!(
        env.anon
            .ok("GET", "/api/channels/WallHost/wall", Value::Null)
            .await
            .to_string()
            .contains("Synthetic reportable post"),
        "Overturning restores removed content"
    );
    let (s, v) = offender
        .call(
            "POST",
            &format!("/api/me/strikes/{strike_one}/appeal"),
            json!({"body": "Again"}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    mod_a
        .ok(
            "POST",
            "/api/admin/users/Offender/restriction/lift",
            json!({"note": "Synthetic lift"}),
        )
        .await;
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/Offender", Value::Null)
            .await,
        StatusCode::OK
    );

    // Interim restrictions: 1-24 hours, never internal accounts, lifted early by staff.
    assert_eq!(
        mod_a
            .status(
                "POST",
                "/api/admin/users/RegularUser/interim-restriction",
                json!({"hours": 25, "note": "x"})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        mod_a
            .status(
                "POST",
                "/api/admin/users/support/interim-restriction",
                json!({"hours": 2, "note": "x"})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    mod_a
        .ok(
            "POST",
            "/api/admin/users/RegularUser/interim-restriction",
            json!({"hours": 2, "note": "Synthetic interim"}),
        )
        .await;
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/RegularUser", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mod_a_plain
            .ok("GET", "/api/admin/reports", Value::Null)
            .await["interim_restrictions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    mod_a
        .ok(
            "DELETE",
            "/api/admin/users/RegularUser/interim-restriction",
            json!({"note": "Synthetic interim lift"}),
        )
        .await;
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/RegularUser", Value::Null)
            .await,
        StatusCode::OK
    );
    let audit = mod_a_plain
        .ok("GET", "/api/admin/moderation-actions", Value::Null)
        .await;
    assert!(audit.to_string().contains("strike_issued"), "{audit}");
    // Severe strikes jump to the indefinite level; the scheduled tick keeps the restriction cache.
    mod_a
        .ok(
            "POST",
            "/api/admin/users/RegularUser/strikes",
            json!({"reason": "hate", "severity": "SEVERE", "note": "Synthetic severe"}),
        )
        .await;
    sver::safety::tick(&env.app).await.unwrap();
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/RegularUser", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    assert!(
        env.count("SELECT count(*) FROM mail_jobs").await >= 1,
        "Standing and report notices use the mail queue"
    );
    env.reset_limits().await;
    println!(
        "Passed: reports, admin 404 parity and step-up, removal with strikes, reporter feedback, appeals with a second reviewer, restrictions and the audit log."
    );
}
async fn erasure(env: &Env) {
    let (gone_id, gone) = env.user("EraseMe", true).await;
    let (_, keeper) = env.user("CouncilKeeper", true).await;
    env.user("Member_A", true).await;
    env.user("Member_B", true).await;
    gone.ok("PUT", "/api/follows/CouncilKeeper", Value::Null)
        .await;
    keeper
        .ok(
            "PUT",
            "/api/me/war-council",
            json!({"members": ["Member_A", "EraseMe", "Member_B"]}),
        )
        .await;
    gone.ok(
        "POST",
        "/api/channels/CouncilKeeper/wall",
        json!({"body": "Leaving soon"}),
    )
    .await;
    let (s, _) = gone
        .upload("/api/me/avatar", &[], Some(&png(300, 300)))
        .await;
    assert_eq!(s, StatusCode::OK);
    let mut tx = env.app.db.begin().await.unwrap();
    sver::profile_jobs::erase(&mut tx, &gone_id).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM users WHERE id='{gone_id}'"
        )))
        .await,
        0
    );
    let channel = env
        .anon
        .ok("GET", "/api/channels/CouncilKeeper", Value::Null)
        .await;
    assert_eq!(
        channel["channel"]["follower_count"], 0,
        "Counts are recomputed"
    );
    let names: Vec<&str> = channel["war_council"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["user"]["username"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Member_A", "Member_B"],
        "War Councils are compacted"
    );
    assert_eq!(env.count("SELECT max(w.position)::bigint FROM war_council w JOIN users u ON u.id=w.user_id WHERE u.username='CouncilKeeper'").await, 2);
    assert!(!channel.to_string().contains("Leaving soon"));
    assert_eq!(
        env.anon
            .status("GET", "/api/channels/EraseMe/resolve", Value::Null)
            .await,
        StatusCode::NOT_FOUND,
        "Erased names never redirect"
    );
    assert_eq!(
        env.anon
            .ok(
                "GET",
                "/api/auth/username-availability?username=EraseMe",
                Value::Null
            )
            .await["available"],
        false,
        "The name is held for 30 days"
    );
    assert!(
        env.count("SELECT count(*) FROM media_objects WHERE delete_after IS NOT NULL")
            .await
            >= 3
    );
    // The scheduled job runs the whole Module 2 sweep without error.
    sver::jobs::tick(&env.app).await.unwrap();
    println!(
        "Passed: erasure removes the account, recomputes counts, compacts councils, holds the name without a redirect and queues media."
    );
}

/// Digests of the tables the profile import must never change.
async fn preserved_digest(env: &Env) -> (String, String, String) {
    sqlx::query_as("SELECT (SELECT md5(coalesce(string_agg(t::text, '|' ORDER BY t.id COLLATE \"C\"), '')) FROM users t), (SELECT md5(coalesce(string_agg(t::text, '|' ORDER BY t.provider COLLATE \"C\", t.subject COLLATE \"C\"), '')) FROM identities t), (SELECT md5(coalesce(string_agg(t::text, '|' ORDER BY t.user_id COLLATE \"C\"), '')) FROM legacy_account_data t)")
        .fetch_one(&env.app.db)
        .await
        .unwrap()
}

fn legacy_export(ids: &HashMap<&str, String>) -> profile_import::Export {
    let id = |k: &str| ids[k].clone();
    let t = |s: &str| format!("2025-0{s}T12:00:00.123+00:00");
    serde_json::from_value(json!({
        "Follow": [
            {"followerId": id("owner"), "followingId": id("fan"), "createdAt": t("1-01")},
            {"followerId": id("fan"), "followingId": id("owner"), "createdAt": t("1-02")},
            {"followerId": id("owner"), "followingId": id("owner"), "createdAt": t("1-03")},
            {"followerId": id("owner"), "followingId": "legacy-missing-user", "createdAt": t("1-04")},
            {"followerId": id("owner"), "followingId": id("fan"), "createdAt": t("1-05")},
        ],
        "ProfileTop8": [
            {"userId": id("owner"), "targetUserId": id("fan"), "position": 2},
            {"userId": id("owner"), "targetUserId": id("owner"), "position": 1},
            {"userId": id("owner"), "targetUserId": "legacy-missing-user", "position": 3},
            {"userId": id("owner"), "targetUserId": id("spot"), "position": 5},
        ],
        "SocialLink": [
            {"userId": id("owner"), "platform": "TWITTER_X", "url": "http://twitter.com/legacyowner", "createdAt": t("1-01")},
            {"userId": id("owner"), "platform": "WEBSITE", "url": "javascript:alert(1)", "createdAt": t("1-02")},
            {"userId": id("owner"), "platform": "YOUTUBE", "url": "https://evil.example/watch", "createdAt": t("1-03")},
            {"userId": id("owner"), "platform": "MYSPACE", "url": "https://myspace.com/legacyowner", "createdAt": t("1-04")},
            {"userId": id("owner"), "platform": "WEBSITE", "url": "https://a.example", "createdAt": t("1-05")},
            {"userId": id("owner"), "platform": "WEBSITE", "url": "https://b.example", "createdAt": t("1-06")},
            {"userId": id("owner"), "platform": "TWITCH", "url": "https://twitch.tv/legacyowner", "createdAt": t("1-07")},
            {"userId": id("owner"), "platform": "KICK", "url": "https://kick.com/legacyowner", "createdAt": t("1-08")},
            {"userId": id("owner"), "platform": "TIKTOK", "url": "https://www.tiktok.com/@legacyowner", "createdAt": t("1-09")},
        ],
        "WallPost": [
            {"id": "lp-p1", "authorId": id("fan"), "wallOwnerUserId": id("owner"), "body": "first legacy post", "createdAt": t("2-01"), "deletedAt": null, "moderationStatus": "APPROVED", "moderatedAt": null},
            {"id": "lp-p2", "authorId": id("owner"), "wallOwnerUserId": id("owner"), "body": "owner post", "createdAt": t("2-02"), "deletedAt": null, "moderationStatus": "APPROVED", "moderatedAt": null},
            {"id": "lp-rej", "authorId": id("fan"), "wallOwnerUserId": id("owner"), "body": "rejected", "createdAt": t("2-03"), "deletedAt": null, "moderationStatus": "REJECTED", "moderatedAt": t("2-04")},
            {"id": "lp-p3", "authorId": id("fan"), "wallOwnerUserId": id("owner"), "body": "deleted", "createdAt": t("2-04"), "deletedAt": t("2-05"), "moderationStatus": "APPROVED", "moderatedAt": null},
            {"id": "lp-p4", "authorId": id("spot"), "wallOwnerUserId": id("owner"), "body": "x".repeat(700), "createdAt": t("2-05"), "deletedAt": null, "moderationStatus": "APPROVED", "moderatedAt": null},
            {"id": "lp-p5", "authorId": id("fan"), "wallOwnerUserId": id("owner"), "body": "pending", "createdAt": t("2-06"), "deletedAt": null, "moderationStatus": "PENDING", "moderatedAt": null},
            {"id": "lp-lost", "authorId": "legacy-missing-user", "wallOwnerUserId": id("owner"), "body": "orphan", "createdAt": t("2-07"), "deletedAt": null, "moderationStatus": "APPROVED", "moderatedAt": null},
        ],
        "WallReply": [
            {"id": "lp-r1", "postId": "lp-p1", "authorId": id("owner"), "body": "reply", "createdAt": t("3-01"), "deletedAt": null},
            {"id": "lp-r2", "postId": "lp-p2", "authorId": id("fan"), "body": "deleted reply", "createdAt": t("3-02"), "deletedAt": t("3-03")},
            {"id": "lp-r3", "postId": "lp-lost", "authorId": id("fan"), "body": "orphan reply", "createdAt": t("3-04"), "deletedAt": null},
        ],
        "WallReaction": [
            {"postId": "lp-p2", "userId": id("fan"), "type": "LIKE", "createdAt": t("4-01")},
            {"postId": "lp-p1", "userId": id("owner"), "type": "VALOR", "createdAt": t("4-02")},
            {"postId": "lp-p2", "userId": id("fan"), "type": "VALOR", "createdAt": t("4-03")},
            {"postId": "lp-p2", "userId": "legacy-missing-user", "type": "LIKE", "createdAt": t("4-04")},
        ],
    }))
    .unwrap()
}

async fn legacy_import(env: &Env, media_dir: &std::path::Path) {
    let app = &env.app;
    // Synthetic imported accounts: users plus their preserved legacy JSON.
    let mut ids = HashMap::new();
    let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let profiles = [
        (
            "owner",
            Some(
                json!({"id": format!("lp-prof-owner-{tag}"), "displayName": "Legacy Owner", "bio": "Old bio", "moodEmoji": "abc", "status": "s".repeat(120), "wallSettings": {"whoCanPost": "SUBSCRIBERS", "requireApproval": true, "autoHideLinks": true}, "pinnedWallPostIds": ["lp-rej", "lp-p1", "lp-p3", "lp-p2", "lp-p4"], "profileSongUrl": "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "profileSongTitle": "Old Title", "avatarUrl": "https://legacy.example/a.png", "bannerUrl": "https://legacy.example/b.png", "createdAt": "2024-05-01T00:00:00.000Z"}),
            ),
        ),
        (
            "fan",
            Some(
                json!({"id": format!("lp-prof-fan-{tag}"), "displayName": "", "moodEmoji": "\u{1F525}", "status": "on air", "profileSongUrl": "https://soundcloud.com/legacy-artist/legacy-track", "avatarUrl": "https://legacy.example/f.png", "bannerUrl": "https://legacy.example/fb.png"}),
            ),
        ),
        (
            "spot",
            Some(
                json!({"id": format!("lp-prof-spot-{tag}"), "profileSongUrl": "https://open.spotify.com/track/abc", "avatarUrl": "https://legacy.example/current.png"}),
            ),
        ),
        ("sys", None),
        ("bare", None),
    ];
    for (key, profile) in &profiles {
        let id = format!("lp-{key}-{tag}");
        // The system account uses a named internal username, so the flag adds nobody.
        let name = if *key == "sys" {
            "SVER".to_string()
        } else {
            format!("lp_{key}_{tag}")
        };
        sqlx::query("INSERT INTO users(id,email,username,email_verified,created_at,date_of_birth) VALUES($1,$2,$3,true,'2024-01-01','1990-01-01')")
            .bind(&id).bind(format!("{name}@example.invalid")).bind(&name)
            .execute(&app.db).await.unwrap();
        let account = json!({"id": id, "username": name, "displayName": if *key == "fan" { json!("Account Fan") } else { Value::Null }, "isSystemAccount": *key == "sys", "createdAt": "2024-01-01T00:00:00.000Z"});
        sqlx::query("INSERT INTO legacy_account_data(user_id,account,profile,identities) VALUES($1,$2,$3,'[]'::jsonb)")
            .bind(&id).bind(account).bind(profile)
            .execute(&app.db).await.unwrap();
        ids.insert(*key, id);
    }
    let id_list: Vec<String> = ids.values().cloned().collect();
    let export = legacy_export(&ids);
    let file = |profile: &str, kind: &str, url: &str, bytes: Vec<u8>| profile_import::MediaFile {
        profile_id: format!("lp-prof-{profile}-{tag}"),
        kind: kind.into(),
        url: url.into(),
        bytes,
    };
    let media = vec![
        file(
            "owner",
            "avatar",
            "https://legacy.example/a.png",
            png(400, 400),
        ),
        file(
            "owner",
            "banner",
            "https://legacy.example/b.png",
            png(1500, 500),
        ),
        file(
            "fan",
            "avatar",
            "https://legacy.example/f.png",
            b"not an image".to_vec(),
        ),
        file(
            "spot",
            "avatar",
            "https://legacy.example/stale.png",
            png(400, 400),
        ),
        // Portrait banner: its centered 3:1 crop (1079x360) is below 1200x400, so the import
        // upscales that crop to the minimum instead of dropping it.
        file(
            "fan",
            "banner",
            "https://legacy.example/fb.png",
            png(1079, 1440),
        ),
    ];
    let before = preserved_digest(env).await;
    let untouched = || async {
        env.count(sqlx::AssertSqlSafe(format!("SELECT (SELECT count(*) FROM profiles WHERE user_id LIKE 'lp-%-{tag}') + (SELECT count(*) FROM follows WHERE follower_id LIKE 'lp-%-{tag}') + (SELECT count(*) FROM wall_posts WHERE id LIKE 'lp-%') + (SELECT count(*) FROM import_runs)"))).await
    };
    let commit = profile_import::Options {
        commit: true,
        ..Default::default()
    };
    let mut uploaded = Vec::new();

    // Rehearsal-only accounts would otherwise be the only legacy rows; the existing schema's
    // users stay untouched throughout.
    // 1. A seeded conflict rolls back.
    env.sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO follows(follower_id,following_id) VALUES('{}','{}')",
        ids["fan"], ids["owner"]
    )))
    .await;
    let err = profile_import::run(app, &export, &media, &commit, &mut uploaded)
        .await
        .err()
        .unwrap();
    assert!(err.contains("already exist"), "{err}");
    env.sql(sqlx::AssertSqlSafe(format!(
        "DELETE FROM follows WHERE follower_id='{}'",
        ids["fan"]
    )))
    .await;
    // 2. More than 3 internal accounts stops the import.
    env.sql(sqlx::AssertSqlSafe(format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":true}}' WHERE user_id IN ('{}','{}','{}')", ids["fan"], ids["spot"], ids["bare"]))).await;
    let err = profile_import::run(app, &export, &media, &commit, &mut uploaded)
        .await
        .err()
        .unwrap();
    assert!(err.contains("stops for review"), "{err}");
    assert!(
        err.contains("adds 3 internal accounts") && err.contains("(4 internal in total)"),
        "{err}"
    );
    env.sql(sqlx::AssertSqlSafe(format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":false}}' WHERE user_id IN ('{}','{}')", ids["fan"], ids["spot"]))).await;
    let saved = std::mem::take(&mut uploaded);
    let err = profile_import::run(app, &export, &media, &commit, &mut uploaded)
        .await
        .err()
        .unwrap();
    assert!(
        err.contains("adds 1 internal accounts"),
        "Even one flagged account stops the import: {err}"
    );
    uploaded = saved;
    env.sql(sqlx::AssertSqlSafe(format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":true}}' WHERE user_id IN ('{}','{}','{}')", ids["fan"], ids["spot"], ids["bare"]))).await;
    // The rehearsal-only preview continues past the stop and reports it.
    let preview = profile_import::Options {
        commit: false,
        preview_extra_internal: true,
        ..Default::default()
    };
    let mut preview_uploads = Vec::new();
    let previewed = profile_import::run(app, &export, &media, &preview, &mut preview_uploads)
        .await
        .unwrap();
    assert_eq!(previewed.counts["accounts.internal"], 4);
    assert_eq!(
        previewed.counts["accounts.internal_stop_bypassed_for_rehearsal_preview"],
        1
    );
    for key in &preview_uploads {
        app.config
            .media
            .storage
            .delete(&app.http, key)
            .await
            .unwrap();
    }
    // The operator decision keeps only the named accounts internal; flagged ones import as public.
    let named_only = profile_import::Options {
        commit: false,
        named_internal_only: true,
        ..Default::default()
    };
    let mut named_uploads = Vec::new();
    let named = profile_import::run(app, &export, &media, &named_only, &mut named_uploads)
        .await
        .unwrap();
    assert_eq!(named.counts["accounts.internal"], 1);
    assert_eq!(named.counts["accounts.internal_added_by_system_flag"], 0);
    assert_eq!(named.counts["accounts.system_flag_imported_as_public"], 3);
    assert!(
        !named
            .counts
            .contains_key("accounts.internal_stop_bypassed_for_rehearsal_preview")
    );
    for key in &named_uploads {
        app.config
            .media
            .storage
            .delete(&app.http, key)
            .await
            .unwrap();
    }
    env.sql(sqlx::AssertSqlSafe(format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":false}}' WHERE user_id IN ('{}','{}','{}')", ids["fan"], ids["spot"], ids["bare"]))).await;
    let before = {
        let _ = before;
        preserved_digest(env).await
    };
    // 3. An injected verification mismatch rolls back (uploaded objects are the caller's to remove).
    let faulty = profile_import::Options {
        commit: true,
        fault: Some(profile_import::Fault::FollowMismatch),
        ..Default::default()
    };
    let err = profile_import::run(app, &export, &media, &faulty, &mut uploaded)
        .await
        .err()
        .unwrap();
    assert!(err.contains("Follow comparison failed"), "{err}");
    assert_eq!(untouched().await, 0, "A failed import left rows behind");
    assert_eq!(
        uploaded.len(),
        6,
        "Avatar and banner variants were uploaded before verification"
    );
    // 4. Check mode (no commit) verifies and rolls back.
    let check = profile_import::Options {
        commit: false,
        ..Default::default()
    };
    let checked = profile_import::run(app, &export, &media, &check, &mut uploaded)
        .await
        .unwrap();
    assert_eq!(untouched().await, 0, "Check mode must roll back");
    for key in &uploaded {
        app.config
            .media
            .storage
            .delete(&app.http, key)
            .await
            .unwrap();
    }
    uploaded.clear();
    // 5. The real run.
    let outcome = profile_import::run(app, &export, &media, &commit, &mut uploaded)
        .await
        .unwrap();
    assert_eq!(checked.counts, outcome.counts);
    let c = |k: &str| outcome.counts.get(k).copied().unwrap_or(0);
    let expected = [
        ("accounts.read", 5),
        ("accounts.without_legacy_profile", 2),
        ("accounts.internal", 1),
        ("accounts.internal_added_by_system_flag", 0),
        ("profiles.imported", 5),
        ("profiles.mood_emptied", 1),
        ("profiles.status_emptied", 1),
        ("songs.imported.youtube", 1),
        ("songs.imported.soundcloud", 1),
        ("songs.spotify_notice", 1),
        ("follows.read", 5),
        ("follows.imported", 2),
        ("follows.dropped.self", 1),
        ("follows.dropped.unknown_user", 1),
        ("follows.dropped.duplicate", 1),
        ("war_council.imported", 2),
        ("war_council.dropped.self", 1),
        ("war_council.dropped.unknown_user", 1),
        ("links.read", 9),
        ("links.imported", 5),
        ("links.upgraded_to_https", 1),
        ("links.dropped.invalid_url", 2),
        ("links.dropped.platform_limit", 1),
        ("links.dropped.over_limit", 1),
        ("links.unknown_platform_as_website", 1),
        ("wall_posts.imported", 6),
        ("wall_posts.dropped.unknown_user", 1),
        ("wall_posts.pinned", 3),
        ("wall_posts.imported_pending", 1),
        ("wall_posts.imported_rejected", 1),
        ("wall_posts.imported_soft_deleted", 1),
        ("wall_replies.imported", 2),
        ("wall_replies.dropped.missing_post", 1),
        ("wall_reactions.imported_as_likes", 2),
        ("wall_reactions.valor_as_like", 1),
        ("wall_reactions.dropped.duplicate", 1),
        ("wall_reactions.dropped.unknown_user", 1),
        ("media.read", 5),
        ("media.processed_avatar", 1),
        ("media.processed_banner", 2),
        ("media.banner_upscaled_to_minimum", 1),
        ("media.dropped.decode_failed", 1),
        ("media.dropped.not_current_image", 1),
        ("media.dropped.too_small_after_crop", 0),
        ("media.objects_verified", 6),
    ];
    for (key, n) in expected {
        assert_eq!(c(key), n, "count {key}");
    }
    assert_eq!(
        preserved_digest(env).await,
        before,
        "users, identities and legacy data are unchanged"
    );
    let (display, mood, status, who, approval, links, internal): (String, String, String, String, bool, bool, bool) = sqlx::query_as("SELECT display_name,mood_emoji,status_text,who_can_post,require_approval,hold_links,internal FROM profiles WHERE user_id=$1")
        .bind(&ids["owner"]).fetch_one(&app.db).await.unwrap();
    assert_eq!(
        (
            display.as_str(),
            mood.as_str(),
            status.as_str(),
            who.as_str(),
            approval,
            links,
            internal
        ),
        ("Legacy Owner", "", "", "NONE", true, true, false)
    );
    let fan: (String, String) =
        sqlx::query_as("SELECT display_name,mood_emoji FROM profiles WHERE user_id=$1")
            .bind(&ids["fan"])
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(fan, ("Account Fan".to_string(), "\u{1F525}".to_string()));
    let fan_banner: Option<String> =
        sqlx::query_scalar("SELECT banner_key FROM profiles WHERE user_id=$1")
            .bind(&ids["fan"])
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        fan_banner.as_deref().is_some_and(|k| k.ends_with("@750")),
        "The upscaled banner keeps only the variant its 1200px source supports: {fan_banner:?}"
    );
    let sys_internal: bool = sqlx::query_scalar("SELECT internal FROM channel_users WHERE id=$1")
        .bind(&ids["sys"])
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(sys_internal);
    let pins: Vec<(String, i32)> = sqlx::query_as("SELECT id,pinned_position FROM wall_posts WHERE pinned_position IS NOT NULL AND wall_owner_id=$1 ORDER BY pinned_position")
        .bind(&ids["owner"]).fetch_all(&app.db).await.unwrap();
    assert_eq!(
        pins,
        vec![
            ("lp-p1".into(), 1),
            ("lp-p2".into(), 2),
            ("lp-p4".into(), 3)
        ]
    );
    let link_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT platform,url FROM social_links WHERE user_id=$1 ORDER BY position")
            .bind(&ids["owner"])
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(
        link_rows[0],
        ("x".into(), "https://twitter.com/legacyowner".into())
    );
    assert_eq!(
        link_rows.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(),
        ["x", "website", "website", "twitch", "kick"]
    );
    let council: Vec<(i32, String)> = sqlx::query_as(
        "SELECT position,member_id FROM war_council WHERE user_id=$1 ORDER BY position",
    )
    .bind(&ids["owner"])
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        council,
        vec![(1, ids["fan"].clone()), (2, ids["spot"].clone())]
    );
    let counts: (i32, i32) =
        sqlx::query_as("SELECT follower_count,following_count FROM profiles WHERE user_id=$1")
            .bind(&ids["owner"])
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(counts, (1, 1));
    let followed_at: i64 = env.count(sqlx::AssertSqlSafe(format!("SELECT (extract(epoch FROM created_at)*1000)::bigint FROM follows WHERE follower_id='{}'", ids["owner"]))).await;
    assert_eq!(
        followed_at, 1735732800123,
        "Follows keep their legacy creation time"
    );
    let long_body: i64 = env
        .count("SELECT length(body)::bigint FROM wall_posts WHERE id='lp-p4'")
        .await;
    assert_eq!(long_body, 700, "Bodies are kept verbatim");
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM wall_likes WHERE post_id='lp-p2' AND user_id='{}'",
            ids["fan"]
        )))
        .await,
        1
    );
    let (avatar, banner): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT avatar_key,banner_key FROM profiles WHERE user_id=$1")
            .bind(&ids["owner"])
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(avatar.is_some() && banner.is_some());
    let stored = std::fs::read_dir(media_dir).unwrap().count();
    assert!(stored > 0);
    assert_eq!(
        env.count(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM profiles WHERE user_id IN ('{}','{}') AND avatar_key IS NULL",
            ids["fan"], ids["spot"]
        )))
        .await,
        2,
        "Failed media keeps the default image"
    );
    let notice: Option<String> =
        sqlx::query_scalar("SELECT song_notice FROM profiles WHERE user_id=$1")
            .bind(&ids["spot"])
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(notice.as_deref(), Some(profile_import::SPOTIFY_NOTICE));
    // 6. A second run is refused.
    let err = profile_import::run(app, &export, &media, &commit, &mut Vec::new())
        .await
        .err()
        .unwrap();
    assert!(err.contains("already imported"), "{err}");
    // 7. The imported SoundCloud song stays hidden until the post-import job resolves it.
    let fan_name = format!("lp_fan_{tag}");
    let page = env
        .anon
        .ok("GET", &format!("/api/channels/{fan_name}"), Value::Null)
        .await;
    assert!(page["channel"]["song"].is_null(), "{page}");
    let processed = profile_import::song_job(app, 10).await.unwrap();
    assert_eq!(processed, 2);
    let page = env
        .anon
        .ok("GET", &format!("/api/channels/{fan_name}"), Value::Null)
        .await;
    assert_eq!(page["channel"]["song"]["media_id"], "123456");
    assert!(page["channel"]["song"]["thumbnail"].is_string());
    let title: Option<String> =
        sqlx::query_scalar("SELECT song_title FROM profiles WHERE user_id=$1")
            .bind(&ids["owner"])
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(
        title.as_deref(),
        Some("Old Title"),
        "Legacy titles are kept"
    );
    assert_eq!(
        profile_import::song_job(app, 10).await.unwrap(),
        0,
        "The job does not retry"
    );
    // 8. Media snapshot loading checks the manifest checksum.
    let snap = std::env::temp_dir().join(format!("sver-media-snapshot-{tag}"));
    std::fs::create_dir_all(snap.join("media")).unwrap();
    let bytes = png(400, 400);
    std::fs::write(snap.join("media/000-avatar.bin"), &bytes).unwrap();
    let digest: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let manifest = |sha: &str| {
        json!([{"profile_id": "p", "kind": "avatar", "url": "u", "key": "k", "content_type": "image/png", "content_length": bytes.len(), "file": "media/000-avatar.bin", "size_bytes": bytes.len(), "sha256": sha}]).to_string()
    };
    std::fs::write(snap.join("media-manifest.json"), manifest(&digest)).unwrap();
    assert_eq!(profile_import::load_media(&snap).unwrap().len(), 1);
    std::fs::write(snap.join("media-manifest.json"), manifest(&"0".repeat(64))).unwrap();
    assert!(profile_import::load_media(&snap).is_err());
    std::fs::remove_dir_all(&snap).unwrap();
    assert_eq!(
        env.count("SELECT count(*) FROM profiles WHERE show_linked_accounts")
            .await,
        0,
        "imported and existing accounts never opt in to Also known as"
    );
    let _ = id_list;
    eprintln!(
        "Passed: legacy profile import mapping, drop counts, pins, media, conflict/mismatch rollback, check mode, second-run refusal, preserved digests and the song job."
    );
}

/// The command-line guards and a complete rehearsal through the binary, with aggregate output.
async fn legacy_import_cli() {
    let bin = env!("CARGO_BIN_EXE_sver-import-check");
    let database = std::env::var("DATABASE_URL").unwrap();
    let dir =
        std::env::temp_dir().join(format!("sver-import-cli-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(dir.join("media")).unwrap();
    let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let user = |k: &str| json!({"id": format!("cli-{k}-{tag}"), "email": format!("cli_{k}_{tag}@example.invalid"), "username": format!("cli_{k}_{tag}"), "passwordHash": null, "dateOfBirth": "1990-01-01T00:00:00.000Z", "emailVerified": true, "createdAt": "2024-01-01T00:00:00.000Z", "deletedAt": null, "twoFactorEnabled": false});
    let avatar = png(300, 300);
    let digest: String = sha2::Sha256::digest(&avatar)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    std::fs::write(dir.join("media/000-avatar.bin"), &avatar).unwrap();
    std::fs::write(dir.join("media-manifest.json"), json!([{"profile_id": format!("cli-prof-{tag}"), "kind": "avatar", "url": "https://legacy.example/cli.png", "file": "media/000-avatar.bin", "sha256": digest}]).to_string()).unwrap();
    std::fs::write(dir.join("accounts.json"), json!({
        "users": [user("a"), user("b")],
        "profiles": [{"id": format!("cli-prof-{tag}"), "userId": format!("cli-a-{tag}"), "displayName": "Cli Secret Name", "avatarUrl": "https://legacy.example/cli.png"}],
        "identities": [],
    }).to_string()).unwrap();
    std::fs::write(dir.join("profiles-export.json"), json!({
        "Follow": [{"followerId": format!("cli-a-{tag}"), "followingId": format!("cli-b-{tag}"), "createdAt": "2025-01-01T00:00:00+00:00"}],
        "ProfileTop8": [], "SocialLink": [{"userId": format!("cli-a-{tag}"), "platform": "WEBSITE", "url": "https://secret-site.example", "createdAt": "2025-01-01T00:00:00+00:00"}],
        "WallPost": [{"id": format!("cli-post-{tag}"), "authorId": format!("cli-b-{tag}"), "wallOwnerUserId": format!("cli-a-{tag}"), "body": "secret wall body", "createdAt": "2025-01-01T00:00:00+00:00", "deletedAt": null, "moderationStatus": "APPROVED", "moderatedAt": null}],
        "WallReply": [], "WallReaction": [],
    }).to_string()).unwrap();
    let run = |args: Vec<String>, db: &str| {
        std::process::Command::new(bin)
            .args(args)
            .env("DATABASE_URL", db)
            .env("LEGACY_TWO_FACTOR_ENCRYPTION_KEY", "0".repeat(32))
            .output()
            .unwrap()
    };
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();
    let args = vec![
        "profiles".to_string(),
        path("profiles-export.json"),
        dir.to_string_lossy().to_string(),
        path("accounts.json"),
    ];
    // Remote and wrong-database targets are refused.
    for target in [
        database
            .replace("localhost", "db.example.com")
            .replace("127.0.0.1", "db.example.com"),
        database.replace("/sver_rebuild", "/sver_stage"),
    ] {
        let out = run(args.clone(), &target);
        assert!(
            !out.status.success()
                && String::from_utf8_lossy(&out.stderr).contains("Target refused")
        );
    }
    // Production configuration is refused.
    let out = std::process::Command::new(bin)
        .args(args.clone())
        .env("APP_ENV", "production")
        .env("LEGACY_TWO_FACTOR_ENCRYPTION_KEY", "0".repeat(32))
        .output()
        .unwrap();
    assert!(!out.status.success() && out.stdout.is_empty());
    // Unknown, repeated or conflicting flags are refused before anything runs.
    for extra in [
        vec!["--bogus"],
        vec!["--named-internal-only", "--named-internal-only"],
        vec!["--preview-extra-internal", "--named-internal-only"],
    ] {
        let mut flagged = args.clone();
        flagged.extend(extra.iter().map(|f| f.to_string()));
        let out = run(flagged, &database);
        assert!(
            !out.status.success() && String::from_utf8_lossy(&out.stderr).contains("Usage"),
            "{extra:?}"
        );
    }
    let mut live_preview = args.clone();
    live_preview[3] = "--check-live".into();
    live_preview.push("--preview-extra-internal".into());
    let out = run(live_preview, &database);
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("Usage"));
    // Sources inside the workspace are refused.
    let mut inside = args.clone();
    inside[1] = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").to_string();
    let out = run(inside, &database);
    assert!(
        !out.status.success()
            && String::from_utf8_lossy(&out.stderr).contains("outside the workspace")
    );
    // Full rehearsal: aggregate-only output and a private report beside the export.
    let out = run(args, &database);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("Profile import rehearsal passed")
            && stdout.contains("follows.imported: 1")
            && stdout.contains("media.processed_avatar: 1")
            && stdout.contains("wall_posts.imported: 1"),
        "{stdout}"
    );
    for secret in [
        tag.as_str(),
        "Cli Secret Name",
        "secret-site",
        "secret wall body",
        "legacy.example",
        "example.invalid",
    ] {
        assert!(
            !stdout.contains(secret),
            "Rehearsal output leaked private data"
        );
    }
    assert!(
        dir.join("profiles-export.profiles-rehearsal-report.json")
            .is_file()
    );
    std::fs::remove_dir_all(&dir).unwrap();
    eprintln!(
        "Passed: import guards (remote, wrong database, production, in-workspace sources) and an aggregate-only rehearsal through the binary."
    );
}

#[test]
fn schedule_dst_and_overnight() {
    use chrono::{Duration, TimeZone, Utc};
    let tz: chrono_tz::Tz = "America/New_York".parse().unwrap();
    // A daily 01:30-03:00 block across the spring-forward gap and the fall-back overlap.
    let blocks: Vec<studio::Block> = (1..=7)
        .map(|d| studio::Block {
            weekday: d,
            start: 90,
            end: 180,
            label: String::new(),
        })
        .collect();
    let spring = studio::expand(
        tz,
        &blocks,
        Utc.with_ymd_and_hms(2026, 3, 8, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 3, 9, 0, 0, 0).unwrap(),
    );
    let on_day: Vec<_> = spring
        .iter()
        .filter(|o| o.0.with_timezone(&tz).date_naive().to_string() == "2026-03-08")
        .collect();
    assert_eq!(on_day.len(), 1);
    assert_eq!(
        on_day[0].1 - on_day[0].0,
        Duration::minutes(30),
        "The gap shortens the block"
    );
    let fall = studio::expand(
        tz,
        &blocks,
        Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap(),
    );
    let on_day: Vec<_> = fall
        .iter()
        .filter(|o| o.0.with_timezone(&tz).date_naive().to_string() == "2026-11-01")
        .collect();
    assert_eq!(on_day.len(), 1, "The overlap never duplicates");
    assert_eq!(
        on_day[0].1 - on_day[0].0,
        Duration::minutes(150),
        "The earlier 01:30 is taken"
    );
    // Overnight blocks end on the next day.
    let late = vec![studio::Block {
        weekday: 5,
        start: 22 * 60,
        end: 60,
        label: "Late".into(),
    }];
    let out = studio::expand(
        tz,
        &late,
        Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 10, 8, 0, 0, 0).unwrap(),
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].1 - out[0].0, Duration::hours(3));
}

#[test]
fn text_rules() {
    use sver::text;
    assert!(text::display_name("Ok Name", "someone").is_ok());
    assert!(text::display_name("\u{200b}", "someone").is_err());
    assert!(text::website_url("https://example.com/a", "url").is_ok());
    assert!(text::website_url("https://localhost/a", "url").is_err());
    assert!(text::website_url("https://user:pw@example.com", "url").is_err());
    assert!(text::social_link("discord", "https://discord.com/invite/abc").is_ok());
    assert!(text::social_link("discord", "https://discord.com/channels/1").is_err());
    assert!(text::social_link("x", "https://twitter.com/me").is_ok());
    assert!(
        text::contains_url("visit example.com today")
            || text::contains_url("visit https://example.com today")
    );
    assert!(!text::valid_mood("ab"));
}
