//! Third-party emotes (docs/DEVELOPER_PLATFORM.md §7): a linked Twitch channel's 7TV emotes are read,
//! copied to S.V.E.R's storage, filtered by the banned-word list, hidden by a report until the
//! streamer shows them again.
use super::chat::{call, person};
use super::*;

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
                    {"id": "01BAD", "name": "badword", "data": {}}
                ]}})))
            }),
        )
        .route(
            "/emote/{id}/{file}",
            get(|| async { b"RIFF\0\0\0\0WEBPVP8 fake".to_vec() }),
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
    sver::outside_emotes::sync(&e.app).await.unwrap();
    assert!(
        media_dir.join("outside/7tv/01seven/112.webp").exists(),
        "images are copied to S.V.E.R's storage"
    );
    let (codes, list) = shown(e).await;
    assert_eq!(codes, ["Pog2", "catJAM"], "banned words are filtered");
    let url = list["emotes"][0]["image"]["28"].as_str().unwrap();
    assert!(
        url.starts_with(&e.app.config.media.public_base),
        "served from S.V.E.R, never the service: {url}"
    );

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
    e.call(
        "PUT",
        "/api/me/outside-emotes",
        json!({"providers": [], "global": false}),
    )
    .await;
    assert!(shown(e).await.0.is_empty(), "turning it off hides them all");
    server.abort();
}
