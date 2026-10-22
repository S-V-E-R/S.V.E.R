//! Verified followers (docs/PROFILES.md): a follow counts (public count and list, the bot's line,
//! the follow bonus) only once the follower has watched the channel live for 15 minutes on a
//! counted lease; the owner's Studio shows the follows still waiting.
use super::Env;
use super::chat::{call, person};
use serde_json::{Value, json};

async fn public_count(e: &Env) -> Value {
    call(e, "GET", "/api/channels/VfOwner", None, Value::Null)
        .await
        .1["channel"]["follower_count"]
        .clone()
}
async fn studio(e: &Env, owner: &str) -> Value {
    call(e, "GET", "/api/me/followers", Some(owner), Value::Null)
        .await
        .1
}
async fn greeted(e: &Env) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM bot_events WHERE channel_id='vf-owner' AND kind='follow'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}
/// One watch interval for everyone on a live lease.
async fn watch(e: &Env) {
    e.sql("UPDATE engagement SET last_watch_at=now()-interval '10 minutes' WHERE channel_id='vf-owner'").await;
    sver::engagement::award_watch(&e.app).await.unwrap();
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "vf-owner", "VfOwner", true).await;
    let fan = person(e, "vf-fan", "VfFan", true).await;
    let bot = person(e, "vf-bot", "VfBot", false).await;

    // A follow from someone who hasn't watched waits: no count, no list entry, no greeting.
    for token in [&fan, &bot] {
        let (_, followed) = call(e, "PUT", "/api/follows/VfOwner", Some(token), Value::Null).await;
        assert_eq!(followed["follower_count"], 0, "{followed}");
    }
    assert_eq!(public_count(e).await, 0);
    let (_, list) = call(
        e,
        "GET",
        "/api/channels/VfOwner/followers",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(list["items"], json!([]), "{list}");
    assert_eq!(
        studio(e, &owner).await,
        json!({"counted": 0, "pending": 2, "watch_minutes": 15})
    );
    assert_eq!(greeted(e).await, 0);

    // Live watch time on counted leases makes it count at 15 minutes (5 per default interval).
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('vf-b','vf-owner','vf-b',1,'LIVE','s','v','c',now(),now(),now(),'skipped')").await;
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('vf-b','u:vf-fan',now()+interval '1 hour','trusted'),('vf-b','u:vf-bot',now()+interval '1 hour','trusted')").await;
    watch(e).await;
    watch(e).await;
    assert_eq!(public_count(e).await, 0, "10 minutes isn't enough");
    watch(e).await;
    assert_eq!(public_count(e).await, 1);
    assert_eq!(
        studio(e, &owner).await,
        json!({"counted": 1, "pending": 1, "watch_minutes": 15}),
        "unverified accounts never earn watch time"
    );
    assert_eq!(
        greeted(e).await,
        1,
        "the bot greets the follow once it counts"
    );
    let bonus: bool = sqlx::query_scalar(
        "SELECT follow_bonus FROM engagement WHERE channel_id='vf-owner' AND user_id='vf-fan'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(bonus, "the follow bonus pays when the follow counts");
    watch(e).await;
    assert_eq!(greeted(e).await, 1, "counted once");

    // Someone who already watched enough counts the moment they follow again.
    call(e, "DELETE", "/api/follows/VfOwner", Some(&fan), Value::Null).await;
    assert_eq!(public_count(e).await, 0);
    let (_, followed) = call(e, "PUT", "/api/follows/VfOwner", Some(&fan), Value::Null).await;
    assert_eq!(followed["follower_count"], 1, "{followed}");
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='vf-b'")
        .await;
}
