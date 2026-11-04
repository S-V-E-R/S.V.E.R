//! Viewer analytics (docs/ANALYTICS.md): visits with their source from heartbeats, card
//! impressions, the streamer's report (sources, click-through, visit lengths, Live Now lift,
//! referring sites) and that it is the owner's only.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn guest(e: &Env, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    e.request(method, path, body, false, true, false).await
}
async fn beat(e: &Env, browser: &str, extra: Value) -> StatusCode {
    let mut body =
        json!({"broadcast_id":"va-live","browser_id":browser,"visible":true,"turnstile":"pass"});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    guest(e, "POST", "/api/channels/VaOwner/live/beat", body)
        .await
        .0
}
async fn visit(e: &Env, browser_digest_like: &str) -> (String, Option<String>, i64) {
    sqlx::query_as("SELECT source,referrer_host,(SELECT count(*) FROM watch_visits w WHERE w.viewer_key=v.viewer_key AND w.broadcast_id='va-live') FROM watch_visits v WHERE v.broadcast_id='va-live' AND v.viewer_key=$1 ORDER BY v.started_at DESC LIMIT 1")
        .bind(browser_digest_like).fetch_one(&e.app.db).await.unwrap()
}
fn key(browser: &str) -> String {
    format!("b:{}", sver::security::digest(browser))
}

