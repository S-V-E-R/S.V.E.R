//! Playback state and heartbeat viewer counting, against a synthetic LIVE broadcast.
use super::Env;
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn guest(e: &Env, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    e.request(method, path, body, false, true, false).await
}
async fn beat(e: &Env, browser: &str) -> (StatusCode, Value) {
    guest(
        e,
        "POST",
        "/api/channels/streamer/live/beat",
        json!({"broadcast_id":"play-1","browser_id":browser}),
    )
    .await
}

pub async fn exercise(e: &Env) {
    let mut tx = e.app.db.begin().await.unwrap();
    sver::profiles::ensure_profile(&mut tx, "stream-owner")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (status, _) = guest(e, "GET", "/api/channels/nobody-here/live", Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, offline) = guest(e, "GET", "/api/channels/Streamer/live", Value::Null).await;
    assert_eq!(offline, json!({"live":false}));

    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('play-1','stream-owner','pub-play',1,'STARTING','s','v','c',now(),now(),now()+interval '15 seconds')").await;
    let (_, starting) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(starting["live"], false, "STARTING is not public");
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["counted"], false);

    e.sql("UPDATE broadcasts SET state='LIVE' WHERE id='play-1'")
        .await;
    let (_, live) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(live["live"], true);
    assert_eq!(live["title"], "Streamer's stream");
    assert_eq!(live["viewers"], 0);
    assert_eq!(live["is_owner"], false);
    assert_eq!(
        live["playback"],
        json!({"webrtc":"https://media.example/rtc/v1/whep/?app=rebuild&stream=pub-play","hls":"https://media.example/rebuild/pub-play.m3u8","preferred":"webrtc"})
    );
    assert!(!live.to_string().contains("key="), "no secret in playback");
    let (_, channel) = guest(e, "GET", "/api/channels/streamer", Value::Null).await;
    assert_eq!(channel["channel"]["live"], true, "channel page shows live");
    let (_, card) = guest(e, "GET", "/api/users/streamer/card", Value::Null).await;
    assert_eq!(card["live"], true, "user card shows live");

    // Guests count once per browser; repeats renew rather than add.
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["counted"], true);
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["counted"], true);
    assert_eq!(beat(e, "browser-bbbbbbbbbbbb").await.1["counted"], true);
    assert_eq!(beat(e, "short").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        beat(e, "browser-<script>-xxxxxx").await.0,
        StatusCode::BAD_REQUEST
    );
    // A beat for another broadcast ID does not count.
    let (_, other) = guest(
        e,
        "POST",
        "/api/channels/streamer/live/beat",
        json!({"broadcast_id":"someone-else","browser_id":"browser-cccccccccccc"}),
    )
    .await;
    assert_eq!(other["counted"], false);
    // The owner's own preview never counts.
    let owner = e
        .call(
            "POST",
            "/api/channels/streamer/live/beat",
            json!({"broadcast_id":"play-1","browser_id":"browser-dddddddddddd"}),
        )
        .await;
    assert_eq!(owner["counted"], false);
    assert_eq!(
        e.call("GET", "/api/channels/streamer/live", Value::Null)
            .await["is_owner"],
        true
    );
    let (_, counted) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(counted["viewers"], 2);

    // A lease without a heartbeat in the last 30 seconds stops counting; reconnect grace stays public.
    e.sql("UPDATE playback_leases SET expires_at=now()-interval '1 second' WHERE viewer_key=(SELECT viewer_key FROM playback_leases ORDER BY created_at LIMIT 1)").await;
    e.sql("UPDATE broadcasts SET state='RECONNECTING',reconnect_deadline=now()+interval '60 seconds' WHERE id='play-1'").await;
    let (_, reconnecting) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(reconnecting["state"], "RECONNECTING");
    assert_eq!(reconnecting["viewers"], 1);

    e.sql("UPDATE broadcasts SET state='ENDED',reconnect_deadline=NULL,ended_at=now(),end_reason='test' WHERE id='play-1'").await;
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["counted"], false);
    let (_, ended) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(ended, json!({"live":false}));
    let (_, card) = guest(e, "GET", "/api/users/streamer/card", Value::Null).await;
    assert_eq!(card["live"], false);
}
