//! Hate-raid defenses (docs/LIVE_STREAMS.md): a minimum account age for chat, the sweep that bans
//! new accounts that just chatted or followed (and removes their follows and messages), raids only
//! from followed channels, and follow alerts that never put a banned word on stream.
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

const BASE: &str = "/api/channels/RdOwner";

async fn say(e: &Env, token: &str, body: &str) -> StatusCode {
    call(
        e,
        "POST",
        &format!("{BASE}/chat"),
        Some(token),
        json!({"id": id(), "body": body}),
    )
    .await
    .0
}
async fn settings(e: &Env, owner: &str, extra: Value) -> (StatusCode, Value) {
    let mut body = json!({"slow_mode_seconds": 0, "block_links": false, "banned_words": ["slurword"], "reason": "raid"});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    call(
        e,
        "PUT",
        &format!("{BASE}/chat/settings"),
        Some(owner),
        body,
    )
    .await
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "rd-owner", "RdOwner", true).await;
    let regular = person(e, "rd-regular", "RdRegular", true).await;
    let raider1 = person(e, "rd-raider1", "RdRaiderOne", true).await;
    let raider2 = person(e, "rd-raider2", "RdRaiderTwo", true).await;
    e.sql("UPDATE users SET created_at=now()-interval '60 days' WHERE id IN ('rd-owner','rd-regular')")
        .await;

    // Minimum account age: new accounts can't chat, older ones and the owner can.
    assert_eq!(
        settings(e, &owner, json!({"min_account_days": 91})).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, _) = settings(e, &owner, json!({"min_account_days": 7})).await;
    assert_eq!(status, StatusCode::OK);
    let (_, view) = call(
        e,
        "GET",
        &format!("{BASE}/chat/moderation"),
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(view["settings"]["min_account_days"], 7, "{view}");
    assert_eq!(say(e, &raider1, "hello").await, StatusCode::FORBIDDEN);
    assert_eq!(say(e, &regular, "hello").await, StatusCode::OK);
    assert_eq!(say(e, &owner, "welcome").await, StatusCode::OK);

    // The sweep: new accounts that chatted or followed in the window are banned, their follows and
    // those messages removed; older accounts and the owner are untouched.
    settings(e, &owner, json!({"min_account_days": 0})).await;
    assert_eq!(say(e, &raider1, "spam spam").await, StatusCode::OK);
    call(
        e,
        "PUT",
        "/api/follows/RdOwner",
        Some(&raider2),
        Value::Null,
    )
    .await;
    let sweep = |body: Value, token: String| async move {
        call(e, "POST", &format!("{BASE}/chat/sweep"), Some(&token), body).await
    };
    assert_eq!(
        sweep(json!({"minutes": 61, "account_days": 7}), owner.clone())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        sweep(json!({"minutes": 10, "account_days": 7}), regular.clone())
            .await
            .0,
        StatusCode::FORBIDDEN,
        "only the channel's team can sweep"
    );
    let (status, swept) = sweep(json!({"minutes": 10, "account_days": 7}), owner.clone()).await;
    assert_eq!(status, StatusCode::OK, "{swept}");
    assert_eq!(swept, json!({"banned": 2, "review": true}));
    let banned: Vec<String> = sqlx::query_scalar("SELECT user_id FROM channel_restrictions WHERE channel_id='rd-owner' AND kind='ban' ORDER BY user_id")
        .fetch_all(&e.app.db).await.unwrap();
    assert_eq!(banned, ["rd-raider1", "rd-raider2"]);
    let left: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM follows WHERE following_id='rd-owner'),(SELECT count(*) FROM chat_messages WHERE channel_id='rd-owner' AND deleted_at IS NULL AND author_id='rd-raider1')")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(left, (0, 0), "their follows and messages are gone");
    let reviews: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_moderation_log WHERE channel_id='rd-owner' AND action='ban' AND needs_review AND detail ? 'chat'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(
        reviews, 2,
        "each ban waits for a reason, with the shared evidence"
    );
    assert_eq!(say(e, &raider1, "back").await, StatusCode::FORBIDDEN);
    assert_eq!(
        sweep(json!({"minutes": 10, "account_days": 7}), owner.clone())
            .await
            .1,
        json!({"banned": 0, "review": false}),
        "already banned"
    );

    // Raids only from followed channels.
    let (status, hosting) = call(e, "PUT", "/api/me/hosting", Some(&owner), json!({"accept_raids": true, "raids_followed_only": true, "accept_hosts": true, "auto_host": false, "auto_list": []})).await;
    assert_eq!(status, StatusCode::OK, "{hosting}");
    assert_eq!(hosting["raids_followed_only"], true);
    let refusal = || async {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT viewer_send_refusal('rd-regular','rd-owner',false)",
        )
        .fetch_one(&e.app.db)
        .await
        .unwrap()
    };
    assert_eq!(refusal().await.as_deref(), Some("followed_only"));
    let hosts: Option<String> =
        sqlx::query_scalar("SELECT viewer_send_refusal('rd-regular','rd-owner',true)")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(hosts, None, "hosting isn't limited");
    call(
        e,
        "PUT",
        "/api/follows/RdRegular",
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(refusal().await, None);
    let (_, kept) = call(
        e,
        "PUT",
        "/api/me/hosting",
        Some(&owner),
        json!({"accept_raids": true, "accept_hosts": true, "auto_host": false, "auto_list": []}),
    )
    .await;
    assert_eq!(
        kept["raids_followed_only"], true,
        "left out, the setting stays"
    );

    // A counted follow whose name has a banned word counts but never alerts.
    let slur = person(e, "rd-slur", "SlurWordFan", true).await;
    e.sql("INSERT INTO engagement(channel_id,user_id,watch_minutes) VALUES('rd-owner','rd-slur',15),('rd-owner','rd-regular',15) ON CONFLICT (channel_id,user_id) DO UPDATE SET watch_minutes=15").await;
    call(e, "PUT", "/api/follows/RdOwner", Some(&slur), Value::Null).await;
    call(
        e,
        "PUT",
        "/api/follows/RdOwner",
        Some(&regular),
        Value::Null,
    )
    .await;
    let alerts: Vec<Value> = sqlx::query_scalar(
        "SELECT data FROM events WHERE topic='channel:rdowner:follows:detail' ORDER BY id",
    )
    .fetch_all(&e.app.db)
    .await
    .unwrap();
    assert_eq!(alerts, [json!({"user": "RdRegular"})]);
    let (_, page) = call(e, "GET", BASE, None, Value::Null).await;
    assert_eq!(page["channel"]["follower_count"], 2);
}
