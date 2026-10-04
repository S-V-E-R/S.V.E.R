//! Raids and hosting: eligibility (live, opt-outs, blocks, bans), countdown and cancel, the
//! 10-minute rule, banned viewers staying put, arrival counting from real leases, raid-to-host,
//! manual hosting, auto-host order and every stop condition.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn live(e: &Env, id: &'static str, owner: &'static str) {
    sqlx::query("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES($1,$2,$1,1,'LIVE','s','v','c',now(),now(),now(),'skipped')")
        .bind(id).bind(owner).execute(&e.app.db).await.unwrap();
}
async fn end(e: &Env, id: &'static str) {
    sqlx::query("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test' WHERE id=$1")
        .bind(id)
        .execute(&e.app.db)
        .await
        .unwrap();
}
async fn hosted(e: &Env, host: &'static str) -> Option<(String, String)> {
    sqlx::query_as("SELECT target_id,source FROM host_state WHERE host_id=$1")
        .bind(host)
        .fetch_optional(&e.app.db)
        .await
        .unwrap()
}
async fn tick(e: &Env) {
    sver::raids::tick(&e.app).await.unwrap();
}
async fn raid(e: &Env, token: &str, target: &str) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        "/api/me/raids",
        Some(token),
        json!({"username":target}),
    )
    .await
}
async fn beat(e: &Env, token: &str, raid: Option<&str>) -> StatusCode {
    call(
        e,
        "POST",
        "/api/channels/RaidB/live/beat",
        Some(token),
        json!({"broadcast_id":"rd-bb","browser_id":"browser-raid-000000001","visible":true,"media_time":1.0,"raid":raid}),
    )
    .await
    .0
}
async fn arrivals(e: &Env, id: &str) -> Option<i32> {
    sqlx::query_scalar("SELECT arrivals FROM raids WHERE id=$1")
        .bind(id)
        .fetch_one(&e.app.db)
        .await
        .unwrap()
}
fn settings(auto: bool, list: &[&str], hosts: bool) -> Value {
    json!({"accept_raids":true,"accept_hosts":hosts,"auto_host":auto,"auto_list":list})
}

