//! Module 2 acceptance checks against real Postgres, the filesystem media adapter and a
//! controlled oEmbed fake (docs/PROFILES.md, "Acceptance"). Synthetic data only.
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
    async fn sql(&self, query: &str) {
        sqlx::query(query).execute(&self.app.db).await.unwrap();
    }
    async fn count(&self, query: &str) -> i64 {
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
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(16)
        .after_connect(move |connection, _| {
            let statement = search_path.clone();
            Box::pin(async move {
                sqlx::query(&statement).execute(connection).await?;
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
            follows_council_and_blocks(&env).await;
            song(&env).await;
            wall(&env).await;
            studio_sections(&env, &media_dir).await;
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
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
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
    let missing = env
        .anon
        .call("GET", "/api/channels/nobody_here", Value::Null)
        .await;
    assert_eq!(missing.0, StatusCode::NOT_FOUND);
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
    let queued = env.count(&format!("SELECT count(*) FROM media_objects WHERE key LIKE '{avatar_key}/%' AND delete_after IS NOT NULL")).await;
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
    assert_eq!(env.count(&format!("SELECT count(*) FROM user_blocks WHERE blocker_id='{owner_id}' AND blocked_id='{newbie_id}'")).await, 1);
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
        .map(|i| json!({"category": "PC", "name": format!("Part {i}")}))
        .collect();
    assert_eq!(
        owner
            .status("PUT", "/api/me/setup", json!({"items": items}))
            .await,
        StatusCode::BAD_REQUEST
    );
    owner.ok("PUT", "/api/me/setup", json!({"items": [{"category": "MICROPHONE", "name": "Synthetic Mic", "note": "Cardioid", "link": "https://mic.example"}]})).await;
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
        env.count(&format!(
            "SELECT count(*) FROM username_holds WHERE user_id='{id}'"
        ))
        .await,
        0
    );
    assert_eq!(
        env.count(&format!(
            "SELECT count(*) FROM username_history WHERE user_id='{id}' AND reason='revert'"
        ))
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
        env.count(&format!("SELECT count(*) FROM users WHERE id='{gone_id}'"))
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
                json!({"id": format!("lp-prof-fan-{tag}"), "displayName": "", "moodEmoji": "\u{1F525}", "status": "on air", "profileSongUrl": "https://soundcloud.com/legacy-artist/legacy-track", "avatarUrl": "https://legacy.example/f.png"}),
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
    ];
    let before = preserved_digest(env).await;
    let untouched = || async {
        env.count(&format!("SELECT (SELECT count(*) FROM profiles WHERE user_id LIKE 'lp-%-{tag}') + (SELECT count(*) FROM follows WHERE follower_id LIKE 'lp-%-{tag}') + (SELECT count(*) FROM wall_posts WHERE id LIKE 'lp-%') + (SELECT count(*) FROM import_runs)")).await
    };
    let commit = profile_import::Options {
        commit: true,
        ..Default::default()
    };
    let mut uploaded = Vec::new();

    // Rehearsal-only accounts would otherwise be the only legacy rows; the existing schema's
    // users stay untouched throughout.
    // 1. A seeded conflict rolls back.
    env.sql(&format!(
        "INSERT INTO follows(follower_id,following_id) VALUES('{}','{}')",
        ids["fan"], ids["owner"]
    ))
    .await;
    let err = profile_import::run(app, &export, &media, &commit, &mut uploaded)
        .await
        .err()
        .unwrap();
    assert!(err.contains("already exist"), "{err}");
    env.sql(&format!(
        "DELETE FROM follows WHERE follower_id='{}'",
        ids["fan"]
    ))
    .await;
    // 2. More than 3 internal accounts stops the import.
    env.sql(&format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":true}}' WHERE user_id IN ('{}','{}','{}')", ids["fan"], ids["spot"], ids["bare"])).await;
    let err = profile_import::run(app, &export, &media, &commit, &mut uploaded)
        .await
        .err()
        .unwrap();
    assert!(err.contains("stops for review"), "{err}");
    assert!(
        err.contains("adds 3 internal accounts") && err.contains("(4 internal in total)"),
        "{err}"
    );
    env.sql(&format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":false}}' WHERE user_id IN ('{}','{}')", ids["fan"], ids["spot"])).await;
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
    env.sql(&format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":true}}' WHERE user_id IN ('{}','{}','{}')", ids["fan"], ids["spot"], ids["bare"])).await;
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
    env.sql(&format!("UPDATE legacy_account_data SET account=account||'{{\"isSystemAccount\":false}}' WHERE user_id IN ('{}','{}','{}')", ids["fan"], ids["spot"], ids["bare"])).await;
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
        5,
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
        ("media.read", 4),
        ("media.processed_avatar", 1),
        ("media.processed_banner", 1),
        ("media.dropped.decode_failed", 1),
        ("media.dropped.not_current_image", 1),
        ("media.objects_verified", 5),
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
    let followed_at: i64 = env.count(&format!("SELECT (extract(epoch FROM created_at)*1000)::bigint FROM follows WHERE follower_id='{}'", ids["owner"])).await;
    assert_eq!(
        followed_at, 1735732800123,
        "Follows keep their legacy creation time"
    );
    let long_body: i64 = env
        .count("SELECT length(body)::bigint FROM wall_posts WHERE id='lp-p4'")
        .await;
    assert_eq!(long_body, 700, "Bodies are kept verbatim");
    assert_eq!(
        env.count(&format!(
            "SELECT count(*) FROM wall_likes WHERE post_id='lp-p2' AND user_id='{}'",
            ids["fan"]
        ))
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
        env.count(&format!(
            "SELECT count(*) FROM profiles WHERE user_id IN ('{}','{}') AND avatar_key IS NULL",
            ids["fan"], ids["spot"]
        ))
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
