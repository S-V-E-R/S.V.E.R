//! The CrowdSync catalog (docs/CROWDSYNC.md "Catalog"): an app's owner sends in a listing, nobody
//! sees it until staff approve it, a change leaves the approved copy up, a suspended app's listing
//! is hidden, and "Add to my boards" copies the board into a library, counted once per channel.
use super::Env;
use super::chat::{call, person};
use super::overlays::upload;
use super::wall_seats::picture;
use axum::http::StatusCode;
use serde_json::{Value, json};

/// A person who can register apps and add boards: verified email and two-factor sign-in.
async fn streamer(e: &Env, id: &str, name: &str) -> String {
    let token = person(e, id, name, true).await;
    for statement in [
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id=$1",
        "UPDATE sessions SET mfa_verified=true WHERE user_id=$1",
    ] {
        sqlx::query(statement)
            .bind(id)
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    token
}

pub async fn exercise(e: &Env) {
    let dev = streamer(e, "cat-dev", "CatDev").await;
    let channel = streamer(e, "cat-channel", "CatChannel").await;
    let viewer = person(e, "cat-viewer", "CatViewer", true).await;
    let admin = super::bans::staff(e, "cat-staff", "CatStaff").await;
    let (dev, channel, viewer, admin) = (&dev, &channel, &viewer, &admin);
    let register = |name: &'static str| async move {
        let (status, made) = call(
            e,
            "POST",
            "/api/me/apps",
            Some(dev),
            json!({"name": name, "redirect_uris": ["https://example.test/cb"]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        made["created"]["client_id"].as_str().unwrap().to_string()
    };
    let app = register("Crowd Game").await;
    let mine = format!("/api/me/apps/{app}/listing");
    let review = |id: String, action: &'static str, note: &'static str| {
        let path = format!("/api/admin/catalog/{id}/review");
        async move {
            call(
                e,
                "POST",
                &path,
                Some(admin),
                json!({"action": action, "note": note}),
            )
            .await
        }
    };
    let detail = |slug: &'static str| async move {
        call(
            e,
            "GET",
            &format!("/api/crowdsync/{slug}"),
            None,
            Value::Null,
        )
        .await
    };

    // ---- Publishing: the app's owner only, and everything is checked ----
    assert_eq!(
        call(e, "GET", &mine, Some(channel), Value::Null).await.0,
        StatusCode::NOT_FOUND,
        "someone else's app"
    );
    assert_eq!(
        call(e, "GET", &mine, Some(viewer), Value::Null).await.0,
        StatusCode::FORBIDDEN,
        "developers need two-factor sign-in"
    );
    let (status, empty) = call(e, "GET", &mine, Some(dev), Value::Null).await;
    assert_eq!(
        (status, &empty["listing"]),
        (StatusCode::OK, &Value::Null),
        "{empty}"
    );
    let (status, shot) = upload(e, dev, &format!("{mine}/screenshot"), picture(256, 3)).await;
    assert_eq!(status, StatusCode::OK, "{shot}");
    let key = shot["key"].as_str().unwrap().to_string();
    // The board uses one of the developer's own Skills, which no other channel has.
    let file = json!({"sver_board": 1, "name": "Crowd Game", "board": {"screens": [{"name": "Main", "controls": [
        {"id": "boom", "kind": "button", "label": "Boom", "cost": 50, "cooldown_seconds": 60, "effect": "confetti"},
        {"id": "mine", "kind": "button", "label": "Mine", "effect": format!("custom:{}", uuid::Uuid::new_v4())}]}]}});
    let listing = |name: &str, board: Value| {
        json!({"name": name, "summary": "Viewers drop obstacles on the runner.", "description": "Line one.\nLine two.",
            "link": "https://example.test/crowd-game", "screenshots": [key], "board": board})
    };
    let send = |body: Value| {
        let mine = mine.clone();
        let dev = dev.clone();
        async move { call(e, "PUT", &mine, Some(&dev), body).await }
    };
    let from_file = json!({"from": "file", "file": file});
    for (field, value, why) in [
        ("link", json!("http://example.test/"), "https only"),
        ("summary", json!("heil hitler"), "the word filter"),
        ("name", json!("ab"), "names are 3 to 40 characters"),
        (
            "screenshots",
            json!(["setup/00000000000000000000000000000000"]),
            "not their upload",
        ),
        ("board", json!({"from": "keep"}), "no board to keep yet"),
        (
            "board",
            json!({"from": "file", "file": {"sver_board": 1, "board": {"screens": [{"name": "Main", "controls": []}]}}}),
            "an empty board",
        ),
    ] {
        let mut body = listing("Crowd Game", from_file.clone());
        body[field] = value;
        assert_eq!(send(body).await.0, StatusCode::BAD_REQUEST, "{why}");
    }
    assert_eq!(
        call(
            e,
            "PUT",
            &mine,
            Some(channel),
            listing("Crowd Game", from_file.clone())
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "only the owner publishes"
    );
    let (status, sent) = send(listing("Crowd Game", from_file.clone())).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let sent = &sent["listing"];
    assert_eq!(
        (&sent["status"], &sent["live"], &sent["slug"]),
        (&json!("PENDING"), &json!(false), &json!("crowd-game"))
    );
    assert_eq!(
        sent["content"]["board"]["screens"][0]["controls"][1]["effect"], "none",
        "a custom Skill can't travel"
    );
    assert_eq!(sent["content"]["screenshots"][0]["key"], key);

    // ---- Review: nothing is public until staff approve it ----
    let (_, list) = call(e, "GET", "/api/crowdsync", None, Value::Null).await;
    assert_eq!(list["items"], json!([]));
    assert_eq!(detail("crowd-game").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/crowdsync/crowd-game/install",
            Some(channel),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for token in [dev, viewer] {
        assert_eq!(
            call(e, "GET", "/api/admin/catalog", Some(token), Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND,
            "staff only"
        );
        let path = format!("/api/admin/catalog/{app}/review");
        assert_eq!(
            call(e, "POST", &path, Some(token), json!({"action": "approve"}))
                .await
                .0,
            StatusCode::NOT_FOUND,
            "staff only"
        );
    }
    let (status, queue) = call(e, "GET", "/api/admin/catalog", Some(admin), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        (
            &queue["items"][0]["client_id"],
            &queue["items"][0]["owner"],
            &queue["items"][0]["pending"]["name"]
        ),
        (&json!(app), &json!("CatDev"), &json!("Crowd Game"))
    );
    let (_, home) = call(e, "GET", "/api/admin/home", Some(admin), Value::Null).await;
    assert_eq!(home["counts"]["catalog"], 1);
    assert_eq!(
        review(app.clone(), "reject", "").await.0,
        StatusCode::BAD_REQUEST,
        "a rejection needs its reason"
    );
    assert_eq!(review(app.clone(), "approve", "").await.0, StatusCode::OK);
    assert_eq!(
        review(app.clone(), "approve", "").await.0,
        StatusCode::CONFLICT,
        "nothing waiting"
    );
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action='catalog_approve' AND target_type='catalog_listing' AND target_id=$1 AND actor_id='cat-staff'")
        .bind(&app)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(logged, 1, "in the staff audit log");

    // ---- Browsing: the approved copy, with no email or internal IDs ----
    let (status, page) = detail("crowd-game").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        (
            &page["name"],
            &page["developer"]["username"],
            &page["installs"]
        ),
        (&json!("Crowd Game"), &json!("CatDev"), &json!(0))
    );
    assert_eq!(page["screenshots"].as_array().unwrap().len(), 1);
    let text = page.to_string();
    for private in ["cat-dev", "example.test\"", "@example", &app] {
        assert!(!text.contains(private), "{private} is not public");
    }
    let found = |q: &'static str| async move {
        let path = format!("/api/crowdsync?q={q}");
        call(e, "GET", &path, None, Value::Null).await.1["items"]
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!((found("crowd").await, found("runner").await), (1, 0));
    assert_eq!(found("%25").await, 0, "a percent sign is only a character");

    // ---- A change waits; the approved copy stays up ----
    let (_, changed) = send(listing("Crowd Game Two", json!({"from": "keep"}))).await;
    assert_eq!(
        (&changed["listing"]["status"], &changed["listing"]["live"]),
        (&json!("PENDING"), &json!(true))
    );
    assert_eq!(detail("crowd-game").await.1["name"], "Crowd Game");
    assert_eq!(
        review(app.clone(), "reject", "The name is misleading.")
            .await
            .0,
        StatusCode::OK
    );
    let (_, told) = call(e, "GET", &mine, Some(dev), Value::Null).await;
    assert_eq!(
        (&told["listing"]["status"], &told["listing"]["reason"]),
        (&json!("REJECTED"), &json!("The name is misleading."))
    );
    assert_eq!(detail("crowd-game").await.1["name"], "Crowd Game");
    // Going back to what's approved needs no second review.
    let (_, back) = send(listing("Crowd Game", json!({"from": "keep"}))).await;
    assert_eq!(back["listing"]["status"], "APPROVED");
    send(listing("Crowd Game Two", json!({"from": "keep"}))).await;
    assert_eq!(review(app.clone(), "approve", "").await.0, StatusCode::OK);
    let (_, page) = detail("crowd-game").await;
    assert_eq!(page["name"], "Crowd Game Two", "the page keeps its address");

    // ---- Installing: into the library, checked like an import, counted once per channel ----
    let install = |token: String| async move {
        call(
            e,
            "POST",
            "/api/crowdsync/crowd-game/install",
            Some(&token),
            Value::Null,
        )
        .await
    };
    assert_eq!(
        install(viewer.clone()).await.0,
        StatusCode::FORBIDDEN,
        "needs two-factor sign-in"
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/crowdsync/crowd-game/install",
            None,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, added) = install(channel.clone()).await;
    assert_eq!(status, StatusCode::OK, "{added}");
    install(channel.clone()).await;
    install(dev.clone()).await;
    let (_, studio) = call(e, "GET", "/api/me/board", Some(channel), Value::Null).await;
    assert_eq!(
        studio["library"],
        json!([{"id": studio["library"][0]["id"], "name": "Crowd Game Two", "updated_at": studio["library"][0]["updated_at"], "from_game": false, "from_catalog": true}]),
        "one entry however often it's added"
    );
    assert_eq!(detail("crowd-game").await.1["installs"], 2, "two channels");
    e.sql("DELETE FROM board_library WHERE channel_id='cat-channel'")
        .await;
    e.sql("INSERT INTO board_library(id,channel_id,name,board) SELECT 'cat-full-'||n,'cat-channel','Board '||n,'{\"screens\":[]}' FROM generate_series(1,20) n").await;
    let (status, full) = install(channel.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{full}");
    assert!(full["error"].as_str().unwrap().contains("20 boards"));

    // ---- A game that sets its own board: nothing to add; connected channels are counted ----
    let game = register("Own Board Game").await;
    let (status, sent) = call(
        e,
        "PUT",
        &format!("/api/me/apps/{game}/listing"),
        Some(dev),
        json!({"name": "Own Board Game", "summary": "It brings its own board.", "description": "Connect it.", "link": "https://example.test/own", "board": {"from": "game"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(review(game.clone(), "approve", "").await.0, StatusCode::OK);
    sqlx::query("INSERT INTO oauth_grants(id,app_id,user_id,scopes) VALUES('cat-grant',$1,'cat-channel',ARRAY['board:control'])")
        .bind(&game)
        .execute(&e.app.db)
        .await
        .unwrap();
    let (_, list) = call(e, "GET", "/api/crowdsync", None, Value::Null).await;
    assert_eq!(
        (
            &list["items"][0]["slug"],
            &list["items"][0]["sets_own_board"],
            &list["items"][0]["installs"],
            &list["items"][1]["slug"]
        ),
        (
            &json!("own-board-game"),
            &json!(true),
            &json!(1),
            &json!("crowd-game")
        ),
        "newest first"
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/crowdsync/own-board-game/install",
            Some(channel),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // ---- Hidden: a suspended app, an under-18 developer's name, a removed image, a removal ----
    let suspend = |on: bool| {
        let path = format!("/api/admin/apps/{app}/suspend");
        async move {
            call(e, "POST", &path, Some(admin), json!({"suspended": on})).await;
        }
    };
    suspend(true).await;
    assert_eq!(detail("crowd-game").await.0, StatusCode::NOT_FOUND);
    assert_eq!(found("crowd").await, 0);
    assert_eq!(
        send(listing("Crowd Game Three", json!({"from": "keep"})))
            .await
            .0,
        StatusCode::CONFLICT
    );
    suspend(false).await;
    e.sql("UPDATE users SET date_of_birth=current_date-interval '16 years' WHERE id='cat-dev'")
        .await;
    assert_eq!(detail("crowd-game").await.1["developer"], Value::Null);
    e.sql("UPDATE users SET date_of_birth='1990-01-01' WHERE id='cat-dev'")
        .await;
    let mut db = e.app.db.acquire().await.unwrap();
    let owners = sver::catalog::remove_media(&mut db, &key).await.unwrap();
    assert_eq!(owners, ["cat-dev"], "Take It Down reaches listings");
    drop(db);
    assert_eq!(detail("crowd-game").await.1["screenshots"], json!([]));
    assert_eq!(
        review(app.clone(), "remove", "Reported and confirmed.")
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(detail("crowd-game").await.0, StatusCode::NOT_FOUND);
    let (_, told) = call(e, "GET", &mine, Some(dev), Value::Null).await;
    assert_eq!(
        (&told["listing"]["status"], &told["listing"]["reason"]),
        (&json!("REMOVED"), &json!("Reported and confirmed."))
    );

    // Deleting the app takes its listing along.
    for id in [&app, &game] {
        let path = format!("/api/me/apps/{id}");
        assert_eq!(
            call(e, "DELETE", &path, Some(dev), Value::Null).await.0,
            StatusCode::OK
        );
    }
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_listings")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
    e.sql("DELETE FROM staff_roles WHERE user_id='cat-staff'")
        .await;
}
