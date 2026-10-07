//! Module 9 acceptance (docs/BEACONS.md "Done when") against real FFmpeg output.
use super::chat::{call, person};
use super::*;

async fn fetch(app: &App, path: &str, cookie: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut request = Request::builder()
        .uri(path)
        .extension(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    if let Some(cookie) = cookie {
        request = request.header("cookie", format!("{}={cookie}", app.config.cookie_name()));
    }
    let response = sver::router(app.clone())
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
}
async fn put_bytes(e: &Env, path: &str, body: Vec<u8>) -> StatusCode {
    let request = Request::builder()
        .method("PUT")
        .uri(path)
        .header("origin", &e.app.config.origin)
        .header("cookie", format!("sver_dev={}", e.cookie))
        .extension(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()))
        .body(Body::from(body))
        .unwrap();
    sver::router(e.app.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}
fn ffmpeg(args: &[&str]) {
    let out = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()
        .expect("FFmpeg is required for Beacon acceptance");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
/// A landscape test video carrying location, device and editing metadata the output must drop.
fn source(path: &std::path::Path, seconds: &str, size: &str) {
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        &format!("testsrc2=size={size}:rate=25"),
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440",
        "-t",
        seconds,
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-g",
        "25",
        "-bf",
        "0",
        "-c:a",
        "aac",
        "-shortest",
        "-metadata",
        "location=+40.7128-074.0060/",
        "-metadata",
        "com.apple.quicktime.make=SyntheticPhone",
        "-metadata",
        "title=Private working title",
        "-metadata",
        "creation_time=2026-01-01T00:00:00Z",
        "-movflags",
        "+use_metadata_tags",
        path.to_str().unwrap(),
    ]);
}
fn probe(path: &std::path::Path) -> Value {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}
/// A grayscale frame at 216×384 for comparing the watermarked copy against the clean one.
fn frame(path: &std::path::Path, at: f64) -> Vec<u8> {
    let out = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{at:.3}"),
            "-i",
        ])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=216:384,format=gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert_eq!(out.stdout.len(), 216 * 384);
    out.stdout
}
/// Mean difference where one corner's mark sits on a 216x384 frame. The left mark spans about
/// 6-43% of the width and the right one 43-80%, so only each mark's outer part is compared.
fn corner_difference(a: &[u8], b: &[u8], corner: sver::beacons::watermark::Corner) -> f64 {
    use sver::beacons::watermark::Corner::*;
    let (x0, x1) = match corner {
        TopLeft | BottomLeft => (13, 70),
        TopRight | BottomRight => (112, 172),
    };
    let (y0, y1) = match corner {
        TopLeft | TopRight => (22, 46),
        BottomLeft | BottomRight => (272, 296),
    };
    let mut total = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            total += (a[y * 216 + x] as f64 - b[y * 216 + x] as f64).abs();
        }
    }
    total / ((x1 - x0) * (y1 - y0)) as f64
}
async fn work(app: &App) {
    while sver::beacons::worker::run_one(app).await.unwrap() {}
}
async fn beacon(e: &Env, id: &str) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(b) FROM beacons b WHERE id=$1")
        .bind(id)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn beacons_publish_watermark_count_and_remove() {
    let (admin, db, schema) = isolated_database().await;
    let root = std::env::temp_dir().join(format!("sver-beacons-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(root.join("private")).unwrap();
    let fake = Arc::new(Mutex::new(Media::default()));
    let mut config = Config::from_env().unwrap();
    config.resend_key.clear();
    config.videos.storage = sver::media::Storage::Filesystem(root.join("private"));
    config.media.storage = sver::media::Storage::Filesystem(root.join("public"));
    config.take_down.purge_url.clear();
    config.beacons.uploads = true;
    config.beacons.tuning.likes_per_user_hour = 4;
    let app = App::new(db.clone(), config).await.unwrap();
    let e = synthetic_owner(app, fake).await;
    let viewer = person(&e, "bc-viewer", "BeaconViewer", true).await;
    let other = person(&e, "bc-other", "OtherCreator", true).await;

    // A clip of the channel: a 16:9 MP4 like the clip worker produces.
    let clip_file = root.join("clip.mp4");
    source(&clip_file, "8", "640x360");
    let clip = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(root.join("private").join(&clip)).unwrap();
    std::fs::copy(
        &clip_file,
        root.join("private").join(&clip).join("clip.mp4"),
    )
    .unwrap();
    sqlx::query("INSERT INTO videos(id,owner_id,clipper_id,kind,status,approval,visibility,title,category,genre,started_at,duration_ms,mp4_key) VALUES($1,'stream-owner','bc-viewer','CLIP','READY','APPROVED','PUBLIC','Big play','Minecraft','sandbox',now(),8000,$2)")
        .bind(&clip).bind(format!("{clip}/clip.mp4")).execute(&db).await.unwrap();
    let crop = json!({"x":0.3,"y":0.0,"width":0.3164});
    let create = |request: &str| json!({"source":"CLIP","clip_id":clip,"crop":crop,"title":"Clutch round","request_id":request});

    // Only people who have streamed at least once can post.
    let first = uuid::Uuid::new_v4().to_string();
    let (status, _) = e
        .request("POST", "/api/me/beacons", create(&first), true, true, false)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,ended_at,confirmed_live_at) VALUES('bc-b1','stream-owner','p',1,'ENDED','s','s','c',now()-interval '1 day',now()-interval '1 day',now()-interval '1 day',now()-interval '20 hours',now()-interval '1 day')").await;
    let studio = e.call("GET", "/api/me/beacons", Value::Null).await;
    assert_eq!(studio["eligible"], true);
    assert_eq!(studio["clips"][0]["id"], clip);

    // From a clip: a chosen 9:16 crop, one durable job, and idempotent retries of the request.
    let created = e.call("POST", "/api/me/beacons", create(&first)).await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(
        e.call("POST", "/api/me/beacons", create(&first)).await["id"],
        id
    );
    assert_eq!(beacon(&e, &id).await["status"], "PROCESSING");
    // A crashed lease is recovered and the job still runs exactly once.
    e.sql("UPDATE beacon_jobs SET lease_until=now()-interval '1 second',lease_token='dead'")
        .await;
    work(&e.app).await;
    let row = beacon(&e, &id).await;
    assert_eq!(row["status"], "PUBLISHED", "{row}");
    assert_eq!(row["clipper_id"], "bc-viewer");
    assert_eq!(
        e.call("GET", "/api/me/beacons", Value::Null).await["last_crop"],
        crop
    );
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT beacon_approved FROM videos WHERE id=$1")
            .bind(&clip)
            .fetch_one(&db)
            .await
            .unwrap()
    );

    // Outputs: 9:16 H.264/AAC at 1080×1920 and 720×1280, fast start, metadata stripped.
    let private = root.join("private");
    let hd = private.join(row["mp4_key"].as_str().unwrap());
    let sd = private.join(row["mp4_small_key"].as_str().unwrap());
    let clean = private.join(row["clean_key"].as_str().unwrap());
    assert!(
        private
            .join(row["thumbnail_key"].as_str().unwrap())
            .exists()
    );
    for (path, width, height) in [(&hd, 1080, 1920), (&sd, 720, 1280), (&clean, 1080, 1920)] {
        let info = probe(path);
        let video = info["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["codec_type"] == "video")
            .unwrap();
        assert_eq!(
            (
                video["width"].as_i64().unwrap(),
                video["height"].as_i64().unwrap()
            ),
            (width, height)
        );
        assert_eq!(video["codec_name"], "h264");
        assert!(
            info["streams"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["codec_name"] == "aac")
        );
        let tags = info["format"]["tags"].to_string().to_lowercase();
        for leaked in [
            "location",
            "syntheticphone",
            "private working title",
            "creation_time",
            "encoder",
        ] {
            assert!(!tags.contains(leaked), "{leaked} survived in {tags}");
        }
        // Fast start: the moov box precedes the media data.
        let bytes = std::fs::read(path).unwrap();
        let moov = bytes.windows(4).position(|w| w == b"moov").unwrap();
        let mdat = bytes.windows(4).position(|w| w == b"mdat").unwrap();
        assert!(moov < mdat);
    }
    // The watermark sits in the planned corner, then moves to the next one.
    let seed: i64 = sqlx::query_scalar("SELECT seed FROM beacons WHERE id=$1")
        .bind(&id)
        .fetch_one(&db)
        .await
        .unwrap();
    let steps = sver::beacons::watermark::plan(seed, 8000, 3000, 6000);
    for (at, corner) in [
        (0.3, steps[0].1),
        (steps[1].0 as f64 / 1000.0 + 0.4, steps[1].1),
    ] {
        let (marked, plain) = (frame(&hd, at), frame(&clean, at));
        let here = corner_difference(&marked, &plain, corner);
        use sver::beacons::watermark::Corner::*;
        for elsewhere in [TopLeft, TopRight, BottomRight, BottomLeft]
            .into_iter()
            .filter(|c| *c != corner)
        {
            let there = corner_difference(&marked, &plain, elsewhere);
            assert!(
                here > there * 3.0 + 0.5,
                "watermark not in {corner:?} at {at}: {here} vs {there} in {elsewhere:?}"
            );
        }
    }

    // Public copies play with a ticket; the clean copy only for its creator.
    let page = call(&e, "GET", &format!("/api/beacons/{id}"), None, Value::Null)
        .await
        .1;
    let hd_url = page["playback"]["hd"].as_str().unwrap().to_string();
    assert_eq!(fetch(&e.app, &hd_url, None).await.0, StatusCode::OK);
    assert_eq!(
        fetch(&e.app, &hd_url.replace("q=hd", "q=clean"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &e,
            "POST",
            &format!("/api/beacons/{id}/download"),
            Some(&viewer),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let clean_url = e
        .call("POST", &format!("/api/beacons/{id}/download"), Value::Null)
        .await["url"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, body) = fetch(&e.app, &clean_url, Some(&e.cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, std::fs::read(&clean).unwrap());
    assert_eq!(
        fetch(&e.app, &clean_url, Some(&viewer)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fetch(&e.app, &clean_url, None).await.0,
        StatusCode::FORBIDDEN
    );

    // Uploads: the server's own probe decides. Not a video, too long and too big never publish.
    let upload = |title: &str| json!({"source":"UPLOAD","title":title,"category_id":"art","request_id":uuid::Uuid::new_v4().to_string()});
    let fake_video = e
        .call("POST", "/api/me/beacons", upload("Not a video"))
        .await;
    let fake_id = fake_video["id"].as_str().unwrap().to_string();
    assert_eq!(
        e.request(
            "POST",
            &format!("/api/beacons/{fake_id}/complete"),
            Value::Null,
            true,
            true,
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        put_bytes(
            &e,
            fake_video["upload"]["url"].as_str().unwrap(),
            b"<html>video.mp4</html>".repeat(100)
        )
        .await,
        StatusCode::OK
    );
    e.call(
        "POST",
        &format!("/api/beacons/{fake_id}/complete"),
        Value::Null,
    )
    .await;
    let long_file = root.join("long.mov");
    source(&long_file, "62", "320x180");
    let long = e.call("POST", "/api/me/beacons", upload("Too long")).await;
    let long_id = long["id"].as_str().unwrap().to_string();
    put_bytes(
        &e,
        long["upload"]["url"].as_str().unwrap(),
        std::fs::read(&long_file).unwrap(),
    )
    .await;
    e.call(
        "POST",
        &format!("/api/beacons/{long_id}/complete"),
        Value::Null,
    )
    .await;
    let big = e.call("POST", "/api/me/beacons", upload("Too big")).await;
    let big_id = big["id"].as_str().unwrap().to_string();
    let big_path = private.join(format!("beacons/{big_id}/upload"));
    std::fs::create_dir_all(big_path.parent().unwrap()).unwrap();
    std::fs::File::create(&big_path)
        .unwrap()
        .set_len(200 * 1024 * 1024 + 1)
        .unwrap();
    e.call(
        "POST",
        &format!("/api/beacons/{big_id}/complete"),
        Value::Null,
    )
    .await;
    let tall_file = root.join("tall.mp4");
    source(&tall_file, "6", "480x360");
    let good = e
        .call("POST", "/api/me/beacons", upload("Workbench build"))
        .await;
    let good_id = good["id"].as_str().unwrap().to_string();
    put_bytes(
        &e,
        good["upload"]["url"].as_str().unwrap(),
        std::fs::read(&tall_file).unwrap(),
    )
    .await;
    e.call(
        "POST",
        &format!("/api/beacons/{good_id}/complete"),
        Value::Null,
    )
    .await;
    work(&e.app).await;
    assert_eq!(
        beacon(&e, &fake_id).await["failure"],
        "That file isn't a video we can read."
    );
    assert_eq!(
        beacon(&e, &long_id).await["failure"],
        "Beacons are 5–60 seconds long."
    );
    assert_eq!(
        beacon(&e, &big_id).await["failure"],
        "Videos can be up to 200 MB."
    );
    for failed in [&fake_id, &long_id, &big_id] {
        assert_eq!(beacon(&e, failed).await["status"], "FAILED");
    }
    let good_row = beacon(&e, &good_id).await;
    assert_eq!(good_row["status"], "PUBLISHED");
    assert_eq!(
        good_row["upload_key"],
        Value::Null,
        "the upload is removed once processed"
    );
    assert!(!private.join(format!("beacons/{good_id}/upload")).exists());
    // Uploads are padded, not stretched: the 4:3 picture keeps black bars above and below
    // (rows 0-110 of 384). Rows 24-46 are skipped, where a top-corner watermark may sit.
    let padded = frame(&private.join(good_row["mp4_key"].as_str().unwrap()), 1.0);
    assert!(padded[..216 * 20].iter().all(|p| *p < 24));
    assert!(padded[216 * 50..216 * 100].iter().all(|p| *p < 24));

    // A Take It Down blocklist match never publishes, including a retry of the same file.
    let blocked = e
        .call("POST", "/api/me/beacons", upload("Blocked copy"))
        .await;
    let blocked_id = blocked["id"].as_str().unwrap().to_string();
    put_bytes(
        &e,
        blocked["upload"]["url"].as_str().unwrap(),
        std::fs::read(&tall_file).unwrap(),
    )
    .await;
    let hash: String = sqlx::query_scalar("SELECT hashes[1] FROM beacons WHERE id=$1")
        .bind(&good_id)
        .fetch_one(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO blocked_media_hashes(hash) VALUES($1)")
        .bind(&hash)
        .execute(&db)
        .await
        .unwrap();
    e.call(
        "POST",
        &format!("/api/beacons/{blocked_id}/complete"),
        Value::Null,
    )
    .await;
    work(&e.app).await;
    assert_eq!(
        beacon(&e, &blocked_id).await["failure"],
        "This video can't be published."
    );
    e.call(
        "POST",
        &format!("/api/beacons/{blocked_id}/retry"),
        Value::Null,
    )
    .await;
    work(&e.app).await;
    assert_eq!(beacon(&e, &blocked_id).await["status"], "FAILED");
    sqlx::query("DELETE FROM blocked_media_hashes WHERE hash=$1")
        .bind(&hash)
        .execute(&db)
        .await
        .unwrap();

    // Ten a day; failures give their slot back.
    e.sql("INSERT INTO beacons(id,owner_id,source,status,title,seed,request_key) SELECT gen_random_uuid()::text,'stream-owner','CLIP','DELETED','Filler',1,gen_random_uuid()::text FROM generate_series(1,8)").await;
    assert_eq!(
        e.call("GET", "/api/me/beacons", Value::Null).await["remaining"],
        0
    );
    let (status, body) = e
        .request(
            "POST",
            "/api/me/beacons",
            create(&uuid::Uuid::new_v4().to_string()),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    e.sql("DELETE FROM beacons WHERE title='Filler'").await;

    // The feed: followed, faction and fair-rotation sources; signed out sees the rotation only.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,ended_at,confirmed_live_at) VALUES('bc-b2','bc-other','q',1,'ENDED','s','s','c',now(),now(),now(),now(),now())").await;
    e.sql("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at) VALUES('bc-viewer','glint',now(),now()),('bc-other','glint',now(),now())").await;
    for n in 0..3 {
        sqlx::query("INSERT INTO beacons(id,owner_id,source,status,title,seed,request_key,published_at,duration_ms) VALUES($1,'bc-other','CLIP','PUBLISHED',$2,1,$1,now()-make_interval(mins=>$3),6000)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(format!("Other {n}")).bind(n).execute(&db).await.unwrap();
    }
    // Huge view and like counts never move a Beacon up.
    e.sql("UPDATE beacons SET views=1000000,likes=1000000 WHERE title='Other 2'")
        .await;
    let guest = call(&e, "GET", "/api/beacons/feed?seed=abc", None, Value::Null)
        .await
        .1;
    let owners: Vec<String> = guest["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["channel"]["username"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(owners.len(), 5);
    // Every creator gets a turn before anyone gets a second.
    assert_ne!(owners[0], owners[1]);
    let others: Vec<&str> = guest["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["channel"]["username"] == "OtherCreator")
        .map(|i| i["beacon"]["title"].as_str().unwrap())
        .collect();
    assert_eq!(others, ["Other 0", "Other 1", "Other 2"]);
    let mine = call(
        &e,
        "GET",
        "/api/beacons/feed?seed=abc",
        Some(&viewer),
        Value::Null,
    )
    .await
    .1;
    // The viewer's faction source leads with the faction creator before the rotation.
    assert_eq!(mine["items"][0]["channel"]["username"], "OtherCreator");
    call(
        &e,
        "PUT",
        "/api/beacons/mutes/OtherCreator",
        Some(&viewer),
        Value::Null,
    )
    .await;
    let muted = call(
        &e,
        "GET",
        "/api/beacons/feed?seed=abc",
        Some(&viewer),
        Value::Null,
    )
    .await
    .1;
    assert!(
        muted["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["channel"]["username"] != "OtherCreator")
    );
    call(
        &e,
        "DELETE",
        "/api/beacons/mutes/OtherCreator",
        Some(&viewer),
        Value::Null,
    )
    .await;
    // 18+ Beacons appear only for viewers who passed the gate.
    e.sql("UPDATE beacons SET mature=true WHERE title='Other 1'")
        .await;
    let gated = call(&e, "GET", "/api/beacons/feed?seed=abc", None, Value::Null)
        .await
        .1;
    assert!(
        gated["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["beacon"]["title"] != "Other 1")
    );
    let passed = call(
        &e,
        "GET",
        "/api/beacons/feed?seed=abc&age_ack=true",
        Some(&viewer),
        Value::Null,
    )
    .await
    .1;
    assert!(
        passed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["beacon"]["title"] == "Other 1")
    );
    e.sql("UPDATE beacons SET mature=false").await;

    // Views: 3 seconds of visible, advancing playback from a counted session, once a day.
    let beat_path = format!("/api/beacons/{id}/beat");
    assert_eq!(
        call(
            &e,
            "POST",
            &beat_path,
            Some(&viewer),
            json!({"media_time":0.0,"visible":true})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let browser = uuid::Uuid::new_v4().to_string();
    let beat =
        |at: f64, visible: bool| json!({"browser_id":browser,"media_time":at,"visible":visible});
    assert_eq!(
        call(&e, "POST", &beat_path, Some(&viewer), beat(0.0, true))
            .await
            .1["counted"],
        false
    );
    // Hidden-tab playback earns nothing.
    e.sql("UPDATE beacon_playback SET updated_at=now()-interval '4 seconds'")
        .await;
    assert_eq!(
        call(&e, "POST", &beat_path, Some(&viewer), beat(4.0, false))
            .await
            .1["counted"],
        false
    );
    e.sql("UPDATE beacon_playback SET updated_at=now()-interval '4 seconds',media_time=0")
        .await;
    assert_eq!(
        call(&e, "POST", &beat_path, Some(&viewer), beat(4.0, true))
            .await
            .1["counted"],
        true
    );
    e.sql("UPDATE beacon_playback SET updated_at=now()-interval '4 seconds'")
        .await;
    call(&e, "POST", &beat_path, Some(&viewer), beat(7.9, true)).await;
    assert_eq!(beacon(&e, &id).await["views"], 1);
    // The creator's own playback is never counted.
    let own = e
        .call(
            "POST",
            &beat_path,
            json!({"browser_id":browser,"media_time":1.0,"visible":true}),
        )
        .await;
    assert_eq!(own["recorded"], false);

    // Likes: signed in only, one per account, can be taken back, rate-limited.
    let like = format!("/api/beacons/{id}/like");
    assert_eq!(
        call(&e, "PUT", &like, None, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&e, "PUT", &like, Some(&viewer), Value::Null).await.1["likes"],
        1
    );
    assert_eq!(
        call(&e, "PUT", &like, Some(&viewer), Value::Null).await.1["likes"],
        1
    );
    assert_eq!(
        call(&e, "DELETE", &like, Some(&viewer), Value::Null)
            .await
            .1["likes"],
        0
    );
    call(&e, "PUT", &like, Some(&viewer), Value::Null).await;
    assert_eq!(
        call(&e, "PUT", &like, Some(&viewer), Value::Null).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );

    // Live now jumps to the stream; the tap becomes a live join once that session is counted.
    let tap = format!("/api/beacons/{id}/live");
    assert_eq!(
        call(
            &e,
            "POST",
            &tap,
            Some(&viewer),
            json!({"browser_id":browser})
        )
        .await
        .1["live"],
        false
    );
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,confirmed_live_at) VALUES('bc-live','stream-owner','p',2,'LIVE','s','s','c',now(),now(),now(),now())").await;
    let feed = call(
        &e,
        "GET",
        "/api/beacons/feed?seed=abc",
        Some(&viewer),
        Value::Null,
    )
    .await
    .1;
    assert!(
        feed["live"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["username"] == "Streamer")
    );
    let jumped = call(
        &e,
        "POST",
        &tap,
        Some(&viewer),
        json!({"browser_id":browser}),
    )
    .await
    .1;
    assert_eq!(jumped["url"], "/Streamer/live");
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('bc-live','u:bc-viewer',now()+interval '1 minute','counted')").await;
    sver::beacons::worker::maintain(&e.app).await.unwrap();
    assert_eq!(
        call(
            &e,
            "PUT",
            "/api/follows/Streamer",
            Some(&viewer),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &e,
            "POST",
            &format!("/api/beacons/{id}/followed"),
            Some(&viewer),
            Value::Null
        )
        .await
        .1["recorded"],
        true
    );
    let studio = e.call("GET", "/api/me/beacons", Value::Null).await;
    let stats = &studio["beacons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["beacon"]["id"] == id)
        .unwrap()["stats"];
    assert_eq!(stats["live_joins"], 1, "{stats}");
    assert_eq!(stats["follows"], 1);
    assert_eq!(stats["completions"], 1);

    // Where Beacons appear: home shelf, channel tab, search and link previews.
    assert!(
        call(&e, "GET", "/api/beacons/shelf", None, Value::Null)
            .await
            .1["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["beacon"]["id"] == id)
    );
    assert!(
        call(
            &e,
            "GET",
            "/api/channels/Streamer/beacons",
            None,
            Value::Null
        )
        .await
        .1["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["beacon"]["id"] == id)
    );
    assert!(
        call(&e, "GET", "/api/search?q=Clutch", None, Value::Null)
            .await
            .1["beacons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["beacon"]["id"] == id)
    );
    let share = call(
        &e,
        "GET",
        &format!("/api/beacons/{id}/share"),
        None,
        Value::Null,
    )
    .await
    .1;
    assert_eq!(
        share["url"],
        format!("{}/beacons/{id}", e.app.config.origin)
    );
    assert!(
        share["thumbnail"]
            .as_str()
            .unwrap()
            .contains("/thumbnail?ticket=")
    );

    // Reports reach the admin queue; the Beacon keeps playing until staff act.
    let (status, _) = call(
        &e,
        "POST",
        "/api/reports",
        Some(&viewer),
        json!({"target_type":"beacon","target_id":id,"reason":"spam"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&e, "GET", &format!("/api/beacons/{id}"), None, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    let staff = person(&e, "bc-staff", "BeaconStaff", true).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='bc-staff'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='bc-staff'")
        .await;
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('bc-staff','admin')")
        .await;
    let queue = call(&e, "GET", "/api/admin/reports", Some(&staff), Value::Null)
        .await
        .1;
    assert!(queue.to_string().contains(&id), "{queue}");
    assert_eq!(
        call(
            &e,
            "POST",
            &format!("/api/admin/beacons/{id}/hide"),
            Some(&viewer),
            json!({"hidden":true,"reason":"Under review"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, body) = call(
        &e,
        "POST",
        &format!("/api/admin/beacons/{id}/hide"),
        Some(&staff),
        json!({"hidden":true,"reason":"Under review"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        call(&e, "GET", &format!("/api/beacons/{id}"), None, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    call(
        &e,
        "POST",
        &format!("/api/admin/beacons/{id}/hide"),
        Some(&staff),
        json!({"hidden":false,"reason":"Cleared"}),
    )
    .await;
    let (status, body) = call(
        &e,
        "POST",
        &format!("/api/admin/reports/beacon/{id}/actions"),
        Some(&staff),
        json!({"action":"remove_content","note":"reviewed"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(beacon(&e, &id).await["status"], "REMOVED");
    assert_eq!(
        call(&e, "GET", &format!("/api/beacons/{id}"), None, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // Deleting removes every rendition, the thumbnail and the clean copy.
    e.call("DELETE", &format!("/api/beacons/{id}"), Value::Null)
        .await;
    work(&e.app).await;
    assert_eq!(beacon(&e, &id).await["status"], "DELETED");
    for path in [&hd, &sd, &clean] {
        assert!(!path.exists());
    }
    assert!(
        !private
            .join(row["thumbnail_key"].as_str().unwrap())
            .exists()
    );
    assert_eq!(fetch(&e.app, &hd_url, None).await.0, StatusCode::FORBIDDEN);

    // Take It Down on a clip removes every Beacon made from it and blocks its exact copies.
    let from_clip = e
        .call(
            "POST",
            "/api/me/beacons",
            create(&uuid::Uuid::new_v4().to_string()),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    work(&e.app).await;
    let hashes: Vec<String> = sqlx::query_scalar("SELECT unnest(hashes) FROM beacons WHERE id=$1")
        .bind(&from_clip)
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(hashes.len(), 4);
    // While a removal hold is on the clip, its Beacons are out of public view.
    sqlx::query("INSERT INTO video_holds(video_id,kind,reference) VALUES($1,'TAKE_DOWN','1')")
        .bind(&clip)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        call(
            &e,
            "GET",
            &format!("/api/beacons/{from_clip}"),
            None,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut tx = db.begin().await.unwrap();
    sver::videos::review::remove(&mut tx, &clip, true, true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    work(&e.app).await;
    assert_eq!(beacon(&e, &from_clip).await["status"], "DELETED");
    let blocked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM blocked_media_hashes WHERE hash=ANY($1)")
            .bind(&hashes)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(blocked, 4);
    let _ = other;

    std::fs::remove_dir_all(&root).ok();
    db.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
}
