//! Platform account bans: who may ban, what a ban does to sessions and writes, timed expiry,
//! one appeal within 14 days, review separation and lifting.
use super::Env;
use super::chat::{call, id, person};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sver::security as sec;

/// A fresh sign-in after the ban revoked the old sessions.
async fn sign_in(e: &Env, user: &str) -> String {
    let token = sec::token();
    sqlx::query("INSERT INTO sessions(id,user_id,token_hash,auth_version,mfa_verified,user_agent) SELECT $1,id,$2,auth_version,false,'synthetic' FROM users WHERE id=$3")
        .bind(id()).bind(sec::digest(&token)).bind(user).execute(&e.app.db).await.unwrap();
    token
}
async fn staff(e: &Env, user: &str, name: &str) -> String {
    let token = person(e, user, name, true).await;
    for statement in [
        "UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id=$1",
        "UPDATE sessions SET mfa_verified=true WHERE user_id=$1",
        "INSERT INTO staff_roles(user_id,role) VALUES($1,'admin')",
    ] {
        sqlx::query(statement)
            .bind(user)
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    token
}
async fn ban(e: &Env, token: &str, name: &str, body: Value) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        &format!("/api/admin/users/{name}/ban"),
        Some(token),
        body,
    )
    .await
}

pub async fn exercise(e: &Env) {
    let target = person(e, "ban-user", "BanUser", true).await;
    let outsider = person(e, "ban-outsider", "BanOutsider", true).await;
    let admin = staff(e, "ban-staff", "BanStaff").await;
    let mut tx = e.app.db.begin().await.unwrap();
    sver::profiles::ensure_profile(&mut tx, "ban-user")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        call(e, "GET", "/api/channels/BanUser", None, Value::Null)
            .await
            .0,
        StatusCode::OK
    );

    // Only MFA staff ban; staff, internal accounts and yourself can't be banned; a reason is required.
    let reasoned = json!({"reason":"Repeated harassment","message_to_user":"You broke the rules."});
    assert_eq!(
        ban(e, &outsider, "BanUser", reasoned.clone()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ban(e, &admin, "BanUser", json!({"reason":" "})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ban(e, &admin, "BanStaff", reasoned.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ban(e, &admin, "BanUser", json!({"reason":"x","hours":0}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, issued) = ban(e, &admin, "BanUser", reasoned.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let ban_id = issued["id"].as_str().unwrap().to_string();
    assert!(issued["until"].is_null(), "indefinite");
    assert_eq!(
        ban(e, &admin, "BanUser", reasoned.clone()).await.0,
        StatusCode::CONFLICT
    );

    // Every session ends and the channel is hidden.
    assert_eq!(
        call(e, "GET", "/api/auth/me", Some(&target), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(e, "GET", "/api/channels/BanUser", None, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // A new sign-in is a restricted session: public writes refused, standing and appeal allowed.
    let again = sign_in(e, "ban-user").await;
    let (status, refused) = call(
        e,
        "POST",
        "/api/channels/streamer/chat",
        Some(&again),
        json!({"id":id(),"body":"hi"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(refused.to_string().contains("banned"), "{refused}");
    assert_eq!(
        call(e, "PUT", "/api/follows/streamer", Some(&again), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, mine) = call(e, "GET", "/api/me/bans", Some(&again), Value::Null).await;
    assert_eq!(mine["bans"][0]["current"], true);
    assert_eq!(mine["bans"][0]["message_to_user"], "You broke the rules.");
    assert!(
        mine["bans"][0].get("staff_note").is_none(),
        "staff notes stay private"
    );

    // One appeal within 14 days, plain text, 1-1000 characters.
    let appeal = format!("/api/me/bans/{ban_id}/appeal");
    assert_eq!(
        call(e, "POST", &appeal, Some(&again), json!({"body":""}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            &appeal,
            Some(&again),
            json!({"body":"x".repeat(1001)})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            &appeal,
            Some(&outsider),
            json!({"body":"not mine"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            e,
            "POST",
            &appeal,
            Some(&again),
            json!({"body":"Please reconsider."})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "POST", &appeal, Some(&again), json!({"body":"Again."}))
            .await
            .0,
        StatusCode::CONFLICT
    );

    // Review separation: the issuer can't decide while another MFA admin exists.
    let reviewer = staff(e, "ban-reviewer", "BanReviewer").await;
    let (_, queue) = call(e, "GET", "/api/admin/bans", Some(&reviewer), Value::Null).await;
    let appeal_id = queue["appeals"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(queue["bans"][0]["username"], "BanUser");
    let decision = format!("/api/admin/ban-appeals/{appeal_id}/decision");
    let overturn = json!({"outcome":"overturned","staff_note":"Context shows a misunderstanding","message_to_user":"Ban removed."});
    assert_eq!(
        call(e, "POST", &decision, Some(&admin), overturn.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            e,
            "POST",
            &decision,
            Some(&reviewer),
            json!({"outcome":"maybe","staff_note":"x"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(e, "POST", &decision, Some(&reviewer), overturn.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "POST", &decision, Some(&reviewer), overturn)
            .await
            .0,
        StatusCode::CONFLICT
    );
    // Overturned: the channel is back and writes work again on the new session.
    assert_eq!(
        call(e, "GET", "/api/channels/BanUser", None, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(e, "PUT", "/api/follows/streamer", Some(&again), Value::Null)
            .await
            .0,
        StatusCode::OK
    );

    // A timed ban ends by database time without a cleanup job.
    let (status, timed) = ban(
        e,
        &reviewer,
        "BanUser",
        json!({"reason":"Cooling off","hours":24}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(timed["until"].is_string());
    let after = sign_in(e, "ban-user").await;
    assert_eq!(
        call(
            e,
            "DELETE",
            "/api/follows/streamer",
            Some(&after),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    e.sql("UPDATE account_bans SET issued_at=now()-interval '2 days',until=now()-interval '1 second' WHERE status='ACTIVE'").await;
    e.sql(
        "UPDATE profiles SET restricted_until=now()-interval '1 second' WHERE user_id='ban-user'",
    )
    .await;
    assert_eq!(
        call(
            e,
            "DELETE",
            "/api/follows/streamer",
            Some(&after),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );

    // Lifting ends a current ban; lifting twice is refused.
    let (_, lifted) = ban(
        e,
        &reviewer,
        "BanUser",
        json!({"reason":"Third","hours":48}),
    )
    .await;
    let lift = format!("/api/admin/bans/{}/lift", lifted["id"].as_str().unwrap());
    assert_eq!(
        call(e, "POST", &lift, Some(&reviewer), json!({})).await.0,
        StatusCode::BAD_REQUEST,
        "note required"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &lift,
            Some(&reviewer),
            json!({"staff_note":"Resolved"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            e,
            "POST",
            &lift,
            Some(&reviewer),
            json!({"staff_note":"Resolved"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(e, "GET", "/api/channels/BanUser", None, Value::Null)
            .await
            .0,
        StatusCode::OK
    );

    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM moderation_actions WHERE target_id='ban-user' ORDER BY created_at",
    )
    .fetch_all(&e.app.db)
    .await
    .unwrap();
    for action in ["account_ban", "ban_appeal_decided", "account_ban_lifted"] {
        assert!(actions.iter().any(|a| a == action), "{action} audited");
    }

    for statement in [
        "DELETE FROM appeals",
        "DELETE FROM account_bans",
        "DELETE FROM moderation_actions",
        "DELETE FROM follows",
        "DELETE FROM staff_roles",
    ] {
        e.sql(statement).await;
    }
    e.sql("DELETE FROM users WHERE id LIKE 'ban-%'").await;
}
