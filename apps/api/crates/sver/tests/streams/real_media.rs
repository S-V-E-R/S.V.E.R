// Opt-in integration: real Rust callbacks + isolated Postgres + SRS + FFmpeg.
// No production endpoints, account imports or browser automation.
use super::*;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const SRS: &str =
    "ossrs/srs@sha256:2e96f38660211b8e8dd324bee0d3ade90f1e44ab815813a1684eb78ab2ad17d6";

fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    cmd
}
fn docker(args: &[&str]) -> std::result::Result<String, &'static str> {
    let mut child = command("docker")
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "Docker unavailable")?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if child
            .try_wait()
            .map_err(|_| "Docker wait failed")?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Docker operation timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .map_err(|_| "Docker output failed")?;
    if !output.status.success() {
        return Err("Docker operation failed (output suppressed)");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}
struct Rig {
    name: String,
    directory: PathBuf,
    containers: Vec<String>,
    network: bool,
}
impl Rig {
    fn new() -> Self {
        if let Ok(host) = std::env::var("DOCKER_HOST") {
            assert!(host.starts_with("npipe:") || host.starts_with("unix:"));
        }
        let context: Value =
            serde_json::from_str(&docker(&["context", "inspect"]).unwrap()).unwrap();
        let host = context[0]["Endpoints"]["docker"]["Host"].as_str().unwrap();
        assert!(
            host.starts_with("npipe:") || host.starts_with("unix:"),
            "Refuse remote Docker"
        );
        for image in [SRS, "nginx:1.28-alpine"] {
            docker(&["image", "inspect", image, "--format", "{{.Id}}"])
                .expect("Preinstall proof images");
        }
        let name = format!("sver-ingest-test-{}", uuid::Uuid::new_v4().simple());
        let directory = std::env::temp_dir().join(&name);
        fs::create_dir(&directory).unwrap();
        Self {
            name,
            directory,
            containers: vec![],
            network: false,
        }
    }
    fn start(&mut self, suffix: &str, args: &[&str]) -> String {
        let name = format!("{}-{suffix}", self.name);
        self.containers.push(name.clone());
        let label = format!("sver.ingest-test={}", self.name);
        let mut all = vec![
            "run",
            "-d",
            "--pull=never",
            "--name",
            &name,
            "--network",
            &self.name,
            "--label",
            &label,
            "--cpus=1",
            "--memory=512m",
        ];
        all.extend(args);
        docker(&all).unwrap();
        name
    }
    fn cleanup(&mut self) -> std::result::Result<(), &'static str> {
        while let Some(name) = self.containers.last() {
            let exists = docker(&["ps", "-aq", "--filter", &format!("name=^{name}$")])?;
            if !exists.is_empty() {
                let label = docker(&[
                    "inspect",
                    "--format",
                    "{{index .Config.Labels \"sver.ingest-test\"}}",
                    name,
                ])?;
                if label != self.name {
                    return Err("Refuse cleanup of an unowned container");
                }
                docker(&["rm", "-f", name])?;
            }
            self.containers.pop();
        }
        if self.network {
            docker(&["network", "rm", &self.name])?;
            self.network = false;
        }
        for name in ["srs.conf", "nginx.conf"] {
            let file = self.directory.join(name);
            if file.exists() {
                fs::remove_file(file).map_err(|_| "Temporary file cleanup failed")?;
            }
        }
        if self.directory.exists() {
            fs::remove_dir(&self.directory).map_err(|_| "Temporary directory cleanup failed")?;
        }
        Ok(())
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        if self.cleanup().is_err() {
            eprintln!("Real-media fixture cleanup failed; inspect labeled test resources.");
        }
    }
}
struct Encoder(Child);
impl Encoder {
    fn new(url: &str) -> Self {
        Self(
            command("ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-re",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=320x180:rate=10",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast",
                    "-tune",
                    "zerolatency",
                    "-pix_fmt",
                    "yuv420p",
                    "-bf",
                    "0",
                    "-g",
                    "10",
                    "-b:v",
                    "400k",
                    "-c:a",
                    "aac",
                    "-b:a",
                    "64k",
                    "-t",
                    "90",
                    "-f",
                    "flv",
                    url,
                ])
                .stdout(Stdio::null())
                .spawn()
                .expect("FFmpeg unavailable"),
        )
    }
    async fn rejected(&mut self) {
        let end = Instant::now() + Duration::from_secs(12);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(
                    !status.success(),
                    "Rejected publisher unexpectedly succeeded"
                );
                return;
            }
            assert!(
                Instant::now() < end,
                "Publisher was not rejected/disconnected"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn port(name: &str, number: &str) -> u16 {
    let binding = docker(&["port", name, number]).unwrap();
    assert!(binding.starts_with("127.0.0.1:"));
    binding.trim_start_matches("127.0.0.1:").parse().unwrap()
}
async fn wait_state(e: &Env, wanted: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(18);
    loop {
        streams::tick(&e.app).await.unwrap();
        let mine = e.mine().await;
        if mine["broadcast"]["state"] == wanted {
            return mine["broadcast"].clone();
        }
        assert!(
            Instant::now() < end,
            "Real ingest did not reach {wanted}; state={}",
            mine["broadcast"]["state"]
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Local Docker Desktop + FFmpeg real media; run explicitly with local development configuration"]
async fn real_srs_ingest() {
    let (admin, db, schema) = isolated_database().await;
    let proof_db = db.clone();
    let result = tokio::spawn(async move { exercise_real(proof_db).await }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let receipt = result.unwrap();
    fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../docs/stream-ingest-local.json"),
        serde_json::to_string_pretty(&receipt).unwrap() + "\n",
    )
    .unwrap();
}
async fn exercise_real(db: sqlx::PgPool) -> Value {
    let mut rig = Rig::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let secret = sec::token();
    fs::write(rig.directory.join("srs.conf"), "listen 1935;\ndaemon off;\nsrs_log_tank console;\nsrs_log_level error;\nhttp_api { enabled on; listen 1985; }\nhttp_server { enabled on; listen 8080; dir /media; }\nvhost __defaultVhost__ {\n play { gop_cache off; }\n hls { enabled on; hls_path /media; hls_ctx off; hls_fragment 1; hls_window 6; hls_wait_keyframe on; hls_ts_file [app]/[stream]-[timestamp]-[seq].ts; }\n http_hooks { enabled on; on_publish http://hooks:8089/publish; on_unpublish http://hooks:8089/unpublish; }\n}\n").unwrap();
    fs::write(rig.directory.join("nginx.conf"), format!("events {{}}\nhttp {{ access_log off; error_log /dev/null; server {{ listen 8089; location / {{ proxy_set_header x-srs-secret {secret}; proxy_pass http://host.docker.internal:{}/api/internal/srs/; }} }} }}", address.port())).unwrap();
    docker(&[
        "network",
        "create",
        "--label",
        &format!("sver.ingest-test={}", rig.name),
        &rig.name,
    ])
    .unwrap();
    rig.network = true;
    let mount = format!(
        "{}:/fixture:ro",
        rig.directory.to_string_lossy().replace('\\', "/")
    );
    let nginx_mount = format!(
        "{}:/etc/nginx/nginx.conf:ro",
        rig.directory
            .join("nginx.conf")
            .to_string_lossy()
            .replace('\\', "/")
    );
    rig.start(
        "hooks",
        &[
            "--network-alias",
            "hooks",
            "-v",
            &nginx_mount,
            "nginx:1.28-alpine",
        ],
    );
    let srs = rig.start(
        "srs",
        &[
            "-p",
            "127.0.0.1::1935",
            "-p",
            "127.0.0.1::1985",
            "-p",
            "127.0.0.1::8080",
            "-v",
            &mount,
            "--tmpfs",
            "/media:size=128m",
            SRS,
            "./objs/srs",
            "-c",
            "/fixture/srs.conf",
        ],
    );
    let api = format!("http://127.0.0.1:{}", port(&srs, "1985/tcp"));
    let ingest = format!("rtmp://127.0.0.1:{}/rebuild", port(&srs, "1935/tcp"));
    let origin = format!("http://127.0.0.1:{}", port(&srs, "8080/tcp"));
    let mut config = Config::from_env().unwrap();
    assert!(!config.production, "Never use production configuration");
    config.resend_key.clear();
    config.streaming = Some(streams::Config {
        api_url: api.clone(),
        ingest_url: ingest.clone(),
        hook_secret: secret,
        hook_ip: "127.0.0.1".parse().unwrap(),
        vhost: "__defaultVhost__".into(),
        app: "rebuild".into(),
    });
    let app = App::new(db, config).await.unwrap();
    let e = synthetic_owner(app.clone(), Arc::new(Mutex::new(Media::default()))).await;
    let api_task = tokio::spawn(async move {
        axum::serve(
            listener,
            sver::router(app).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    // Keep the listener task scoped even if a media assertion panics.
    let abort = api_task.abort_handle();
    let result = tokio::spawn(async move {
        let initial = e.mine().await;
        let catalog = e.call("GET", "/api/categories", Value::Null).await;
        e.call("PATCH", "/api/me/stream", json!({"title":"Real isolated media", "category_id":catalog["categories"][0]["id"], "revision":initial["settings"]["revision"]})).await;
        let key = e.key("create").await;
        let id = key.split('?').next().unwrap();
        let ready: Value = e.app.http.get(format!("{api}/api/v1/versions")).send().await.unwrap().json().await.unwrap();
        let control_check = e.app.http.get(format!("{api}/api/v1/streams?start=0&count=1000")).send().await.unwrap();
        assert_eq!(control_check.status(), StatusCode::FOUND);
        assert_eq!(control_check.headers()["location"], "/api/v1/streams/?start=0&count=1000");
        let mut invalid = Encoder::new(&format!("{ingest}/{id}?key=wrong"));
        invalid.rejected().await;
        assert!(e.mine().await["broadcast"].is_null());
        let publisher = Encoder::new(&format!("{ingest}/{key}"));
        let first = wait_state(&e, "LIVE").await;
        eprintln!("Real media: initial publish is LIVE");
        assert_eq!(first["health"]["video_codec"], "H264");
        assert_eq!(first["health"]["audio_codec"], "AAC");
        let mut competing = Encoder::new(&format!("{ingest}/{key}")); competing.rejected().await;
        assert_eq!(e.mine().await["broadcast"]["id"], first["id"]);
        let media_url = format!("{origin}/rebuild/{id}.m3u8");
        let mut decode = command("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-i", &media_url, "-map", "0:v:0", "-map", "0:a:0", "-t", "2", "-f", "null", "-"]).stdout(Stdio::null()).spawn().unwrap();
        let decode_status = tokio::time::timeout(Duration::from_secs(20), async {
            loop { if let Some(status) = decode.try_wait().unwrap() { break status; } tokio::time::sleep(Duration::from_millis(100)).await; }
        }).await;
        let _ = decode.kill(); let _ = decode.wait();
        assert!(decode_status.expect("HLS decoder timed out").success(), "Real HLS audio/video decode failed");
        eprintln!("Real media: HLS audio/video decoded");
        drop(publisher);
        wait_state(&e, "RECONNECTING").await;
        // SRS sends on_unpublish before releasing the old media connection.
        // OBS retries; wait for that release before this single FFmpeg attempt.
        let release_deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let inventory: Value = e.app.http.get(format!("{api}/api/v1/streams/")).send().await.unwrap().json().await.unwrap();
            if !inventory["streams"].as_array().unwrap().iter().any(|row|row["publish"]["active"]==true) { break; }
            assert!(Instant::now()<release_deadline, "Previous media publisher was not released");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        // A publisher can disappear from inventory before SRS releases its
        // exclusive stream token. Retry like OBS, within the persisted grace.
        let retry_deadline = Instant::now() + Duration::from_secs(30);
        let mut reconnect_attempts = 0;
        let mut resumed = loop {
            reconnect_attempts += 1;
            let mut attempt = Encoder::new(&format!("{ingest}/{key}"));
            tokio::time::sleep(Duration::from_secs(2)).await;
            if attempt.0.try_wait().unwrap().is_none() { break attempt; }
            assert!(Instant::now()<retry_deadline, "SRS did not release the retired publisher token");
        };
        let second = wait_state(&e, "LIVE").await;
        eprintln!("Real media: reconnect is LIVE");
        assert_eq!(first["id"], second["id"]);
        assert_eq!(first["started_at"], second["started_at"]);
        let rotated = e.key("rotate").await;
        resumed.rejected().await;
        wait_state(&e, "ENDED").await;
        let mut old = Encoder::new(&format!("{ingest}/{key}")); old.rejected().await;
        let mut final_publisher = Encoder::new(&format!("{ingest}/{rotated}"));
        let third = wait_state(&e, "LIVE").await;
        eprintln!("Real media: rotated publisher is LIVE");
        assert_ne!(third["id"], first["id"]);
        e.call("POST", "/api/me/stream/stop", Value::Null).await;
        final_publisher.rejected().await;
        wait_state(&e, "ENDED").await;
        let mut auto_reconnect = Encoder::new(&format!("{ingest}/{rotated}")); auto_reconnect.rejected().await;
        let end = Instant::now()+Duration::from_secs(12);
        while e.mine().await["disconnect_pending"] == true {
            assert!(Instant::now()<end, "Real disconnect was never confirmed");
            streams::tick(&e.app).await.unwrap(); tokio::time::sleep(Duration::from_millis(500)).await;
        }
        json!({"checked_at":chrono::Utc::now(),"scope":"isolated Rust API, Postgres, SRS and FFmpeg", "passed":true,
            "srs_version":ready["data"]["version"], "srs_image":SRS, "authenticated_real_callbacks":true,
            "key_rejection":true,"single_publisher":true,"hls_audio_video_decode":true,"reconnect_preserves_broadcast":true,
            "reconnect_publish_attempts":reconnect_attempts,
            "rotation_disconnects_and_rejects_old_key":true,"stop_confirms_disconnect_and_rejects_reconnect":true,
            "latency_acceptance":false,"not_established":["OBS UI","browser playback","CDN","capacity","live deployment"]})
    }).await;
    abort.abort();
    let _ = api_task.await;
    rig.cleanup().unwrap();
    result.unwrap()
}
