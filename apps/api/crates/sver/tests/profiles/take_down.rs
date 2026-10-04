use super::*;

fn submission(e: &Env, name: &str) -> Value {
    json!({"name":"Synthetic Requester","email":"requester@example.invalid","capacity":"shown","authority":"","locations":[format!("{}/{name}?report=profile&id={name}&field=avatar",e.app.config.origin)],"description":"Synthetic non-sensitive fixture","good_faith":true,"signature":"Synthetic Requester","signed_on":chrono::Utc::now().date_naive(),"extra":"","turnstile_token":"synthetic"})
}
async fn staff(e: &Env) -> Client {
    let (id, _) = e.user("TidStaff", true).await;
    sqlx::query("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id=$1")
        .bind(&id)
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO staff_roles(user_id,role) VALUES($1,'admin')")
        .bind(&id)
        .execute(&e.app.db)
        .await
        .unwrap();
    e.session(&id, true).await
}
pub async fn exercise(e: &Env, dir: &std::path::Path) {
    let staff = staff(e).await;
    let (owner_id, owner) = e.user("TidOwner", true).await;
    let (_, copy_owner) = e.user("TidCopy", true).await;
    let original = png(1500, 600);
    let (s, _) = owner.upload("/api/me/avatar", &[], Some(&original)).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = copy_owner
        .upload("/api/me/banner", &[], Some(&original))
        .await;
    assert_eq!(s, StatusCode::OK);
    let processed = media::process(&original, media::Kind::Avatar, None).unwrap();
    let key = &processed.variants[0].key;
    assert!(dir.join(key).exists());
    let valid = submission(e, "TidOwner");
    let mut invalid = valid.clone();
    invalid["good_faith"] = json!(false);
    assert_eq!(
        e.anon.status("POST", "/api/take-it-down", invalid).await,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = valid.clone();
    invalid["turnstile_token"] = json!("");
    assert_eq!(
        e.anon.status("POST", "/api/take-it-down", invalid).await,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = valid.clone();
    invalid["locations"] = json!(["https://sver.tv.evil.invalid/image"]);
    assert_eq!(
        e.anon.status("POST", "/api/take-it-down", invalid).await,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = valid.clone();
    invalid["capacity"] = json!("authorized");
    assert_eq!(
        e.anon.status("POST", "/api/take-it-down", invalid).await,
        StatusCode::BAD_REQUEST
    );
    let receipt = e.anon.ok("POST", "/api/take-it-down", valid.clone()).await;
    let number = receipt["number"].as_str().unwrap();
    assert!(number.starts_with("TID-"));
    assert_eq!(receipt["media_hidden"], true);
    assert!(!dir.join(key).exists());
    assert!(
        e.count("SELECT count(*) FROM media_removal_holds").await >= 2,
        "The same source under a different media kind is hidden too"
    );
    let saved: String = sqlx::query_scalar("SELECT saved FROM media_removal_holds WHERE root=$1")
        .bind(&processed.stored)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(saved.starts_with("v1."));
    assert_eq!(
        owner.upload("/api/me/avatar", &[], Some(&original)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        e.anon
            .status("GET", "/api/admin/take-it-down", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        owner
            .status("GET", "/api/admin/take-it-down", Value::Null)
            .await,
        StatusCode::NOT_FOUND
    );
    let queue = staff
        .ok("GET", "/api/admin/take-it-down", Value::Null)
        .await;
    let received =
        chrono::DateTime::parse_from_rfc3339(queue["requests"][0]["received_at"].as_str().unwrap())
            .unwrap();
    let deadline =
        chrono::DateTime::parse_from_rfc3339(queue["requests"][0]["deadline"].as_str().unwrap())
            .unwrap();
    assert_eq!((deadline - received).num_hours(), 48);
    let lookup =
        json!({"number":number,"email":"requester@example.invalid","turnstile_token":"synthetic"});
    assert_eq!(
        e.anon
            .ok("POST", "/api/take-it-down/status", lookup.clone())
            .await["status"],
        "received"
    );
    let mut wrong = lookup.clone();
    wrong["email"] = json!("wrong@example.invalid");
    assert_eq!(
        e.anon
            .status("POST", "/api/take-it-down/status", wrong)
            .await,
        StatusCode::BAD_REQUEST
    );
    // A second independent request holds the same bytes until both are dismissed.
    let second = e.anon.ok("POST", "/api/take-it-down", valid.clone()).await;
    staff
        .ok(
            "POST",
            &format!("/api/admin/take-it-down/{number}"),
            json!({"action":"dismiss","reason":"Synthetic invalid report"}),
        )
        .await;
    assert!(!dir.join(key).exists());
    staff
        .ok(
            "POST",
            &format!(
                "/api/admin/take-it-down/{}",
                second["number"].as_str().unwrap()
            ),
            json!({"action":"dismiss","reason":"Synthetic invalid report"}),
        )
        .await;
    assert_eq!(
        std::fs::read(dir.join(key)).unwrap(),
        processed.variants[0].bytes
    );
    assert_eq!(
        e.count("SELECT count(*) FROM strikes WHERE severity='SEVERE'")
            .await,
        0
    );
    assert_eq!(
        e.anon.ok("POST", "/api/take-it-down/status", lookup).await["status"],
        "not_removed"
    );
    e.reset_limits().await;
    let receipt = e.anon.ok("POST", "/api/take-it-down", valid).await;
    let number = receipt["number"].as_str().unwrap();
    staff
        .ok(
            "POST",
            &format!("/api/admin/take-it-down/{number}"),
            json!({"action":"review","reason":"Reviewing the provided location"}),
        )
        .await;
    staff
        .ok(
            "POST",
            &format!("/api/admin/take-it-down/{number}"),
            json!({"action":"remove","reason":"Synthetic valid request"}),
        )
        .await;
    assert!(!dir.join(key).exists());
    assert_eq!(
        e.count("SELECT count(*) FROM media_removal_holds WHERE permanent AND saved IS NOT NULL")
            .await,
        0
    );
    assert_eq!(
        e.count("SELECT count(*) FROM strikes WHERE severity='SEVERE' AND ban_review_open")
            .await,
        2
    );
    let (_, new_user) = e.user("TidReupload", true).await;
    assert_eq!(
        new_user
            .upload("/api/me/avatar", &[], Some(&original))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        new_user
            .upload(
                "/api/me/avatar",
                &[],
                Some(&processed.variants.last().unwrap().bytes)
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let lookup =
        json!({"number":number,"email":"requester@example.invalid","turnstile_token":"synthetic"});
    let status = e.anon.ok("POST", "/api/take-it-down/status", lookup).await;
    assert_eq!(status["status"], "removed");
    assert!(status["resolved_at"].is_string());
    let record = staff
        .ok(
            "GET",
            &format!("/api/admin/take-it-down/{number}"),
            Value::Null,
        )
        .await;
    assert_eq!(
        record["notices"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["audience"] == "uploader")
            .count(),
        2,
        "Each uploader's notice belongs to the removal record"
    );
    let payloads: Vec<String> =
        sqlx::query_scalar("SELECT payload FROM mail_jobs WHERE user_id IS NULL")
            .fetch_all(&e.app.db)
            .await
            .unwrap();
    assert!(payloads.len() >= 5);
    for payload in payloads {
        let mail = sec::unseal(&e.app, "mail", &payload).unwrap();
        assert!(mail.contains("requester@example.invalid"));
        assert!(!mail.contains("Synthetic non-sensitive fixture"));
    }
    // Account erasure does not erase the anonymous legal request.
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(owner_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    assert_eq!(e.count("SELECT count(*) FROM take_down_requests").await, 3);
    failure_and_preservation(e, &staff, dir).await;
    text_restoration(e, &staff).await;
    notification_delivery(e, &staff).await;
    e.sql("DELETE FROM take_down_requests").await;
    e.sql("DELETE FROM blocked_media_hashes").await;
    e.sql("DELETE FROM media_removal_holds").await;
    e.sql("DELETE FROM users WHERE username LIKE 'Tid%'").await;
    e.sql("DELETE FROM mail_jobs").await;
    e.reset_limits().await;
}

async fn failure_and_preservation(e: &Env, staff: &Client, dir: &std::path::Path) {
    let (_, owner) = e.user("TidPreserve", true).await;
    let bytes = png(1600, 700);
    assert_eq!(
        owner.upload("/api/me/avatar", &[], Some(&bytes)).await.0,
        StatusCode::OK
    );
    let processed = media::process(&bytes, media::Kind::Avatar, None).unwrap();
    let mut config = (*e.app.config).clone();
    config.take_down.purge_url = format!(
        "{}/purge",
        config.turnstile_url.trim_end_matches("/turnstile")
    );
    config.take_down.purge_token = "synthetic-purge-denied".into();
    let bad_app = App::new(e.app.db.clone(), config.clone()).await.unwrap();
    let bad = Client {
        app: sver::router(bad_app),
        ..e.anon.clone()
    };
    let bad_staff = Client {
        app: bad.app.clone(),
        ..staff.clone()
    };
    let receipt = bad
        .ok("POST", "/api/take-it-down", submission(e, "TidPreserve"))
        .await;
    assert_eq!(
        receipt["media_hidden"], false,
        "A failed cache purge must never be reported as complete"
    );
    let number = receipt["number"].as_str().unwrap();
    let path = format!("/api/admin/take-it-down/{number}");
    assert_eq!(
        bad_staff
            .status(
                "POST",
                &path,
                json!({"action":"remove","reason":"Synthetic valid report"})
            )
            .await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM take_down_requests WHERE number=$1 AND resolved_at IS NULL",
    )
    .bind(number)
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(pending, 1);
    assert!(!dir.join(&processed.variants[0].key).exists());
    let original_payload: String =
        sqlx::query_scalar("SELECT saved FROM media_removal_holds WHERE root=$1")
            .bind(&processed.stored)
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert!(
        sec::unseal(&e.app, "media-quarantine", &original_payload)
            .unwrap()
            .contains(&processed.variants[0].key)
    );
    config.take_down.purge_token = "synthetic-purge-ok".into();
    let good_app = App::new(e.app.db.clone(), config).await.unwrap();
    let good_staff = Client {
        app: sver::router(good_app.clone()),
        ..staff.clone()
    };
    assert_eq!(
        good_staff
            .status(
                "POST",
                &path,
                json!({"action":"remove","reason":"Synthetic valid report","minor":true})
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    // Retry the worker after the cache service recovers. It must preserve the original evidence.
    sver::media::removal::quarantine(&good_app, &processed.stored)
        .await
        .unwrap();
    let evidence_path = format!("{path}/media/{}", processed.variants[0].key);
    assert_eq!(
        e.anon.status("GET", &evidence_path, Value::Null).await,
        StatusCode::NOT_FOUND
    );
    let (status, _, headers) = good_staff
        .send(
            good_staff
                .builder("GET", &evidence_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    good_staff
        .ok(
            "POST",
            &path,
            json!({"action":"review","reason":"Synthetic review","minor":true}),
        )
        .await;
    assert_eq!(
        good_staff
            .status(
                "POST",
                &path,
                json!({"action":"remove","reason":"Synthetic valid report","minor":false})
            )
            .await,
        StatusCode::BAD_REQUEST,
        "A subsequent action cannot silently clear a minor preservation flag"
    );
    good_staff.ok("POST",&path,json!({"action":"review","reason":"Synthetic preservation arranged","preservation_reference":"Synthetic preservation record"})).await;
    good_staff
        .ok(
            "POST",
            &path,
            json!({"action":"review","reason":"Synthetic continued review"}),
        )
        .await;
    let record = good_staff.ok("GET", &path, Value::Null).await;
    assert_eq!(
        record["preservation_reference"],
        "Synthetic preservation record"
    );
    assert_eq!(record["minor"], true);
    assert!(
        record["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["detail"] == "Synthetic preservation arranged")
    );
    good_staff
        .ok(
            "POST",
            &path,
            json!({"action":"remove","reason":"Synthetic valid report"}),
        )
        .await;
    let retained:bool=sqlx::query_scalar("SELECT legal_hold AND permanent AND saved IS NOT NULL FROM media_removal_holds WHERE root=$1").bind(&processed.stored).fetch_one(&e.app.db).await.unwrap();
    assert!(retained);
    // A completed minor case retains its legal hold; ordinary closed records expire at three years.
    e.sql("UPDATE take_down_requests SET received_at=now()-interval '4 years' WHERE resolved_at IS NOT NULL").await;
    sver::take_down::tick(&e.app).await.unwrap();
    assert_eq!(e.count("SELECT count(*) FROM take_down_requests").await, 1);
    assert_eq!(
        e.count("SELECT count(*) FROM take_down_requests WHERE minor")
            .await,
        1
    );
    // Reminder cadence does not depend on the success of storage/index work.
    e.reset_limits().await;
    let mut unseen = submission(e, "TidUnknown");
    unseen["description"] = json!("A username and timestamp for staff to locate");
    let receipt = e.anon.ok("POST", "/api/take-it-down", unseen).await;
    let number = receipt["number"].as_str().unwrap();
    sqlx::query("UPDATE take_down_requests SET received_at=now()-interval '25 hours',last_alert_at=now()-interval '2 hours' WHERE number=$1").bind(number).execute(&e.app.db).await.unwrap();
    sver::take_down::tick(&e.app).await.unwrap();
    sver::take_down::tick(&e.app).await.unwrap();
    let reminders:i64=sqlx::query_scalar("SELECT count(*) FROM take_down_events e JOIN take_down_requests r ON r.id=e.request_id WHERE r.number=$1 AND e.action='staff_alert_queued'").bind(number).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(reminders, 2, "One initial alert plus one hourly reminder");
    let events = good_staff.ok("GET", &path, Value::Null).await;
    assert!(
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "evidence_viewed")
    );
}

async fn text_restoration(e: &Env, staff: &Client) {
    let (owner, _) = e.user("TidText", true).await;
    for kind in ["wall_post", "chat_message"] {
        e.reset_limits().await;
        let id = uuid::Uuid::new_v4().to_string();
        let insert = if kind == "wall_post" {
            "INSERT INTO wall_posts(id,wall_owner_id,author_id,body,status) VALUES($1,$2,$2,'Synthetic text link','APPROVED')"
        } else {
            "INSERT INTO chat_messages(id,channel_id,author_id,body) VALUES($1,$2,$2,'Synthetic text link')"
        };
        sqlx::query(insert)
            .bind(&id)
            .bind(&owner)
            .execute(&e.app.db)
            .await
            .unwrap();
        let mut request = submission(e, "TidText");
        request["locations"] = json!([format!(
            "{}/TidText?report={kind}&id={id}",
            e.app.config.origin
        )]);
        let visibility = if kind == "wall_post" {
            "SELECT status='APPROVED' FROM wall_posts WHERE id=$1"
        } else {
            "SELECT deleted_at IS NULL FROM chat_messages WHERE id=$1"
        };
        let first = e
            .anon
            .ok("POST", "/api/take-it-down", request.clone())
            .await;
        let second = e
            .anon
            .ok("POST", "/api/take-it-down", request.clone())
            .await;
        for (index, receipt) in [first, second].iter().enumerate() {
            staff
                .ok(
                    "POST",
                    &format!(
                        "/api/admin/take-it-down/{}",
                        receipt["number"].as_str().unwrap()
                    ),
                    json!({"action":"dismiss","reason":"Synthetic mistaken request"}),
                )
                .await;
            let visible: bool = sqlx::query_scalar(visibility)
                .bind(&id)
                .fetch_one(&e.app.db)
                .await
                .unwrap();
            assert_eq!(
                visible,
                index == 1,
                "{kind} stays hidden until the last holding request is dismissed"
            );
        }
        let third = e.anon.ok("POST", "/api/take-it-down", request).await;
        let delete = if kind == "wall_post" {
            "UPDATE wall_posts SET deleted_at=clock_timestamp() WHERE id=$1"
        } else {
            "UPDATE chat_messages SET deleted_at=clock_timestamp() WHERE id=$1"
        };
        sqlx::query(delete)
            .bind(&id)
            .execute(&e.app.db)
            .await
            .unwrap();
        staff
            .ok(
                "POST",
                &format!(
                    "/api/admin/take-it-down/{}",
                    third["number"].as_str().unwrap()
                ),
                json!({"action":"dismiss","reason":"Synthetic mistaken request"}),
            )
            .await;
        let deleted = if kind == "wall_post" {
            "SELECT deleted_at IS NOT NULL FROM wall_posts WHERE id=$1"
        } else {
            "SELECT deleted_at IS NOT NULL FROM chat_messages WHERE id=$1"
        };
        assert!(
            sqlx::query_scalar::<_, bool>(deleted)
                .bind(&id)
                .fetch_one(&e.app.db)
                .await
                .unwrap(),
            "Dismissal must not reverse a later deletion of {kind}"
        );
    }
}

async fn notification_delivery(e: &Env, staff: &Client) {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use web_push_native::p256::{SecretKey, elliptic_curve::sec1::ToEncodedPoint};
    e.reset_limits().await;
    let (reset_id, _) = e.user("TidReset", true).await;
    let user: sver::auth::User = sqlx::query_as("SELECT * FROM users WHERE id=$1")
        .bind(&reset_id)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    let mut tx = e.app.db.begin().await.unwrap();
    sver::jobs::queue_email(&e.app, &mut tx, &user, "verify")
        .await
        .unwrap();
    let urgent = sver::jobs::queue_address(
        &e.app,
        &mut tx,
        Some(&reset_id),
        &user.email,
        "Synthetic removal notice",
        "Synthetic case receipt",
    )
    .await
    .unwrap();
    sver::auth::invalidate(&mut tx, &reset_id, None)
        .await
        .unwrap();
    let kept: Vec<String> = sqlx::query_scalar("SELECT id FROM mail_jobs WHERE user_id=$1")
        .bind(&reset_id)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        kept,
        vec![urgent],
        "Security resets cancel old login links and keep queued removal notices"
    );
    tx.commit().await.unwrap();
    let mut config = (*e.app.config).clone();
    config.staff_push.private_key = URL_SAFE_NO_PAD.encode([8; 32]);
    config.staff_push.public_key = URL_SAFE_NO_PAD.encode(
        SecretKey::from_slice(&[8; 32])
            .unwrap()
            .public_key()
            .to_encoded_point(false),
    );
    let push_app = App::new(e.app.db.clone(), config.clone()).await.unwrap();
    let push_staff = Client {
        app: sver::router(push_app),
        ..staff.clone()
    };
    let mut subscription = json!({"endpoint":"https://fcm.googleapis.com/synthetic-tid-subscription","keys":{"p256dh":URL_SAFE_NO_PAD.encode(SecretKey::from_slice(&[7;32]).unwrap().public_key().to_encoded_point(false)),"auth":URL_SAFE_NO_PAD.encode([9;16])}});
    assert_eq!(
        e.anon
            .status("POST", "/api/admin/push", subscription.clone())
            .await,
        StatusCode::NOT_FOUND
    );
    subscription["endpoint"] = json!("https://127.0.0.1/private");
    assert_eq!(
        push_staff
            .status("POST", "/api/admin/push", subscription.clone())
            .await,
        StatusCode::BAD_REQUEST
    );
    subscription["endpoint"] = json!("https://fcm.googleapis.com/synthetic-tid-subscription");
    push_staff.ok("POST", "/api/admin/push", subscription).await;
    let receipt = e
        .anon
        .ok("POST", "/api/take-it-down", submission(e, "TidUnknown"))
        .await;
    let number = receipt["number"].as_str().unwrap();
    let path = format!("/api/admin/take-it-down/{number}");
    assert_eq!(
        e.anon.status("GET", &path, Value::Null).await,
        StatusCode::NOT_FOUND
    );
    let record = staff.ok("GET", &path, Value::Null).await;
    assert!(
        record["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["channel"] == "push" && n["state"] == "queued")
    );
    let request_id: i64 = sqlx::query_scalar("SELECT id FROM take_down_requests WHERE number=$1")
        .bind(number)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    let mail: String = sqlx::query_scalar(
        "SELECT mail_id FROM take_down_deliveries WHERE request_id=$1 AND audience='requester'",
    )
    .bind(request_id)
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    let staff_mail:String=sqlx::query_scalar("SELECT mail_id FROM take_down_deliveries WHERE request_id=$1 AND audience='staff' AND channel='email'").bind(request_id).fetch_one(&e.app.db).await.unwrap();
    let reliable:i64=sqlx::query_scalar("SELECT count(*) FROM mail_jobs WHERE id=ANY($1) AND retry_until_expiry AND expires_at>=created_at+interval '7 days'").bind(vec![&mail,&staff_mail]).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(
        reliable, 2,
        "Requester and staff mail both survive the normal login retry limit"
    );

    // Queue a normal login email alongside the urgent notice. Only the urgent one retries past ten.
    let user: sver::auth::User = sqlx::query_as("SELECT * FROM users WHERE username='TidStaff'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    sver::jobs::queue_email(
        &e.app,
        &mut e.app.db.acquire().await.unwrap(),
        &user,
        "verify",
    )
    .await
    .unwrap();
    let login:String=sqlx::query_scalar("SELECT id FROM mail_jobs WHERE user_id=$1 AND NOT retry_until_expiry ORDER BY created_at DESC LIMIT 1").bind(&user.id).fetch_one(&e.app.db).await.unwrap();
    e.sql("UPDATE mail_jobs SET available_at=now()+interval '1 day'")
        .await;
    sqlx::query(
        "UPDATE mail_jobs SET attempts=10,available_at=now()-interval '1 minute' WHERE id=ANY($1)",
    )
    .bind(vec![&mail, &login])
    .execute(&e.app.db)
    .await
    .unwrap();
    // No push request ever leaves this test; expiry runs even when delivery is unconfigured.
    config.staff_push = Default::default();
    config.resend_url = format!(
        "{}/mail",
        config.turnstile_url.trim_end_matches("/turnstile")
    );
    config.resend_key = "synthetic-mail-failure".into();
    let failing = App::new(e.app.db.clone(), config.clone()).await.unwrap();
    sver::jobs::tick(&failing).await.unwrap();
    let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM mail_jobs WHERE id=$1")
        .bind(&mail)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(attempts, 11);
    let login_attempts: i32 = sqlx::query_scalar("SELECT attempts FROM mail_jobs WHERE id=$1")
        .bind(&login)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(login_attempts, 10);
    let record = staff.ok("GET", &path, Value::Null).await;
    assert!(
        record["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["audience"] == "requester"
                && n["state"] == "retrying"
                && n["attempts"] == 1)
    );

    config.resend_key = "synthetic-mail-ok".into();
    let recovered = App::new(e.app.db.clone(), config).await.unwrap();
    sqlx::query("UPDATE mail_jobs SET available_at=now() WHERE id=$1")
        .bind(&mail)
        .execute(&e.app.db)
        .await
        .unwrap();
    sver::jobs::tick(&recovered).await.unwrap();
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mail_jobs WHERE id=$1)")
        .bind(&mail)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert!(!exists);
    sqlx::query("UPDATE mail_jobs SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(&staff_mail)
        .execute(&e.app.db)
        .await
        .unwrap();
    e.sql("UPDATE staff_push_jobs SET created_at=now()-interval '3 days'")
        .await;
    sver::jobs::tick(&recovered).await.unwrap();
    assert_eq!(e.count("SELECT count(*) FROM staff_push_jobs").await, 0);
    let record = staff.ok("GET", &path, Value::Null).await;
    let notices = record["notices"].as_array().unwrap();
    assert!(notices.iter().any(|n| n["audience"] == "requester"
        && n["state"] == "accepted"
        && n["accepted_at"].is_string()
        && n["attempts"] == 2));
    assert!(notices.iter().any(|n| n["audience"] == "staff"
        && n["channel"] == "email"
        && n["state"] == "expired"
        && n["attempts"] == 0));
    assert!(
        notices
            .iter()
            .any(|n| n["channel"] == "push" && n["state"] == "expired" && n["attempts"] == 0)
    );
    assert!(
        record["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["action"] == "requester_email_accepted")
    );
}
