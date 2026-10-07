//! MAGNet Hype through the database: eligibility, the first pick, the countdown and switch, a
//! chat-burst moment on a small stream (Hype-side messages never count), the holding card, staff
//! force/release/stop with audit and the decision log, Studio settings, flags and history.
use super::Env;
use super::bans::staff;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn live(
    e: &Env,
    id: &'static str,
    owner: &'static str,
    category: &'static str,
    minutes: i32,
) {
    sqlx::query("INSERT INTO stream_settings(owner_id,title,category_id) VALUES($1,$1||' stream',$2) ON CONFLICT(owner_id) DO UPDATE SET category_id=$2")
        .bind(owner).bind(category).execute(&e.app.db).await.unwrap();
    sqlx::query("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES($1,$2,$1,1,'LIVE','s','v','c',now()-make_interval(mins=>$3),now(),now(),'skipped')")
        .bind(id).bind(owner).bind(minutes).execute(&e.app.db).await.unwrap();
}
/// A signed-in viewer whose session viewer integrity has counted.
async fn watching(e: &Env, broadcast: &str, user: &str) {
    sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,created_at,expires_at,level,ever_counted,signed_in,verified) VALUES($1,'u:'||$2,now()-interval '5 minutes',now()+interval '30 seconds','counted',true,true,true) ON CONFLICT DO NOTHING")
        .bind(broadcast).bind(user).execute(&e.app.db).await.unwrap();
}
async fn tick(e: &Env, lane: &str) {
    sver::magnet::tick_lane(&e.app, lane).await.unwrap();
}
async fn current(e: &Env, lane: &'static str) -> (Option<String>, Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT current_broadcast,current_kind,pending_broadcast FROM magnet_lanes WHERE id=$1",
    )
    .bind(lane)
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}
async fn get(e: &Env, path: &str, token: Option<&str>) -> (StatusCode, Value) {
    call(e, "GET", path, token, Value::Null).await
}

