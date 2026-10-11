//! Native alerts and overlays (docs/OVERLAYS.md) over a real WebSocket: the private link, the
//! opening state, alerts from live events with the channel's settings applied, the goal bar,
//! test alerts and revocation.
use super::Env;
use super::chat::{call, next_json, person};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::net::SocketAddr;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

async fn emit(e: &Env, kind: &str, data: Value) {
    let mut db = e.app.db.acquire().await.unwrap();
    sver::events::emit(&mut db, "stream-owner", kind, data)
        .await
        .unwrap();
    drop(db);
    sver::events::drain(&e.app).await.unwrap();
}

pub async fn exercise(e: &Env) {
    sver::events::drain(&e.app).await.unwrap();
    let mut settings = e.call("GET", "/api/me/overlays", Value::Null).await["settings"].clone();
    settings["tribute"]["min"] = json!(100);
    settings["goal"] = json!({"kind": "followers", "target": 10, "label": "Road to 10"});
    e.call("PUT", "/api/me/overlays", settings.clone()).await;
    let url = e.call("POST", "/api/me/overlays/link", Value::Null).await["url"]
        .as_str()
        .unwrap()
        .to_string();
    let token = url.rsplit('/').next().unwrap().to_string();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let connect = |token: String, origin: String| async move {
        let mut request = format!("ws://{address}/api/overlays/ws?token={token}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        tokio_tungstenite::connect_async(request).await
    };
    let origin = e.app.config.origin.clone();
    assert!(
        connect(token.clone(), "https://evil.example".into())
            .await
            .is_err(),
        "foreign origin"
    );
    assert!(
        connect("wrong".into(), origin.clone()).await.is_err(),
        "unknown link"
    );
    let (mut ws, _) = connect(token.clone(), origin.clone()).await.unwrap();
    let hello = next_json(&mut ws).await;
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["settings"]["goal"]["label"], "Road to 10");
    let followers = hello["goal_count"].as_i64().unwrap();

    // A follow moves the goal bar and plays its alert.
    let fan = person(e, "ov-fan", "OverlayFan", true).await;
    call(e, "PUT", "/api/follows/Streamer", Some(&fan), Value::Null).await;
    sver::events::drain(&e.app).await.unwrap();
    let goal = next_json(&mut ws).await;
    assert_eq!(
        (goal["type"].as_str(), goal["goal_count"].as_i64()),
        (Some("goal"), Some(followers + 1))
    );
    let followed = next_json(&mut ws).await;
    assert_eq!(followed["text"], "OverlayFan followed!", "{followed}");

    // A tribute under the minimum stays off; one over it plays with its message.
    emit(
        e,
        "tributes",
        json!({"user": "OverlayFan", "valor": 50, "message": "small"}),
    )
    .await;
    emit(
        e,
        "tributes",
        json!({"user": "OverlayFan", "valor": 150, "message": "gg"}),
    )
    .await;
    let tribute = next_json(&mut ws).await;
    assert_eq!(
        (tribute["text"].as_str(), tribute["message"].as_str()),
        (Some("OverlayFan paid 150 Valor"), Some("gg"))
    );

    // Test alerts reach the open overlay only.
    e.call("POST", "/api/me/overlays/test", json!({"kind": "raid"}))
        .await;
    assert_eq!(next_json(&mut ws).await["text"], "TestViewer is raiding!");

    // Saving updates the overlay at once; a type turned off stays off.
    settings["sub"]["on"] = json!(false);
    e.call("PUT", "/api/me/overlays", settings.clone()).await;
    assert_eq!(next_json(&mut ws).await["type"], "settings");
    emit(
        e,
        "subs",
        json!({"user": "OverlayFan", "tier": 1, "months": 1}),
    )
    .await;
    emit(
        e,
        "skills",
        json!({"user": "OverlayFan", "skill": "Confetti", "valor": 300}),
    )
    .await;
    assert_eq!(
        next_json(&mut ws).await["text"],
        "OverlayFan played Confetti!",
        "the sub alert was off"
    );

    // Bad settings are refused; a new link stops the old one.
    let mut bad = settings.clone();
    bad["follow"]["seconds"] = json!(60);
    let (status, _) = e
        .request("PUT", "/api/me/overlays", bad, true, true, false)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    e.call("POST", "/api/me/overlays/link", Value::Null).await;
    assert!(
        connect(token.clone(), origin.clone()).await.is_err(),
        "the old link is gone"
    );
    e.call("DELETE", "/api/me/overlays/link", Value::Null).await;
    assert_eq!(
        e.call("GET", "/api/me/overlays", Value::Null).await["linked"],
        false
    );

    // Uploaded images and sounds per alert type: images go through the image pipeline, sounds are
    // re-encoded to WAV, and saving settings never overwrites them.
    let image = image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 90]));
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let (status, saved) = upload(
        e,
        &e.cookie,
        "/api/me/overlays/follow/image",
        png.into_inner(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(
        saved["settings"]["follow"]["image"]
            .as_str()
            .unwrap()
            .starts_with("alerts/"),
        "{saved}"
    );
    let tone = tone();
    let (status, saved) = upload(e, &e.cookie, "/api/me/overlays/follow/sound", tone).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let audio = saved["settings"]["follow"]["audio"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(audio.starts_with("alerts/sounds/") && audio.ends_with(".wav"));
    assert_eq!(
        upload(
            e,
            &e.cookie,
            "/api/me/overlays/follow/sound",
            b"not a sound".to_vec()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        upload(e, &e.cookie, "/api/me/overlays/nope/sound", b"x".to_vec())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let kept = e.call("PUT", "/api/me/overlays", settings.clone()).await;
    assert_eq!(kept["settings"]["follow"]["audio"], audio.as_str());
    let removed = e
        .call("DELETE", "/api/me/overlays/follow/image", Value::Null)
        .await;
    assert!(removed["settings"]["follow"]["image"].is_null());
    e.call("DELETE", "/api/me/overlays/follow/sound", Value::Null)
        .await;
}

/// A multipart upload of one `file` as the person with `token`.
pub(super) async fn upload(e: &Env, token: &str, path: &str, file: Vec<u8>) -> (StatusCode, Value) {
    use axum::{body::Body, extract::ConnectInfo, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let mut body = b"--alert\r\nContent-Disposition: form-data; name=\"file\"; filename=\"alert.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    body.extend(file);
    body.extend(b"\r\n--alert--\r\n");
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "multipart/form-data; boundary=alert")
        .header("cookie", format!("sver_dev={token}"))
        .header("origin", &e.app.config.origin)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    let response = sver::router(e.app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// One second of a 440 Hz tone as 8 kHz mono WAV.
pub(super) fn tone() -> Vec<u8> {
    let samples: Vec<u8> = (0..8000)
        .flat_map(|i| {
            (((i as f64 * 440.0 * std::f64::consts::TAU / 8000.0).sin() * 8000.0) as i16)
                .to_le_bytes()
        })
        .collect();
    let mut tone = b"RIFF".to_vec();
    tone.extend((36 + samples.len() as u32).to_le_bytes());
    tone.extend(b"WAVEfmt ");
    for v in [16u32, 0x0001_0001, 8000, 16000, 0x0010_0002] {
        tone.extend(v.to_le_bytes());
    }
    tone.extend(b"data");
    tone.extend((samples.len() as u32).to_le_bytes());
    tone.extend(samples);
    tone
}