pub async fn exercise(e: &Env) {
    let raider = person(e, "rd-a", "RaidA", true).await;
    let target = person(e, "rd-b", "RaidB", true).await;
    person(e, "rd-c", "RaidC", true).await;
    person(e, "rd-d", "RaidD", true).await;
    let closed = person(e, "rd-e", "RaidE", true).await;
    let banned = person(e, "rd-x", "RaidX", true).await;
    let fans = [
        person(e, "rd-v1", "RaidFanOne", true).await,
        person(e, "rd-v2", "RaidFanTwo", true).await,
        person(e, "rd-v3", "RaidFanThree", true).await,
    ];

    // Only a live owner can raid, and only a live target.
    assert_eq!(raid(e, &raider, "RaidB").await.0, StatusCode::BAD_REQUEST);
    live(e, "rd-ba", "rd-a").await;
    assert_eq!(
        raid(e, &raider, "RaidB").await.0,
        StatusCode::BAD_REQUEST,
        "target offline"
    );
    assert_eq!(
        raid(e, &raider, "RaidA").await.0,
        StatusCode::BAD_REQUEST,
        "self"
    );
    live(e, "rd-bb", "rd-b").await;
    // A target that blocked the raider, blocks its raids, bans it or turns raids off can't be raided.
    live(e, "rd-bd", "rd-d").await;
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('rd-d','rd-a')")
        .await;
    assert_eq!(raid(e, &raider, "RaidD").await.0, StatusCode::BAD_REQUEST);
    live(e, "rd-be", "rd-e").await;
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/hosting",
        Some(&closed),
        json!({"accept_raids":false,"accept_hosts":true,"auto_host":false,"auto_list":[]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = raid(e, &raider, "RaidE").await;
    assert_eq!(
        (status, body["error"].as_str().unwrap_or("")),
        (
            StatusCode::BAD_REQUEST,
            "That channel isn't accepting raids."
        )
    );
    let (status, body) = call(
        e,
        "POST",
        "/api/me/raid-blocks",
        Some(&target),
        json!({"username":"RaidA"}),
    )
    .await;
    assert_eq!(
        (status, body["raid_blocks"].clone()),
        (StatusCode::OK, json!(["RaidA"]))
    );
    assert_eq!(raid(e, &raider, "RaidB").await.0, StatusCode::BAD_REQUEST);
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('rd-b','rd-a','ban')")
        .await;
    call(
        e,
        "DELETE",
        "/api/me/raid-blocks/RaidA",
        Some(&target),
        Value::Null,
    )
    .await;
    assert_eq!(
        raid(e, &raider, "RaidB").await.0,
        StatusCode::BAD_REQUEST,
        "banned by the target"
    );
    e.sql("DELETE FROM channel_restrictions WHERE channel_id='rd-b' AND user_id='rd-a'")
        .await;

    // Countdown: viewers of the raider see it, except a signed-in viewer banned from the target.
    let (status, body) = raid(e, &raider, "@RaidB").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["raid"]["target"]["username"], "RaidB");
    let (_, watching) = call(e, "GET", "/api/channels/RaidA/live", None, Value::Null).await;
    assert_eq!(watching["raid"]["id"], body["raid"]["id"]);
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('rd-b','rd-x','ban')")
        .await;
    let (_, watching) = call(
        e,
        "GET",
        "/api/channels/RaidA/live",
        Some(&banned),
        Value::Null,
    )
    .await;
    assert_eq!(
        watching["raid"],
        Value::Null,
        "banned from the target: stays put"
    );
    assert_eq!(raid(e, &raider, "RaidB").await.0, StatusCode::CONFLICT);
    // Cancel during the countdown; a cancelled raid doesn't use the 10-minute allowance.
    assert_eq!(
        call(e, "DELETE", "/api/me/raids", Some(&raider), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    let (_, watching) = call(e, "GET", "/api/channels/RaidA/live", None, Value::Null).await;
    assert_eq!(watching["raid"], Value::Null);
    let (status, body) = raid(e, &raider, "RaidB").await;
    assert_eq!(status, StatusCode::OK);
    let id = body["raid"]["id"].as_str().unwrap().to_string();

    // A viewer already watching the target before the raid isn't an arrival.
    assert_eq!(beat(e, &fans[2], None).await, StatusCode::OK);
    e.sql("UPDATE playback_leases SET created_at=now()-interval '5 minutes' WHERE broadcast_id='rd-bb'").await;
    // The countdown ends: the player asks, and the raid moves.
    e.sql("UPDATE raids SET execute_at=now()-interval '1 second' WHERE status='countdown'")
        .await;
    let (_, moved) = call(e, "GET", &format!("/api/raids/{id}"), None, Value::Null).await;
    assert_eq!(moved["status"], "moved");
    for fan in &fans {
        assert_eq!(beat(e, fan, Some(&id)).await, StatusCode::OK);
    }
    // Counted 60 seconds after the move from leases on the target, never the raider's count.
    tick(e).await;
    assert_eq!(
        arrivals(e, &id).await,
        None,
        "still inside the 60-second window"
    );
    e.sql("UPDATE raids SET execute_at=now()-interval '61 seconds' WHERE status='moved'")
        .await;
    tick(e).await;
    assert_eq!(arrivals(e, &id).await, Some(2));
    // A raid explains a burst of arrivals for viewer integrity.
    let mut conn = e.app.db.acquire().await.unwrap();
    assert!(
        sver::raids::explains_burst(&mut conn, "rd-bb")
            .await
            .unwrap()
    );
    drop(conn);
    // One raid per broadcast every 10 minutes.
    assert_eq!(raid(e, &raider, "RaidB").await.0, StatusCode::CONFLICT);

    // The raider ends: their channel hosts the target, and the page shows it.
    end(e, "rd-ba").await;
    tick(e).await;
    assert_eq!(
        hosted(e, "rd-a").await,
        Some(("rd-b".into(), "raid".into()))
    );
    let (_, page) = call(e, "GET", "/api/channels/RaidA/live", None, Value::Null).await;
    assert_eq!(
        (&page["live"], &page["hosting"]["username"]),
        (&json!(false), &json!("RaidB"))
    );
    // Hosting stops when the host goes live; a raid hosts only once.
    live(e, "rd-ba2", "rd-a").await;
    tick(e).await;
    assert_eq!(hosted(e, "rd-a").await, None);
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/hosting/target",
        Some(&raider),
        json!({"username":"RaidB"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "only while offline");
    end(e, "rd-ba2").await;
    tick(e).await;
    assert_eq!(hosted(e, "rd-a").await, None, "the raid hosts once");

    // Manual hosting, stopped when the target goes offline.
    let (status, body) = call(
        e,
        "PUT",
        "/api/me/hosting/target",
        Some(&raider),
        json!({"username":"RaidB"}),
    )
    .await;
    assert_eq!(
        (status, &body["hosting"]["source"]),
        (StatusCode::OK, &json!("manual"))
    );
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/me/hosting/target",
            Some(&raider),
            json!({"username":"RaidC"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "offline target"
    );
    end(e, "rd-bb").await;
    tick(e).await;
    assert_eq!(hosted(e, "rd-a").await, None);

    // Auto-host takes the first live channel on the list and moves on when it ends.
    let too_many: Vec<String> = (0..11).map(|n| format!("RaidFan{n}")).collect();
    assert_eq!(
        call(
            e,
            "PUT",
            "/api/me/hosting",
            Some(&raider),
            json!({"accept_raids":true,"accept_hosts":true,"auto_host":true,"auto_list":too_many})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/hosting",
        Some(&raider),
        settings(true, &["RaidC", "RaidB"], true),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    live(e, "rd-bb2", "rd-b").await;
    live(e, "rd-bc", "rd-c").await;
    tick(e).await;
    assert_eq!(
        hosted(e, "rd-a").await,
        Some(("rd-c".into(), "auto".into()))
    );
    end(e, "rd-bc").await;
    tick(e).await;
    assert_eq!(
        hosted(e, "rd-a").await,
        Some(("rd-b".into(), "auto".into()))
    );
    // Targets can opt out of being hosted.
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/hosting",
        Some(&target),
        settings(false, &[], false),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    tick(e).await;
    assert_eq!(hosted(e, "rd-a").await, None);
    let (_, mine) = call(e, "GET", "/api/me/hosting", Some(&raider), Value::Null).await;
    assert_eq!(mine["auto_list"], json!(["RaidC", "RaidB"]));

    e.sql("DELETE FROM users WHERE id LIKE 'rd-%'").await;
}
