//! Guild admission, succession, discovery and co-stream isolation against real Postgres.
use super::{
    Env,
    chat::{call, id, person},
};
use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

async fn emblem(e: &Env, token: &str, slug: &str) -> (StatusCode, Value) {
    use axum::{body::Body, extract::ConnectInfo, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let image = image::RgbImage::from_fn(112, 112, |x, y| image::Rgb([x as u8, y as u8, 90]));
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mut body = b"--team\r\nContent-Disposition: form-data; name=\"file\"; filename=\"team.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    body.extend(png.into_inner());
    body.extend(b"\r\n--team--\r\n");
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/guilds/{slug}/media/avatar"))
        .header("content-type", "multipart/form-data; boundary=team")
        .header("cookie", format!("sver_dev={token}"))
        .header("origin", &e.app.config.origin)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    let response = sver::router(e.app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

async fn ok(e: &Env, token: &str, method: &str, path: &str, body: Value) -> Value {
    let (s, v) = call(e, method, path, Some(token), body).await;
    assert_eq!(s, StatusCode::OK, "{method} {path}: {v}");
    v
}
async fn action(e: &Env, token: &str, slug: &str, body: Value) -> Value {
    ok(
        e,
        token,
        "POST",
        &format!("/api/guilds/{slug}/actions"),
        body,
    )
    .await
}
async fn live(e: &Env, id: &str) {
    sqlx::query("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state,confirmed_live_at) VALUES($1,$1,$1,1,'LIVE','s','v','c',now(),now(),now(),'skipped',now())")
        .bind(id).execute(&e.app.db).await.unwrap();
}
async fn end(e: &Env, id: &str) {
    sqlx::query("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test' WHERE id=$1")
        .bind(id)
        .execute(&e.app.db)
        .await
        .unwrap();
}
pub async fn exercise(e: &Env) {
    let mut tokens = Vec::new();
    for n in 0..6 {
        let id = format!("team-{n}");
        tokens.push(person(e, &id, &format!("TeamPerson{n}"), true).await);
        live(e, &id).await;
    }
    let fan = person(e, "team-fan", "TeamViewer", true).await;
    let a = &tokens[0];
    let b = &tokens[1];
    let c = &tokens[2];
    let branding = |n| json!({"name":format!("Test Team {n}"),"slug":format!("test_team_{n}"),"tag":format!("TM{n}"),"about":"A cross-faction team"});
    assert_eq!(
        call(e, "POST", "/api/guilds", Some(a), branding(0)).await.0,
        StatusCode::FORBIDDEN,
        "2FA required"
    );
    for n in 0..6 {
        let user = format!("team-{n}");
        sqlx::query("UPDATE users SET mfa_enabled=true,mfa_secret=$2 WHERE id=$1")
            .bind(&user)
            .bind(
                sver::security::seal(&e.app, &format!("totp:{user}"), "JBSWY3DPEHPK3PXP").unwrap(),
            )
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id LIKE 'team-%'")
        .await;
    let mut guilds = Vec::new();
    for (n, token) in tokens.iter().take(4).enumerate() {
        guilds.push(ok(e, token, "POST", "/api/guilds", branding(n)).await);
    }
    assert_eq!(
        call(e, "POST", "/api/guilds", Some(a), branding(4)).await.0,
        StatusCode::CONFLICT,
        "one led guild"
    );
    // Invitation is consent to apply, never membership.
    action(
        e,
        a,
        "test_team_0",
        json!({"action":"invite","username":"TeamPerson4"}),
    )
    .await;
    let page = ok(e, &tokens[4], "GET", "/api/guilds/test_team_0", Value::Null).await;
    assert!(page["viewer"]["role"].is_null());
    assert_eq!(page["viewer"]["invited"], true);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/guilds/test_team_0/actions",
            Some(a),
            json!({"action":"accept","username":"TeamPerson4"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    action(
        e,
        &tokens[4],
        "test_team_0",
        json!({"action":"apply","message":"I make games."}),
    )
    .await;
    assert_eq!(
        call(
            e,
            "POST",
            "/api/guilds/test_team_0/actions",
            Some(&tokens[4]),
            json!({"action":"apply","message":"Duplicate"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    // Existing applications can be reviewed after recruitment closes.
    let mut closed = branding(0);
    closed["revision"] = json!(0);
    closed["recruiting"] = json!(false);
    ok(e, a, "PATCH", "/api/guilds/test_team_0", closed).await;
    action(
        e,
        a,
        "test_team_0",
        json!({"action":"accept","username":"TeamPerson4"}),
    )
    .await;
    action(
        e,
        a,
        "test_team_0",
        json!({"action":"officer","username":"TeamPerson4","enabled":true}),
    )
    .await;
    assert_eq!(
        call(
            e,
            "POST",
            "/api/guilds/test_team_0/actions",
            Some(&tokens[4]),
            json!({"action":"remove","username":"TeamPerson0"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // Cap includes the guild a member leads; no direct-add route can bypass it.
    for (n, token) in tokens.iter().enumerate().take(3).skip(1) {
        action(
            e,
            a,
            &format!("test_team_{n}"),
            json!({"action":"apply","message":"Let's build together."}),
        )
        .await;
        action(
            e,
            token,
            &format!("test_team_{n}"),
            json!({"action":"accept","username":"TeamPerson0"}),
        )
        .await;
    }
    assert_eq!(
        call(
            e,
            "POST",
            "/api/guilds/test_team_3/actions",
            Some(a),
            json!({"action":"apply","message":"Fourth"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    ok(
        e,
        a,
        "PUT",
        "/api/me/guilds/badge",
        json!({"guild_id":guilds[1]["id"]}),
    )
    .await;
    let message = ok(
        e,
        a,
        "POST",
        "/api/channels/TeamPerson5/chat",
        json!({"id":id(),"body":"Team badge"}),
    )
    .await;
    assert_eq!(message["message"]["author"]["guild"]["tag"], "TM1");
    action(e, &fan, "test_team_0", json!({"action":"follow"})).await;
    let following = ok(e, &fan, "GET", "/api/me/following?live=true", Value::Null).await;
    assert_eq!(following["items"].as_array().unwrap().len(), 2);
    action(
        e,
        &tokens[5],
        "test_team_1",
        json!({"action":"apply","message":"Please consider me"}),
    )
    .await;
    action(
        e,
        b,
        "test_team_1",
        json!({"action":"decline","username":"TeamPerson5","note":"Try next week"}),
    )
    .await;
    assert_eq!(
        call(
            e,
            "POST",
            "/api/guilds/test_team_1/actions",
            Some(&tokens[5]),
            json!({"action":"apply","message":"Again"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let notices = ok(e, &tokens[5], "GET", "/api/me/notifications", Value::Null).await;
    assert!(notices.to_string().contains("guild_decision"));
    let staff = super::bans::staff(e, "team-staff", "TeamStaff").await;
    assert_eq!(
        emblem(e, &fan, "test_team_1").await.0,
        StatusCode::FORBIDDEN
    );
    let (status, response) = emblem(e, b, "test_team_1").await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let badge = ok(
        e,
        a,
        "POST",
        "/api/channels/TeamPerson5/chat",
        json!({"id":id(),"body":"Emblem uploaded"}),
    )
    .await;
    assert!(
        badge["message"]["author"]["guild"]["image"]
            .as_str()
            .unwrap()
            .ends_with("/28.webp")
    );
    let guild_id = guilds[1]["id"].as_str().unwrap();
    let admin_path = format!("/api/admin/guilds/{guild_id}");
    // Emblem-only removal must not quarantine an unrelated guild banner.
    sqlx::query("UPDATE guilds SET banner_key='synthetic/banner' WHERE id=$1")
        .bind(guild_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    let mut db = e.app.db.acquire().await.unwrap();
    let located = sver::safety::take_down::locate(&mut db, "guild_emblem", guild_id, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(located.roots.len(), 1);
    assert!(
        sver::guilds::referenced(&mut db, &located.roots[0])
            .await
            .unwrap()
    );
    drop(db);
    ok(
        e,
        &staff,
        "POST",
        &admin_path,
        json!({"action":"remove_emblem","note":"Synthetic review"}),
    )
    .await;
    assert!(
        ok(e, a, "GET", "/api/guilds/test_team_1", Value::Null).await["guild"]["avatar"].is_null()
    );
    let mut tx = e.app.db.begin().await.unwrap();
    sver::guilds::restore(&mut tx, guild_id, "true", true)
        .await
        .unwrap();
    sver::safety::take_down::remove_media(&mut tx, &located.roots[0])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        ok(e, a, "GET", "/api/guilds/test_team_1", Value::Null).await["guild"]["avatar"].is_null()
    );
    action(
        e,
        b,
        "test_team_1",
        json!({"action":"verification","message":"Synthetic organization evidence"}),
    )
    .await;
    assert_eq!(
        call(e, "GET", "/api/admin/guilds", Some(&fan), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ok(e, &staff, "GET", "/api/admin/guilds", Value::Null).await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    ok(
        e,
        &staff,
        "POST",
        &format!("/api/admin/guilds/{}", guilds[1]["id"].as_str().unwrap()),
        json!({"action":"verify","note":"Evidence checked"}),
    )
    .await;
    assert_eq!(
        ok(e, a, "GET", "/api/guilds/test_team_1", Value::Null).await["guild"]["verified"],
        true
    );
    // Schedules use the same DST-aware expansion as channel pages.
    e.sql("INSERT INTO schedule_events(id,user_id,title,start_at,end_at) VALUES('team-event','team-0','Build together',now()+interval '1 day',now()+interval '1 day 1 hour')").await;
    let page = ok(e, &fan, "GET", "/api/guilds/test_team_1", Value::Null).await;
    assert!(page["schedule"].to_string().contains("Build together"));
    // Live state, blocks and schedules are read once for the whole guild.
    let members = |page: &Value| -> Vec<(String, bool)> {
        page["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["user"]["username"].as_str().unwrap().to_owned(),
                    m["live"].as_bool().unwrap(),
                )
            })
            .collect()
    };
    let listed = members(&page);
    assert!(
        listed.contains(&("TeamPerson0".into(), true)) && listed.iter().all(|m| m.1),
        "{page}"
    );
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('team-0','team-fan')")
        .await;
    let page = ok(e, &fan, "GET", "/api/guilds/test_team_1", Value::Null).await;
    assert!(
        members(&page).iter().all(|m| m.0 != "TeamPerson0"),
        "a block hides the member"
    );
    assert!(!page["schedule"].to_string().contains("Build together"));
    e.sql("DELETE FROM user_blocks WHERE blocker_id='team-0'")
        .await;
    // Studio finds live guildmates in one call, never the viewer and never someone blocked.
    let mates = |token: String| async move {
        ok(e, &token, "GET", "/api/me/guilds/live", Value::Null).await["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["username"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let found = mates(b.clone()).await;
    assert!(
        found.contains(&"TeamPerson0".to_string()) && !found.contains(&"TeamPerson1".to_string()),
        "{found:?}"
    );
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('team-1','team-0')")
        .await;
    assert!(!mates(b.clone()).await.contains(&"TeamPerson0".to_string()));
    e.sql("DELETE FROM user_blocks WHERE blocker_id='team-1'")
        .await;
    ok(e,&fan,"POST","/api/reports",json!({"target_type":"guild","target_id":guilds[1]["id"],"reason":"impersonation","note":"Synthetic report"})).await;
    assert!(
        ok(
            e,
            &staff,
            "GET",
            "/api/admin/reports?kind=guild",
            Value::Null
        )
        .await["groups"]
            .to_string()
            .contains("Test Team 1")
    );
    // Invalid notification updates are atomic; muting a guild hides its notices.
    assert_eq!(call(e,"PUT","/api/me/notifications/settings",Some(&fan),json!({"site":false,"push":false,"email":false,"community":[{"kind":"unknown","site":false,"push":false}]})).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(
        ok(
            e,
            &fan,
            "GET",
            "/api/me/notifications/settings",
            Value::Null
        )
        .await["site"],
        true
    );
    action(e, &tokens[5], "test_team_1", json!({"action":"mute"})).await;
    assert!(
        !ok(e, &tokens[5], "GET", "/api/me/notifications", Value::Null)
            .await
            .to_string()
            .contains("guild_decision")
    );
    action(e, a, "test_team_0", json!({"action":"leave"})).await;
    let page = ok(e, &tokens[4], "GET", "/api/guilds/test_team_0", Value::Null).await;
    assert_eq!(
        page["viewer"]["role"], "leader",
        "oldest eligible officer succeeds"
    );
    action(e, &tokens[4], "test_team_0", json!({"action":"leave"})).await;
    assert_eq!(
        ok(e, a, "GET", "/api/guilds/test_team_0", Value::Null).await["guild"]["status"],
        "ARCHIVED"
    );

    // Four streams, accepted invitations, private-to-squad shared messages.
    let squad = ok(e, a, "POST", "/api/me/squads", json!({"mode":"MERGED"})).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/api/squads/{squad}");
    for n in 1..=3 {
        ok(
            e,
            a,
            "POST",
            &format!("{path}/invites"),
            json!({"username":format!("TeamPerson{n}")}),
        )
        .await;
    }
    assert_eq!(
        call(
            e,
            "POST",
            &format!("{path}/invites"),
            Some(a),
            json!({"username":"TeamPerson4"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("{path}/answer"),
            Some(&fan),
            json!({"accept":true})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    for token in tokens.iter().take(4).skip(1) {
        ok(
            e,
            token,
            "POST",
            &format!("{path}/answer"),
            json!({"accept":true}),
        )
        .await;
    }
    assert_eq!(
        ok(e, a, "GET", &path, Value::Null).await["members"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    // Pages and shared chat never wait on the membership lock.
    let mut held = e.app.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('squad-membership',0))")
        .execute(&mut *held)
        .await
        .unwrap();
    let wait = std::time::Duration::from_secs(3);
    assert!(
        tokio::time::timeout(wait, ok(e, a, "GET", &path, Value::Null))
            .await
            .is_ok()
    );
    assert!(
        tokio::time::timeout(
            wait,
            call(e, "GET", &format!("{path}/chat"), Some(&fan), Value::Null)
        )
        .await
        .is_ok()
    );
    held.rollback().await.unwrap();
    // While merged, a member's own chat takes no posts; the refusal names the shared chat.
    let (status, refused) = call(
        e,
        "POST",
        "/api/channels/TeamPerson2/chat",
        Some(&fan),
        json!({"id":id(),"body":"Solo"}),
    )
    .await;
    assert_eq!(
        (status, refused["squad"].as_str()),
        (StatusCode::CONFLICT, Some(squad.as_str()))
    );
    // A retry of a line stored before the merge is acknowledged with it, not refused.
    let stored = id();
    sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body) VALUES($1,'team-2','team-fan','Before the merge')")
        .bind(&stored).execute(&e.app.db).await.unwrap();
    let retried = ok(
        e,
        &fan,
        "POST",
        "/api/channels/TeamPerson2/chat",
        json!({"id":stored,"body":"Before the merge"}),
    )
    .await;
    assert_eq!(retried["message"]["id"], stored.as_str());
    // Any member's watch page finds the shared view; a channel outside it does not.
    let found = ok(
        e,
        &fan,
        "GET",
        "/api/channels/TeamPerson2/squad",
        Value::Null,
    )
    .await;
    assert_eq!(found["squad"]["id"], squad.as_str());
    assert_eq!(found["squad"]["members"].as_array().unwrap().len(), 4);
    // Reads never wait on membership changes: with the membership lock held elsewhere, the
    // co-stream page, a member's watch page and the shared chat still answer at once.
    let mut holder = e.app.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('squad-membership',0))")
        .execute(&mut *holder)
        .await
        .unwrap();
    for read in [
        path.clone(),
        format!("{path}/chat"),
        "/api/channels/TeamPerson2/squad".to_string(),
    ] {
        let answered = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            call(e, "GET", &read, Some(&fan), Value::Null),
        )
        .await;
        assert!(
            answered.is_ok_and(|r| r.0 == StatusCode::OK),
            "{read} waited on the membership lock"
        );
    }
    holder.rollback().await.unwrap();
    assert!(
        ok(
            e,
            &fan,
            "GET",
            "/api/channels/TeamPerson4/squad",
            Value::Null
        )
        .await["squad"]
            .is_null()
    );
    let chat = format!("{path}/chat");
    e.sql("INSERT INTO magnet_lanes(id,current_broadcast,current_since) VALUES('global','team-0',now()-interval '1 hour')").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let open = |token: Option<&str>| {
        let mut request = format!("ws://{address}/api/chat/ws?squad={squad}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", e.app.config.origin.parse().unwrap());
        if let Some(token) = token {
            request
                .headers_mut()
                .insert("cookie", format!("sver_dev={token}").parse().unwrap());
        }
        async move { tokio_tungstenite::connect_async(request).await.unwrap().0 }
    };
    // Every member's emotes render in shared chat, marked with their channel; the host's wins a
    // code both have.
    e.sql("INSERT INTO channel_emotes(id,channel_id,code,image_key) VALUES('sq-host','team-0','SquadClash','emotes/sq-host'),('sq-guest','team-2','GuestHype','emotes/sq-guest'),('sq-clash','team-2','SquadClash','emotes/sq-clash')").await;
    for member in ["team-0", "team-2"] {
        e.app
            .chat
            .publish(member, None, 0, json!({"type":"emotes"}));
    }
    let mut guest = open(None).await;
    let snapshot = super::chat::next_json(&mut guest).await;
    let emotes = snapshot["emotes"].as_array().unwrap();
    let owner = |code: &str| {
        let found: Vec<_> = emotes.iter().filter(|e| e["code"] == code).collect();
        assert_eq!(found.len(), 1, "{code} once");
        found[0]["channel"].as_str().unwrap().to_owned()
    };
    assert_eq!(owner("GuestHype"), "TeamPerson2");
    assert_eq!(owner("SquadClash"), "TeamPerson0");
    // A member's live panels reach the shared chat, marked with their channel; their own chat's
    // lines don't.
    e.app.chat.publish(
        "team-1",
        None,
        0,
        json!({"type":"message","message":{"id":id(),"body":"own chat"}}),
    );
    e.app
        .chat
        .publish("team-1", None, 0, json!({"type":"poll","poll":null}));
    let poll = super::chat::next_json(&mut guest).await;
    assert_eq!(
        (poll["type"].as_str(), poll["source"].as_str()),
        (Some("poll"), Some("TeamPerson1"))
    );
    // A channel ban closes the banned viewer's shared chat at once, and only theirs.
    let mut banned = open(Some(&tokens[4])).await;
    super::chat::next_json(&mut banned).await;
    let mut bystander = open(Some(&fan)).await;
    super::chat::next_json(&mut bystander).await;
    ok(
        e,
        b,
        "POST",
        "/api/channels/TeamPerson1/chat/restrictions",
        json!({"username":"TeamPerson4","kind":"ban","reason":"Synthetic ban"}),
    )
    .await;
    let closed = tokio::time::timeout(std::time::Duration::from_secs(4), async {
        while let Some(Ok(frame)) = banned.next().await {
            if frame.is_close() {
                break;
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the banned viewer's socket closed");
    e.app
        .chat
        .publish("team-3", None, 0, json!({"type":"rally"}));
    assert_eq!(
        super::chat::next_of(&mut bystander, "rally").await["source"],
        "TeamPerson3"
    );
    e.sql("DELETE FROM channel_restrictions WHERE user_id='team-4'")
        .await;
    // An under-18 viewer's open shared chat closes for good (4003) once a member is labeled mature.
    let kid = person(e, "team-kid", "TeamKid", true).await;
    e.sql("UPDATE users SET date_of_birth=current_date-interval '15 years' WHERE id='team-kid'")
        .await;
    let mut minor = open(Some(&kid)).await;
    assert_eq!(super::chat::next_json(&mut minor).await["type"], "snapshot");
    e.sql("INSERT INTO stream_settings(owner_id,title,mature) VALUES('team-2','Squad',true) ON CONFLICT(owner_id) DO UPDATE SET mature=true").await;
    let code = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        while let Some(Ok(frame)) = minor.next().await {
            if let tokio_tungstenite::tungstenite::Message::Close(close) = frame {
                return close.map(|c| u16::from(c.code));
            }
        }
        None
    })
    .await
    .unwrap();
    assert_eq!(code, Some(4003));
    e.sql("UPDATE stream_settings SET mature=false WHERE owner_id='team-2'")
        .await;
    e.sql("DELETE FROM channel_emotes WHERE id LIKE 'sq-%'")
        .await;
    drop((guest, bystander));
    // The socket's snapshot names the member channel the viewer's lines credit (the dock's
    // Rewards), and emote combos count the whole shared room, whichever channel each line
    // credits, playing on the stream of the channel whose emote it is.
    e.sql("INSERT INTO engagement(channel_id,user_id,balance,earned) VALUES('team-2','team-fan',1000,1000)").await;
    let mut credited = open(Some(&fan)).await;
    assert_eq!(
        super::chat::next_of(&mut credited, "snapshot").await["credit"],
        "TeamPerson2"
    );
    e.sql("INSERT INTO channel_emotes(id,channel_id,code,image_key) VALUES('sq-combo','team-2','ComboHype','emotes/sq-combo')").await;
    for token in tokens.iter().take(4).chain([&fan]) {
        e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat-%'")
            .await;
        ok(
            e,
            token,
            "POST",
            &chat,
            json!({"id":id(),"body":"ComboHype"}),
        )
        .await;
    }
    let shower = super::chat::next_of(&mut credited, "board_effect").await;
    assert_eq!(
        (&shower["label"], &shower["source"]),
        (&json!("ComboHype"), &json!("TeamPerson2"))
    );
    drop(credited);
    e.sql("DELETE FROM channel_emotes WHERE id='sq-combo'")
        .await;
    e.sql("DELETE FROM chat_messages WHERE body='ComboHype'")
        .await;
    e.sql("DELETE FROM engagement WHERE user_id='team-fan'")
        .await;
    // In shared chat a sub badge names its channel: the host's when they sub there, otherwise
    // the member channel they've subbed to longest.
    e.sql("INSERT INTO channel_subs(channel_id,user_id,tier,paid_through,months) VALUES('team-2','team-fan',1,now()+interval '1 month',3),('team-3','team-fan',2,now()+interval '1 month',7)").await;
    let mut request = format!("ws://{address}/api/chat/ws?squad={squad}")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", e.app.config.origin.parse().unwrap());
    request
        .headers_mut()
        .insert("cookie", format!("sver_dev={fan}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(
        super::chat::next_json(&mut socket).await["type"],
        "snapshot"
    );
    let msgid = id();
    ok(
        e,
        &fan,
        "POST",
        &chat,
        json!({"id":msgid,"body":"Shared room"}),
    )
    .await;
    ok(e, &fan, "POST", &chat, json!({"id":msgid,"body":"Retry"})).await;
    let shared = super::chat::next_json(&mut socket).await;
    assert_eq!(shared["message"]["body"], "Shared room");
    assert_eq!(
        (
            &shared["message"]["sub"]["channel"],
            &shared["message"]["sub"]["months"]
        ),
        (&json!("TeamPerson3"), &json!(7))
    );
    e.sql("INSERT INTO channel_subs(channel_id,user_id,tier,paid_through,months) VALUES('team-0','team-fan',1,now()+interval '1 month',1)").await;
    assert_eq!(
        ok(e, &fan, "GET", &chat, Value::Null).await["messages"][0]["sub"]["channel"],
        "TeamPerson0"
    );
    e.sql("DELETE FROM channel_subs WHERE user_id='team-fan'")
        .await;
    // A featured merged co-stream: MAGNet chat merges with its shared room (docs/MAGNET.md).
    let room = ok(e, &fan, "GET", "/api/magnet/global/chat", Value::Null).await;
    assert_eq!(room["co_stream"], true);
    assert!(
        room["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "Shared room"),
        "MAGNet chat shows the featured squad's shared room"
    );
    assert_eq!(
        ok(e, &fan, "GET", &chat, Value::Null).await["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ok(
            e,
            &fan,
            "GET",
            "/api/channels/TeamPerson0/chat",
            Value::Null
        )
        .await["messages"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    // A member's moderator moderates only the shared room.
    e.sql("INSERT INTO channel_moderators(channel_id,user_id) VALUES('team-1','team-5')")
        .await;
    assert_eq!(
        call(
            e,
            "DELETE",
            &format!("/api/channels/TeamPerson0/chat/messages/{msgid}"),
            Some(&tokens[5]),
            json!({"reason":"test"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    ok(
        e,
        &tokens[5],
        "DELETE",
        &format!("{chat}/messages/{msgid}"),
        json!({"reason":"test"}),
    )
    .await;
    assert_eq!(super::chat::next_json(&mut socket).await["type"], "delete");
    ok(
        e,
        &tokens[5],
        "POST",
        &format!("{chat}/restrictions"),
        json!({"username":"TeamViewer","kind":"timeout","seconds":60,"reason":"Synthetic timeout"}),
    )
    .await;
    assert_eq!(
        call(e, "GET", &chat, Some(&fan), Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    // Existing connections are revoked too, not just the next HTTP request.
    let closed = tokio::time::timeout(std::time::Duration::from_secs(8), socket.next())
        .await
        .unwrap();
    assert!(
        closed.is_none() || closed.is_some_and(|r| r.is_err() || r.is_ok_and(|m| m.is_close()))
    );
    server.abort();
    ok(
        e,
        &tokens[5],
        "DELETE",
        &format!("{chat}/restrictions/TeamViewer/timeout"),
        json!({"reason":"Timeout lifted"}),
    )
    .await;
    ok(e, &fan, "GET", &chat, Value::Null).await;
    // Shared-chat deletes mirror channel ones: staff delete a member's moderator's line, and a
    // member streamer can't delete another member streamer's. Each is logged in every member.
    let line = id();
    ok(
        e,
        &tokens[5],
        "POST",
        &chat,
        json!({"id":line,"body":"Moderator line"}),
    )
    .await;
    ok(
        e,
        &staff,
        "DELETE",
        &format!("{chat}/messages/{line}"),
        json!({}),
    )
    .await;
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_moderation_log WHERE action='delete_message' AND message_id=$1 AND detail->>'squad'=$2")
        .bind(&line).bind(&squad).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(logged, 4);
    let host_line = id();
    ok(
        e,
        a,
        "POST",
        &chat,
        json!({"id":host_line,"body":"Host line"}),
    )
    .await;
    assert_eq!(
        call(
            e,
            "DELETE",
            &format!("{chat}/messages/{host_line}"),
            Some(b),
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // A shared-chat timeout without a reason acts now, clears the person's recent lines, and
    // waits for review in every member's log; undoing it in Studio lifts it.
    let gone = id();
    ok(
        e,
        &fan,
        "POST",
        &chat,
        json!({"id":gone,"body":"Cleared by the timeout"}),
    )
    .await;
    let timed = ok(
        e,
        &tokens[5],
        "POST",
        &format!("{chat}/restrictions"),
        json!({"username":"TeamViewer","kind":"timeout","seconds":60}),
    )
    .await;
    assert_eq!(timed["review"], true);
    let cleared: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM chat_messages WHERE id=$1")
            .bind(&gone)
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert!(cleared, "the timeout cleared their shared-chat line");
    let waiting: Vec<(String, String)> = sqlx::query_as("SELECT id,channel_id FROM channel_moderation_log WHERE detail->>'squad'=$1 AND action='timeout' AND target_id='team-fan' AND needs_review AND detail ? 'chat' ORDER BY channel_id")
        .bind(&squad).fetch_all(&e.app.db).await.unwrap();
    assert_eq!(
        waiting.iter().map(|w| w.1.as_str()).collect::<Vec<_>>(),
        ["team-0", "team-1", "team-2", "team-3"]
    );
    assert_eq!(
        call(e, "GET", &chat, Some(&fan), Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    ok(
        e,
        b,
        "POST",
        &format!("/api/channels/TeamPerson1/chat/reviews/{}", waiting[1].0),
        json!({"keep":false}),
    )
    .await;
    let left: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM squad_restrictions WHERE squad_id=$1 AND user_id='team-fan')+(SELECT count(*) FROM channel_moderation_log WHERE detail->>'squad'=$1 AND needs_review)")
        .bind(&squad).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(left, 0, "undone for every member");
    ok(e, &fan, "GET", &chat, Value::Null).await;
    // A shared-chat ban is also a channel ban in the channels its moderator moderates (team-5:
    // team-1's only), never a partner's; lifting the shared ban lifts it, but never a channel ban
    // the channel gave on its own.
    let fan_bans = || async {
        sqlx::query_scalar::<_, String>("SELECT channel_id FROM channel_restrictions WHERE user_id='team-fan' AND kind='ban' ORDER BY channel_id")
            .fetch_all(&e.app.db)
            .await
            .unwrap()
    };
    let shared_ban = || async {
        ok(
            e,
            &tokens[5],
            "POST",
            &format!("{chat}/restrictions"),
            json!({"username":"TeamViewer","kind":"ban","reason":"Synthetic ban"}),
        )
        .await;
    };
    let shared_lift = || async {
        ok(
            e,
            &tokens[5],
            "DELETE",
            &format!("{chat}/restrictions/TeamViewer/ban"),
            json!({}),
        )
        .await;
    };
    let bans = ok(
        e,
        &tokens[5],
        "GET",
        &format!("{chat}/moderation"),
        Value::Null,
    )
    .await;
    assert_eq!(bans["bans"], json!(["TeamPerson1"]));
    shared_ban().await;
    assert_eq!(fan_bans().await, ["team-1"]);
    let marked: Vec<String> = sqlx::query_scalar("SELECT channel_id FROM channel_moderation_log WHERE detail->>'squad'=$1 AND action='ban' AND target_id='team-fan' AND detail @> '{\"channel_ban\":true}'")
        .bind(&squad).fetch_all(&e.app.db).await.unwrap();
    assert_eq!(marked, ["team-1"]);
    shared_lift().await;
    assert!(
        fan_bans().await.is_empty(),
        "lifting the shared ban lifts it"
    );
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('team-1','team-fan','ban')").await;
    shared_ban().await;
    shared_lift().await;
    assert_eq!(fan_bans().await, ["team-1"], "a ban of its own stays");
    e.sql("DELETE FROM channel_restrictions WHERE user_id='team-fan'")
        .await;
    // Undoing one shared ban on review lifts the channel bans it added, never those of a later
    // shared ban that another channel's owner gave.
    let restrictions = format!("{chat}/restrictions");
    ok(
        e,
        &tokens[5],
        "POST",
        &restrictions,
        json!({"username":"TeamViewer","kind":"ban"}),
    )
    .await;
    ok(
        e,
        c,
        "POST",
        &restrictions,
        json!({"username":"TeamViewer","kind":"ban","reason":"Synthetic ban"}),
    )
    .await;
    assert_eq!(fan_bans().await, ["team-1", "team-2"]);
    let review: String = sqlx::query_scalar("SELECT id FROM channel_moderation_log WHERE channel_id='team-1' AND detail->>'squad'=$1 AND action='ban' AND needs_review")
        .bind(&squad).fetch_one(&e.app.db).await.unwrap();
    ok(
        e,
        &tokens[5],
        "POST",
        &format!("/api/channels/TeamPerson1/chat/reviews/{review}"),
        json!({"keep":false}),
    )
    .await;
    assert_eq!(
        fan_bans().await,
        ["team-2"],
        "team-2's own shared ban stays"
    );
    // Lifting the shared ban lifts its channel bans in channels that have left since, too
    // (team-4 stands in for one), and logs the lift there.
    shared_ban().await;
    sqlx::query("INSERT INTO channel_moderation_log(id,channel_id,actor_id,actor_role,action,target_id,detail,reason,created_at) SELECT 'gone-'||id,'team-4',actor_id,actor_role,action,target_id,detail,reason,created_at FROM channel_moderation_log WHERE channel_id='team-1' AND detail->>'squad'=$1 AND action='ban' AND detail @> '{\"channel_ban\":true}' ORDER BY created_at DESC LIMIT 1")
        .bind(&squad).execute(&e.app.db).await.unwrap();
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind,created_at) SELECT 'team-4',user_id,kind,created_at FROM channel_restrictions WHERE channel_id='team-1' AND user_id='team-fan'").await;
    assert_eq!(fan_bans().await, ["team-1", "team-2", "team-4"]);
    shared_lift().await;
    assert!(
        fan_bans().await.is_empty(),
        "every shared ban's channel bans"
    );
    let gone: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_moderation_log WHERE channel_id='team-4' AND action='lift_ban' AND detail @> '{\"channel_ban\":true}'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(gone, 1);
    e.sql("DELETE FROM channel_moderation_log WHERE channel_id='team-4'")
        .await;
    ok(e, &fan, "GET", &chat, Value::Null).await;
    // Shared chat credits one member channel per line: where the chatter has the most lifetime
    // Engagement Valor (here team-2, not the host), once, and its loyalty badge is that channel's.
    let room = chat.as_str();
    let said = |token: String, body: &'static str| async move {
        e.sql("DELETE FROM rate_limits WHERE key LIKE 'chat-%'")
            .await;
        ok(e, &token, "POST", room, json!({"id":id(),"body":body})).await["message"].clone()
    };
    e.sql("DELETE FROM engagement WHERE user_id='team-fan'")
        .await;
    e.sql("INSERT INTO engagement(channel_id,user_id,balance,earned) VALUES('team-0','team-fan',300,300),('team-2','team-fan',1000,1000)").await;
    let loyal = said(fan.clone(), "Loyal line").await;
    let earned: Vec<(String, i64)> = sqlx::query_as(
        "SELECT channel_id,earned FROM engagement WHERE user_id='team-fan' ORDER BY channel_id",
    )
    .fetch_all(&e.app.db)
    .await
    .unwrap();
    assert_eq!(
        earned,
        [("team-0".to_owned(), 300), ("team-2".to_owned(), 1005)]
    );
    assert_eq!(
        loyal["loyalty"], 2,
        "team-2's Devoted, not the host's Regular"
    );
    assert_eq!(
        ok(e, &fan, "GET", &chat, Value::Null).await["credit"],
        "TeamPerson2",
        "the dock offers team-2's rewards"
    );
    // The host's !command answers first, in the shared room; a command only a member has runs
    // when the host lacks it. Neither reaches a member's own chat.
    e.sql("INSERT INTO chat_commands(channel_id,name,reply,cooldown_seconds) VALUES('team-0','sqcmd','Host reply',0),('team-1','sqcmd','Partner reply',0),('team-2','sqonly','Member reply',0)").await;
    said(fan.clone(), "!sqcmd").await;
    said(fan.clone(), "!sqonly").await;
    let bot: Vec<(String, String)> = sqlx::query_as("SELECT channel_id,body FROM outside_chat_messages WHERE squad_id=$1 AND platform='bot' ORDER BY seq")
        .bind(&squad).fetch_all(&e.app.db).await.unwrap();
    assert_eq!(
        bot,
        [
            ("team-0".to_owned(), "Host reply".to_owned()),
            ("team-2".to_owned(), "Member reply".to_owned())
        ]
    );
    let shown = ok(e, &fan, "GET", &chat, Value::Null).await;
    assert_eq!(
        shown["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["outside"]["platform"] == "bot")
            .count(),
        2
    );
    for member in ["TeamPerson0", "TeamPerson2"] {
        let own = ok(
            e,
            &fan,
            "GET",
            &format!("/api/channels/{member}/chat"),
            Value::Null,
        )
        .await;
        assert!(
            own["messages"]
                .as_array()
                .unwrap()
                .iter()
                .all(|m| m["outside"]["platform"] != "bot"),
            "{member}'s own chat"
        );
    }
    // A counter belongs to the first member that has it; only its owner and moderators change it.
    e.sql(
        "INSERT INTO counters(channel_id,id,kind,label) VALUES('team-1','falls','deaths','Falls')",
    )
    .await;
    said(fan.clone(), "!falls").await;
    said(tokens[5].clone(), "!falls").await;
    let falls: i32 =
        sqlx::query_scalar("SELECT value FROM counters WHERE channel_id='team-1' AND id='falls'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(falls, 1, "team-5 moderates team-1; the viewer doesn't");
    // !rally rallies once, on the credited channel's broadcast, though the fan watches every member.
    e.sql("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at) VALUES('team-fan','glint',now(),now())").await;
    e.sql("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) SELECT id,'u:team-fan',now()+interval '1 minute','counted' FROM broadcasts WHERE id IN ('team-0','team-1','team-2','team-3')").await;
    said(fan.clone(), "!rally").await;
    let rallies: Vec<String> =
        sqlx::query_scalar("SELECT broadcast_id FROM rallies WHERE user_id='team-fan'")
            .fetch_all(&e.app.db)
            .await
            .unwrap();
    assert_eq!(rallies, ["team-2"]);
    let surge: Vec<String> =
        sqlx::query_scalar("SELECT broadcast_id FROM surge_participation WHERE user_id='team-fan'")
            .fetch_all(&e.app.db)
            .await
            .unwrap();
    assert_eq!(surge, ["team-2"]);
    e.sql("DELETE FROM rallies WHERE user_id='team-fan'").await;
    e.sql("DELETE FROM surge_participation WHERE user_id='team-fan'")
        .await;
    e.sql("DELETE FROM playback_leases WHERE viewer_key='u:team-fan'")
        .await;
    e.sql("DELETE FROM faction_members WHERE user_id='team-fan'")
        .await;
    e.sql("DELETE FROM engagement WHERE user_id='team-fan'")
        .await;
    e.sql("DELETE FROM chat_commands WHERE name IN ('sqcmd','sqonly')")
        .await;
    e.sql("DELETE FROM counters WHERE id='falls'").await;
    e.sql("INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('team-2','team-fan','ban')").await;
    assert_eq!(
        call(
            e,
            "POST",
            &chat,
            Some(&fan),
            json!({"id":id(),"body":"Blocked"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(e, "GET", &chat, Some(&fan), Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    // A channel that has left can no longer undo the shared chat's restrictions on review, though
    // undoing there still lifts the channel ban the shared ban gave it (below, after the end).
    ok(
        e,
        &tokens[5],
        "POST",
        &format!("{chat}/restrictions"),
        json!({"username":"TeamViewer","kind":"ban"}),
    )
    .await;
    ok(e, b, "POST", &format!("{path}/leave"), Value::Null).await;
    let review: String = sqlx::query_scalar("SELECT id FROM channel_moderation_log WHERE channel_id='team-1' AND detail->>'squad'=$1 AND action='ban' AND needs_review")
        .bind(&squad).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(
        ok(e, a, "GET", &path, Value::Null).await["members"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    end(e, "team-0").await;
    assert_eq!(ok(e, c, "GET", &path, Value::Null).await["ended"], true);
    // The shared ban's channel ban in team-1 outlasts the co-stream: no chat, no signed-in playback.
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/TeamPerson1/chat",
            Some(&fan),
            json!({"id":id(),"body":"Still banned"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let watch = ok(
        e,
        &fan,
        "GET",
        "/api/channels/TeamPerson1/live",
        Value::Null,
    )
    .await;
    assert_eq!(
        (&watch["banned"], &watch["playback"]),
        (&json!(true), &Value::Null)
    );
    // Undoing it on review there lifts that channel ban, though not the shared restriction.
    let undone = ok(
        e,
        b,
        "POST",
        &format!("/api/channels/TeamPerson1/chat/reviews/{review}"),
        json!({"keep":false}),
    )
    .await;
    assert_eq!(undone["lifted"], true);
    let (channel_ban, shared): (bool, bool) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id='team-1' AND user_id='team-fan'),EXISTS(SELECT 1 FROM squad_restrictions WHERE squad_id=$1 AND user_id='team-fan')")
        .bind(&squad).fetch_one(&e.app.db).await.unwrap();
    assert_eq!((channel_ban, shared), (false, true));
    let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_moderation_log WHERE detail->>'squad'=$1 AND action='ban' AND needs_review")
        .bind(&squad).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(waiting, 3, "the members still in it review their own");
    // Studio's view repairs only the viewer's own co-stream; the worker repairs the rest.
    e.sql("INSERT INTO squads(id,host_id,mode) VALUES('sq-stale','team-5','SEPARATE')")
        .await;
    e.sql("INSERT INTO squad_members(squad_id,user_id,broadcast_id) VALUES('sq-stale','team-5','team-0')").await;
    let ended = |id: &str| {
        let id = id.to_string();
        async move {
            sqlx::query_scalar::<_, bool>("SELECT ended_at IS NOT NULL FROM squads WHERE id=$1")
                .bind(id)
                .fetch_one(&e.app.db)
                .await
                .unwrap()
        }
    };
    assert!(!ended(&squad).await, "reads don't repair");
    ok(e, c, "GET", "/api/me/squads", Value::Null).await;
    assert!(ended(&squad).await, "Studio repairs the viewer's own");
    assert!(!ended("sq-stale").await, "and nobody else's");
    sver::squads::tick(&e.app).await.unwrap();
    assert!(ended("sq-stale").await);
    assert_eq!(
        call(e, "GET", &chat, None, Value::Null).await.0,
        StatusCode::CONFLICT
    );
    // Separate mode has no shared-chat endpoint, and channel bans prohibit invitations.
    let id = ok(e, b, "POST", "/api/me/squads", json!({"mode":"SEPARATE"})).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/api/squads/{id}");
    assert_eq!(
        call(e, "GET", &format!("{path}/chat"), None, Value::Null)
            .await
            .0,
        StatusCode::CONFLICT
    );
    e.sql(
        "INSERT INTO channel_restrictions(channel_id,user_id,kind) VALUES('team-2','team-1','ban')",
    )
    .await;
    assert_eq!(
        call(
            e,
            "POST",
            &format!("{path}/invites"),
            Some(b),
            json!({"username":"TeamPerson2"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // The Plays channel joins at once, from its configured host only (docs/PLAYS.md).
    e.sql("INSERT INTO plays_runtime(channel_id,game,bridge_hash,costream_host_id) VALUES('team-3','Synthetic game',repeat('0',64),'team-1')").await;
    let other = ok(
        e,
        &tokens[4],
        "POST",
        "/api/me/squads",
        json!({"mode":"SEPARATE"}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let waiting = ok(
        e,
        &tokens[4],
        "POST",
        &format!("/api/squads/{other}/invites"),
        json!({"username":"TeamPerson3"}),
    )
    .await;
    assert_eq!(
        waiting["accepted"],
        Value::Null,
        "anyone else's invitation waits"
    );
    ok(
        e,
        &tokens[4],
        "POST",
        &format!("/api/squads/{other}/leave"),
        Value::Null,
    )
    .await;
    let joined = ok(
        e,
        b,
        "POST",
        &format!("{path}/invites"),
        json!({"username":"TeamPerson3"}),
    )
    .await;
    assert_eq!(joined["accepted"], true);
    assert_eq!(
        ok(e, b, "GET", &path, Value::Null).await["members"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    ok(e, b, "POST", &format!("{path}/leave"), Value::Null).await;
    for n in 1..6 {
        end(e, &format!("team-{n}")).await;
    }
    // Staff identity changes and disbanding affect public visibility immediately.
    let admin_path = format!("/api/admin/guilds/{}", guilds[3]["id"].as_str().unwrap());
    ok(e, &staff, "POST", &admin_path, json!({"action":"rename","note":"Synthetic correction","branding":{"name":"Renamed Team","slug":"renamed_team","tag":"RNT"}})).await;
    ok(e, &fan, "GET", "/api/guilds/renamed_team", Value::Null).await;
    ok(
        e,
        &staff,
        "POST",
        &admin_path,
        json!({"action":"disband","note":"Synthetic disband"}),
    )
    .await;
    assert_eq!(
        call(
            e,
            "GET",
            "/api/guilds/renamed_team",
            Some(&fan),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    action(
        e,
        c,
        "test_team_2",
        json!({"action":"officer","username":"TeamPerson0","enabled":true}),
    )
    .await;
    e.sql("DELETE FROM users WHERE id='team-2'").await;
    sver::guilds::tick(&e.app).await.unwrap();
    assert_eq!(
        ok(e, a, "GET", "/api/guilds/test_team_2", Value::Null).await["viewer"]["role"],
        "leader"
    );
    e.sql("DELETE FROM users WHERE id='team-1'").await;
    sver::guilds::tick(&e.app).await.unwrap();
    assert_eq!(
        call(e, "GET", "/api/guilds/test_team_1", Some(&fan), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let costream: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM events WHERE topic LIKE 'channel:%:costream' AND jsonb_array_length(data->'members')>1)",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(costream, "co-stream rosters are live events");
}
