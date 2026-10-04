//! The 15-minute sliding staff window: unlock on sign-in or confirmation, extend on each staff
//! action, an 8-hour cap, and authenticator confirmation that never unlocks account security.
use super::Env;
use super::bans::staff;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sver::security as sec;

/// Any gated staff action; a missing case answers 409 once past the window check, 403 before it.
async fn staff_action(e: &Env, token: &str) -> StatusCode {
    call(
        e,
        "POST",
        "/api/admin/integrity/missing-case/decision",
        Some(token),
        json!({"outcome":"dismiss","note":"window check"}),
    )
    .await
    .0
}
async fn set(e: &Env, statement: &'static str) {
    sqlx::query(statement)
        .bind("win-staff")
        .execute(&e.app.db)
        .await
        .unwrap();
}

pub async fn exercise(e: &Env) {
    let admin = staff(e, "win-staff", "WinStaff").await;
    let outsider = person(e, "win-user", "WinUser", true).await;
    let gated = StatusCode::FORBIDDEN;
    let passed = StatusCode::CONFLICT;

    // Fresh sign-in: unlocked.
    assert_eq!(staff_action(e, &admin).await, passed);
    // Signed in 20 minutes ago with no staff activity since: locked.
    set(e, "UPDATE sessions SET authenticated_at=now()-interval '20 minutes', staff_active_at=NULL WHERE user_id=$1").await;
    assert_eq!(staff_action(e, &admin).await, gated);
    // Still working (last staff action 10 minutes ago): the window slides and is extended again.
    set(
        e,
        "UPDATE sessions SET staff_active_at=now()-interval '10 minutes' WHERE user_id=$1",
    )
    .await;
    assert_eq!(staff_action(e, &admin).await, passed);
    let refreshed: bool = sqlx::query_scalar(
        "SELECT staff_active_at>now()-interval '5 seconds' FROM sessions WHERE user_id='win-staff'",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(refreshed, "each staff action extends the window");
    // The 8-hour cap holds even with continuous activity.
    set(e, "UPDATE sessions SET authenticated_at=now()-interval '9 hours', staff_active_at=now()-interval '1 minute' WHERE user_id=$1").await;
    assert_eq!(staff_action(e, &admin).await, gated);

    // Confirming with an authenticator or recovery code unlocks staff tools again...
    let code = "WINDOW-RECOVERY-0001";
    // A real (encrypted) authenticator secret, as accounts have; the bans helper uses a placeholder.
    sqlx::query("UPDATE users SET mfa_secret=$1 WHERE id='win-staff'")
        .bind(sec::seal(&e.app, "totp:win-staff", "JBSWY3DPEHPK3PXP").unwrap())
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO recovery_codes(id,user_id,code_hash) VALUES($1,'win-staff',$2)")
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(sec::digest(&code.replace('-', "")))
        .execute(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/confirm",
            Some(&admin),
            json!({"code":"wrong-code"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/confirm",
            Some(&admin),
            json!({"code":code})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(staff_action(e, &admin).await, passed);
    // ...but is not a fresh primary sign-in for account-security changes.
    let (_, me) = call(e, "GET", "/api/auth/me", Some(&admin), Value::Null).await;
    assert_eq!(me["reauthenticated"], false);
    // Only staff can use the confirmation.
    assert_eq!(
        call(
            e,
            "POST",
            "/api/admin/confirm",
            Some(&outsider),
            json!({"code":code})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    for statement in ["DELETE FROM moderation_actions", "DELETE FROM staff_roles"] {
        e.sql(statement).await;
    }
    e.sql("DELETE FROM users WHERE id LIKE 'win-%'").await;
}
