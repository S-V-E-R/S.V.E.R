//! Go-live alerts: fan-out once per broadcast, the 6-hour throttle, who never gets alerts,
//! per-channel and per-method opt-outs, the notifications list, one-click email unsubscribe,
//! stale push cleanup and 30-day retention.
use super::Env;
use super::chat::{call, person};
use std::sync::{Arc, Mutex};

use axum::{
    Json,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use sver::security as sec;
use tower::ServiceExt;

async fn count(e: &Env, query: &'static str) -> i64 {
    sqlx::query_scalar(query)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}
async fn go_live(e: &Env, id: &'static str, state: &'static str, started: &'static str) {
    sqlx::query("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES($1,'al-owner',$1,1,$2,'s','v','c',now()-$3::interval,now(),now())")
        .bind(id).bind(state).bind(started).execute(&e.app.db).await.unwrap();
}
async fn end(e: &Env, id: &'static str) {
    sqlx::query("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test' WHERE id=$1")
        .bind(id)
        .execute(&e.app.db)
        .await
        .unwrap();
}
async fn fan_out(e: &Env) {
    sver::alerts::fan_out(&e.app).await.unwrap();
}

pub async fn exercise(e: &Env) {
    person(e, "al-owner", "AlertOwner", true).await;
    let fan = person(e, "al-a", "AlertFanA", true).await;
    let quiet = person(e, "al-b", "AlertFanB", true).await;
    let mailer = person(e, "al-c", "AlertFanC", true).await;
    let unverified = person(e, "al-d", "AlertFanD", false).await;
    for (id, name) in [
        ("al-e", "AlertFanE"),
        ("al-f", "AlertFanF"),
        ("al-g", "AlertFanG"),
    ] {
        person(e, id, name, true).await;
    }
    for fan in ["al-a", "al-b", "al-c", "al-d", "al-e", "al-f", "al-g"] {
        sqlx::query("INSERT INTO follows(follower_id,following_id) VALUES($1,'al-owner')")
            .bind(fan)
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    // Never alerted: blocked by the owner, banned from the channel, deleted, unverified.
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('al-owner','al-e')")
        .await;
    e.sql(
        "INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('al-owner','al-f','ban')",
    )
    .await;
    e.sql("UPDATE users SET deleted_at=now() WHERE id='al-g'")
        .await;

    // Per-channel opt-out without unfollowing; only followers can toggle it.
    let (status, body) = call(
        e,
        "PATCH",
        "/api/channels/AlertOwner/follow",
        Some(&quiet),
        json!({"alerts":false}),
    )
    .await;
    assert_eq!(
        (status, body["alerts"].clone()),
        (StatusCode::OK, json!(false))
    );
    let stranger = person(e, "al-h", "AlertFanH", true).await;
    assert_eq!(
        call(
            e,
            "PATCH",
            "/api/channels/AlertOwner/follow",
            Some(&stranger),
            json!({"alerts":false})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // Defaults: in-site and push on, email off. Email needs a verified address.
    let (status, settings) = call(
        e,
        "GET",
        "/api/me/notifications/settings",
        Some(&mailer),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&settings["site"], &settings["push"], &settings["email"]),
        (&json!(true), &json!(true), &json!(false))
    );
    let (status, settings) = call(
        e,
        "PUT",
        "/api/me/notifications/settings",
        Some(&mailer),
        json!({"site":false,"push":true,"email":true}),
    )
    .await;
    assert_eq!((status, &settings["email"]), (StatusCode::OK, &json!(true)));
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/me/notifications/settings",
            Some(&unverified),
            json!({"site":true,"push":true,"email":true})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // A push subscription for the default fan (never contacted: delivery has no key here).
    e.sql(
        "INSERT INTO push_subscriptions(id,user_id,subscription) VALUES('al-push','al-a','sealed')",
    )
    .await;
    // Phone tokens. al-b muted the channel, al-j turned push off, al-h does not follow.
    let held = person(e, "al-j", "AlertFanJ", true).await;
    sqlx::query("INSERT INTO follows(follower_id,following_id) VALUES('al-j','al-owner')")
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO notification_settings(user_id,site,push,email) VALUES('al-j',false,false,false)")
        .execute(&e.app.db)
        .await
        .unwrap();
    let token_a = "ExponentPushToken[alertfana000000000001]";
    let token_b = "ExponentPushToken[alertfanb000000000001]";
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            None,
            json!({"token": token_a})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&fan),
            json!({"token": "https://evil.example/push"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&quiet),
            json!({"token": token_a})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&fan),
            json!({"token": token_a})
        )
        .await
        .0,
        StatusCode::OK,
        "the same phone moves to the account that registers it"
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM expo_devices WHERE user_id='al-a'").await,
        1
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM expo_devices WHERE user_id='al-b'").await,
        0
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&quiet),
            json!({"token": token_b})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&held),
            json!({"token": "ExponentPushToken[alertfanj000000000001]"})
        )
        .await
        .0,
        StatusCode::OK
    );
    for n in 0..10 {
        assert_eq!(
            call(
                e,
                "POST",
                "/api/me/push/expo",
                Some(&stranger),
                json!({"token": format!("ExponentPushToken[alertfanh{n:02}0000000001]")})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&stranger),
            json!({"token": "ExponentPushToken[alertfanh110000000001]"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, phone) = call(
        e,
        "GET",
        "/api/me/notifications/settings",
        Some(&fan),
        Value::Null,
    )
    .await;
    assert_eq!(phone["expo_devices"], 1);

    // STARTING sends nothing; reaching LIVE fans out once.
    go_live(e, "al-1", "STARTING", "1 hour").await;
    fan_out(e).await;
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM notifications WHERE channel_id='al-owner'"
        )
        .await,
        0
    );
    e.sql("UPDATE broadcasts SET state='LIVE' WHERE id='al-1'")
        .await;
    fan_out(e).await;
    let recipients: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM notifications WHERE broadcast_id='al-1'")
            .fetch_all(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        recipients,
        vec!["al-a".to_string()],
        "eligible followers with in-site on"
    );
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM push_jobs WHERE broadcast_id='al-1'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM expo_push_jobs WHERE broadcast_id='al-1'"
        )
        .await,
        1,
        "only the follower with push on and a phone"
    );
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM expo_push_jobs j JOIN expo_devices d ON d.id=j.device_id WHERE d.user_id IN ('al-b','al-j','al-h')"
        )
        .await,
        0
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM mail_jobs WHERE user_id='al-c'").await,
        1
    );
    assert_eq!(count(e, "SELECT count(*) FROM mail_jobs WHERE user_id IN ('al-a','al-b','al-d','al-e','al-f','al-g','al-owner')").await, 0);
    // A reconnect keeps the same broadcast, so nothing more is sent.
    e.sql("UPDATE broadcasts SET state='RECONNECTING',reconnect_deadline=now()+interval '60 seconds' WHERE id='al-1'").await;
    e.sql("UPDATE broadcasts SET state='LIVE',reconnect_deadline=NULL WHERE id='al-1'")
        .await;
    fan_out(e).await;
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM notifications WHERE channel_id='al-owner'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM expo_push_jobs WHERE broadcast_id='al-1'"
        )
        .await,
        1,
        "a reconnect does not queue the phone again"
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM mail_jobs WHERE user_id='al-c'").await,
        1
    );

    // Within 6 hours of the last alert a new broadcast is throttled; after that it alerts again.
    end(e, "al-1").await;
    go_live(e, "al-2", "LIVE", "0 seconds").await;
    fan_out(e).await;
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM broadcasts WHERE id='al-2' AND alert_state='throttled'"
        )
        .await,
        1
    );
    end(e, "al-2").await;
    e.sql(
        "UPDATE broadcasts SET started_at=started_at-interval '7 hours' WHERE owner_id='al-owner'",
    )
    .await;
    go_live(e, "al-3", "LIVE", "0 seconds").await;
    fan_out(e).await;
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM notifications WHERE broadcast_id='al-3'"
        )
        .await,
        1
    );
    // A broadcast that ends before reaching LIVE never alerts.
    end(e, "al-3").await;
    go_live(e, "al-x", "STARTING", "0 seconds").await;
    end(e, "al-x").await;
    fan_out(e).await;
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM broadcasts WHERE id='al-x' AND alert_state='skipped'"
        )
        .await,
        1
    );
    e.sql("UPDATE broadcasts SET state='LIVE',ended_at=NULL,end_reason=NULL WHERE id='al-3'")
        .await;

    // The list, the bell count and mark-as-read.
    let (status, list) = call(e, "GET", "/api/me/notifications", Some(&fan), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["unread"], 2);
    assert_eq!(list["items"][0]["channel"]["username"], "AlertOwner");
    assert_eq!(list["items"][0]["live"], true);
    assert_eq!(list["items"][1]["live"], false);
    let (_, alerts) = call(e, "GET", "/api/me/alerts", Some(&fan), Value::Null).await;
    assert_eq!(alerts["notifications"], 2);
    call(
        e,
        "POST",
        "/api/me/notifications/read",
        Some(&fan),
        Value::Null,
    )
    .await;
    let (_, alerts) = call(e, "GET", "/api/me/alerts", Some(&fan), Value::Null).await;
    assert_eq!(alerts["notifications"], 0);
    let (_, list) = call(e, "GET", "/api/me/notifications", Some(&quiet), Value::Null).await;
    assert_eq!(list["items"], json!([]));

    // One-click email unsubscribe works without a session or an Origin (mail providers post it).
    let sealed: String = sqlx::query_scalar(
        "SELECT payload FROM mail_jobs WHERE user_id='al-c' ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    let mail: Value = serde_json::from_str(&sec::unseal(&e.app, "mail", &sealed).unwrap()).unwrap();
    assert!(
        mail["text"]
            .as_str()
            .unwrap()
            .ends_with("SVER LLC, 4030 Wake Forest Road, Suite 349, Raleigh, NC 27609"),
        "optional email ends with the postal address"
    );
    assert!(mail["text"].as_str().unwrap().contains("/AlertOwner"));
    assert_eq!(
        mail["headers"]["List-Unsubscribe-Post"],
        "List-Unsubscribe=One-Click"
    );
    let path = mail["headers"]["List-Unsubscribe"]
        .as_str()
        .unwrap()
        .trim_matches(['<', '>'])
        .trim_start_matches(e.app.config.origin.as_str())
        .to_string();
    let unsubscribe = |uri: String| {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .extension(axum::extract::ConnectInfo(
                "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
            ))
            .body(Body::from("List-Unsubscribe=One-Click"))
            .unwrap()
    };
    let response = sver::router(e.app.clone())
        .oneshot(unsubscribe(path))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM notification_settings WHERE user_id='al-c' AND NOT email"
        )
        .await,
        1
    );
    let forged = sver::router(e.app.clone())
        .oneshot(unsubscribe(
            "/api/notifications/unsubscribe?token=v1.forged.token".into(),
        ))
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::BAD_REQUEST);

    // Phone delivery: opt-out is rechecked, a dead Expo token is removed, ended streams are not sent.
    expo_delivery(e, token_a).await;
    assert_eq!(
        call(
            e,
            "DELETE",
            "/api/me/push/expo",
            Some(&fan),
            json!({"token": token_b})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM expo_devices WHERE user_id='al-b'").await,
        1,
        "one account cannot remove another's phone"
    );
    assert_eq!(
        call(
            e,
            "DELETE",
            "/api/me/push/expo",
            Some(&quiet),
            json!({"token": token_b})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM expo_devices WHERE user_id='al-b'").await,
        0
    );

    // Pushes for ended streams are dropped; in-site notifications are kept 30 days.
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM push_jobs WHERE broadcast_id IN ('al-1','al-3')"
        )
        .await,
        2
    );
    end(e, "al-3").await;
    e.sql("UPDATE notifications SET created_at=now()-interval '31 days' WHERE broadcast_id='al-1'")
        .await;
    sver::alerts::deliver(&e.app).await.unwrap();
    assert_eq!(count(e, "SELECT count(*) FROM push_jobs").await, 0);
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM notifications WHERE channel_id='al-owner'"
        )
        .await,
        1
    );

    e.sql("DELETE FROM mail_jobs WHERE user_id LIKE 'al-%'")
        .await;
    e.sql("DELETE FROM users WHERE id LIKE 'al-%'").await;
}

