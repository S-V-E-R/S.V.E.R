//! Achievements and cosmetics (docs/ACHIEVEMENTS.md): earned once from records, shown publicly,
//! and cosmetics limited to what the account unlocked or earned.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

pub async fn exercise(e: &Env) {
    // Its own preconditions: a finished stream (First Light) and a follow (First Follow).
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,ended_at,confirmed_live_at) VALUES('ach-b','stream-owner','ach-b',1,'ENDED','s','v','c',now()-interval '2 hours',now()-interval '2 hours',now()-interval '2 hours',now()-interval '1 hour',now()-interval '2 hours')").await;
    person(e, "ach-fan", "AchFan", true).await;
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('ach-fan','stream-owner') ON CONFLICT DO NOTHING").await;
    sver::achievements::award(&e.app).await.unwrap();
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM user_achievements")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    let first = count().await;
    sver::achievements::award(&e.app).await.unwrap();
    assert_eq!(count().await, first, "earned once");
    let (status, public) = call(
        e,
        "GET",
        "/api/users/Streamer/achievements",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let keys: Vec<&str> = public["achievements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["key"].as_str().unwrap())
        .collect();
    assert!(keys.contains(&"first_light"), "{public}");
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM user_achievements WHERE key='first_follow' AND user_id='ach-fan')"
        )
        .fetch_one(&e.app.db)
        .await
        .unwrap()
    );

    // Cosmetics: only what's unlocked or earned.
    e.sql("DELETE FROM xp_days WHERE user_id='stream-owner'")
        .await;
    let mine = e.call("GET", "/api/me/cosmetics", Value::Null).await;
    assert_eq!(
        (mine["level"].as_i64(), mine["max_frame"].as_i64()),
        (Some(1), Some(0))
    );
    for bad in [
        json!({"name_style": "bold", "showcase": []}),
        json!({"name_style": "plain", "frame": 1, "showcase": []}),
        json!({"name_style": "plain", "showcase": ["first_light", "first_light"]}),
        json!({"name_style": "plain", "showcase": ["level_100"]}),
        json!({"name_style": "rainbow", "showcase": []}),
    ] {
        let (status, _) = e
            .request("PUT", "/api/me/cosmetics", bad.clone(), true, true, false)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    // Enough XP for level 10 unlocks Bold and the first frame.
    e.sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO xp_days(user_id,day,source,xp) VALUES('stream-owner',current_date,'chat',{})",
        sver::progression::xp_for(10)
    )))
    .await;
    let saved = e
        .call(
            "PUT",
            "/api/me/cosmetics",
            json!({"name_style": "bold", "frame": 1, "showcase": ["first_light"]}),
        )
        .await;
    assert_eq!(
        (saved["name_style"].as_str(), saved["frame"].as_i64()),
        (Some("bold"), Some(1))
    );
    // The style rides on every chip; the showcase on the user card.
    let chat = e
        .call("GET", "/api/channels/streamer/chat", Value::Null)
        .await;
    let own = chat["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["author"]["username"] == "Streamer");
    if let Some(own) = own {
        assert_eq!(own["author"]["name_style"], "bold");
    }
    let (_, card) = call(e, "GET", "/api/users/Streamer/card", None, Value::Null).await;
    assert_eq!(card["showcase"][0]["key"], "first_light", "{card}");
    assert_eq!(card["frame"], 1);

    // One pass grants every level reached and leaves an earned one as it was.
    e.sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO xp_days(user_id,day,source,xp) VALUES('ach-fan',current_date,'chat',{})",
        sver::progression::xp_for(25)
    )))
    .await;
    e.sql("INSERT INTO user_achievements(user_id,key,earned_at) VALUES('ach-fan','level_10',now()-interval '3 days')").await;
    sver::achievements::award(&e.app).await.unwrap();
    let levels: Vec<(String, bool)> = sqlx::query_as("SELECT key,earned_at<now()-interval '1 day' FROM user_achievements WHERE user_id='ach-fan' AND key LIKE 'level_%' ORDER BY key")
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        levels,
        [
            ("level_10".into(), true),
            ("level_20".into(), false),
            ("level_25".into(), false),
            ("level_5".into(), false)
        ]
    );

    // Unlock notices: one per achievement for a later earner, none for a new one's backfill.
    person(e, "ach-two", "AchTwo", true).await;
    e.sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO xp_days(user_id,day,source,xp) VALUES('ach-two',current_date,'chat',{})",
        sver::progression::xp_for(10)
    )))
    .await;
    e.sql("INSERT INTO creator_tiers(user_id,tier,checked_at) VALUES('ach-fan',3,now())")
        .await;
    sver::achievements::award(&e.app).await.unwrap();
    sver::achievements::award(&e.app).await.unwrap();
    let notices = |user: &'static str| async move {
        sqlx::query_scalar::<_, String>("SELECT event_key FROM notifications WHERE user_id=$1 AND kind='achievement' ORDER BY event_key")
            .bind(user)
            .fetch_all(&e.app.db)
            .await
            .unwrap()
    };
    let two = notices("ach-two").await;
    assert!(
        two.contains(&"achievement:level_10".to_string())
            && two.contains(&"achievement:level_5".to_string()),
        "{two:?}"
    );
    assert_eq!(
        two.len(),
        two.iter().collect::<std::collections::HashSet<_>>().len(),
        "one notice per achievement"
    );
    assert!(
        !notices("ach-fan")
            .await
            .contains(&"achievement:tier_3".to_string()),
        "a new achievement's backfill is quiet"
    );
    let earned_tier: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM user_achievements WHERE user_id='ach-fan' AND key='tier_3')",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(earned_tier);

    // The owner sees locked achievements with progress; visitors see earned ones with rarity.
    let own = e
        .call("GET", "/api/users/Streamer/achievements", Value::Null)
        .await;
    let locked = own["locked"].as_array().expect("owner gets locked");
    let hours = locked
        .iter()
        .find(|a| a["key"] == "hours_10")
        .expect("hours_10 locked");
    assert_eq!(
        (hours["progress"].as_i64(), hours["need"].as_i64()),
        (Some(1), Some(10)),
        "{hours}"
    );
    assert_eq!(
        own["achievements"].as_array().unwrap().len() + locked.len(),
        own["total"].as_u64().unwrap() as usize
    );
    let (_, visitor) = call(
        e,
        "GET",
        "/api/users/Streamer/achievements",
        None,
        Value::Null,
    )
    .await;
    assert!(visitor["locked"].is_null(), "{visitor}");
    let light = visitor["achievements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["key"] == "first_light")
        .unwrap();
    assert!(light["rarity"].as_f64().unwrap() > 0.0, "{light}");

    // Studio → Growth: the week's numbers and streamer achievements only.
    let growth = e.call("GET", "/api/me/growth", Value::Null).await;
    assert!(
        growth["this_week"]["streams"].as_i64().unwrap() >= 1,
        "{growth}"
    );
    assert!(
        growth["this_week"]["hours"].as_f64().unwrap() >= 1.0,
        "{growth}"
    );
    let streamer = growth["achievements"].as_array().unwrap();
    assert!(streamer.iter().all(|a| a["streamer"] == true));
    assert!(
        streamer
            .iter()
            .all(|a| a["category"].is_string() && a["band"].is_u64())
    );
    assert!(
        streamer
            .iter()
            .any(|a| a["key"] == "first_light" && !a["earned_at"].is_null())
    );

    // A tier promotion notifies by site and queues an email to a verified address.
    let mut db = e.app.db.acquire().await.unwrap();
    let mails = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mail_jobs WHERE user_id='stream-owner'")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    let before = mails().await;
    sver::tiers::promoted(&e.app, &mut db, "stream-owner", 1)
        .await
        .unwrap();
    let title: String = sqlx::query_scalar("SELECT payload->>'title' FROM notifications WHERE user_id='stream-owner' AND kind='tier' AND event_key='tier:1'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(title, "You reached the Trailblazer creator tier");
    let verified: bool =
        sqlx::query_scalar("SELECT email_verified FROM users WHERE id='stream-owner'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(mails().await, before + i64::from(verified));
}
