use super::*;

pub async fn exercise(e: &Env) {
    review_queues_skip_deletion_grace(e).await;
    readiness_has_persona_steps(e).await;
}

/// A submitter in deletion grace drops out of the wall and fan-art review queues and their
/// counts; cancelling the deletion brings everything back (docs/PROFILES.md, account deletion).
async fn review_queues_skip_deletion_grace(e: &Env) {
    let (_, owner) = e.user("AuditWallOwner", true).await;
    let (fan_id, fan) = e.user("AuditWallGrace", true).await;
    owner.ok("PUT", "/api/me/wall/settings", json!({"who_can_post": "ANYONE", "require_approval": true, "hold_links": false, "hold_new_accounts": false})).await;
    let post = owner
        .ok(
            "POST",
            "/api/channels/AuditWallOwner/wall",
            json!({"body": "Welcome"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let held = fan
        .ok(
            "POST",
            "/api/channels/AuditWallOwner/wall",
            json!({"body": "Hello there"}),
        )
        .await;
    assert_eq!(held["status"], "PENDING");
    fan.ok(
        "POST",
        &format!("/api/wall/posts/{post}/replies"),
        json!({"body": "Nice wall"}),
    )
    .await;
    owner
        .ok("PUT", "/api/me/fan-art/settings", json!({"enabled": true}))
        .await;
    let (s, v) = fan
        .upload(
            "/api/channels/AuditWallOwner/fan-art",
            &[("attest", "true")],
            Some(&png(800, 600)),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let owner = &owner;
    let queues = |count: i64, items: usize, art: usize| async move {
        let wall = owner.ok("GET", "/api/me/wall/pending", Value::Null).await;
        assert_eq!(wall["pending_count"], count, "{wall}");
        assert_eq!(wall["items"].as_array().unwrap().len(), items, "{wall}");
        let fan_art = owner
            .ok("GET", "/api/me/fan-art/pending", Value::Null)
            .await;
        assert_eq!(
            fan_art["pending"].as_array().unwrap().len(),
            art,
            "{fan_art}"
        );
    };
    queues(2, 2, 1).await;
    sqlx::query("UPDATE users SET deleted_at=now() WHERE id=$1")
        .bind(&fan_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    queues(0, 0, 0).await;
    sqlx::query("UPDATE users SET deleted_at=NULL WHERE id=$1")
        .bind(&fan_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    queues(2, 2, 1).await;
}

/// The Studio checklist asks for the Persona-pass steps too, like "Finish your page" on Home.
async fn readiness_has_persona_steps(e: &Env) {
    let (owner_id, owner) = e.user("AuditReadyHost", true).await;
    let done = |key: &'static str| {
        let owner = owner.clone();
        async move {
            let ready = owner.ok("GET", "/api/me/readiness", Value::Null).await;
            ready["steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["key"] == key && s["done"] == true)
        }
    };
    let ready = owner.ok("GET", "/api/me/readiness", Value::Null).await;
    assert_eq!(ready["total"], 12);
    let keys: Vec<&str> = ready["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "avatar",
            "banner",
            "bio",
            "intro",
            "currently",
            "links",
            "song",
            "trailer",
            "about_me",
            "council",
            "showcase",
            "schedule"
        ]
    );
    for key in ["intro", "currently", "about_me", "showcase"] {
        assert!(!done(key).await, "{key} starts undone");
    }

    let rev = owner.ok("GET", "/api/me/header", Value::Null).await["revision"].clone();
    owner
        .ok(
            "PUT",
            "/api/me/header",
            json!({"intro_body": "Hi", "revision": rev}),
        )
        .await;
    assert!(done("intro").await);
    owner
        .ok(
            "PUT",
            "/api/me/persona",
            json!({"currently_verb": "PLAYING", "currently_text": "Celeste"}),
        )
        .await;
    assert!(done("currently").await);
    let rev = owner.ok("GET", "/api/me/page-blocks", Value::Null).await["revision"].clone();
    owner
        .ok("PUT", "/api/me/blocks", json!({"revision": rev, "items": [{"type": "PROMPT", "config": {"question": "hot_take", "answer": "Speedruns are art"}}]}))
        .await;
    assert!(done("about_me").await);
    sqlx::query("INSERT INTO user_cosmetics(user_id,showcase) VALUES($1,ARRAY['first_follow'])")
        .bind(&owner_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    assert!(done("showcase").await);
}
