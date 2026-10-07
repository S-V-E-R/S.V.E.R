//! Go-live alerts: fan-out once per broadcast, the 6-hour throttle, who never gets alerts,
//! per-channel and per-method opt-outs, the notifications list, one-click email unsubscribe,
//! stale push cleanup and 30-day retention.
use super::Env;
use super::chat::{call, person};
use axum::{
    body::Body,
    http::{Request, StatusCode},
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
