//! Module 5 discovery: home shelves, fair rotation through the API, browse, search, suggestions,
//! automatic and staff spotlights, empty states, and that size never changes the order.
use super::Env;
use super::bans::staff;
use super::chat::{call, person};
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
/// This test's channels only: other tests in the suite may leave their own streams live.
fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["username"].as_str())
        .filter(|n| n.starts_with("Dv"))
        .map(str::to_string)
        .collect()
}
async fn get(e: &Env, path: &str, token: Option<&str>) -> (StatusCode, Value) {
    call(e, "GET", path, token, Value::Null).await
}

pub async fn exercise(e: &Env) {
    let admin = staff(e, "dv-staff", "DvStaff").await;
    let viewer = person(e, "dv-viewer", "DvViewer", true).await;
    for (id, name) in [
        ("dv-art", "DvArtist"),
        ("dv-fps", "DvShooter"),
        ("dv-old", "DvVeteran"),
        ("dv-new", "DvNewcomer"),
        ("dv-hide", "DvHidden"),
        ("dv-off", "DvOffline"),
    ] {
        person(e, id, name, true).await;
    }
    e.sql("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at) VALUES('dv-viewer','aetheron',now(),now()),('dv-art','aetheron',now(),now()),('dv-fps','myria',now(),now())").await;
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('dv-viewer','dv-fps')")
        .await;
    // History: DvVeteran was last live 40 days ago, the others recently; DvNewcomer never.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,ended_at,end_reason,alert_state) VALUES
        ('dv-old-1','dv-old','dv-old-1',1,'ENDED','s','v','c',now()-interval '41 days',now(),now(),now()-interval '40 days','ended','skipped'),
        ('dv-fps-1','dv-fps','dv-fps-1',1,'ENDED','s','v','c',now()-interval '1 day',now(),now(),now()-interval '23 hours','ended','skipped'),
        ('dv-art-1','dv-art','dv-art-1',1,'ENDED','s','v','c',now()-interval '2 days',now(),now(),now()-interval '47 hours','ended','skipped'),
        ('dv-off-1','dv-off','dv-off-1',1,'ENDED','s','v','c',now()-interval '3 days',now(),now(),now()-interval '2 days','ended','skipped')").await;

    // Nothing live: never an empty page; recently live channels instead.
    let (status, home) = get(e, "/api/discovery/home", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(names(&home["live"]).is_empty());
    let recent: Vec<&str> = home["recent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["user"]["username"].as_str().unwrap())
        .collect();
    assert!(recent.contains(&"DvShooter") && recent.contains(&"DvOffline"));
    assert!(!recent.contains(&"DvVeteran"), "only the last 14 days");

    live(e, "dv-b-art", "dv-art", "art", 50).await;
    live(e, "dv-b-fps", "dv-fps", "valorant", 40).await;
    live(e, "dv-b-old", "dv-old", "minecraft", 20).await;
    live(e, "dv-b-new", "dv-new", "art", 10).await;
    live(e, "dv-b-hide", "dv-hide", "art", 5).await;
    // A restricted channel never appears; a blocked one is hidden from the blocker only.
    e.sql("INSERT INTO profiles(user_id,display_name,restricted_until) VALUES('dv-hide','DvHidden',now()+interval '1 day') ON CONFLICT(user_id) DO UPDATE SET restricted_until=EXCLUDED.restricted_until")
        .await;
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('dv-viewer','dv-old')")
        .await;
    // Size never matters: a large counted audience on one stream changes nothing.
    let (_, before) = get(e, "/api/discovery/home", None).await;
    for n in 0..40 {
        sqlx::query("INSERT INTO playback_leases(broadcast_id,viewer_key,expires_at,level) VALUES('dv-b-fps',$1,now()+interval '30 seconds','counted')")
            .bind(format!("b:size-{n}")).execute(&e.app.db).await.unwrap();
    }
    let (_, after) = get(e, "/api/discovery/home", None).await;
    assert_eq!(
        names(&before["live"]),
        names(&after["live"]),
        "order ignores viewer counts"
    );
    let anon = names(&after["live"]);
    assert_eq!(anon.len(), 4);
    assert!(!anon.contains(&"DvHidden".to_string()));
    let card = |list: &Value, name: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|c| c["username"] == name)
            .cloned()
            .unwrap()
    };
    assert_eq!(card(&after["live"], "DvShooter")["viewers"], 40);
    thumbnails(e).await;

    // Signed in: following, own faction, no blocked channel; labels and spotlights.
    let (_, mine) = get(e, "/api/discovery/home", Some(&viewer)).await;
    assert_eq!(names(&mine["following"]), ["DvShooter"]);
    assert_eq!(names(&mine["faction"]), ["DvArtist"]);
    assert!(!names(&mine["live"]).contains(&"DvVeteran".to_string()));
    assert_eq!(after["faction"], Value::Null);
    let spotlights: Vec<(String, String)> = after["spotlights"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["stream"]["username"].as_str().unwrap().to_string(),
                s["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(spotlights.contains(&("DvNewcomer".into(), "first_stream".into())));
    assert!(spotlights.contains(&("DvVeteran".into(), "returning".into())));
    assert_eq!(spotlights.len(), 2);
    let newcomer = card(&after["live"], "DvNewcomer");
    assert_eq!(
        (&newcomer["label"], &newcomer["fresh"]),
        (&json!("New creator"), &json!(true))
    );
    assert_eq!(names(&after["fresh"]), ["DvNewcomer", "DvVeteran"]);

    // Browse: genres with categories and live counts; filtered lists keep rotation order.
    let (_, browse) = get(e, "/api/discovery/browse", None).await;
    let art = browse["genres"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "art")
        .cloned()
        .unwrap();
    assert_eq!(
        (&art["live"], &art["home"]),
        (&json!(2), &json!("aetheron"))
    );
    let (_, list) = get(e, "/api/discovery/live?genre=art", None).await;
    let mut art_names = names(&list["items"]);
    art_names.sort();
    assert_eq!(art_names, ["DvArtist", "DvNewcomer"]);
    // Mature label: under-18 viewers lose labeled streams (after ordering) from lists and search;
    // everyone else sees them tagged.
    e.sql("INSERT INTO stream_settings(owner_id,title,mature) VALUES('dv-art','DvArtist''s stream',true) ON CONFLICT(owner_id) DO UPDATE SET mature=true")
        .await;
    let minor = person(e, "dv-minor", "DvMinor", true).await;
    e.sql("UPDATE users SET date_of_birth=current_date-interval '15 years' WHERE id='dv-minor'")
        .await;
    let (_, young) = get(e, "/api/discovery/live?genre=art", Some(&minor)).await;
    assert_eq!(names(&young["items"]), ["DvNewcomer"]);
    let (_, found) = get(e, "/api/search?q=DvArt", Some(&minor)).await;
    assert_eq!(found["channels"], json!([]));
    let (_, adult) = get(e, "/api/discovery/live?genre=art", Some(&viewer)).await;
    let tagged = adult["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["display_name"] == "DvArtist")
        .cloned()
        .unwrap();
    assert_eq!(tagged["mature"], true);
    e.sql("UPDATE stream_settings SET mature=false WHERE owner_id='dv-art'")
        .await;
    // Stream language: cards carry it; filters narrow without reordering; "mine" uses the viewer's
    // chosen languages, else the browser's (fallback); home can show only those.
    e.sql("UPDATE stream_settings SET language=CASE owner_id WHEN 'dv-art' THEN 'en' WHEN 'dv-new' THEN 'pt' END WHERE owner_id IN ('dv-art','dv-new')").await;
    let (_, pt) = get(e, "/api/discovery/live?genre=art&language=pt", None).await;
    assert_eq!(names(&pt["items"]), ["DvNewcomer"]);
    assert_eq!(pt["items"][0]["language"], "pt");
    let (_, mine) = get(
        e,
        "/api/discovery/live?genre=art&language=mine&fallback=en,xx",
        None,
    )
    .await;
    assert_eq!(
        names(&mine["items"]),
        ["DvArtist"],
        "browser languages for guests"
    );
    let (status, _) = call(
        e,
        "PUT",
        "/api/me/preferences",
        Some(&viewer),
        json!({"languages": ["xx"]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    call(
        e,
        "PUT",
        "/api/me/preferences",
        Some(&viewer),
        json!({"languages": ["pt"], "only_my_languages": true}),
    )
    .await;
    let (_, mine) = get(
        e,
        "/api/discovery/live?genre=art&language=mine&fallback=en",
        Some(&viewer),
    )
    .await;
    assert_eq!(
        names(&mine["items"]),
        ["DvNewcomer"],
        "chosen languages win"
    );
    let (_, home) = get(e, "/api/discovery/home", Some(&viewer)).await;
    assert!(
        names(&home["live"]).iter().all(|n| n == "DvNewcomer"),
        "{home}"
    );
    call(
        e,
        "PUT",
        "/api/me/preferences",
        Some(&viewer),
        json!({"languages": [], "only_my_languages": false}),
    )
    .await;
    let (_, list) = get(e, "/api/discovery/live?faction=myria", None).await;
    assert_eq!(names(&list["items"]), ["DvShooter"]);
    let (_, list) = get(e, "/api/discovery/live?category=chess", None).await;
    assert_eq!(list["items"], json!([]));
    assert!(!list["recent"].as_array().unwrap().is_empty());
    assert_eq!(
        get(e, "/api/discovery/live?faction=nope", None).await.0,
        StatusCode::BAD_REQUEST
    );

    // Search: live channels first, categories by name, wildcards are literal.
    let (status, found) = get(e, "/api/search?q=dv", None).await;
    assert_eq!(status, StatusCode::OK);
    let channels: Vec<&str> = found["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["username"].as_str().unwrap())
        .collect();
    let live_names = ["DvArtist", "DvShooter", "DvVeteran", "DvNewcomer"];
    assert!(
        channels[..4].iter().all(|n| live_names.contains(n)),
        "{channels:?}"
    );
    assert!(channels.contains(&"DvOffline") && !channels.contains(&"DvHidden"));
    let (_, found) = get(e, "/api/search?q=valor", None).await;
    assert_eq!(found["categories"][0]["id"], "valorant");
    sqlx::query("INSERT INTO stream_categories(id,name,genre) VALUES('dv-pokemon','Pokémon Dv','mmos_rpgs')")
        .execute(&e.app.db).await.unwrap();
    for q in ["pokemon%20dv", "POK%C3%89MON"] {
        let (_, found) = get(e, &format!("/api/search?q={q}"), None).await;
        assert_eq!(
            found["categories"][0]["id"], "dv-pokemon",
            "accents ignored: {q}"
        );
    }
    let (_, found) = get(e, "/api/search?q=d%25", None).await;
    assert_eq!(found["channels"], json!([]), "% is matched literally");
    assert_eq!(
        get(e, "/api/search?q=x", None).await.0,
        StatusCode::BAD_REQUEST
    );

    // Suggestions: same genre, then same faction, then anything live; never the channel itself.
    let (_, next) = get(e, "/api/channels/DvArtist/suggestions", None).await;
    let next = names(&next["items"]);
    assert_eq!(next[0], "DvNewcomer");
    assert!(!next.contains(&"DvArtist".to_string()) && next.len() == 3);
    // Still works the moment a stream ends (the stream-end countdown).
    e.sql(
        "UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='ended' WHERE id='dv-b-art'",
    )
    .await;
    let (_, next) = get(e, "/api/channels/DvArtist/suggestions", None).await;
    assert_eq!(names(&next["items"])[0], "DvNewcomer");

    // Staff spotlights: staff only, at most 14 days, one active per channel, 7-day cooldown.
    let body = json!({"username":"DvOffline","reason":"Community pick","days":7});
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/spotlights",
            Some(&viewer),
            body.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/spotlights",
            Some(&admin),
            json!({"username":"DvOffline","reason":"Too long","days":15})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, list) = call(
        e,
        "POST",
        "/api/admin/spotlights",
        Some(&admin),
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = list["items"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/spotlights",
            Some(&admin),
            body.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, shelves) = get(e, "/api/discovery/home", None).await;
    let pick = &shelves["spotlights"][0];
    assert_eq!(
        (
            &pick["kind"],
            &pick["reason"],
            &pick["user"]["username"],
            &pick["stream"]
        ),
        (
            &json!("staff"),
            &json!("Community pick"),
            &json!("DvOffline"),
            &Value::Null
        )
    );
    assert_eq!(
        call(
            e,
            "POST",
            &format!("/api/admin/spotlights/{id}/end"),
            Some(&admin),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/spotlights",
            Some(&admin),
            body.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "7-day cooldown after it ends"
    );
    sqlx::query("UPDATE spotlights SET ended_early_at=now()-interval '8 days' WHERE id=$1")
        .bind(&id)
        .execute(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        call(e, "POST", "/api/admin/spotlights", Some(&admin), body)
            .await
            .0,
        StatusCode::OK
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action IN ('create_spotlight','end_spotlight') AND actor_id='dv-staff'")
        .fetch_one(&e.app.db).await.unwrap();
    assert_eq!(audited, 3);

    for statement in [
        "DELETE FROM moderation_actions WHERE actor_id='dv-staff'",
        "DELETE FROM staff_roles WHERE user_id='dv-staff'",
        "DELETE FROM users WHERE id LIKE 'dv-%'",
    ] {
        e.sql(statement).await;
    }
    open_data(e).await;
}

async fn thumbnails(e: &Env) {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let dir = std::env::temp_dir().join(format!("sver-thumbnails-{}", uuid::Uuid::new_v4()));
    let mut config = (*e.app.config).clone();
    config.media.storage = sver::media::Storage::Filesystem(dir.clone());
    let mut app = e.app.clone();
    app.config = std::sync::Arc::new(config);
    let request = || async {
        sver::router(app.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/discovery/thumbnails/dv-b-art?v=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    };
    assert_eq!(
        request().await.status(),
        StatusCode::NOT_FOUND,
        "no first capture yet"
    );
    // One URL follows replacements even when the page was loaded before either capture.
    for n in 1..=2 {
        let key = format!("thumbs/dv-b-art/{n}.webp");
        let bytes = vec![n; 64];
        app.config
            .media
            .storage
            .put(&app.http, &key, bytes.clone())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE broadcasts SET thumbnail_key=$1,thumbnail_at=now() WHERE id='dv-b-art'",
        )
        .bind(&key)
        .execute(&app.db)
        .await
        .unwrap();
        if n == 2 {
            app.config
                .media
                .storage
                .delete(&app.http, "thumbs/dv-b-art/1.webp")
                .await
                .unwrap();
        }
        let response = request().await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["content-type"], "image/webp");
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .as_ref(),
            bytes
        );
    }
    e.sql("UPDATE broadcasts SET thumbnail_at=now()-interval '4 minutes' WHERE id='dv-b-art'")
        .await;
    assert_eq!(
        request().await.status(),
        StatusCode::NOT_FOUND,
        "stale captures expire"
    );
    e.sql("UPDATE broadcasts SET thumbnail_at=now(),state='RECONNECTING',reconnect_deadline=now()+interval '30 seconds' WHERE id='dv-b-art'").await;
    assert_eq!(
        request().await.status(),
        StatusCode::OK,
        "last good frame during a short reconnect"
    );
    e.sql("UPDATE broadcasts SET reconnect_deadline=now()-interval '1 second' WHERE id='dv-b-art'")
        .await;
    assert_eq!(request().await.status(), StatusCode::NOT_FOUND);
    e.sql("UPDATE broadcasts SET state='LIVE',reconnect_deadline=NULL WHERE id='dv-b-art'")
        .await;
    e.sql("INSERT INTO media_removal_holds(root,hidden_at) VALUES('thumbs/dv-b-art',now())")
        .await;
    assert_eq!(
        request().await.status(),
        StatusCode::NOT_FOUND,
        "held media is hidden"
    );
    e.sql("DELETE FROM media_removal_holds WHERE root='thumbs/dv-b-art'")
        .await;
    e.sql("INSERT INTO profiles(user_id,display_name,restricted_until) VALUES('dv-art','DvArtist',now()+interval '1 hour') ON CONFLICT(user_id) DO UPDATE SET restricted_until=EXCLUDED.restricted_until")
        .await;
    assert_eq!(
        request().await.status(),
        StatusCode::NOT_FOUND,
        "restricted channels are hidden"
    );
    e.sql("UPDATE profiles SET restricted_until=NULL WHERE user_id='dv-art'")
        .await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='dv-b-art'")
        .await;
    assert_eq!(
        request().await.status(),
        StatusCode::NOT_FOUND,
        "ending hides the saved image URL"
    );
    sver::probe::sweep_thumbnails(&app).await.unwrap();
    assert_eq!(
        app.config
            .media
            .storage
            .head(&app.http, "thumbs/dv-b-art/2.webp")
            .await
            .unwrap(),
        None
    );
    e.sql("UPDATE broadcasts SET state='LIVE',ended_at=NULL,reconnect_deadline=NULL WHERE id='dv-b-art'").await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// Open data: weekly figures with "fewer than 10" for small counts, money folded until 10 creators
/// were paid, and CSVs that match.
async fn open_data(e: &Env) {
    for n in 0..10 {
        person(e, &format!("od-{n}"), &format!("OpenData{n}"), true).await;
    }
    // A 20-day stream on the 24/7 channel: its hours land in the weeks they happened, so last
    // week is a full 168, shown apart from creators' hours (withheld: fewer than 10 creators).
    let plays: Option<String> = sqlx::query_scalar("SELECT channel_id FROM plays_runtime")
        .fetch_optional(&e.app.db)
        .await
        .unwrap();
    match &plays {
        Some(_) => e.sql("UPDATE plays_runtime SET channel_id='od-0'").await,
        None => e.sql("INSERT INTO plays_runtime(channel_id,game,bridge_hash) VALUES('od-0','Synthetic game',repeat('0',64))").await,
    }
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,ended_at,confirmed_live_at) VALUES('od-b','od-0','od-b',1,'ENDED','s','v','c',now()-interval '20 days',now()-interval '20 days',now()-interval '20 days',now(),now()-interval '20 days')").await;
    // Three members, one of them deleted: faction counts are exact and leave deleted accounts out.
    e.sql("INSERT INTO faction_members(user_id,faction,chosen_at,joined_at) VALUES('od-1','aetheron',now(),now()),('od-2','aetheron',now(),now()),('od-3','aetheron',now(),now()) ON CONFLICT(user_id) DO UPDATE SET faction='aetheron'").await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM faction_members f JOIN users u ON u.id=f.user_id WHERE faction='aetheron' AND u.deleted_at IS NULL AND f.user_id NOT IN ('od-1','od-2','od-3')").fetch_one(&e.app.db).await.unwrap();
    e.sql("UPDATE users SET deleted_at=now() WHERE id='od-3'")
        .await;
    sver::open_data::compute(&e.app).await.unwrap();
    match &plays {
        Some(channel) => {
            sqlx::query("UPDATE plays_runtime SET channel_id=$1")
                .bind(channel)
                .execute(&e.app.db)
                .await
                .unwrap();
        }
        None => e.sql("DELETE FROM plays_runtime").await,
    }
    let (status, data) = get(e, "/api/open-data", None).await;
    assert_eq!(status, StatusCode::OK);
    let weeks = data["weekly"].as_array().unwrap();
    assert_eq!(weeks.len(), 12);
    let this = weeks.last().unwrap();
    assert_eq!(this["complete"], false, "this week is still filling up");
    let last = weeks[10]["hours_always_on"].as_f64().unwrap();
    assert!((168.0..200.0).contains(&last), "{last}");
    assert_eq!(
        weeks[10]["hours_streamed"],
        Value::Null,
        "fewer than 10 creators"
    );
    assert!(this["accounts_total"].as_f64().unwrap() >= 10.0, "{this}");
    assert_eq!(
        this["faction_aetheron"].as_f64().unwrap(),
        (before + 2) as f64,
        "faction counts are exact (the directory is public): {this}"
    );
    assert_eq!(
        data["monthly"],
        json!([]),
        "no month has had 10 paid creators yet"
    );
    let (status, csv) = get_text(e, "/api/open-data/weekly.csv").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        csv.starts_with("period,complete,accounts_total,") && csv.contains("fewer than 10"),
        "{csv}"
    );
    // Refunds and chargebacks come off the month they happen in, by category.
    let month = || async {
        let rows: Vec<(String, f64)> = sqlx::query_as("SELECT metric,value FROM public_stats WHERE period=to_char(now(),'YYYY-MM') AND metric IN ('creators_cents','platform_cents','wall_cents')")
            .fetch_all(&e.app.db).await.unwrap();
        let get = |m: &str| rows.iter().find(|r| r.0 == m).map(|r| r.1).unwrap_or(0.0);
        (
            get("creators_cents"),
            get("platform_cents"),
            get("wall_cents"),
        )
    };
    let before = month().await;
    let mut tx = e.app.db.begin().await.unwrap();
    for (kind, reference, intent, entries) in [
        // A $10 subscription with a $7 creator share, then $4 of it refunded.
        (
            "sub_payment",
            "od-pay",
            "pi_od",
            vec![
                ("usd:stripe", 10_000),
                ("usd:sales", -10_000),
                ("usd:earnings:od-0", 7_000),
                ("usd:platform", -7_000),
            ],
        ),
        (
            "refund",
            "od-refund",
            "pi_od",
            vec![
                ("usd:stripe", -4_000),
                ("usd:sales", 4_000),
                ("usd:earnings:od-0", -2_800),
                ("usd:platform", 2_800),
            ],
        ),
        // A $25 Wall seat refunded in full.
        (
            "wall_purchase",
            "od-wall",
            "pi_od_wall",
            vec![("usd:stripe", 25_000), ("usd:wall", -25_000)],
        ),
        (
            "refund",
            "od-wall-refund",
            "pi_od_wall",
            vec![("usd:stripe", -25_000), ("usd:wall", 25_000)],
        ),
    ] {
        let entries: Vec<(&str, &str, i64)> =
            entries.into_iter().map(|(a, n)| (a, "usd", n)).collect();
        let cents = entries[0].2.abs() / 10;
        sver::ledger::post(
            &mut tx,
            kind,
            reference,
            json!({"payment_intent": intent, "cents": cents}),
            &entries,
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    sver::open_data::compute(&e.app).await.unwrap();
    let after = month().await;
    assert_eq!(
        (after.0 - before.0, after.1 - before.1, after.2 - before.2),
        (420.0, 180.0, 0.0),
        "creators keep $4.20, S.V.E.R $1.80, and the refunded seat nets to nothing"
    );
    let (_, csv) = get_text(e, "/api/open-data/monthly.csv").await;
    assert!(
        csv.starts_with(
            "period,from,complete,creators_cents,platform_cents,payouts_cents,wall_cents,supporters_cents,ads_cents\n"
        ),
        "Wall and platform supporters are their own lines: {csv}"
    );
}
async fn get_text(e: &Env, path: &str) -> (StatusCode, String) {
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let response = sver::router(e.app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
