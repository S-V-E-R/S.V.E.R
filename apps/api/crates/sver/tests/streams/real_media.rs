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
        let recordings = self.directory.join("recordings");
        if recordings.exists() {
            assert!(
                recordings
                    .canonicalize()
                    .unwrap()
                    .starts_with(self.directory.canonicalize().unwrap())
            );
            fs::remove_dir_all(recordings).map_err(|_| "Recording fixture cleanup failed")?;
        }
        for name in ["srs.conf", "nginx.conf", "secret.conf"] {
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
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
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
    fs::write(rig.directory.join("srs.conf"), "listen 1935;\ndaemon off;\nsrs_log_tank console;\nsrs_log_level error;\nhttp_api { enabled on; listen 1985; }\nhttp_server { enabled on; listen 8080; dir /media; }\nvhost __defaultVhost__ {\n play { gop_cache off; }\n hls { enabled on; hls_path /media; hls_ctx off; hls_fragment 1; hls_window 6; hls_wait_keyframe on; hls_ts_file [app]/[stream]-[timestamp]-[seq].ts; }\n http_hooks { enabled on; on_publish http://hooks:8089/publish; on_unpublish http://hooks:8089/unpublish; on_hls http://hooks:8089/segment; }\n}\n").unwrap();
    let playback_config = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../infra/media/nginx-stream-playback.conf"),
    )
    .unwrap()
    .replace("127.0.0.1:8090", "origin:8080")
    .replace("127.0.0.1:1986", "origin:1985")
    .replace(
        "127.0.0.1:18080",
        &format!("host.docker.internal:{}", address.port()),
    )
    .replace("proxy_bind 127.0.0.2;", "")
    .replace(
        "/etc/nginx/sver-rebuild-hook-secret.conf",
        "/fixture/secret.conf",
    );
    fs::write(
        rig.directory.join("secret.conf"),
        format!("proxy_set_header x-srs-secret {secret};\n"),
    )
    .unwrap();
    fs::write(rig.directory.join("nginx.conf"), format!("events {{}}\nhttp {{ access_log off; error_log /dev/null; server {{ listen 8089; location / {{ proxy_set_header x-srs-secret {secret}; proxy_pass http://host.docker.internal:{}/api/internal/srs/; }} }} server {{ listen 8091; {playback_config} }} }}", address.port())).unwrap();
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
    let srs = rig.start(
        "srs",
        &[
            "--network-alias",
            "origin",
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
    let proxy = rig.start(
        "hooks",
        &[
            "--network-alias",
            "hooks",
            "-p",
            "127.0.0.1::8091",
            "-v",
            &nginx_mount,
            "-v",
            &mount,
            "nginx:1.28-alpine",
        ],
    );
    let api = format!("http://127.0.0.1:{}", port(&srs, "1985/tcp"));
    let ingest = format!("rtmp://127.0.0.1:{}/rebuild", port(&srs, "1935/tcp"));
    let origin = format!("http://127.0.0.1:{}", port(&srs, "8080/tcp"));
    let public = format!("http://127.0.0.1:{}", port(&proxy, "8091/tcp"));
    let mut config = Config::from_env().unwrap();
    assert!(!config.production, "Never use production configuration");
    config.resend_key.clear();
    config.staff_push = Default::default();
    config.turnstile_secret = "1x000-synthetic-local-turnstile".into();
    config.turnstile_url = format!("http://{address}/synthetic/turnstile");
    config.videos.storage = sver::media::Storage::Filesystem(rig.directory.join("recordings"));
    config.videos.segment_base = origin.clone();
    config.streaming = Some(streams::Config {
        api_url: api.clone(),
        ingest_url: ingest.clone(),
        whip_url: None,
        srt_url: None,
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
            sver::router(app)
                .route(
                    "/synthetic/turnstile",
                    axum::routing::post(|| async { Json(json!({"success":true})) }),
                )
                .into_make_service_with_connect_info::<SocketAddr>(),
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
        let media_url = format!("{public}/rebuild/{id}.m3u8");
        let mut decode = command("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-i", &media_url, "-map", "0:v:0", "-map", "0:a:0", "-t", "2", "-f", "null", "-"]).stdout(Stdio::null()).spawn().unwrap();
        let decode_status = tokio::time::timeout(Duration::from_secs(20), async {
            loop { if let Some(status) = decode.try_wait().unwrap() { break status; } tokio::time::sleep(Duration::from_millis(100)).await; }
        }).await;
        let _ = decode.kill(); let _ = decode.wait();
        assert!(decode_status.expect("HLS decoder timed out").success(), "Real HLS audio/video decode failed");
        eprintln!("Real media: HLS audio/video decoded");
        let playlist = e.app.http.get(&media_url).send().await.unwrap();
        assert_eq!(playlist.headers()["cache-control"],"no-store");
        let private_gate = e.app.http.get(format!("{public}/_rebuild_playback_auth")).send().await.unwrap();
        assert_eq!(private_gate.status(),StatusCode::NOT_FOUND,"Clients cannot invoke the internal authorization subrequest");
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
        tokio::time::sleep(Duration::from_secs(4)).await;
        let rotated = e.key("rotate").await;
        resumed.rejected().await;
        wait_state(&e, "ENDED").await;
        let vod: String = sqlx::query_scalar("SELECT id FROM videos WHERE broadcast_id=$1 AND kind='VOD'").bind(first["id"].as_str().unwrap()).fetch_one(&e.app.db).await.expect("Real SRS on_hls callbacks must record the broadcast");
        let recording_deadline = Instant::now() + Duration::from_secs(55);
        loop {
            while sver::videos::worker::run_one(&e.app,true).await.unwrap() {}
            sver::videos::worker::maintain(&e.app).await.unwrap();
            while sver::videos::worker::run_one(&e.app,false).await.unwrap() {}
            let page = e.call("GET", &format!("/api/videos/{vod}"),Value::Null).await;
            if page["video"]["status"] == "READY" { break; }
            assert!(Instant::now()<recording_deadline,"VOD did not become ready within a minute");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let (status,page)=chat::call(&e,"GET",&format!("/api/videos/{vod}"),None,Value::Null).await;
        assert_eq!(status,StatusCode::OK);
        let playback=format!("http://{address}{}",page["playback"].as_str().unwrap());
        let manifest=e.app.http.get(&playback).send().await.unwrap().text().await.unwrap();
        assert!(manifest.contains("#EXT-X-DISCONTINUITY"),"Reconnect marks its changed media timestamps");
        assert!(manifest.contains("#EXT-X-ENDLIST"));
        let output=command("ffmpeg").args(["-hide_banner","-loglevel","error","-i",&playback,"-map","0:v:0","-map","0:a:0","-f","null","-"]).stdout(Stdio::null()).spawn().unwrap();
        let mut output=Encoder(output);
        let deadline=Instant::now()+Duration::from_secs(30);
        loop {
            if let Some(status)=output.0.try_wait().unwrap() { assert!(status.success(),"Private VOD must decode across a real reconnect"); break; }
            assert!(Instant::now()<deadline,"VOD decoding timed out");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        eprintln!("Real media: private recording ready within a minute and decoded across reconnect");
        let cut_end=page["video"]["duration_ms"].as_i64().unwrap().min(60000);
        let cut=e.call("POST",&format!("/api/videos/{vod}/cuts"),json!({"kind":"CLIP","title":"Reconnect moment","start_ms":0,"end_ms":cut_end,"request_id":uuid::Uuid::new_v4().to_string()})).await;
        while sver::videos::worker::run_one(&e.app,false).await.unwrap() {}
        let (_,clip)=chat::call(&e,"GET",&format!("/api/videos/{}",cut["id"].as_str().unwrap()),None,Value::Null).await;
        assert_eq!(clip["video"]["status"],"READY");
        let file=format!("http://{address}{}",clip["playback"].as_str().unwrap());
        let mut probe=tokio::process::Command::new("ffprobe");
        probe.args(["-v","error","-show_entries","format=duration","-of","json",&file])
            .stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        probe.creation_flags(0x08000000);
        let probe=tokio::time::timeout(Duration::from_secs(20),probe.output()).await.expect("Reconnect clip probe timed out").unwrap();
        assert!(probe.status.success(),"Reconnect clip must be a readable MP4");
        let probe:Value=serde_json::from_slice(&probe.stdout).unwrap();
        let seconds:f64=probe["format"]["duration"].as_str().unwrap().parse().unwrap();
        let expected=clip["video"]["duration_ms"].as_i64().unwrap() as f64/1000.0;
        assert!((seconds-expected).abs()<1.0,"Reconnect MP4 timeline {seconds} differs from selected segment duration {expected}");
        eprintln!("Real media: MP4 cut preserves the reconnect timeline");
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
        assert_eq!(e.app.http.get(&media_url).send().await.unwrap().status(),StatusCode::FORBIDDEN,"A retired public URL stays closed after key rotation");

        // Exercise the anonymous request and staff review against an actual publisher and
        // the repository's Nginx playback config, including a previously saved segment URL.
        let removal_key = e.key("rotate").await;
        let removal_public_id = removal_key.split('?').next().unwrap();
        let mut removal_publisher = Encoder::new(&format!("{ingest}/{removal_key}"));
        let removal_broadcast = wait_state(&e,"LIVE").await;
        let removal_url = format!("{public}/rebuild/{removal_public_id}.m3u8");
        let until = Instant::now()+Duration::from_secs(12);
        let saved_segment = loop {
            let response = e.app.http.get(&removal_url).send().await.unwrap();
            if response.status().is_success() {
                let body=response.text().await.unwrap();
                if let Some(segment) = body.lines().rfind(|l| !l.starts_with('#') && l.ends_with(".ts")) { break segment.to_string(); }
            }
            assert!(Instant::now()<until,"HLS did not publish a test segment");
            tokio::time::sleep(Duration::from_millis(200)).await;
        };
        let segment_url = format!("{public}/rebuild/{saved_segment}");
        assert_eq!(e.app.http.get(&segment_url).send().await.unwrap().status(),StatusCode::OK);
        let (status,receipt)=chat::call(&e,"POST","/api/take-it-down",None,json!({"name":"Synthetic Requester","email":"requester@example.invalid","capacity":"shown","locations":[format!("{}/Streamer/live?report=live_stream&id={}",e.app.config.origin,removal_broadcast["id"].as_str().unwrap())],"description":"Synthetic color-bar broadcast only","good_faith":true,"signature":"Synthetic Requester","signed_on":chrono::Utc::now().date_naive(),"turnstile_token":"synthetic"})).await;
        assert_eq!(status,StatusCode::OK,"{receipt}");
        assert_eq!(e.app.http.get(&segment_url).send().await.unwrap().status(),StatusCode::OK,"A live report waits for staff review before stopping media");
        let staff=chat::person(&e,"removal-staff","RemovalStaff",true).await;
        e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='removal-staff'").await;
        e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='removal-staff'").await;
        e.sql("INSERT INTO staff_roles(user_id,role) VALUES('removal-staff','admin')").await;
        let decision=format!("/api/admin/take-it-down/{}",receipt["number"].as_str().unwrap());
        let (status,body)=chat::call(&e,"POST",&decision,Some(&staff),json!({"action":"review","reason":"Synthetic staff review"})).await;
        assert_eq!(status,StatusCode::OK,"{body}");
        for url in [&removal_url,&segment_url] {
            assert_eq!(e.app.http.get(url).send().await.unwrap().status(),StatusCode::FORBIDDEN,"Stopped media must be inaccessible even while SRS retains the file");
            assert_eq!(e.app.http.head(url).send().await.unwrap().status(),StatusCode::FORBIDDEN);
        }
        let whep=e.app.http.post(format!("{public}/rebuild/whep/?app=rebuild&stream={removal_public_id}")).body("synthetic SDP").send().await.unwrap();
        assert_eq!(whep.status(),StatusCode::FORBIDDEN,"New WebRTC sessions cannot reconnect to a removed stream");
        let origin_saved=e.app.http.get(format!("{origin}/rebuild/{saved_segment}")).send().await.unwrap();
        assert_eq!(origin_saved.status(),StatusCode::OK,"The check must prove denial before SRS deletes its rolling files");
        removal_publisher.rejected().await;
        let disconnect_deadline=Instant::now()+Duration::from_secs(30);
        loop {
            let (status,body)=chat::call(&e,"POST",&decision,Some(&staff),json!({"action":"remove","reason":"Synthetic valid removal"})).await;
            if status==StatusCode::OK { break; }
            assert_eq!(status,StatusCode::SERVICE_UNAVAILABLE,"{body}");
            assert!(Instant::now()<disconnect_deadline,"Removal never confirmed the SRS disconnect: {body}");
            streams::tick(&e.app).await.unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let (status,outcome)=chat::call(&e,"POST","/api/take-it-down/status",None,json!({"number":receipt["number"],"email":"requester@example.invalid","turnstile_token":"synthetic"})).await;
        assert_eq!(status,StatusCode::OK);
        assert_eq!(outcome["status"],"removed");
        let mut removed_reconnect=Encoder::new(&format!("{ingest}/{removal_key}")); removed_reconnect.rejected().await;
        assert!(streams::confirm_stopped(&e.app,"stream-owner").await.unwrap());
        eprintln!("Real media: anonymous removal stops SRS, denies saved HLS/WHEP URLs and rejects republishing");
        e.app.db.close().await;
        assert_eq!(e.app.http.get(&segment_url).send().await.unwrap().status(),StatusCode::INTERNAL_SERVER_ERROR,"An unavailable authorization database must not expose retained origin files");
        json!({"checked_at":chrono::Utc::now(),"scope":"isolated Rust API, Postgres, SRS and FFmpeg", "passed":true,
            "srs_version":ready["data"]["version"], "srs_image":SRS, "authenticated_real_callbacks":true,
            "key_rejection":true,"single_publisher":true,"hls_audio_video_decode":true,"reconnect_preserves_broadcast":true,
            "reconnect_publish_attempts":reconnect_attempts,
            "rotation_disconnects_and_rejects_old_key":true,"stop_confirms_disconnect_and_rejects_reconnect":true,
            "take_down_stops_publisher":true,"take_down_blocks_saved_hls_and_whep":true,"take_down_rejects_republish":true,
            "playback_denies_when_authorization_unavailable":true,"recording_callbacks":true,"recording_ready_within_one_minute":true,"recording_decodes_across_reconnect":true,"recording_mp4_reconnect_timeline":true,
            "latency_acceptance":false,"not_established":["OBS UI","browser playback","CDN","capacity","live deployment"]})
    }).await;
    abort.abort();
    let _ = api_task.await;
    rig.cleanup().unwrap();
    result.unwrap()
}
