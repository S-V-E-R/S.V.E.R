//! Playback state and heartbeat viewer counting, against a synthetic LIVE broadcast.
use super::Env;
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::net::SocketAddr;
use tower::ServiceExt;

async fn guest(e: &Env, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    e.request(method, path, body, false, true, false).await
}
async fn beat(e: &Env, browser: &str) -> (StatusCode, Value) {
    // Guests pass the (synthetic) security check; integrity.rs covers failing it.
    guest(
        e,
        "POST",
        "/api/channels/streamer/live/beat",
        json!({"broadcast_id":"play-1","browser_id":browser,"visible":true,"turnstile":"pass"}),
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
    // The OBS overlay feed: public, readable from a local file, nothing but page-level numbers.
    let overlay = || async {
        let response = sver::router(e.app.clone())
            .oneshot(
                Request::get("/api/channels/streamer/overlay")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    };
    assert_eq!(
        overlay().await,
        json!({"isLive":false,"stream":null,"stats":{"followerCount":0}})
    );

    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('play-1','stream-owner','pub-play',1,'STARTING','s','v','c',now(),now(),now()+interval '15 seconds')").await;
    let (_, starting) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(starting["live"], false, "STARTING is not public");
    assert!(
        guest(e, "GET", "/api/discovery/home", Value::Null).await.1["live"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["recorded"], false);

    e.sql("UPDATE broadcasts SET state='LIVE' WHERE id='play-1'")
        .await;
    let (_, live) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(live["live"], true);
    let on_air = overlay().await;
    assert_eq!(
        (&on_air["isLive"], &on_air["stream"]["title"]),
        (&json!(true), &live["title"])
    );
    assert_eq!(live["title"], "Streamer's stream");
    assert_eq!(live["viewers"], 0);
    let (status, directory) = guest(e, "GET", "/api/discovery/home", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(directory["live"][0]["username"], "Streamer");
    assert_eq!(directory["live"][0]["viewers"], 0);
    assert!(
        !directory.to_string().contains("pub-play"),
        "Directory exposes no playback identifiers"
    );
    e.sql(
        "UPDATE profiles SET restricted_until=now()+interval '1 day' WHERE user_id='stream-owner'",
    )
    .await;
    assert!(
        guest(e, "GET", "/api/discovery/home", Value::Null).await.1["live"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    e.sql("UPDATE profiles SET restricted_until=NULL WHERE user_id='stream-owner'")
        .await;
    assert_eq!(live["is_owner"], false);
    assert_eq!(
        live["playback"],
        json!({"webrtc":"https://media.example/rtc/v1/whep/?app=rebuild&stream=pub-play","hls":"https://media.example/rebuild/pub-play.m3u8","preferred":"webrtc"})
    );
    assert!(!live.to_string().contains("key="), "no secret in playback");
    let (_, hls_only) = guest(
        e,
        "GET",
        "/api/channels/streamer/live?transport=hls",
        Value::Null,
    )
    .await;
    assert_eq!(
        hls_only["playback"]["preferred"], "hls",
        "a viewer may ask for HLS"
    );
    let (_, channel) = guest(e, "GET", "/api/channels/streamer", Value::Null).await;
    assert_eq!(channel["channel"]["live"], true, "channel page shows live");
    let (_, card) = guest(e, "GET", "/api/users/streamer/card", Value::Null).await;
    assert_eq!(card["live"], true, "user card shows live");

    // Guests count once per browser; repeats renew rather than add.
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["recorded"], true);
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["recorded"], true);
    assert_eq!(beat(e, "browser-bbbbbbbbbbbb").await.1["recorded"], true);
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
    assert_eq!(other["recorded"], false);
    // The owner's own preview never counts.
    let owner = e
        .call(
            "POST",
            "/api/channels/streamer/live/beat",
            json!({"broadcast_id":"play-1","browser_id":"browser-dddddddddddd"}),
        )
        .await;
    assert_eq!(owner["recorded"], false);
    assert_eq!(
        e.call("GET", "/api/channels/streamer/live", Value::Null)
            .await["is_owner"],
        true
    );
    // New sessions are pending for their first minute, then count.
    let (_, pending) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(pending["viewers"], 0);
    e.sql("UPDATE playback_leases SET created_at=created_at-interval '61 seconds'")
        .await;
    sver::integrity::tick(&e.app).await.unwrap();
    let (_, counted) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(counted["viewers"], 2);
    assert_eq!(
        guest(e, "GET", "/api/discovery/home", Value::Null).await.1["live"][0]["viewers"],
        2
    );

    // A lease without a heartbeat in the last 30 seconds stops counting; reconnect grace stays public.
    e.sql("UPDATE playback_leases SET expires_at=now()-interval '1 second' WHERE viewer_key=(SELECT viewer_key FROM playback_leases ORDER BY created_at LIMIT 1)").await;
    e.sql("UPDATE broadcasts SET state='RECONNECTING',reconnect_deadline=now()+interval '60 seconds' WHERE id='play-1'").await;
    let (_, reconnecting) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(reconnecting["state"], "RECONNECTING");
    assert_eq!(reconnecting["viewers"], 1);
    e.sql("UPDATE broadcasts SET reconnect_deadline=now()-interval '1 second' WHERE id='play-1'")
        .await;
    assert!(
        guest(e, "GET", "/api/discovery/home", Value::Null).await.1["live"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    e.sql("UPDATE broadcasts SET state='ENDED',reconnect_deadline=NULL,ended_at=now(),end_reason='test' WHERE id='play-1'").await;
    assert_eq!(beat(e, "browser-aaaaaaaaaaaa").await.1["recorded"], false);
    let (_, ended) = guest(e, "GET", "/api/channels/streamer/live", Value::Null).await;
    assert_eq!(ended, json!({"live":false}));
    let (_, recent) = guest(e, "GET", "/api/discovery/home", Value::Null).await;
    assert_eq!(recent["recent"][0]["user"]["username"], "Streamer");
    e.sql("UPDATE broadcasts SET end_reason='revoked' WHERE id='play-1'")
        .await;
    assert!(
        guest(e, "GET", "/api/discovery/home", Value::Null).await.1["recent"]
            .as_array()
            .unwrap()
            .is_empty(),
        "Revoked streams stay off public shelves"
    );
    let (_, card) = guest(e, "GET", "/api/users/streamer/card", Value::Null).await;
    assert_eq!(card["live"], false);
    media_gate(e).await;
}

async fn authorize(e: &Env, uri: &str, peer: &str, secret: &str) -> StatusCode {
    let response = sver::router(e.app.clone())
        .oneshot(
            Request::builder()
                .uri("/api/internal/streams/playback")
                .header("x-original-uri", uri)
                .header("x-srs-secret", secret)
                .header("x-real-ip", "127.0.0.1")
                .extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    if response.status() == StatusCode::NO_CONTENT {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    response.status()
}
async fn media_gate(e: &Env) {
    let id = "0123456789abcdef0123456789abcdef";
    let path = format!("/rebuild/{id}.m3u8");
    let secret = &e.app.config.streaming.as_ref().unwrap().hook_secret;
    e.sql("INSERT INTO stream_credentials(owner_id,public_id,generation,secret_hash,secret_cipher) VALUES('stream-owner','0123456789abcdef0123456789abcdef',1,'synthetic','synthetic')").await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('play-gate','stream-owner','0123456789abcdef0123456789abcdef',1,'STARTING','s','v','c',now(),now(),now()+interval '15 seconds')").await;
    assert_eq!(
        authorize(e, &path, "127.0.0.1:1234", secret).await,
        StatusCode::FORBIDDEN
    );
    e.sql("UPDATE broadcasts SET state='LIVE' WHERE id='play-gate'")
        .await;
    for uri in [
        &path,
        &format!("/rebuild/{id}-1728000000000-1.ts"),
        &format!("/rebuild/whep/?app=rebuild&stream={id}"),
    ] {
        assert_eq!(
            authorize(e, uri, "127.0.0.1:1234", secret).await,
            StatusCode::NO_CONTENT,
            "{uri}"
        );
    }
    assert_eq!(
        authorize(e, &path, "127.0.0.1:1234", "wrong").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        authorize(e, &path, "127.0.0.2:1234", secret).await,
        StatusCode::FORBIDDEN,
        "A forwarded header cannot impersonate the media proxy"
    );
    for uri in [
        format!("/live/{id}.m3u8"),
        format!("/rebuild/../{id}.m3u8"),
        format!("/rebuild/{id}-x-1.ts"),
        format!("/rebuild/{id}-1-2-3.ts"),
        format!("/rebuild/%30{id}.m3u8"),
        format!("/rebuild/whep/?app=live&stream={id}"),
        format!("/rebuild/whep/?app=rebuild&stream={id}&stream={id}"),
        format!("/rebuild/whep/?app=rebuild&stream={id}&vhost=other"),
        format!("https://elsewhere.invalid/rebuild/{id}.m3u8"),
    ] {
        assert_eq!(
            authorize(e, &uri, "127.0.0.1:1234", secret).await,
            StatusCode::FORBIDDEN,
            "{uri}"
        );
    }
    e.sql("UPDATE broadcasts SET state='RECONNECTING',reconnect_deadline=now()+interval '1 minute' WHERE id='play-gate'").await;
    assert_eq!(
        authorize(e, &path, "127.0.0.1:1234", secret).await,
        StatusCode::NO_CONTENT
    );
    e.sql(
        "UPDATE broadcasts SET reconnect_deadline=now()-interval '1 second' WHERE id='play-gate'",
    )
    .await;
    assert_eq!(
        authorize(e, &path, "127.0.0.1:1234", secret).await,
        StatusCode::FORBIDDEN
    );
    e.sql("UPDATE broadcasts SET state='LIVE',reconnect_deadline=NULL WHERE id='play-gate'")
        .await;
    e.sql("UPDATE stream_credentials SET revoked_at=now(),secret_hash=NULL,secret_cipher=NULL WHERE owner_id='stream-owner'").await;
    assert_eq!(
        authorize(e, &path, "127.0.0.1:1234", secret).await,
        StatusCode::FORBIDDEN
    );
    e.sql("UPDATE stream_credentials SET revoked_at=NULL,generation=2,secret_hash='synthetic',secret_cipher='synthetic' WHERE owner_id='stream-owner'").await;
    assert_eq!(
        authorize(e, &path, "127.0.0.1:1234", secret).await,
        StatusCode::FORBIDDEN,
        "Retired generations cannot replay media"
    );
    e.sql("UPDATE stream_credentials SET generation=1 WHERE owner_id='stream-owner'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='play-gate'")
        .await;
    assert_eq!(
        authorize(e, &path, "127.0.0.1:1234", secret).await,
        StatusCode::FORBIDDEN
    );
    e.sql("DELETE FROM broadcasts WHERE id='play-gate'").await;
    e.sql("DELETE FROM stream_credentials WHERE owner_id='stream-owner'")
        .await;
}
