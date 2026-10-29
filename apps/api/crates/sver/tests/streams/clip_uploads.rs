//! Clip uploads from S.V.E.R Desktop (docs/DEVELOPER_PLATFORM.md "Desktop clip upload") against
//! real FFmpeg output: create, PUT, complete, the UPLOAD job, and where the clip then shows.
use super::*;

fn ffmpeg(args: &[&str]) {
    let out = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()
        .expect("FFmpeg is required for clip uploads");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
/// A 3-second test clip carrying a title tag the published file must drop.
fn clip(path: &std::path::Path, video: &str) {
    let path = path.to_str().unwrap();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x180:rate=25",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440",
        "-t",
        "3",
        "-c:v",
        video,
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-metadata",
        "title=Private device name",
        "-f",
        "mp4",
        path,
    ]);
}
async fn put(e: &Env, path: &str, body: Vec<u8>) -> StatusCode {
    // The ticket is the permission: no cookie, as from S.V.E.R Desktop.
    let request = Request::builder()
        .method("PUT")
        .uri(path)
        .header("origin", &e.app.config.origin)
        .extension(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()))
        .body(Body::from(body))
        .unwrap();
    sver::router(e.app.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}
async fn work(app: &App) {
    while sver::videos::worker::run_one(app, false).await.unwrap() {}
}
async fn upload(e: &Env, title: &str, file: &std::path::Path) -> String {
    let created = e
        .call(
            "POST",
            "/api/me/clips",
            json!({"title":title,"request_id":uuid::Uuid::new_v4().to_string()}),
        )
        .await;
    assert_eq!(created["status"], "UPLOADING");
    let url = created["upload"]["url"].as_str().unwrap();
    assert_eq!(
        put(e, url, std::fs::read(file).unwrap()).await,
        StatusCode::OK
    );
    let id = created["id"].as_str().unwrap().to_string();
    let done = e
        .call("POST", &format!("/api/me/clips/{id}/complete"), Value::Null)
        .await;
    assert_eq!(done["status"], "PROCESSING");
    id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn desktop_clip_uploads_check_remux_and_publish() {
    let (admin, db, schema) = isolated_database().await;
    let root = std::env::temp_dir().join(format!("sver-uploads-{}", uuid::Uuid::new_v4().simple()));
    let private = root.join("private");
    std::fs::create_dir_all(&private).unwrap();
    let mut config = Config::from_env().unwrap();
    config.resend_key.clear();
    config.videos.storage = sver::media::Storage::Filesystem(private.clone());
    config.media.storage = sver::media::Storage::Filesystem(root.join("public"));
    let app = App::new(db.clone(), config).await.unwrap();
    let e = synthetic_owner(app, Arc::new(Mutex::new(Media::default()))).await;
    let (good, mpeg4) = (root.join("good.mp4"), root.join("mpeg4.mp4"));
    clip(&good, "libx264");
    clip(&mpeg4, "mpeg4");

    // Input is checked before anything is made.
    let request_id = uuid::Uuid::new_v4().to_string();
    let (status, _) = e
        .request(
            "POST",
            "/api/me/clips",
            json!({"title":"","request_id":request_id}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = e
        .request(
            "POST",
            "/api/me/clips",
            json!({"title":"Clutch","request_id":"nope"}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = e
        .request(
            "POST",
            "/api/me/clips",
            json!({"title":"Clutch","request_id":request_id}),
            false,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "signed in only");

    // Create is idempotent; complete needs the file; a ticket only fits its own clip.
    let body = json!({"title":"Clutch round","request_id":request_id});
    let created = e.call("POST", "/api/me/clips", body.clone()).await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(
        e.call("POST", "/api/me/clips", body.clone()).await["id"],
        id
    );
    let (status, _) = e
        .request(
            "POST",
            &format!("/api/me/clips/{id}/complete"),
            Value::Null,
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "nothing uploaded yet");
    let url = created["upload"]["url"].as_str().unwrap().to_string();
    let other = e
        .call(
            "POST",
            "/api/me/clips",
            json!({"title":"Other","request_id":uuid::Uuid::new_v4().to_string()}),
        )
        .await;
    let stolen = url.replace(&id, other["id"].as_str().unwrap());
    assert_eq!(put(&e, &stolen, vec![1]).await, StatusCode::FORBIDDEN);
    assert_eq!(
        put(&e, &url, std::fs::read(&good).unwrap()).await,
        StatusCode::OK
    );
    let done = e
        .call("POST", &format!("/api/me/clips/{id}/complete"), Value::Null)
        .await;
    assert_eq!(done["status"], "PROCESSING");
    assert_eq!(
        put(&e, &url, vec![1]).await,
        StatusCode::BAD_REQUEST,
        "upload is closed"
    );
    let again = e.call("POST", "/api/me/clips", body).await;
    assert_eq!(
        (again["id"].as_str(), &again["upload"]),
        (Some(id.as_str()), &Value::Null)
    );

    // The job checks, remuxes without metadata and publishes; thumbnail and frames follow.
    work(&e.app).await;
    let state = e
        .call("GET", &format!("/api/me/clips/{id}"), Value::Null)
        .await;
    assert_eq!(state["status"], "READY", "{state}");
    assert!(
        state["url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/clips/{id}"))
    );
    let row: (i64, Option<String>, Option<String>, String) =
        sqlx::query_as("SELECT duration_ms,mp4_key,thumbnail_key,approval FROM videos WHERE id=$1")
            .bind(&id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert!((2900..=3100).contains(&row.0), "{row:?}");
    assert!(row.2.is_some(), "thumbnail from the MP4");
    assert_eq!(row.3, "APPROVED", "the streamer's own clip");
    let published = private.join(row.1.unwrap());
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(&published)
        .output()
        .unwrap();
    let info: Value = serde_json::from_slice(&probe.stdout).unwrap();
    assert_eq!(sver::videos::upload::check(&info).map(|_| ()), Ok(()));
    assert_eq!(
        info["format"]["tags"]["title"],
        Value::Null,
        "metadata stripped"
    );
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM frame_sources WHERE source=$1)")
            .bind(format!("video:{id}"))
            .fetch_one(&db)
            .await
            .unwrap(),
        "Take It Down frame matching ran"
    );
    let listed = e
        .call("GET", "/api/channels/Streamer/videos", Value::Null)
        .await;
    assert!(
        listed["videos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["video"]["id"] == id && v["thumbnail"].is_string()),
        "{listed}"
    );
    let latest = e.call("GET", "/api/clips/latest", Value::Null).await;
    assert!(
        latest["clips"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["video"]["id"] == id),
        "{latest}"
    );
    sver::videos::worker::maintain(&e.app).await.unwrap();
    assert!(
        !private.join(format!("{id}/upload")).exists(),
        "the original is removed"
    );

    // A file that isn't H.264, or isn't a video, fails the clip with a reason; it never shows.
    let refused = upload(&e, "Old codec", &mpeg4).await;
    work(&e.app).await;
    let state = e
        .call("GET", &format!("/api/me/clips/{refused}"), Value::Null)
        .await;
    assert_eq!(
        (state["status"].as_str(), state["failure"].as_str()),
        (Some("FAILED"), Some("Clips need H.264 video."))
    );
    let junk = root.join("junk.mp4");
    std::fs::write(&junk, b"not a video at all").unwrap();
    let unreadable = upload(&e, "Junk", &junk).await;
    work(&e.app).await;
    let state = e
        .call("GET", &format!("/api/me/clips/{unreadable}"), Value::Null)
        .await;
    assert_eq!(state["status"], "FAILED", "{state}");
    let latest = e.call("GET", "/api/clips/latest", Value::Null).await;
    assert_eq!(latest["clips"].as_array().unwrap().len(), 1);

    // An upload never completed is deleted once its link has long expired.
    let abandoned = other["id"].as_str().unwrap();
    e.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE videos SET created_at=now()-interval '3 hours' WHERE id='{abandoned}'"
    )))
    .await;
    sver::videos::worker::maintain(&e.app).await.unwrap();
    work(&e.app).await;
    let status: String = sqlx::query_scalar("SELECT status FROM videos WHERE id=$1")
        .bind(abandoned)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "DELETED");

    std::fs::remove_dir_all(&root).ok();
    db.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
}
