//! Uses the shared isolated Postgres/media fixtures for Module 3 upload and safety acceptance.
use super::*;
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub async fn exercise(e: &Env, dir: &std::path::Path) {
    e.reset_limits().await;
    let (owner_id, owner) = e.user("EmoteOwner", true).await;
    let (_, other) = e.user("EmoteOther", true).await;
    let (staff_id, _) = e.user("EmoteStaff", true).await;
    sqlx::query("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id=$1")
        .bind(&staff_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO staff_roles(user_id,role) VALUES($1,'admin')")
        .bind(&staff_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    let staff = e.session(&staff_id, true).await;
    let image = png(117, 117);
    let upload = |client: Client, code: String| {
        let image = image.clone();
        async move {
            client
                .upload("/api/me/emotes", &[("code", &code)], Some(&image))
                .await
        }
    };
    assert_eq!(
        upload(e.anon.clone(), "Wave".into()).await.0,
        StatusCode::UNAUTHORIZED
    );
    for bad in [
        "ab",
        "has_space",
        "with space",
        "émoji",
        "a234567890123456789012",
    ] {
        assert_eq!(
            upload(owner.clone(), bad.into()).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for bytes in [b"fake PNG".to_vec(), png(111, 111), png(113, 112)] {
        assert_eq!(
            owner
                .upload("/api/me/emotes", &[("code", "Wave")], Some(&bytes))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        owner
            .upload(
                "/api/me/emotes",
                &[("code", "Wave")],
                Some(&vec![0; 1024 * 1024 + 1])
            )
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    sqlx::query("INSERT INTO chat_settings(channel_id,banned_words,block_links) VALUES($1,ARRAY['blocked'],true)").bind(&owner_id).execute(&e.app.db).await.unwrap();
    assert_eq!(
        upload(owner.clone(), "BlockedCode".into()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        upload(owner.clone(), "example.com".into()).await.0,
        StatusCode::BAD_REQUEST
    );
    e.reset_limits().await;

    // Real socket snapshots and invalidations carry only the current channel's catalog.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut request = format!("ws://{address}/api/chat/ws?channel=EmoteOwner")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", e.app.config.origin.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let first: Value =
        serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(first["emotes"], json!([]));
    let (status, wave) = upload(owner.clone(), "Wave".into()).await;
    assert_eq!(status, StatusCode::OK, "{wave}");
    let event = tokio::time::timeout(std::time::Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let event: Value = serde_json::from_str(event.to_text().unwrap()).unwrap();
    assert_eq!(event["type"], "emotes");
    assert_eq!(event["emotes"][0]["code"], "Wave");
    assert_eq!(
        upload(owner.clone(), "Wave".into()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(upload(owner.clone(), "wave".into()).await.0, StatusCode::OK);
    assert_eq!(
        e.anon
            .ok("GET", "/api/channels/EmoteOther/chat", Value::Null)
            .await["emotes"],
        json!([])
    );
    let id = wave["id"].as_str().unwrap();
    assert_eq!(
        other
            .status("DELETE", &format!("/api/me/emotes/{id}"), Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    let processed = media::process(&image, media::Kind::Emote, None).unwrap();
    for v in &processed.variants {
        assert!(dir.join(&v.key).exists());
        assert_eq!(image::load_from_memory(&v.bytes).unwrap().width(), v.width);
    }

    // Nine slots and two concurrent uploads must leave exactly ten, never eleven.
    for n in 2..9 {
        assert_eq!(
            upload(owner.clone(), format!("Slot{n}")).await.0,
            StatusCode::OK
        );
    }
    let (a, b) = tokio::join!(
        upload(owner.clone(), "LastOne".into()),
        upload(owner.clone(), "LastTwo".into())
    );
    assert!(matches!(
        (a.0, b.0),
        (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
    ));
    assert_eq!(
        owner.ok("GET", "/api/me/emotes", Value::Null).await["items"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    owner
        .ok("DELETE", &format!("/api/me/emotes/{id}"), Value::Null)
        .await;
    media::cleanup(&e.app).await.unwrap();
    assert!(
        dir.join(&processed.variants[0].key).exists(),
        "Deleting one reference preserves other codes using these bytes"
    );
    drop(socket);
    task.abort();

    assert_eq!(
        owner.status("GET", "/api/admin/media", Value::Null).await,
        StatusCode::NOT_FOUND
    );
    let queue = staff.ok("GET", "/api/admin/media", Value::Null).await;
    let target = queue["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["code"] == "wave")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    other.ok("POST","/api/reports",json!({"target_type":"emote","target_id":target,"reason":"other","note":"Synthetic fixture"})).await;
    let action = format!("/api/admin/reports/emote/{target}/actions");
    assert_eq!(
        staff
            .status("POST", &action, json!({"action":"remove_content"}))
            .await,
        StatusCode::BAD_REQUEST
    );
    let result = staff.ok("POST",&action,json!({"action":"remove_content","note":"Synthetic review","strike":{"reason":"other","severity":"STANDARD","message_to_user":"Synthetic"}})).await;
    assert!(result["strike_id"].is_string());
    assert_eq!(result["closed_reports"], 1);
    assert!(
        !sver::emotes::catalog(&e.app, &owner_id)
            .await
            .unwrap()
            .iter()
            .any(|v| v["id"] == target)
    );
    assert_eq!(e.count("SELECT count(*) FROM moderation_actions WHERE target_type='emote' AND action='remove_content'").await,1);
    // Standing's restoration interface revives the original row; user deletion still wins.
    sver::emotes::restore(&mut e.app.db.acquire().await.unwrap(), target, "VISIBLE")
        .await
        .unwrap();
    assert!(
        sver::emotes::catalog(&e.app, &owner_id)
            .await
            .unwrap()
            .iter()
            .any(|v| v["id"] == target)
    );

    // Anonymous removal locates emotes, hides every exact copy, restores on dismissal and
    // permanently removes all channel references on a valid request; reuploads stay blocked.
    let (_, copy_owner) = e.user("EmoteCopy", true).await;
    let (_, copy) = upload(copy_owner.clone(), "Copy".into()).await;
    // A full review page remains navigable even when its images are quarantined.
    for n in 0..5 {
        let (channel, _) = e.user(&format!("EmoteReview{n}"), true).await;
        sqlx::query("INSERT INTO channel_emotes(id,channel_id,code,image_key) SELECT $1||'-'||n,$1,'Review'||n,$2 FROM generate_series(1,10) n")
            .bind(channel).bind(&processed.stored).execute(&e.app.db).await.unwrap();
    }
    let page = staff.ok("GET", "/api/admin/media", Value::Null).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 50);
    let next = page["next_cursor"].as_str().unwrap();
    let next = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", next)
        .finish();
    let more = staff
        .ok("GET", &format!("/api/admin/media?{next}"), Value::Null)
        .await;
    assert!(!more["items"].as_array().unwrap().is_empty());
    assert!(more["next_cursor"].is_null());
    let submit = json!({"name":"Synthetic Requester","email":"emote-request@example.invalid","capacity":"shown","authority":"","locations":[format!("{}/take-it-down?report=emote&id={}",e.app.config.origin,copy["id"].as_str().unwrap())],"description":"Synthetic non-sensitive fixture","good_faith":true,"signature":"Synthetic Requester","signed_on":chrono::Utc::now().date_naive(),"extra":"","turnstile_token":"synthetic"});
    e.reset_limits().await;
    let receipt = e.anon.ok("POST", "/api/take-it-down", submit.clone()).await;
    assert_eq!(receipt["media_hidden"], true);
    let queue = staff.ok("GET", "/api/admin/media", Value::Null).await;
    assert!(
        queue["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["unavailable"] == true && row.get("image").is_none())
    );
    assert!(
        sver::emotes::catalog(&e.app, &owner_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!dir.join(&processed.variants[0].key).exists());
    assert_eq!(
        upload(copy_owner.clone(), "Again".into()).await.0,
        StatusCode::BAD_REQUEST
    );
    staff
        .ok(
            "POST",
            &format!(
                "/api/admin/take-it-down/{}",
                receipt["number"].as_str().unwrap()
            ),
            json!({"action":"dismiss","reason":"Synthetic invalid request"}),
        )
        .await;
    assert!(
        !sver::emotes::catalog(&e.app, &owner_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(dir.join(&processed.variants[0].key).exists());
    let receipt = e.anon.ok("POST", "/api/take-it-down", submit).await;
    staff
        .ok(
            "POST",
            &format!(
                "/api/admin/take-it-down/{}",
                receipt["number"].as_str().unwrap()
            ),
            json!({"action":"remove","reason":"Synthetic valid request"}),
        )
        .await;
    assert_eq!(e.count("SELECT count(*) FROM channel_emotes").await, 0);
    let (_, reupload) = e.user("EmoteReupload", true).await;
    assert_eq!(
        upload(reupload.clone(), "Again".into()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        reupload
            .upload(
                "/api/me/emotes",
                &[("code", "Again")],
                Some(&processed.variants[2].bytes)
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    // A unique image is collected on account erasure through the FK trigger.
    let (erase_id, erase) = e.user("EmoteErase", true).await;
    let image = png(119, 119);
    assert_eq!(
        erase
            .upload("/api/me/emotes", &[("code", "ByeBye")], Some(&image))
            .await
            .0,
        StatusCode::OK
    );
    let key = media::process(&image, media::Kind::Emote, None)
        .unwrap()
        .variants[0]
        .key
        .clone();
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(erase_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    media::cleanup(&e.app).await.unwrap();
    assert!(!dir.join(key).exists());
}
