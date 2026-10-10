//! GIFs in chat (docs/COMMUNITY.md): KLIPY references only, the channel's GIF setting, and the
//! normal chat rules on the same send path.
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

const BASE: &str = "/api/channels/streamer";

fn gif(still: &str) -> Value {
    json!({"slug":"victory-1--kOqubu","title":"Victory\u{7}","still":still,
        "play":"https://static.klipy.com/ii/abc/58/7b/play.webp","width":220,"height":124})
}
async fn send(e: &Env, token: &str, gif: Value) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("{BASE}/chat"),
        Some(token),
        json!({"id":id(),"body":"","gif":gif}),
    )
    .await
}
async fn setting(e: &Env, who: &str) -> StatusCode {
    e.request("PUT", &format!("{BASE}/chat/settings"),
        json!({"slow_mode_seconds":0,"block_links":false,"banned_words":[],"reason":"gifs","gifs":who}), true, true, false)
        .await
        .0
}
const STILL: &str = "https://static.klipy.com/ii/abc/58/7b/still.jpg";

pub async fn exercise(e: &Env) {
    let fan = person(e, "gif-fan", "GifFan", true).await;
    let stranger = person(e, "gif-stranger", "GifStranger", true).await;
    assert_eq!(setting(e, "everyone").await, StatusCode::OK);

    // Without a KLIPY key GIFs are off everywhere.
    assert_eq!(send(e, &fan, gif(STILL)).await.0, StatusCode::FORBIDDEN);
    // SAFETY: only this test reads KLIPY_APP_KEY.
    unsafe { std::env::set_var("KLIPY_APP_KEY", "synthetic-key") };
    let (_, chat) = call(e, "GET", &format!("{BASE}/chat"), None, Value::Null).await;
    assert_eq!(
        chat["gifs"],
        json!({"key":"synthetic-key","who":"everyone"})
    );

    // Only KLIPY media URLs are stored.
    for bad in [
        "https://evil.example/x.jpg",
        "https://static.klipy.com.evil.example/x.jpg",
        "https://static.klipy.com/a b.jpg",
        "http://static.klipy.com/x.jpg",
    ] {
        assert_eq!(
            send(e, &fan, gif(bad)).await.0,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    let (status, sent) = send(e, &fan, gif(STILL)).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(
        sent["message"]["body"], "GIF: Victory",
        "control characters dropped"
    );
    assert_eq!(sent["message"]["gif"]["still"], STILL);
    assert!(sent["message"]["gif"].get("title").is_none());
    let (_, chat) = call(e, "GET", &format!("{BASE}/chat"), None, Value::Null).await;
    assert!(
        chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["gif"]["slug"] == "victory-1--kOqubu")
    );

    // Followers only: the channel's setting is enforced; the owner is exempt.
    assert_eq!(setting(e, "nobody").await, StatusCode::BAD_REQUEST);
    assert_eq!(setting(e, "followers").await, StatusCode::OK);
    assert_eq!(
        send(e, &stranger, gif(STILL)).await.0,
        StatusCode::FORBIDDEN
    );
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('gif-stranger','stream-owner') ON CONFLICT DO NOTHING").await;
    assert_eq!(send(e, &stranger, gif(STILL)).await.0, StatusCode::OK);
    assert_eq!(setting(e, "subscribers").await, StatusCode::OK);
    assert_eq!(
        send(e, &stranger, gif(STILL)).await.0,
        StatusCode::FORBIDDEN
    );

    // Off: the composer gets nothing, and a send is refused; text still works.
    assert_eq!(setting(e, "off").await, StatusCode::OK);
    let (_, chat) = call(e, "GET", &format!("{BASE}/chat"), None, Value::Null).await;
    assert_eq!(chat["gifs"], Value::Null);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(send(e, &fan, gif(STILL)).await.0, StatusCode::FORBIDDEN);
    let (status, _) = call(
        e,
        "POST",
        &format!("{BASE}/chat"),
        Some(&fan),
        json!({"id":id(),"body":"words"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(setting(e, "everyone").await, StatusCode::OK);
}