async fn expo_delivery(e: &Env, live_token: &str) {
    let reader = person(e, "al-i", "AlertFanI", true).await;
    let (status, saved) = call(
        e,
        "PUT",
        "/api/me/notifications/settings",
        Some(&reader),
        json!({"site":true,"push":true,"email":false,"community":[{"kind":"dm","site":true,"push":false},{"kind":"light","site":true,"push":true}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let dm = saved["community"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "dm")
        .unwrap();
    assert_eq!(dm["push"], false);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&reader),
            json!({"token": "ExponentPushToken[alertfani000000000001]"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut db = e.app.db.acquire().await.unwrap();
    sver::alerts::community(
        &mut db,
        "al-i",
        "al-h",
        "dm",
        "expo-off",
        None,
        &json!({"title":"New message","body":"AlertFanH sent you a message.","url":"/messages?with=AlertFanH"}),
    )
    .await
    .unwrap();
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM expo_push_jobs j JOIN expo_devices d ON d.id=j.device_id WHERE d.user_id='al-i'"
        )
        .await,
        0,
        "a type with push off does not queue the phone"
    );
    call(
        e,
        "PUT",
        "/api/me/notifications/settings",
        Some(&reader),
        json!({"site":true,"push":true,"email":false,"community":[{"kind":"dm","site":true,"push":true}]}),
    )
    .await;
    sver::alerts::community(
        &mut db,
        "al-i",
        "al-h",
        "dm",
        "expo-on",
        None,
        &json!({"title":"New message","body":"AlertFanH sent you a message.","url":"/messages?with=AlertFanH"}),
    )
    .await
    .unwrap();
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM expo_push_jobs j JOIN expo_devices d ON d.id=j.device_id WHERE d.user_id='al-i'"
        )
        .await,
        1
    );
    // Turn it off again before the worker runs. The worker reads the switch, not the queued flag.
    call(
        e,
        "PUT",
        "/api/me/notifications/settings",
        Some(&reader),
        json!({"site":true,"push":true,"email":false,"community":[{"kind":"dm","site":true,"push":false}]}),
    )
    .await;
    drop(db);

    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = seen.clone();
    let router = axum::Router::new().route(
        "/push",
        axum::routing::post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let capture = capture.clone();
            async move {
                let authorized = headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    == Some("Bearer synthetic-expo-access-token");
                if !authorized {
                    return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"})));
                }
                capture.lock().unwrap().push(body.clone());
                let dead = body["to"].as_str().unwrap_or("").contains("DEADTOKEN");
                let data = if dead {
                    json!({"status":"error","message":"not registered","details":{"error":"DeviceNotRegistered"}})
                } else {
                    json!({"status":"ok","id":"synthetic-ticket"})
                };
                (StatusCode::OK, Json(json!({"data": data})))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut config = (*e.app.config).clone();
    config.expo_access_token = "synthetic-expo-access-token".into();
    config.expo_push_url = format!("http://{address}/push");
    let sender = sver::App::new(e.app.db.clone(), config).await.unwrap();
    sver::alerts::deliver_expo(&sender).await.unwrap();
    let messages = seen.lock().unwrap().clone();
    assert_eq!(messages.len(), 1, "the muted direct message is not sent");
    assert_eq!(messages[0]["to"], json!(live_token));
    assert_eq!(messages[0]["title"], "AlertOwner is live");
    assert_eq!(messages[0]["body"], "Watch now on S.V.E.R.");
    assert_eq!(messages[0]["data"]["url"], "/AlertOwner");
    assert_eq!(messages[0]["channelId"], "live");
    let text = messages[0].to_string();
    assert!(!text.to_lowercase().contains("boost"));
    assert!(!text.to_lowercase().contains("pay"));
    assert_eq!(
        count(
            e,
            "SELECT count(*) FROM expo_push_jobs j JOIN expo_devices d ON d.id=j.device_id WHERE d.user_id='al-i'"
        )
        .await,
        0
    );

    let dead = "ExponentPushToken[DEADTOKEN000000000001]";
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/push/expo",
            Some(&reader),
            json!({"token": dead})
        )
        .await
        .0,
        StatusCode::OK
    );
    // reader is not the live follower. Queue a go-live job directly so the dead-token path runs
    // without depending on another broadcast. The device id is the digest of the token.
    sqlx::query(
        "INSERT INTO expo_push_jobs(id,device_id,broadcast_id) VALUES('al-dead',$1,'al-3')",
    )
    .bind(sec::digest(dead))
    .execute(&e.app.db)
    .await
    .unwrap();
    sver::alerts::deliver_expo(&sender).await.unwrap();
    let dead_left: i64 = sqlx::query_scalar("SELECT count(*) FROM expo_devices WHERE id=$1")
        .bind(sec::digest(dead))
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        dead_left, 0,
        "Expo reporting DeviceNotRegistered removes the phone"
    );
    assert_eq!(
        count(e, "SELECT count(*) FROM expo_devices WHERE user_id='al-a'").await,
        1,
        "a dead phone does not remove the other device"
    );
    server.abort();
}