pub async fn exercise(e: &Env) {
    // Earlier tests may leave streams live; this test only looks at its own broadcasts.
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE state<>'ENDED'").await;
    sver::magnet::tick(&e.app).await.unwrap();
    let admin = staff(e, "mg-staff", "MgStaff").await;
    let small = person(e, "mg-small", "MgSmall", true).await;
    let banned = person(e, "mg-banned", "MgBanned", true).await;
    for (id, name) in [
        ("mg-big", "MgBig"),
        ("mg-out", "MgOptedOut"),
        ("mg-case", "MgCase"),
        ("mg-new", "MgJustLive"),
        ("mg-c1", "MgChatOne"),
        ("mg-c2", "MgChatTwo"),
        ("mg-c3", "MgChatThree"),
        ("mg-c4", "MgChatFour"),
    ] {
        person(e, id, name, true).await;
    }
    e.sql("UPDATE users SET created_at=now()-interval '30 days' WHERE id LIKE 'mg-c%'")
        .await;
    live(e, "mg-b-big", "mg-big", "art", 30).await;
    live(e, "mg-b-small", "mg-small", "art", 20).await;
    live(e, "mg-b-out", "mg-out", "art", 20).await;
    live(e, "mg-b-case", "mg-case", "valorant", 20).await;
    live(e, "mg-b-new", "mg-new", "art", 0).await;
    // Ineligible: opted out, an open integrity case, or live for less than 60 seconds.
    e.sql("INSERT INTO magnet_settings(user_id,opt_out) VALUES('mg-out',true)")
        .await;
    e.sql("INSERT INTO integrity_cases(id,broadcast_id,owner_id,evidence) VALUES('mg-ic','mg-b-case','mg-case','{}')").await;
    // A big audience on one stream must not matter.
    for n in 0..30 {
        sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('mg-b-big',$1,now()+interval '30 seconds','counted')")
            .bind(format!("b:mg-{n}")).execute(&e.app.db).await.unwrap();
    }

    // First pick: a fair turn among eligible streams only (the earlier-started one wins the tie).
    tick(e, "global").await;
    let (first, kind, _) = current(e, "global").await;
    assert_eq!(
        (first.as_deref(), kind.as_deref()),
        (Some("mg-b-big"), Some("fair"))
    );
    let (_, page) = get(e, "/api/magnet/global", None).await;
    assert_eq!(page["featured"]["stream"]["username"], "MgBig");
    assert_eq!(page["featured"]["stream"]["label"], "First feature");
    assert!(
        page["featured"]["reason"]
            .as_str()
            .unwrap()
            .starts_with("Fair turn")
    );
    // The art lane runs its own engine; the FPS lane only has an ineligible stream.
    tick(e, "art").await;
    assert!(current(e, "art").await.0.is_some());
    tick(e, "fps_battle_royale").await;
    assert_eq!(current(e, "fps_battle_royale").await.0, None);

    // A chat burst on the small stream: 4 verified chatters in the last minute, none before. Only
    // people actually watching count.
    e.sql("UPDATE magnet_lanes SET current_since=now()-interval '50 seconds' WHERE id='global'")
        .await;
    for author in ["mg-c1", "mg-c2", "mg-c3", "mg-c4"] {
        sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body) VALUES(gen_random_uuid()::text,'mg-small',$1,'what a play from '||$1)")
            .bind(author).execute(&e.app.db).await.unwrap();
    }
    tick(e, "global").await;
    assert_eq!(
        current(e, "global").await.2,
        None,
        "chatters who aren't watching don't count"
    );
    for author in ["mg-c1", "mg-c2", "mg-c3", "mg-c4"] {
        watching(e, "mg-b-small", author).await;
    }
    // Hype-side messages never count toward a burst (no feedback loop).
    for author in ["mg-c1", "mg-c2", "mg-c3", "mg-c4"] {
        sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body,origin) VALUES(gen_random_uuid()::text,'mg-big',$1,'hi from hype','global')")
            .bind(author).execute(&e.app.db).await.unwrap();
    }
    tick(e, "global").await;
    let (now_showing, _, pending) = current(e, "global").await;
    assert_eq!(
        now_showing.as_deref(),
        Some("mg-b-big"),
        "a countdown first"
    );
    assert_eq!(pending.as_deref(), Some("mg-b-small"));
    assert!(
        !sver::magnet::explains_burst(&mut e.app.db.acquire().await.unwrap(), "mg-b-small")
            .await
            .unwrap(),
        "a countdown is not a committed handoff"
    );
    let (_, page) = get(e, "/api/magnet/global", None).await;
    assert_eq!(page["next"]["stream"]["username"], "MgSmall");
    assert_eq!(page["next"]["reason"], "Chat is going off");
    // The countdown ends: the switch happens and is recorded as a moment.
    e.sql("UPDATE magnet_lanes SET switch_at=now()-interval '1 second' WHERE id='global'")
        .await;
    tick(e, "global").await;
    let (showing, kind, pending) = current(e, "global").await;
    assert_eq!(
        (showing.as_deref(), kind.as_deref(), pending),
        (Some("mg-b-small"), Some("moment"), None)
    );
    let decisions: i64 = sqlx::query_scalar("SELECT count(*) FROM magnet_decisions WHERE lane='global' AND kind='moment' AND jsonb_array_length(candidates)>=2")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(decisions, 1);
    // "You just watched": the stream MAGNet moved on from stays one tap away to follow.
    let (_, page) = get(e, "/api/magnet/global", None).await;
    assert_eq!(page["previous"]["members"][0]["username"], "MgBig");
    assert_eq!(page["previous"]["members"][0]["following"], Value::Null);
    assert_eq!(page["previous"]["members"][0]["live"], true);
    assert!(
        page["previous"]["reason"]
            .as_str()
            .unwrap()
            .starts_with("Fair turn")
    );
    let (_, page) = get(e, "/api/magnet/global", Some(&small)).await;
    assert_eq!(
        (
            &page["previous"]["members"][0]["following"],
            &page["previous"]["members"][0]["own"]
        ),
        (&json!(false), &json!(false))
    );
    let (status, _) = call(e, "PUT", "/api/follows/MgBig", Some(&small), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let (_, page) = get(e, "/api/magnet/global", Some(&small)).await;
    assert_eq!(page["previous"]["members"][0]["following"], true);
    // Blocked either way: no card.
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('mg-big','mg-banned')")
        .await;
    let (_, page) = get(e, "/api/magnet/global", Some(&banned)).await;
    assert_eq!(page["previous"], Value::Null);
    e.sql("DELETE FROM user_blocks WHERE blocker_id='mg-big'")
        .await;
    e.sql("DELETE FROM follows WHERE follower_id='mg-small' AND following_id='mg-big'")
        .await;
    let candidate_users: Vec<String> = sqlx::query_scalar("SELECT c->>'username' FROM magnet_decisions d, jsonb_array_elements(d.candidates) c WHERE d.lane='global' AND d.kind='moment'")
        .fetch_all(&e.app.db).await.unwrap();
    assert!(!candidate_users.contains(&"MgOptedOut".to_string()));
    assert!(!candidate_users.contains(&"MgCase".to_string()));
    assert!(!candidate_users.contains(&"MgJustLive".to_string()));
    // Next is a fair turn: no back-to-back moment even with another burst.
    e.sql("UPDATE magnet_lanes SET current_since=now()-interval '3 minutes' WHERE id='global'")
        .await;
    tick(e, "global").await;
    assert_eq!(current(e, "global").await.2, None, "alternation holds");

    // A viewer banned from the featured channel gets the holding card, never the stream.
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('mg-small','mg-banned','ban')").await;
    let (_, held) = get(e, "/api/magnet/global", Some(&banned)).await;
    assert_eq!(held["featured"]["holding"], true);
    assert!(held["featured"]["moves_on_by"].is_string());
    let (_, open) = get(e, "/api/magnet/global", None).await;
    assert_eq!(open["featured"]["holding"], false);
    // Hype viewers count for the featured stream and are tagged for the streamer's history.
    let (status, beat) = call(e, "POST", "/api/channels/MgSmall/live/beat", None,
        json!({"broadcast_id":"mg-b-small","browser_id":"browser-magnet-0000001","visible":true,"media_time":1.0,"magnet":"global"})).await;
    assert_eq!((status, &beat["recorded"]), (StatusCode::OK, &json!(true)));
    let tagged: i64 = sqlx::query_scalar("SELECT count(*) FROM playback_leases WHERE broadcast_id='mg-b-small' AND magnet_lane='global'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(tagged, 1);

    // Hype chat, merged with the featured channel (MgSmall): the message lands in MgSmall's own
    // chat with the MAGNet mark, under the channel's rules plus a 3-second Hype slow mode.
    let hyper = person(e, "mg-hype", "MgHyper", true).await;
    let say = |token: String, body: &'static str| async move {
        call(
            e,
            "POST",
            "/api/magnet/global/chat",
            Some(&token),
            json!({"id":id(),"body":body}),
        )
        .await
    };
    let (status, sent) = say(hyper.clone(), "hello from MAGNet").await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["message"]["origin"], "global");
    let (_, channel_chat) = get(e, "/api/channels/MgSmall/chat", None).await;
    assert!(
        channel_chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "hello from MAGNet" && m["origin"] == "global")
    );
    assert_eq!(
        say(hyper.clone(), "too fast").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    e.sql("INSERT INTO chat_settings(channel_id,banned_words) VALUES('mg-small','{forbidden}') ON CONFLICT(channel_id) DO UPDATE SET banned_words='{forbidden}'").await;
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'hype-slow:%'")
        .await;
    assert_eq!(
        say(hyper.clone(), "a forbidden word").await.0,
        StatusCode::BAD_REQUEST,
        "the channel's rules apply"
    );
    e.sql("UPDATE chat_settings SET banned_words='{}' WHERE channel_id='mg-small'")
        .await;
    // A viewer banned from the featured channel reads Hype chat but can't send.
    let (_, room) = get(e, "/api/magnet/global/chat", Some(&banned)).await;
    assert_eq!(
        (
            &room["holding"],
            &room["can_send"],
            &room["merged_with"]["username"]
        ),
        (&json!(true), &json!(false), &json!("MgSmall"))
    );
    assert!(
        room["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "hello from MAGNet")
    );
    let (status, refused) = say(banned.clone(), "let me in").await;
    assert_eq!(
        (status, refused["error"].as_str().unwrap_or("")),
        (StatusCode::FORBIDDEN, "Chat resumes when MAGNet moves on.")
    );
    // With chat merging off the room is its own; its messages can be reported and removed by staff.
    e.sql("INSERT INTO magnet_settings(user_id,chat_merge) VALUES('mg-small',false)")
        .await;
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'hype-slow:%'")
        .await;
    let (status, own) = say(hyper.clone(), "just the hype room").await;
    assert_eq!(status, StatusCode::OK);
    let own_id = own["message"]["id"].as_str().unwrap().to_string();
    let channel_of: Option<String> =
        sqlx::query_scalar("SELECT channel_id FROM chat_messages WHERE id=$1")
            .bind(&own_id)
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(channel_of, None);
    let (_, channel_chat) = get(e, "/api/channels/MgSmall/chat", None).await;
    assert!(
        !channel_chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "just the hype room")
    );
    let (_, room) = get(e, "/api/magnet/global/chat", None).await;
    assert_eq!(room["merged_with"], Value::Null);
    assert!(
        room["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "just the hype room")
    );
    let (status, _) = call(e, "POST", "/api/reports", Some(&banned), json!({"target_type":"chat_message","target_id":own_id,"reason":"harassment","note":"synthetic"})).await;
    assert_eq!(status, StatusCode::OK, "Hype room messages can be reported");
    let (status, _) = call(
        e,
        "POST",
        &format!("/api/admin/reports/chat_message/{own_id}/actions"),
        Some(&admin),
        json!({"action":"remove_content","note":"reviewed"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, room) = get(e, "/api/magnet/global/chat", None).await;
    assert!(
        !room["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "just the hype room")
    );
    e.sql("DELETE FROM magnet_settings WHERE user_id='mg-small'")
        .await;

    // Detach on switch: the merge ends, messages from it stay in the room, new channel chat doesn't
    // cross. The feature is restored afterwards for the Studio checks below.
    let ended: Vec<String> = sqlx::query_scalar("UPDATE magnet_features SET ended_at=now() WHERE lane='global' AND ended_at IS NULL RETURNING id")
        .fetch_all(&e.app.db).await.unwrap();
    e.sql("UPDATE magnet_lanes SET current_broadcast=NULL WHERE id='global'")
        .await;
    let (status, _) = call(
        e,
        "POST",
        "/api/channels/MgSmall/chat",
        Some(&small),
        json!({"id":id(),"body":"after the switch"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, room) = get(e, "/api/magnet/global/chat", None).await;
    let bodies: Vec<&str> = room["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["body"].as_str())
        .collect();
    assert_eq!(room["merged_with"], Value::Null, "detached");
    assert!(
        bodies.contains(&"hello from MAGNet"),
        "messages from the merge stay in the room's history"
    );
    assert!(
        !bodies.contains(&"after the switch"),
        "new channel chat stays in the channel"
    );
    sqlx::query("UPDATE magnet_features SET ended_at=NULL WHERE id=ANY($1)")
        .bind(&ended)
        .execute(&e.app.db)
        .await
        .unwrap();
    e.sql("UPDATE magnet_lanes SET current_broadcast='mg-b-small' WHERE id='global'")
        .await;

    // Studio: featured now, history, settings and the flag cooldown.
    let (_, mine) = get(e, "/api/me/magnet", Some(&small)).await;
    assert_eq!(mine["featured_now"][0]["lane"], "global");
    assert_eq!(mine["history"][0]["kind"], "moment");
    assert_eq!(mine["history"][0]["hype_viewers"], 1);
    assert_eq!(
        call(e, "POST", "/api/me/magnet/flag", Some(&small), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "POST", "/api/me/magnet/flag", Some(&small), Value::Null)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(e, "POST", "/api/me/magnet/flag", Some(&banned), Value::Null)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // Opting out takes effect at the next tick: the lane falls back to another stream at once.
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/magnet",
        Some(&small),
        json!({"opt_out":true,"chat_merge":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    tick(e, "global").await;
    let (showing, kind, _) = current(e, "global").await;
    assert_eq!(
        (showing.as_deref(), kind.as_deref()),
        (Some("mg-b-big"), Some("only"))
    );

    // Staff: private, force and release, emergency stop; every action audited.
    assert_eq!(
        get(e, "/api/admin/magnet", Some(&small)).await.0,
        StatusCode::NOT_FOUND
    );
    call(
        e,
        "PUT",
        "/api/me/magnet",
        Some(&small),
        json!({"opt_out":false,"chat_merge":true}),
    )
    .await;
    let (status, _) = call(
        e,
        "PUT",
        "/api/admin/magnet/global/force",
        Some(&admin),
        json!({"username":"MgSmall"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    tick(e, "global").await;
    e.sql("UPDATE magnet_lanes SET switch_at=now()-interval '1 second' WHERE id='global'")
        .await;
    tick(e, "global").await;
    assert_eq!(current(e, "global").await.1.as_deref(), Some("forced"));
    call(
        e,
        "PUT",
        "/api/admin/magnet/global/force",
        Some(&admin),
        json!({"username":null}),
    )
    .await;
    let (status, log) = call(
        e,
        "POST",
        "/api/admin/magnet/stop",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        log["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["kind"] == "forced")
    );
    tick(e, "global").await;
    assert_eq!(
        current(e, "global").await.0,
        None,
        "a stopped lane shows nothing"
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE actor_id='mg-staff' AND action LIKE 'magnet_%'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(audited, 3);
    e.sql("UPDATE magnet_lanes SET enabled=true").await;

    // Co-streams (docs/MAGNET.md "Co-streams on MAGNet"): a merged squad is one candidate.
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE id LIKE 'mg-%'").await;
    let guest = person(e, "mg-sq-guest", "MgSqGuest", true).await;
    for (id, name) in [
        ("mg-sq-host", "MgSqHost"),
        ("mg-sq-out", "MgSqOut"),
        ("mg-sq-solo", "MgSqSolo"),
    ] {
        person(e, id, name, true).await;
    }
    live(e, "mg-sq-b-host", "mg-sq-host", "art", 20).await;
    live(e, "mg-sq-b-guest", "mg-sq-guest", "art", 20).await;
    live(e, "mg-sq-b-out", "mg-sq-out", "art", 20).await;
    live(e, "mg-sq-b-solo", "mg-sq-solo", "art", 20).await;
    e.sql("INSERT INTO squads(id,host_id,mode,created_at) VALUES('mg-squad','mg-sq-host','MERGED',now()-interval '20 minutes')").await;
    e.sql("INSERT INTO squad_members(squad_id,user_id,broadcast_id) VALUES('mg-squad','mg-sq-host','mg-sq-b-host'),('mg-squad','mg-sq-guest','mg-sq-b-guest'),('mg-squad','mg-sq-out','mg-sq-b-out')").await;
    // An opted-out member is left out of the featured squad.
    e.sql("INSERT INTO magnet_settings(user_id,opt_out) VALUES('mg-sq-out',true)")
        .await;
    tick(e, "global").await;
    let (showing, kind, _) = current(e, "global").await;
    assert_eq!(
        (showing.as_deref(), kind.as_deref()),
        (Some("mg-sq-b-host"), Some("fair"))
    );
    let units: Value = sqlx::query_scalar(
        "SELECT candidates FROM magnet_decisions WHERE lane='global' ORDER BY at DESC LIMIT 1",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    let units = units.as_array().unwrap();
    assert_eq!(units.len(), 2, "the squad and the solo stream: {units:?}");
    assert!(
        units
            .iter()
            .any(|u| u["members"] == json!(["mg-sq-b-host", "mg-sq-b-guest"]))
    );
    // Every featured member is on the feature (shared cooldown) and sees it in Studio.
    let featured: Vec<String> = sqlx::query_scalar("SELECT owner_id FROM magnet_features WHERE lane='global' AND ended_at IS NULL ORDER BY owner_id")
        .fetch_all(&e.app.db).await.unwrap();
    assert_eq!(featured, ["mg-sq-guest", "mg-sq-host"]);
    // An ended feature still explains its recent arrivals, but the window is bounded even
    // when the lane continues showing this broadcast. Use one DB clock for the exact boundary.
    let mut tx = e.app.db.begin().await.unwrap();
    sqlx::query("UPDATE magnet_features SET ended_at=now() WHERE broadcast_id='mg-sq-b-guest'")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sver::magnet::explains_burst(&mut tx, "mg-sq-b-guest")
            .await
            .unwrap()
    );
    sqlx::query("UPDATE magnet_features SET ended_at=NULL,started_at=now()-interval '2 minutes' WHERE broadcast_id='mg-sq-b-guest'").execute(&mut *tx).await.unwrap();
    assert!(
        !sver::magnet::explains_burst(&mut tx, "mg-sq-b-guest")
            .await
            .unwrap()
    );
    sqlx::query("UPDATE magnet_features SET started_at=now()+interval '1 second' WHERE broadcast_id='mg-sq-b-guest'").execute(&mut *tx).await.unwrap();
    assert!(
        !sver::magnet::explains_burst(&mut tx, "mg-sq-b-guest")
            .await
            .unwrap()
    );
    tx.rollback().await.unwrap();
    // A real MAGNet switch explains arrivals on every featured member, not on an opted-out
    // member or an unrelated stream. A client-supplied lane label cannot grant this exemption.
    for broadcast in [
        "mg-sq-b-host",
        "mg-sq-b-guest",
        "mg-sq-b-out",
        "mg-sq-b-solo",
    ] {
        sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,turnstile_ok,magnet_lane) SELECT $1,'b:handoff-'||g,now()+interval '2 minutes',true,'global' FROM generate_series(1,25) g")
            .bind(broadcast).execute(&e.app.db).await.unwrap();
    }
    e.sql("UPDATE playback_leases SET hard_excluded=true WHERE broadcast_id='mg-sq-b-host' AND viewer_key='b:handoff-1'").await;
    e.sql("UPDATE playback_leases SET provisional_until=now()+interval '5 minutes' WHERE broadcast_id='mg-sq-b-host' AND viewer_key='b:handoff-2'").await;
    sver::integrity::tick(&e.app).await.unwrap();
    let held: Vec<(String, i64)> = sqlx::query_as("SELECT broadcast_id,count(*) FROM playback_leases WHERE broadcast_id LIKE 'mg-sq-%' AND provisional_until>now() GROUP BY broadcast_id ORDER BY broadcast_id")
        .fetch_all(&e.app.db).await.unwrap();
    assert_eq!(
        held,
        [
            ("mg-sq-b-host".into(), 1),
            ("mg-sq-b-out".into(), 25),
            ("mg-sq-b-solo".into(), 25)
        ],
        "an existing provisional hold survives the handoff"
    );
    let hard: String = sqlx::query_scalar("SELECT level FROM playback_leases WHERE broadcast_id='mg-sq-b-host' AND viewer_key='b:handoff-1'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(hard, "excluded", "a handoff never overrides hard evidence");
    e.sql("DELETE FROM playback_leases WHERE viewer_key LIKE 'b:handoff-%'")
        .await;
    let (_, studio) = get(e, "/api/me/magnet", Some(&guest)).await;
    assert_eq!(studio["featured_now"][0]["lane"], "global");
    // Viewers get tabs for the featured members; any member's ban holds the viewer.
    let (_, page) = get(e, "/api/magnet/global", None).await;
    assert_eq!(page["featured"]["squad"]["mode"], "MERGED");
    let tabs: Vec<&str> = page["featured"]["squad"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["username"].as_str().unwrap())
        .collect();
    assert_eq!(tabs, ["MgSqHost", "MgSqGuest"]);
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('mg-sq-guest','mg-banned','ban')").await;
    let (_, held) = get(e, "/api/magnet/global", Some(&banned)).await;
    assert_eq!(held["featured"]["holding"], true);
    // MAGNet chat merges with the squad's shared chat, under its rules and with the MAGNet mark.
    e.sql("DELETE FROM rate_limits WHERE key LIKE 'hype-slow:%' OR key LIKE 'chat-%:mg-hype'")
        .await;
    let (status, sent) = say(hyper.clone(), "hello co-stream").await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let stored: (Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT channel_id,squad_id,origin FROM chat_messages WHERE id=$1")
            .bind(sent["message"]["id"].as_str().unwrap())
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(
        stored,
        (
            Some("mg-sq-host".into()),
            Some("mg-squad".into()),
            Some("global".into())
        )
    );
    let (_, shared) = get(e, "/api/squads/mg-squad/chat", None).await;
    assert!(
        shared["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "hello co-stream")
    );
    let (_, room) = get(e, "/api/magnet/global/chat", Some(&banned)).await;
    assert_eq!(
        (&room["co_stream"], &room["can_send"]),
        (&json!(true), &json!(false))
    );
    // A fair turn to the solo stream; the squad then cools down as a whole, so a burst in its
    // shared chat waits until the cooldown passes.
    e.sql("UPDATE magnet_lanes SET current_since=now()-interval '9 minutes' WHERE id='global'")
        .await;
    tick(e, "global").await;
    e.sql("UPDATE magnet_lanes SET switch_at=now()-interval '1 second' WHERE id='global'")
        .await;
    tick(e, "global").await;
    assert_eq!(
        current(e, "global").await.0.as_deref(),
        Some("mg-sq-b-solo")
    );
    // Shared-chat viewers may watch any member.
    for (author, watched) in [
        ("mg-c1", "mg-sq-b-host"),
        ("mg-c2", "mg-sq-b-guest"),
        ("mg-c3", "mg-sq-b-host"),
        ("mg-c4", "mg-sq-b-guest"),
    ] {
        watching(e, watched, author).await;
        sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body,squad_id) VALUES(gen_random_uuid()::text,'mg-sq-host',$1,'what a play by '||$1,'mg-squad')")
            .bind(author).execute(&e.app.db).await.unwrap();
    }
    // Outside the 2-minute gap after the earlier moment, so only the cooldown can hold it back.
    e.sql("UPDATE magnet_lanes SET current_since=now()-interval '50 seconds',last_moment_at=NULL WHERE id='global'")
        .await;
    tick(e, "global").await;
    assert_eq!(
        current(e, "global").await.2,
        None,
        "the squad is cooling down"
    );
    e.sql("UPDATE magnet_features SET started_at=now()-interval '45 minutes',ended_at=now()-interval '40 minutes' WHERE owner_id IN ('mg-sq-host','mg-sq-guest')").await;
    tick(e, "global").await;
    assert_eq!(
        current(e, "global").await.2.as_deref(),
        Some("mg-sq-b-host"),
        "a burst in the shared chat is the squad's moment"
    );
    e.sql("DELETE FROM squads WHERE id='mg-squad'").await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE id LIKE 'mg-%'").await;

    hardening(e, &admin).await;
    genre_lane(e).await;

    for statement in [
        "DELETE FROM moderation_actions WHERE actor_id='mg-staff'",
        "DELETE FROM staff_roles WHERE user_id='mg-staff'",
        "DELETE FROM integrity_cases WHERE id='mg-ic'",
        "DELETE FROM users WHERE id LIKE 'mg-%'",
    ] {
        e.sql(statement).await;
    }
}

/// One stream's moment signals on the Global lane.
async fn signals(e: &Env, broadcast: &str) -> sver::magnet::Signals {
    let mut db = e.app.db.acquire().await.unwrap();
    sver::magnet::candidates(&mut db, "global", &e.app.config.magnet)
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.1.broadcast == broadcast)
        .map(|c| c.2)
        .unwrap()
}
async fn say_in(e: &Env, channel: &str, author: &str, body: &str, extra: &str) {
    sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO chat_messages(id,channel_id,author_id,body{extra}) VALUES(gen_random_uuid()::text,$1,$2,$3{})", if extra.is_empty() { "" } else { ",10" })))
        .bind(channel).bind(author).bind(body).execute(&e.app.db).await.unwrap();
}

/// Hardening (docs/MAGNET.md "Hardening"): only real viewers count, never MAGNet's own audience or
/// paid messages; low-effort chat counts for less; a raid counts when it brought someone, once a
/// day per pair; the lane recovers at once when the next stream leaves during a countdown; an
/// ended staff pick is released; MAGNet's arrivals don't look like a viewbot spike.
async fn hardening(e: &Env, admin: &str) {
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE id LIKE 'mg-%'").await;
    e.sql("UPDATE magnet_features SET ended_at=now() WHERE ended_at IS NULL")
        .await;
    e.sql("UPDATE magnet_lanes SET current_broadcast=NULL,current_kind=NULL,current_reason=NULL,current_since=NULL,pending_broadcast=NULL,pending_kind=NULL,pending_reason=NULL,switch_at=NULL,last_kind=NULL,last_moment_at=NULL,forced_broadcast=NULL WHERE id='global'").await;
    for (id, name) in [
        ("mg-ha", "MgHardA"),
        ("mg-hb", "MgHardB"),
        ("mg-hc", "MgHardC"),
        ("mg-hd", "MgHardD"),
    ] {
        person(e, id, name, true).await;
    }
    live(e, "mg-h-a", "mg-ha", "art", 30).await;
    live(e, "mg-h-b", "mg-hb", "art", 20).await;
    live(e, "mg-h-c", "mg-hc", "art", 20).await;
    live(e, "mg-h-d", "mg-hd", "art", 20).await;
    let chatters = ["mg-c1", "mg-c2", "mg-c3", "mg-c4"];

    // People MAGNet brought never count, so a feature can't keep itself going.
    for author in chatters {
        watching(e, "mg-h-b", author).await;
        say_in(e, "mg-hb", author, &format!("nice one {author}"), "").await;
    }
    e.sql("UPDATE playback_leases SET magnet_lane='global' WHERE broadcast_id='mg-h-b'")
        .await;
    assert_eq!(signals(e, "mg-h-b").await.chatters, 0.0);
    e.sql("UPDATE playback_leases SET magnet_lane=NULL WHERE broadcast_id='mg-h-b'")
        .await;
    assert_eq!(signals(e, "mg-h-b").await.chatters, 4.0);

    // Copy-paste and emote-only messages count half; paid messages (tributes, Skills) never count.
    for author in chatters {
        watching(e, "mg-h-c", author).await;
        say_in(e, "mg-hc", author, "GG", "").await;
    }
    assert_eq!(signals(e, "mg-h-c").await.chatters, 2.0);
    for author in chatters {
        say_in(e, "mg-hc", author, &format!("paid {author}"), ",tribute").await;
    }
    e.sql("INSERT INTO chat_messages(id,channel_id,author_id,body,skill) VALUES(gen_random_uuid()::text,'mg-hc','mg-c1','a skill','sticker')").await;
    assert_eq!(signals(e, "mg-h-c").await.chatters, 2.0);
    e.sql("INSERT INTO channel_emotes(id,channel_id,code,image_key) VALUES('mg-emote','mg-ha','HypeA','x')").await;
    for (n, author) in chatters.iter().enumerate() {
        watching(e, "mg-h-a", author).await;
        say_in(e, "mg-ha", author, &vec!["HypeA"; n + 1].join(" "), "").await;
    }
    assert_eq!(signals(e, "mg-h-a").await.chatters, 2.0);

    // Follows count from real viewers only, and never from MAGNet's audience.
    for follower in ["mg-c1", "mg-c2", "mg-c3", "mg-hc"] {
        sqlx::query("INSERT INTO follows(follower_id,following_id) VALUES($1,'mg-hd')")
            .bind(follower)
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    for author in ["mg-c1", "mg-c2", "mg-c3"] {
        watching(e, "mg-h-d", author).await;
    }
    assert_eq!(signals(e, "mg-h-d").await.follows, 3.0);
    e.sql("UPDATE playback_leases SET magnet_lane='global' WHERE broadcast_id='mg-h-d' AND viewer_key='u:mg-c1'").await;
    assert_eq!(signals(e, "mg-h-d").await.follows, 2.0);

    // A raid that brought nobody isn't a moment; one that did is, unless the two channels already
    // raided each other within the day.
    e.sql("INSERT INTO raids(id,raider_id,broadcast_id,target_id,target_broadcast_id,execute_at,status) VALUES('mg-raid','mg-hc','mg-h-c','mg-hd','mg-h-d',now()-interval '30 seconds','moved')").await;
    assert_eq!(signals(e, "mg-h-d").await.raided_by, None);
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level,raid_id) VALUES('mg-h-d','b:mg-raider',now()+interval '30 seconds','pending','mg-raid')").await;
    assert!(signals(e, "mg-h-d").await.raided_by.is_some());
    e.sql("INSERT INTO raids(id,raider_id,broadcast_id,target_id,target_broadcast_id,execute_at,status) VALUES('mg-raid-back','mg-hd','mg-h-d','mg-hc','mg-h-c',now()-interval '3 hours','moved')").await;
    assert_eq!(signals(e, "mg-h-d").await.raided_by, None);

    // The next stream ends during the countdown, and so does the current one: the lane falls back
    // in the same tick instead of showing nothing until the next.
    tick(e, "global").await;
    assert_eq!(current(e, "global").await.0.as_deref(), Some("mg-h-a"));
    e.sql("UPDATE magnet_lanes SET current_since=now()-interval '50 seconds' WHERE id='global'")
        .await;
    tick(e, "global").await;
    assert_eq!(current(e, "global").await.2.as_deref(), Some("mg-h-b"));
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test' WHERE id IN ('mg-h-a','mg-h-b')").await;
    e.sql("UPDATE magnet_lanes SET switch_at=now()-interval '1 second' WHERE id='global'")
        .await;
    tick(e, "global").await;
    assert_eq!(
        current(e, "global").await,
        (Some("mg-h-c".into()), Some("fallback".into()), None)
    );
    let cancelled: i64 = sqlx::query_scalar("SELECT count(*) FROM magnet_decisions WHERE lane='global' AND kind='cancelled' AND chosen='mg-h-b'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(cancelled, 1);

    // A staff pick whose broadcast ended is released.
    e.sql("UPDATE magnet_lanes SET forced_broadcast='mg-h-b' WHERE id='global'")
        .await;
    tick(e, "global").await;
    let forced: Option<String> =
        sqlx::query_scalar("SELECT forced_broadcast FROM magnet_lanes WHERE id='global'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(forced, None);
    let (_, admin_view) = get(e, "/api/admin/magnet", Some(admin)).await;
    let global = admin_view["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "global")
        .unwrap()
        .clone();
    assert_eq!(global["stalled"], false);
    assert!(
        admin_view["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["kind"] == "released")
    );

    // A failing lane says so to staff, and clears once a tick succeeds.
    e.sql("UPDATE magnet_lanes SET failing_since=now()-interval '2 minutes',failures=12 WHERE id='global'").await;
    let (_, admin_view) = get(e, "/api/admin/magnet", Some(admin)).await;
    assert!(
        admin_view["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["id"] == "global" && l["stalled"] == true)
    );
    tick(e, "global").await;
    let failing: (Option<chrono::DateTime<chrono::Utc>>, i32) =
        sqlx::query_as("SELECT failing_since,failures FROM magnet_lanes WHERE id='global'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(failing, (None, 0));

    // Viewers MAGNet brought don't look like a viewbot spike; other sudden arrivals still do. The
    // feature started over two minutes ago, so only the lane tag explains the MAGNet arrivals.
    e.sql("UPDATE magnet_features SET started_at=now()-interval '5 minutes' WHERE broadcast_id='mg-h-c'").await;
    for n in 0..25 {
        sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,magnet_lane) VALUES('mg-h-c',$1,now()+interval '30 seconds','global')")
            .bind(format!("b:mg-hype-{n}")).execute(&e.app.db).await.unwrap();
    }
    sver::integrity::tick(&e.app).await.unwrap();
    let provisional = || async {
        sqlx::query_as::<_, (i64, i64)>("SELECT count(*) FILTER (WHERE magnet_lane IS NOT NULL), count(*) FILTER (WHERE magnet_lane IS NULL) FROM playback_leases WHERE broadcast_id='mg-h-c' AND provisional_until IS NOT NULL")
            .fetch_one(&e.app.db).await.unwrap()
    };
    assert_eq!(provisional().await, (0, 0));
    for n in 0..25 {
        sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at) VALUES('mg-h-c',$1,now()+interval '30 seconds')")
            .bind(format!("b:mg-burst-{n}")).execute(&e.app.db).await.unwrap();
    }
    sver::integrity::tick(&e.app).await.unwrap();
    assert_eq!(provisional().await, (0, 25));
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE id LIKE 'mg-%'").await;
}

/// A genre lane runs its own moments: a follow burst from real viewers on a small stream there
/// starts a countdown with its reason, and a restricted channel is never a candidate.
async fn genre_lane(e: &Env) {
    e.sql("UPDATE magnet_features SET ended_at=now() WHERE ended_at IS NULL")
        .await;
    e.sql("UPDATE magnet_lanes SET current_broadcast=NULL,current_kind=NULL,current_reason=NULL,current_since=NULL,pending_broadcast=NULL,pending_kind=NULL,pending_reason=NULL,switch_at=NULL,last_kind=NULL,last_moment_at=NULL,forced_broadcast=NULL WHERE id='art'").await;
    for (id, name) in [
        ("mg-ga", "MgGenreA"),
        ("mg-gb", "MgGenreB"),
        ("mg-gr", "MgRestricted"),
    ] {
        person(e, id, name, true).await;
    }
    live(e, "mg-g-a", "mg-ga", "art", 30).await;
    live(e, "mg-g-b", "mg-gb", "art", 20).await;
    live(e, "mg-g-r", "mg-gr", "art", 25).await;
    e.sql("INSERT INTO profiles(user_id,display_name,restricted_until) VALUES('mg-gr','MgRestricted',now()+interval '1 day') ON CONFLICT(user_id) DO UPDATE SET restricted_until=EXCLUDED.restricted_until").await;
    let mut db = e.app.db.acquire().await.unwrap();
    let candidates: Vec<String> = sver::magnet::candidates(&mut db, "art", &e.app.config.magnet)
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.1.broadcast)
        .collect();
    drop(db);
    assert!(candidates.contains(&"mg-g-a".to_string()));
    assert!(
        !candidates.contains(&"mg-g-r".to_string()),
        "a restricted channel is never a candidate"
    );
    tick(e, "art").await;
    assert_eq!(
        current(e, "art").await.0.as_deref(),
        Some("mg-g-a"),
        "first a fair turn"
    );
    // Four verified viewers of the small stream follow it within the minute.
    e.sql("UPDATE magnet_lanes SET current_since=now()-interval '50 seconds' WHERE id='art'")
        .await;
    for follower in ["mg-c1", "mg-c2", "mg-c3", "mg-c4"] {
        watching(e, "mg-g-b", follower).await;
        sqlx::query("INSERT INTO follows(follower_id,following_id) VALUES($1,'mg-gb') ON CONFLICT DO NOTHING")
            .bind(follower).execute(&e.app.db).await.unwrap();
    }
    tick(e, "art").await;
    assert_eq!(current(e, "art").await.2.as_deref(), Some("mg-g-b"));
    let (_, page) = get(e, "/api/magnet/art", None).await;
    assert_eq!(page["next"]["stream"]["username"], "MgGenreB");
    assert_eq!(page["next"]["reason"], "New follows are pouring in");
    e.sql("DELETE FROM follows WHERE following_id='mg-gb'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE id LIKE 'mg-g-%'").await;
    e.sql("UPDATE magnet_features SET ended_at=now() WHERE ended_at IS NULL")
        .await;
}
