//! Channel moderation: the owner/moderator/staff matrix, peer protection, deletes, timeouts,
//! bans (including signed-in playback), chat rules, immediate demotion and the audit log.
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

const BASE: &str = "/api/channels/streamer";

async fn owner(e: &Env, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    e.request(method, path, body, true, true, false).await
}
async fn say(e: &Env, token: &str, body: &str) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("{BASE}/chat"),
        Some(token),
        json!({"id":id(),"body":body}),
    )
    .await
}
async fn restrict(
    e: &Env,
    token: &str,
    user: &str,
    kind: &str,
    seconds: Option<i64>,
) -> StatusCode {
    call(
        e,
        "POST",
        &format!("{BASE}/chat/restrictions"),
        Some(token),
        json!({"username":user,"kind":kind,"seconds":seconds,"reason":"test"}),
    )
    .await
    .0
}

pub async fn exercise(e: &Env) {
    let moderator = person(e, "mod-user", "ModUser", true).await;
    let chatter = person(e, "mod-chatter", "ModChatter", true).await;
    let outsider = person(e, "mod-outsider", "ModOutsider", true).await;
    let staff = person(e, "mod-staff", "ModStaff", true).await;
    person(e, "mod-unverified", "ModUnverified", false).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='mod-staff'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='mod-staff'")
        .await;
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('mod-staff','admin')")
        .await;

    // Only the owner, appointed moderators and MFA staff can moderate.
    assert_eq!(
        call(
            e,
            "GET",
            &format!("{BASE}/chat/moderation"),
            None,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            e,
            "GET",
            &format!("{BASE}/chat/moderation"),
            Some(&outsider),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        owner(e, "GET", &format!("{BASE}/chat/moderation"), Value::Null)
            .await
            .1["role"],
        "owner"
    );
    assert_eq!(
        call(
            e,
            "GET",
            &format!("{BASE}/chat/moderation"),
            Some(&staff),
            Value::Null
        )
        .await
        .1["role"],
        "staff"
    );

    // Appointment: owner only, verified accounts in good standing only.
    assert_eq!(
        call(
            e,
            "POST",
            &format!("{BASE}/moderators"),
            Some(&outsider),
            json!({"username":"ModUser"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        owner(
            e,
            "POST",
            &format!("{BASE}/moderators"),
            json!({"username":"ModUnverified"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        owner(
            e,
            "POST",
            &format!("{BASE}/moderators"),
            json!({"username":"moduser"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "GET",
            &format!("{BASE}/chat/moderation"),
            Some(&moderator),
            Value::Null
        )
        .await
        .1["role"],
        "moderator"
    );

    // Deletes: reason required, idempotent, tombstoned out of history; moderators can't delete the owner.
    let (_, posted) = say(e, &chatter, "delete me").await;
    let message = posted["message"]["id"].as_str().unwrap().to_string();
    let path = format!("{BASE}/chat/messages/{message}");
    assert_eq!(
        call(e, "DELETE", &path, Some(&moderator), json!({"reason":" "}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(e, "DELETE", &path, Some(&outsider), json!({"reason":"no"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            e,
            "DELETE",
            &path,
            Some(&moderator),
            json!({"reason":"spam"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "DELETE",
            &path,
            Some(&moderator),
            json!({"reason":"spam"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, history) = call(e, "GET", &format!("{BASE}/chat"), None, Value::Null).await;
    assert!(!history.to_string().contains("delete me"));
    // The owner sent two messages at the end of the chat test; stay inside two per second.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (_, own) = owner(
        e,
        "POST",
        &format!("{BASE}/chat"),
        json!({"id":id(),"body":"owner says"}),
    )
    .await;
    let own_id = own["message"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("owner send failed: {own}"));
    assert_eq!(
        call(
            e,
            "DELETE",
            &format!("{BASE}/chat/messages/{own_id}"),
            Some(&moderator),
            json!({"reason":"no"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, mod_message) = say(e, &moderator, "mod here").await;
    assert_eq!(mod_message["message"]["role"], "moderator");

    // Peer protection: nobody restricts the owner, themselves, a moderator or staff.
    for (target, who) in [
        ("Streamer", &moderator),
        ("ModUser", &moderator),
        ("ModStaff", &moderator),
        ("ModUser", &staff),
    ] {
        assert_eq!(
            restrict(e, who, target, "ban", None).await,
            StatusCode::FORBIDDEN,
            "{target}"
        );
    }
    assert_eq!(
        owner(
            e,
            "POST",
            &format!("{BASE}/chat/restrictions"),
            json!({"username":"ModUser","kind":"ban","reason":"x"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "owner removes a moderator first"
    );

    // Timeouts: 60 s to 14 days, enforced on send, liftable.
    assert_eq!(
        restrict(e, &moderator, "ModChatter", "timeout", Some(59)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        restrict(e, &moderator, "ModChatter", "timeout", Some(1_209_601)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        restrict(e, &moderator, "ModChatter", "timeout", Some(60)).await,
        StatusCode::OK
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (status, timed_out) = say(e, &chatter, "let me talk").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(timed_out.to_string().contains("timed out"));
    assert_eq!(
        call(
            e,
            "DELETE",
            &format!("{BASE}/chat/restrictions/ModChatter/timeout"),
            Some(&moderator),
            json!({"reason":"ok"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(say(e, &chatter, "thanks").await.0, StatusCode::OK);

    // A ban stops chat and signed-in playback; logged-out viewing still gets playback.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('mod-b','stream-owner','pub-mod',1,'LIVE','s','v','c',now(),now(),now()+interval '15 seconds')").await;
    assert_eq!(
        restrict(e, &moderator, "ModChatter", "ban", None).await,
        StatusCode::OK
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(
        say(e, &chatter, "banned?")
            .await
            .1
            .to_string()
            .contains("banned")
    );
    let (_, banned_view) = call(
        e,
        "GET",
        &format!("{BASE}/live"),
        Some(&chatter),
        Value::Null,
    )
    .await;
    assert_eq!(
        (
            banned_view["banned"].as_bool(),
            banned_view["playback"].is_null()
        ),
        (Some(true), true)
    );
    let (_, guest_view) = call(e, "GET", &format!("{BASE}/live"), None, Value::Null).await;
    assert!(guest_view["playback"]["hls"].is_string());
    let (_, beat) = call(
        e,
        "POST",
        &format!("{BASE}/live/beat"),
        Some(&chatter),
        json!({"broadcast_id":"mod-b","browser_id":"browser-banned-0000"}),
    )
    .await;
    assert_eq!(beat["recorded"], false);
    e.sql("DELETE FROM broadcasts WHERE id='mod-b'").await;

    // Chat rules: banned phrases (no exemption), links (moderators exempt), slow mode.
    // Slow mode is off (0) or 3-120 seconds; nothing in between and nothing above 120 is valid.
    for seconds in [1, 2, 121, 3601] {
        assert_eq!(
            call(
                e,
                "PUT",
                &format!("{BASE}/chat/settings"),
                Some(&moderator),
                json!({"slow_mode_seconds":seconds,"block_links":false,"banned_words":[],"reason":"x"})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST,
            "{seconds}"
        );
    }
    assert_eq!(
        call(
            e,
            "PUT",
            &format!("{BASE}/chat/settings"),
            Some(&moderator),
            json!({"slow_mode_seconds":3,"block_links":false,"banned_words":[],"reason":"x"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let too_many: Vec<String> = (0..201).map(|i| format!("word{i}")).collect();
    assert_eq!(
        call(
            e,
            "PUT",
            &format!("{BASE}/chat/settings"),
            Some(&moderator),
            json!({"slow_mode_seconds":0,"block_links":false,"banned_words":too_many,"reason":"x"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, saved) = call(e, "PUT", &format!("{BASE}/chat/settings"), Some(&moderator), json!({"slow_mode_seconds":30,"block_links":true,"banned_words":["Bad   Word","bad word"],"reason":"rules"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["banned_words"], json!(["bad word"]));
    // The OBS chat overlay's fade time: 10-120 seconds, kept when a save leaves it out.
    let rules = |fade: i64| json!({"slow_mode_seconds":30,"block_links":true,"banned_words":["bad word"],"reason":"rules","overlay_fade_seconds":fade});
    assert_eq!(
        call(
            e,
            "PUT",
            &format!("{BASE}/chat/settings"),
            Some(&moderator),
            rules(5)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "PUT",
            &format!("{BASE}/chat/settings"),
            Some(&moderator),
            rules(45)
        )
        .await
        .0,
        StatusCode::OK
    );
    call(e, "PUT", &format!("{BASE}/chat/settings"), Some(&moderator), json!({"slow_mode_seconds":30,"block_links":true,"banned_words":["bad word"],"reason":"rules"})).await;
    let (_, read) = call(e, "GET", &format!("{BASE}/chat"), None, Value::Null).await;
    assert_eq!(read["overlay_fade_seconds"], 45);
    // Signature emotes: one open emote, usable everywhere as Streamer/Code once reviewed; live lookup
    // stops it as soon as it's removed; the picker lists followed channels'; chats can opt out.
    e.sql("INSERT INTO channel_emotes(id,channel_id,code,image_key,tier,reviewed_at) VALUES('sig-open','stream-owner','Walnut','emotes/sig-open',NULL,NULL),('sig-tier','stream-owner','Acorn','emotes/sig-tier',1,now())").await;
    assert_eq!(
        e.request(
            "PUT",
            "/api/me/emotes/sig-tier/signature",
            json!({"on": true}),
            true,
            true,
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        e.request(
            "PUT",
            "/api/me/emotes/sig-open/signature",
            json!({"on": true}),
            true,
            true,
            false
        )
        .await
        .0,
        StatusCode::OK
    );
    let lookup = || {
        call(
            e,
            "GET",
            "/api/emotes/signatures?codes=streamer/Walnut,nobody/Thing",
            None,
            Value::Null,
        )
    };
    assert_eq!(
        lookup().await.1["emotes"],
        json!([]),
        "not until staff review it"
    );
    e.sql("UPDATE channel_emotes SET reviewed_at=now() WHERE id='sig-open'")
        .await;
    let (_, found) = lookup().await;
    assert_eq!(found["emotes"][0]["code"], "Streamer/Walnut", "{found}");
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('mod-outsider','stream-owner') ON CONFLICT DO NOTHING").await;
    let (_, picker) = call(
        e,
        "GET",
        "/api/me/signature-emotes",
        Some(&outsider),
        Value::Null,
    )
    .await;
    assert_eq!(picker["emotes"][0]["code"], "Streamer/Walnut");
    e.sql("UPDATE channel_emotes SET status='REMOVED' WHERE id='sig-open'")
        .await;
    assert_eq!(
        lookup().await.1["emotes"],
        json!([]),
        "removal stops it everywhere"
    );
    let (status, _) = call(e, "PUT", &format!("{BASE}/chat/settings"), Some(&moderator), json!({"slow_mode_seconds":30,"block_links":true,"banned_words":["bad word"],"reason":"rules","allow_signatures":false})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(e, "GET", &format!("{BASE}/chat"), None, Value::Null)
            .await
            .1["allow_signatures"],
        false
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        say(e, &outsider, "this has a BAD\nword in it").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        say(e, &outsider, "see Example.COM").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        say(e, &moderator, "official: example.com").await.0,
        StatusCode::OK
    );
    assert_eq!(
        say(e, &moderator, "bad word").await.0,
        StatusCode::BAD_REQUEST,
        "no exemption for banned words"
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(say(e, &outsider, "first").await.0, StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (status, slowed) = say(e, &outsider, "second").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(slowed.to_string().contains("Slow mode"));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        say(e, &moderator, "mods skip slow mode").await.0,
        StatusCode::OK
    );

    // Staff can lift; the owner removing a moderator takes effect on their next request.
    assert_eq!(
        call(
            e,
            "DELETE",
            &format!("{BASE}/chat/restrictions/ModChatter/ban"),
            Some(&staff),
            json!({"reason":"appeal"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        owner(
            e,
            "DELETE",
            &format!("{BASE}/moderators/ModUser"),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "GET",
            &format!("{BASE}/chat/moderation"),
            Some(&moderator),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, view) = owner(e, "GET", &format!("{BASE}/chat/moderation"), Value::Null).await;
    let actions: Vec<&str> = view["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["action"].as_str().unwrap())
        .collect();
    for action in [
        "appoint_moderator",
        "delete_message",
        "timeout",
        "lift_timeout",
        "ban",
        "settings",
        "lift_ban",
        "remove_moderator",
    ] {
        assert!(actions.contains(&action), "{action} audited");
    }
    assert_eq!(
        actions.iter().filter(|a| **a == "delete_message").count(),
        1,
        "retried delete logged once"
    );

    for table in ["chat_settings", "chat_messages", "channel_moderation_log"] {
        e.sql(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .await;
    }
    e.sql("DELETE FROM users WHERE id LIKE 'mod-%'").await;
}
