//! Channel moderation: the owner/moderator/staff matrix, peer protection, deletes, timeouts,
//! bans (including signed-in playback), chat rules, immediate demotion and the audit log.
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
use sver::security as sec;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

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

/// An app's call with an access token (no cookie).
async fn bearer(e: &Env, method: &str, path: &str, token: &str, body: Value) -> StatusCode {
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("sver-client-id", "mod-app")
        .extension(axum::extract::ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    tower::ServiceExt::oneshot(sver::router(e.app.clone()), request)
        .await
        .unwrap()
        .status()
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
        call(e, "DELETE", &path, Some(&outsider), json!({"reason":"no"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(e, "DELETE", &path, Some(&moderator), json!({}))
            .await
            .0,
        StatusCode::OK,
        "deleting needs no reason"
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
    // An app token acts for the owner or an appointed moderator only; staff moderate other
    // channels from their own signed-in session.
    e.sql("INSERT INTO dev_apps(id,owner_id,name,redirect_uris) VALUES('mod-app','mod-staff','Mod app',ARRAY['https://app.example.test/callback'])").await;
    e.sql("INSERT INTO oauth_grants(id,app_id,user_id,scopes) VALUES('mod-grant','mod-app','mod-staff',ARRAY['channel:moderate'])").await;
    let token = sec::token();
    sqlx::query("INSERT INTO oauth_tokens(token_hash,grant_id,kind,expires_at) VALUES($1,'mod-grant','access',now()+interval '1 hour')")
        .bind(sec::digest(&token))
        .execute(&e.app.db)
        .await
        .unwrap();
    let (_, posted) = say(e, &chatter, "staff token test").await;
    let path = format!(
        "{BASE}/chat/messages/{}",
        posted["message"]["id"].as_str().unwrap()
    );
    assert_eq!(
        bearer(e, "DELETE", &path, &token, json!({})).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(e, "DELETE", &path, Some(&staff), json!({})).await.0,
        StatusCode::OK
    );
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

    // Act first, review later: a timeout with no reason takes effect at once, carries the chat
    // before it as evidence and waits for its moderator to keep it (with a reason) or undo it.
    let quick = |token: String| async move {
        call(
            e,
            "POST",
            &format!("{BASE}/chat/restrictions"),
            Some(&token),
            json!({"username":"ModChatter","kind":"timeout","seconds":600}),
        )
        .await
    };
    let (status, body) = quick(moderator.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["review"], true);
    let view = |token: String| async move {
        call(
            e,
            "GET",
            &format!("{BASE}/chat/moderation"),
            Some(&token),
            Value::Null,
        )
        .await
        .1
    };
    let pending = view(moderator.clone()).await["reviews"].clone();
    assert_eq!(pending.as_array().map(Vec::len), Some(1), "{pending}");
    assert!(
        pending[0]["chat"].to_string().contains("thanks"),
        "{pending}"
    );
    let entry = pending[0]["id"].as_str().unwrap().to_string();
    let review = |token: String, body: Value| {
        let path = format!("{BASE}/chat/reviews/{entry}");
        async move { call(e, "POST", &path, Some(&token), body).await.0 }
    };
    assert_eq!(
        review(moderator.clone(), json!({"keep":true})).await,
        StatusCode::BAD_REQUEST,
        "keeping needs a reason"
    );
    assert_eq!(
        review(outsider.clone(), json!({"keep":true,"reason":"x"})).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        review(moderator.clone(), json!({"keep":true,"reason":"spam"})).await,
        StatusCode::OK
    );
    assert_eq!(view(moderator.clone()).await["reviews"], json!([]));
    // Undo lifts it.
    quick(moderator.clone()).await;
    let entry = view(moderator.clone()).await["reviews"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        call(
            e,
            "POST",
            &format!("{BASE}/chat/reviews/{entry}"),
            Some(&moderator),
            json!({"keep":false})
        )
        .await
        .0,
        StatusCode::OK
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(say(e, &chatter, "back again").await.0, StatusCode::OK);
    // Undo lifts only that entry's own timeout, never a newer one the owner gave since.
    quick(moderator.clone()).await;
    let entry = view(moderator.clone()).await["reviews"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        owner(
            e,
            "POST",
            &format!("{BASE}/chat/restrictions"),
            json!({"username":"ModChatter","kind":"timeout","seconds":1200,"reason":"owner's call"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, undone) = call(
        e,
        "POST",
        &format!("{BASE}/chat/reviews/{entry}"),
        Some(&moderator),
        json!({"keep":false}),
    )
    .await;
    assert_eq!((status, &undone["lifted"]), (StatusCode::OK, &json!(false)));
    let kept: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE user_id='mod-chatter' AND kind='timeout' AND until>now()+interval '15 minutes')")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(kept, "the owner's timeout stays");
    assert_eq!(
        owner(
            e,
            "DELETE",
            &format!("{BASE}/chat/restrictions/ModChatter/timeout"),
            json!({"reason":"ok"})
        )
        .await
        .0,
        StatusCode::OK
    );

    // A ban stops chat and signed-in playback; logged-out viewing still gets playback.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('mod-b','stream-owner','pub-mod',1,'LIVE','s','v','c',now(),now(),now()+interval '15 seconds')").await;

    // The viewer list names people who chatted during this broadcast, for the owner and mods.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(say(e, &chatter, "live now").await.0, StatusCode::OK);
    let (status, list) = owner(e, "GET", &format!("{BASE}/chatters"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let names: Vec<&str> = list["chatters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["username"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["ModChatter"],
        "earlier chat and the owner aren't listed: {list}"
    );
    assert_eq!(list["watching"], 0);
    assert_eq!(
        call(
            e,
            "GET",
            &format!("{BASE}/chatters"),
            Some(&outsider),
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        restrict(e, &moderator, "ModChatter", "ban", None).await,
        StatusCode::OK
    );
    // The ban took their lines of the last 10 minutes out of chat; its evidence still has them.
    let (_, history) = call(e, "GET", &format!("{BASE}/chat"), None, Value::Null).await;
    assert!(!history.to_string().contains("live now"), "{history}");
    let evidence: String = sqlx::query_scalar("SELECT detail->>'chat' FROM channel_moderation_log WHERE action='ban' AND target_id='mod-chatter' ORDER BY created_at DESC LIMIT 1")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(evidence.contains("live now"), "{evidence}");
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
    // Invisible characters don't carry a banned word or a link past the rules.
    let words = ["bad word".to_string()];
    assert!(!sver::moderation::code_allowed(
        &words,
        false,
        "ba\u{200B}d word"
    ));
    assert!(!sver::moderation::code_allowed(
        &[],
        true,
        "evil\u{200B}.com"
    ));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    for body in ["a ba\u{200B}d word", "see evil\u{200B}.com"] {
        assert_eq!(
            say(e, &outsider, body).await.0,
            StatusCode::BAD_REQUEST,
            "{body}"
        );
    }
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
    // A send refused after the rules passed (here: not enough Valor) leaves the slow-mode turn.
    e.sql("INSERT INTO payout_accounts(user_id,stripe_account,details_submitted,payouts_enabled) VALUES('stream-owner','acct_mod_tribute',true,true) ON CONFLICT DO NOTHING").await;
    let (status, poor) = call(
        e,
        "POST",
        &format!("{BASE}/chat"),
        Some(&outsider),
        json!({"id":id(),"body":"a tribute","tribute":50}),
    )
    .await;
    assert_eq!(
        (status, poor["error"].as_str()),
        (
            StatusCode::BAD_REQUEST,
            Some("You don't have enough Valor.")
        )
    );
    e.sql("DELETE FROM payout_accounts WHERE stripe_account='acct_mod_tribute'")
        .await;
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
        2,
        "the retried delete is logged once, beside staff's"
    );

    shared_chat_and_evidence(e).await;

    for table in ["chat_settings", "chat_messages", "channel_moderation_log"] {
        e.sql(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .await;
    }
    e.sql("DELETE FROM users WHERE id LIKE 'mod-%'").await;
}

/// A merged co-stream's shared chat: a raider who only chatted there is swept and the shared room
/// drops the line, while partners are left alone. Evidence goes after 90 days, and an erased
/// account's lines leave it at once.
async fn shared_chat_and_evidence(e: &Env) {
    person(e, "mod-partner", "ModPartner", true).await;
    person(e, "mod-raider", "ModRaider", true).await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline) VALUES('mod-sq-host','stream-owner','pub-mod-sq-host',1,'LIVE','s','v','c',now(),now(),now()+interval '15 seconds'),('mod-sq-guest','mod-partner','pub-mod-sq-guest',1,'LIVE','s','v','c2',now(),now(),now()+interval '15 seconds')").await;
    e.sql("INSERT INTO squads(id,host_id,mode) VALUES('mod-sq','stream-owner','MERGED')")
        .await;
    e.sql("INSERT INTO squad_members(squad_id,user_id,broadcast_id) VALUES('mod-sq','stream-owner','mod-sq-host'),('mod-sq','mod-partner','mod-sq-guest')").await;
    e.sql("INSERT INTO chat_messages(id,channel_id,author_id,body,squad_id) VALUES('mod-shared-raid','stream-owner','mod-raider','raid line','mod-sq'),('mod-shared-partner','stream-owner','mod-partner','hi all','mod-sq')").await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let mut request = format!("ws://{address}/api/chat/ws?squad=mod-sq")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", e.app.config.origin.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(
        super::chat::next_json(&mut socket).await["type"],
        "snapshot"
    );
    let (status, swept) = owner(
        e,
        "POST",
        &format!("{BASE}/chat/sweep"),
        json!({"minutes":10,"account_days":1,"reason":"raid"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{swept}");
    let deleted = loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a delete on the shared room")
            .unwrap()
            .unwrap();
        let event: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        if event["type"] == "delete" {
            break event["id"].clone();
        }
    };
    assert_eq!(deleted, "mod-shared-raid");
    server.abort();
    let banned = |user: &'static str| async move {
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id='stream-owner' AND user_id=$1 AND kind='ban')")
            .bind(user)
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    assert!(banned("mod-raider").await);
    assert!(
        !banned("mod-partner").await,
        "co-stream partners are left alone"
    );
    let gone: bool = sqlx::query_scalar(
        "SELECT deleted_at IS NOT NULL FROM chat_messages WHERE id='mod-shared-raid'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(gone);

    // Evidence older than 90 days loses its chat; newer evidence keeps it.
    let has_chat = |user: &'static str| async move {
        sqlx::query_scalar::<_, bool>("SELECT coalesce(bool_or(detail ? 'chat'),false) FROM channel_moderation_log WHERE target_id=$1")
            .bind(user)
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    e.sql("UPDATE channel_moderation_log SET created_at=now()-interval '91 days' WHERE target_id='mod-raider'").await;
    assert!(sver::safety::tick(&e.app).await.is_ok());
    assert!(!has_chat("mod-raider").await);
    assert!(has_chat("mod-chatter").await);

    // An erased account's lines leave every evidence capture.
    let quoted = || async {
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM channel_moderation_log WHERE detail->'chat' @> '[{\"user\":\"ModOutsider\"}]')")
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    assert!(quoted().await);
    let mut tx = e.app.db.begin().await.unwrap();
    sver::profile_jobs::erase(&mut tx, "mod-outsider")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!quoted().await);
    assert!(has_chat("mod-chatter").await, "other lines stay");

    e.sql("DELETE FROM squads WHERE id='mod-sq'").await;
    e.sql("DELETE FROM broadcasts WHERE id LIKE 'mod-sq-%'")
        .await;
}
