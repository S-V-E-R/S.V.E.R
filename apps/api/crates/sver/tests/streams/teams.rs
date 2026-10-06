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
    assert_eq!(
        super::chat::next_json(&mut socket).await["message"]["body"],
        "Shared room"
    );
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
    ok(e, b, "POST", &format!("{path}/leave"), Value::Null).await;
    assert_eq!(
        ok(e, a, "GET", &path, Value::Null).await["members"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    end(e, "team-0").await;
    assert_eq!(ok(e, c, "GET", &path, Value::Null).await["ended"], true);
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
}