pub async fn exercise(e: &Env) {
    let owner = person(e, "va-owner", "VaOwner", true).await;
    let other = person(e, "va-rival", "VaRival", true).await;
    person(e, "va-fan", "VaFan", true).await;
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('va-live','va-owner','va-live',1,'LIVE','s','v','c',now()-interval '2 hours',now(),now(),'skipped')").await;

    // Heartbeats: the first one of a visit sets its source; later ones only extend it.
    assert_eq!(
        beat(e, "an-browser-home-0001", json!({"source":"home"})).await,
        StatusCode::OK
    );
    assert_eq!(
        beat(e, "an-browser-home-0001", json!({"source":"search"})).await,
        StatusCode::OK
    );
    assert_eq!(
        visit(e, &key("an-browser-home-0001")).await,
        ("home".into(), None, 1)
    );
    // Unknown claims are "other", server-decided sources can't be claimed, no claim is direct.
    beat(e, "an-browser-odd-00001", json!({"source":"raid"})).await;
    assert_eq!(visit(e, &key("an-browser-odd-00001")).await.0, "other");
    beat(e, "an-browser-none-0001", json!({})).await;
    assert_eq!(visit(e, &key("an-browser-none-0001")).await.0, "direct");
    // A referring site keeps its host name only, and only for outside links.
    beat(
        e,
        "an-browser-ext-00001",
        json!({"source":"external","referrer":"WWW.Discord.com"}),
    )
    .await;
    assert_eq!(
        visit(e, &key("an-browser-ext-00001")).await,
        ("external".into(), Some("discord.com".into()), 1)
    );
    beat(
        e,
        "an-browser-ref-00001",
        json!({"source":"home","referrer":"discord.com"}),
    )
    .await;
    assert_eq!(visit(e, &key("an-browser-ref-00001")).await.1, None);
    // A visit that went quiet for 30 seconds ends; the next beat starts a new one.
    e.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE watch_visits SET last_at=now()-interval '1 minute' WHERE viewer_key='{}'",
        key("an-browser-home-0001")
    )))
    .await;
    e.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE playback_leases SET expires_at=now()-interval '1 second' WHERE viewer_key='{}'",
        key("an-browser-home-0001")
    )))
    .await;
    beat(e, "an-browser-home-0001", json!({"source":"search"})).await;
    assert_eq!(
        visit(e, &key("an-browser-home-0001")).await,
        ("search".into(), None, 2)
    );

    // Impressions: live broadcasts only, a known surface, at most 48 cards.
    for _ in 0..2 {
        let (status, _) = guest(
            e,
            "POST",
            "/api/discovery/seen",
            json!({"surface":"home","broadcasts":["va-live","no-such-broadcast"]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(
        guest(
            e,
            "POST",
            "/api/discovery/seen",
            json!({"surface":"nowhere","broadcasts":["va-live"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let many: Vec<String> = (0..49).map(|n| format!("b{n}")).collect();
    assert_eq!(
        guest(
            e,
            "POST",
            "/api/discovery/seen",
            json!({"surface":"home","broadcasts":many})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // The owner seeing their own card doesn't count.
    call(
        e,
        "POST",
        "/api/discovery/seen",
        Some(&owner),
        json!({"surface":"home","broadcasts":["va-live"]}),
    )
    .await;
    let shown: i32 = sqlx::query_scalar(
        "SELECT shown FROM discovery_impressions WHERE broadcast_id='va-live' AND surface='home'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(shown, 2);

    // A known history for the report: only counted visits count.
    e.sql("DELETE FROM watch_visits WHERE broadcast_id='va-live'")
        .await;
    e.sql("INSERT INTO watch_visits(broadcast_id,channel_id,viewer_key,source,referrer_host,follower,started_at,last_at,seconds,counted) VALUES
        ('va-live','va-owner','u:va-fan','home',NULL,false,now()-interval '90 minutes',now()-interval '60 minutes',1800,true),
        ('va-live','va-owner','b:g1','home',NULL,false,now()-interval '80 minutes',now()-interval '79 minutes',40,true),
        ('va-live','va-owner','b:g2','external','discord.com',false,now()-interval '70 minutes',now()-interval '60 minutes',600,true),
        ('va-live','va-owner','b:g3','search',NULL,false,now()-interval '20 minutes',now()-interval '10 minutes',600,false)").await;
    // VaFan followed during the visit: a follow that came from Home.
    e.sql("INSERT INTO follows(follower_id,following_id,created_at) VALUES('va-fan','va-owner',now()-interval '70 minutes')").await;
    // On top of Live Now for the 3-minute slot when the first visit started.
    e.sql("INSERT INTO rotation_turns(slot,broadcast_id) SELECT floor(extract(epoch FROM now()-interval '90 minutes')/180)::bigint,'va-live'").await;

    let (status, report) = call(
        e,
        "GET",
        "/api/me/analytics?days=7",
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let t = &report["totals"];
    assert_eq!(
        (&t["visits"], &t["viewers"], &t["new_viewers"]),
        (&json!(3), &json!(3), &json!(3))
    );
    assert_eq!(t["watch_hours"], json!(0.7), "{t}");
    assert_eq!(t["new_follows"], json!(1));
    let home = report["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source"] == "home")
        .unwrap();
    assert_eq!(
        (&home["visits"], &home["follows"], &home["stayed_5_share"]),
        (&json!(2), &json!(1), &json!(0.5))
    );
    assert!(
        report["sources"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["source"] != "search"),
        "uncounted visits are left out"
    );
    let exposure = report["exposure"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["surface"] == "home")
        .unwrap();
    assert_eq!(
        (
            &exposure["shown"],
            &exposure["visits"],
            &exposure["conversion"]
        ),
        (&json!(2), &json!(2), &json!(1.0))
    );
    // Visit lengths: one under a minute, two of 5–15 minutes or more.
    let buckets: Vec<(i64, i64)> = report["lengths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["bucket"].as_i64().unwrap(), b["visits"].as_i64().unwrap()))
        .collect();
    assert_eq!(buckets, vec![(0, 1), (2, 1), (3, 1)]);
    // One arrival during the Live Now turn; the rest of the live time had two.
    assert_eq!(
        report["lift"]["live_now"]["arrivals"],
        json!(1),
        "{}",
        report["lift"]
    );
    assert_eq!(report["lift"]["otherwise"]["arrivals"], json!(2));
    assert_eq!(
        report["referrers"],
        json!([{"host":"discord.com","visits":1}])
    );
    assert_eq!(report["broadcasts"][0]["id"], "va-live");
    assert_eq!(report["broadcasts"][0]["top_source"], "home");

    // Only 7, 30 or 90 days; only the signed-in streamer's own numbers.
    assert_eq!(
        call(
            e,
            "GET",
            "/api/me/analytics?days=5",
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        guest(e, "GET", "/api/me/analytics", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, theirs) = call(e, "GET", "/api/me/analytics", Some(&other), Value::Null).await;
    assert_eq!(theirs["totals"]["visits"], json!(0));

    // The platform view: staff only, every channel, with the top channels.
    let admin = super::bans::staff(e, "va-staff", "VaStaff").await;
    assert_eq!(
        call(
            e,
            "GET",
            "/api/admin/analytics?days=7",
            Some(&owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, all) = call(
        e,
        "GET",
        "/api/admin/analytics?days=7",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert!(all["totals"]["visits"].as_i64().unwrap() >= 3, "{all}");
    assert!(
        all["channels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["username"] == "VaOwner"),
        "{all}"
    );
    assert!(
        all["lift"]["live_now"]["arrivals"].as_i64().unwrap() >= 1,
        "{}",
        all["lift"]
    );
    assert_eq!(
        report["channels"],
        json!([]),
        "a creator's report lists no other channels"
    );
    // Open data: the weekly viewer figures are computed and withheld under 10 channels.
    sver::open_data::compute(&e.app).await.unwrap();
    let hours: f64 = sqlx::query_scalar("SELECT value FROM public_stats WHERE period=to_char(now(),'IYYY-\"W\"IW') AND metric='watch_hours'")
        .fetch_one(&e.app.db).await.unwrap();
    assert!(hours >= 0.6, "{hours}");
    let (_, data) = guest(e, "GET", "/api/open-data", Value::Null).await;
    let this = data["weekly"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        (&this["watch_hours"], &this["discovery_lift"]),
        (&Value::Null, &Value::Null),
        "{this}"
    );

    // Deleting a viewer's account removes their visits.
    let mut db = e.app.db.acquire().await.unwrap();
    sver::analytics::forget_viewer(&mut db, "va-fan")
        .await
        .unwrap();
    drop(db);
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM watch_visits WHERE viewer_key='u:va-fan'")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert_eq!(left, 0);
    growth(e).await;
}

/// The growth tracker's endpoint: hidden without its token, platform totals only with it.
async fn growth(e: &Env) {
    use axum::{body::Body, extract::ConnectInfo, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let token = "growth-token-for-tests-0123456789abcdef";
    let get = |path: &str, auth: Option<String>| {
        let mut r = Request::get(path)
            .header("x-real-ip", "127.0.0.1")
            .extension(ConnectInfo(
                "127.0.0.1:1".parse::<std::net::SocketAddr>().unwrap(),
            ));
        if let Some(a) = auth {
            r = r.header("authorization", a);
        }
        let app = e.app.clone();
        let req = r.body(Body::empty()).unwrap();
        async move {
            let res = sver::router(app).oneshot(req).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    // Unset: the route looks like any unknown path, even with a token.
    assert_eq!(
        get("/api/growth", Some(format!("Bearer {token}"))).await.0,
        StatusCode::NOT_FOUND
    );
    // SAFETY: the stream tests run one after another; nothing else reads this variable.
    unsafe { std::env::set_var("GROWTH_TOKEN", token) };
    assert_eq!(get("/api/growth", None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        get("/api/growth", Some("Bearer wrong".into())).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get("/api/growth?days=91", Some(format!("Bearer {token}")))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, body) = get("/api/growth?days=7", Some(format!("Bearer {token}"))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for key in [
        "accounts_total",
        "accounts_new",
        "active_users",
        "streams_live",
        "streams_started",
        "chat_messages",
        "peak_viewers",
        "viewers",
    ] {
        assert!(body[key].is_number(), "{key}: {body}");
    }
    assert!(body["accounts_total"].as_i64().unwrap() >= 3, "{body}");
    assert!(body["streams_live"].as_i64().unwrap() >= 1, "{body}");
    // Totals only: no usernames or channel lists.
    assert!(
        !body.to_string().contains("VaOwner") && body.get("channels").is_none(),
        "{body}"
    );
    unsafe { std::env::remove_var("GROWTH_TOKEN") };
}
