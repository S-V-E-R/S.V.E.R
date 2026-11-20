//! Third-party emotes (docs/DEVELOPER_PLATFORM.md §7): a linked Twitch channel's 7TV emotes are read,
//! copied to S.V.E.R's storage (with stills for animated ones), filtered by the banned-word list,
//! hidden by a report until the streamer shows them again. Chats are told only when something
//! changed, and an image that fails to copy waits out a backoff.
use super::chat::{call, next_json, person};
use super::*;
use futures_util::StreamExt;
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
/// How many `emotes` frames carrying `outside: true` arrive within a second.
async fn outside_frames(ws: &mut Socket) -> usize {
    let mut count = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while let Ok(Some(Ok(frame))) = tokio::time::timeout_at(deadline, ws.next()).await {
        let value: Value =
            serde_json::from_str(frame.to_text().unwrap_or("null")).unwrap_or_default();
        if value["type"] == "emotes" && value["outside"] == true {
            count += 1;
        }
    }
    count
}
fn webp() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        8,
        8,
        image::Rgba([9, 99, 199, 255]),
    ))
    .write_to(&mut out, image::ImageFormat::WebP)
    .unwrap();
    out.into_inner()
}
/// An emote upload form as the streamer.
async fn upload_emote(e: &Env, code: &str) -> StatusCode {
    let mut file = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(112, 112, image::Rgb([1, 2, 3])))
        .write_to(&mut file, image::ImageFormat::Png)
        .unwrap();
    let mut body = format!("--emote\r\nContent-Disposition: form-data; name=\"code\"\r\n\r\n{code}\r\n--emote\r\nContent-Disposition: form-data; name=\"file\"; filename=\"e.png\"\r\nContent-Type: image/png\r\n\r\n").into_bytes();
    body.extend(file.into_inner());
    body.extend(b"\r\n--emote--\r\n");
    let request = Request::builder()
        .method("POST")
        .uri("/api/me/emotes")
        .header("content-type", "multipart/form-data; boundary=emote")
        .header("cookie", format!("sver_dev={}", e.cookie))
        .header("origin", &e.app.config.origin)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    sver::router(e.app.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

async fn shown(e: &Env) -> (Vec<String>, Value) {
    let list = e
        .call("GET", "/api/channels/streamer/outside-emotes", Value::Null)
        .await;
    let mut codes: Vec<String> = list["emotes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["code"].as_str().unwrap().to_string())
        .collect();
    codes.sort();
    (codes, list)
}

pub async fn exercise(e: &Env, media_dir: &std::path::Path) {
    let gone_hits = Arc::new(AtomicUsize::new(0));
    let hits = gone_hits.clone();
    let image = webp();
    let service = Router::new()
        .route(
            "/v3/users/twitch/{id}",
            get(|Path(id): Path<String>| async move {
                if id != "tw-1" {
                    return Err(StatusCode::NOT_FOUND);
                }
                Ok(Json(json!({"emote_set": {"emotes": [
                    {"id": "01SEVEN", "name": "catJAM", "data": {"animated": true}},
                    {"id": "01OTHER", "name": "Pog2", "data": {}},
                    {"id": "01BAD", "name": "badword", "data": {}},
                    {"id": "01GONE", "name": "Gone", "data": {}}
                ]}})))
            }),
        )
        .route(
            "/emote/{id}/{file}",
            get(move |Path((id, _)): Path<(String, String)>| async move {
                if id == "01GONE" {
                    hits.fetch_add(1, Ordering::SeqCst);
                    return Err(StatusCode::NOT_FOUND);
                }
                Ok(image.clone())
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    // SAFETY: only this test reads these variables, and it sets them before the sync runs.
    unsafe {
        std::env::set_var("OUTSIDE_EMOTES", "7tv");
        std::env::set_var("SEVENTV_API", format!("http://{address}"));
        std::env::set_var("SEVENTV_CDN", format!("http://{address}"));
    }

    let studio = e
        .call(
            "PUT",
            "/api/me/outside-emotes",
            json!({"providers": ["7tv"], "global": false}),
        )
        .await;
    assert_eq!(studio["twitch_linked"], false);
    e.sql(
        "INSERT INTO identities(provider,subject,user_id) VALUES('twitch','tw-1','stream-owner')",
    )
    .await;
    e.sql("UPDATE chat_settings SET banned_words='{badword}' WHERE channel_id='stream-owner'")
        .await;

    // An open chat hears about the new emotes once they change, and not on an unchanged refresh.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let chat_address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let chat_server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let mut request = format!("ws://{chat_address}/api/chat/ws?channel=streamer")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", e.app.config.origin.parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(next_json(&mut ws).await["type"], "snapshot");

    sver::outside_emotes::sync(&e.app).await.unwrap();
    assert!(
        media_dir.join("outside/7tv/01seven/112.webp").exists(),
        "images are copied to S.V.E.R's storage"
    );
    assert!(
        media_dir
            .join("outside/7tv/01seven/112.still.webp")
            .exists(),
        "an animated emote gets a first-frame still"
    );
    assert!(outside_frames(&mut ws).await > 0, "chat is told");
    let (codes, list) = shown(e).await;
    assert_eq!(codes, ["Pog2", "catJAM"], "banned words are filtered");
    let url = list["emotes"][0]["image"]["28"].as_str().unwrap();
    assert!(
        url.starts_with(&e.app.config.media.public_base),
        "served from S.V.E.R, never the service: {url}"
    );
    let cat = list["emotes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["code"] == "catJAM")
        .unwrap();
    assert!(
        cat["still"]["28"]
            .as_str()
            .unwrap()
            .ends_with("28.still.webp")
    );
    let plain = list["emotes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["code"] == "Pog2");
    assert!(
        plain.unwrap()["still"].is_null(),
        "still images have no still"
    );
    assert_eq!(gone_hits.load(Ordering::SeqCst), 1);

    // An unchanged refresh tells no one, and the failed image waits out its backoff.
    e.sql("UPDATE chat_settings SET outside_synced_at=NULL WHERE channel_id='stream-owner'")
        .await;
    sver::outside_emotes::sync(&e.app).await.unwrap();
    assert_eq!(outside_frames(&mut ws).await, 0, "nothing changed");
    assert_eq!(
        gone_hits.load(Ordering::SeqCst),
        1,
        "not retried inside its backoff"
    );
    drop(ws);
    chat_server.abort();

    // A viewer's report hides it until the streamer shows it again.
    let fan = person(e, "oe-fan", "OeFan", true).await;
    let report = "/api/channels/streamer/outside-emotes/7tv/01SEVEN/report";
    assert_eq!(
        call(e, "POST", report, Some(&fan), Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/streamer/outside-emotes/7tv/NOPE/report",
            Some(&fan),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(shown(e).await.0, ["Pog2"]);
    let studio = e.call("GET", "/api/me/outside-emotes", Value::Null).await;
    assert!(
        studio["emotes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["code"] == "catJAM" && x["hidden"] == "report")
    );
    e.call(
        "PUT",
        "/api/me/outside-emotes/7tv/01SEVEN",
        json!({"hidden": false}),
    )
    .await;
    assert_eq!(shown(e).await.0, ["Pog2", "catJAM"]);

    // The streamer's review sticks: a later report doesn't hide it again.
    let again = person(e, "oe-again", "OeAgain", true).await;
    let (status, body) = call(e, "POST", report, Some(&again), Value::Null).await;
    assert_eq!((status, &body["hidden"]), (StatusCode::OK, &json!(false)));
    assert_eq!(shown(e).await.0, ["Pog2", "catJAM"]);

    // Only someone who could chat here can report: not someone banned from the channel.
    let banned = person(e, "oe-banned", "OeBanned", true).await;
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('stream-owner','oe-banned','ban')")
        .await;
    let other = "/api/channels/streamer/outside-emotes/7tv/01OTHER/report";
    assert_eq!(
        call(e, "POST", other, Some(&banned), Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    let unverified = person(e, "oe-new", "OeNew", false).await;
    assert_eq!(
        call(e, "POST", other, Some(&unverified), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(shown(e).await.0, ["Pog2", "catJAM"]);

    // A duplicate code is refused before the image is processed or stored.
    e.sql("INSERT INTO channel_emotes(id,channel_id,code,image_key) VALUES('oe-dup','stream-owner','Walnut','emotes/oe-dup')")
        .await;
    let media: i64 = sqlx::query_scalar("SELECT count(*) FROM media_objects")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(upload_emote(e, "Walnut").await, StatusCode::CONFLICT);
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM media_objects")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(after, media, "no media row for a refused upload");

    e.call(
        "PUT",
        "/api/me/outside-emotes",
        json!({"providers": [], "global": false}),
    )
    .await;
    assert!(shown(e).await.0.is_empty(), "turning it off hides them all");
    server.abort();
}
