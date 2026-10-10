//! Username claims (Phase 4): only names of accounts inactive for 12 months, the holder keeps the
//! name by signing in, staff release it after 30 days and both accounts are renamed and audited.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

pub async fn exercise(e: &Env) {
    let new = person(e, "uc-new", "ClaimNew", true).await;
    let other = person(e, "uc-other", "ClaimOther", true).await;
    person(e, "uc-old", "OldName", true).await;
    person(e, "uc-back", "BackName", true).await;
    person(e, "uc-active", "ActiveName", true).await;
    let staff = person(e, "uc-staff", "ClaimStaff", true).await;
    e.sql("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id='uc-staff'")
        .await;
    e.sql("UPDATE sessions SET mfa_verified=true WHERE user_id='uc-staff'")
        .await;
    e.sql("INSERT INTO staff_roles(user_id,role) VALUES('uc-staff','admin')")
        .await;
    // Two inactive holders: old accounts with no session for a year.
    e.sql("UPDATE users SET created_at=now()-interval '2 years' WHERE id IN ('uc-old','uc-back')")
        .await;
    e.sql("DELETE FROM sessions WHERE user_id IN ('uc-old','uc-back')")
        .await;
    let claim = |token: &str, name: &str| {
        let (token, name) = (token.to_string(), name.to_string());
        async move {
            call(
                e,
                "POST",
                "/api/me/username-claims",
                Some(&token),
                json!({"username": name}),
            )
            .await
        }
    };
    assert_eq!(
        claim(&new, "ActiveName").await.0,
        StatusCode::CONFLICT,
        "active accounts keep their names"
    );
    person(e, "uc-internal", "InternalName", true).await;
    e.sql("UPDATE users SET created_at=now()-interval '2 years' WHERE id='uc-internal'")
        .await;
    e.sql("DELETE FROM sessions WHERE user_id='uc-internal'")
        .await;
    e.sql("INSERT INTO profiles(user_id,display_name,internal) VALUES('uc-internal','InternalName',true) ON CONFLICT (user_id) DO UPDATE SET internal=true")
        .await;
    assert_eq!(
        claim(&new, "InternalName").await.0,
        StatusCode::CONFLICT,
        "internal accounts keep their names"
    );
    let (status, made) = claim(&new, "OldName").await;
    assert_eq!(status, StatusCode::OK, "{made}");
    assert_eq!(
        claim(&other, "OldName").await.0,
        StatusCode::CONFLICT,
        "one open claim per name"
    );
    let mailed: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mail_jobs WHERE user_id='uc-old')")
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    assert!(mailed, "the holder is told");
    let (_, kept) = claim(&other, "BackName").await;
    let kept = kept["id"].as_str().unwrap().to_string();

    // Before 30 days nothing can be decided; the holder of BackName comes back and keeps it.
    let id = made["id"].as_str().unwrap().to_string();
    let decide = |claim: String, action: &'static str| {
        let token = staff.clone();
        async move {
            call(
                e,
                "POST",
                &format!("/api/admin/username-claims/{claim}"),
                Some(&token),
                json!({"action": action, "reason": "Synthetic decision"}),
            )
            .await
            .0
        }
    };
    assert_eq!(decide(id.clone(), "release").await, StatusCode::CONFLICT);
    person(e, "uc-back-session", "Unused", true).await;
    e.sql("UPDATE sessions SET user_id='uc-back' WHERE user_id='uc-back-session'")
        .await;
    e.sql("UPDATE username_claims SET created_at=now()-interval '31 days'")
        .await;
    sver::username_claims::tick(&e.app).await.unwrap();
    let status = |claim: String| async move {
        sqlx::query_scalar::<_, String>("SELECT status FROM username_claims WHERE id=$1")
            .bind(claim)
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    assert_eq!(status(kept).await, "KEPT");
    assert_eq!(status(id.clone()).await, "READY");

    // Release: the requester gets the name, the holder a generated one, the old name redirects.
    assert_eq!(decide(id.clone(), "release").await, StatusCode::OK);
    let names: Vec<(String, String)> =
        sqlx::query_as("SELECT id,username FROM users WHERE id IN ('uc-new','uc-old') ORDER BY id")
            .fetch_all(&e.app.db)
            .await
            .unwrap();
    assert_eq!(names[0], ("uc-new".into(), "OldName".into()));
    assert!(names[1].1.starts_with("user"), "{names:?}");
    let held: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM username_holds WHERE handle_canonical='claimnew' AND user_id='uc-new')").fetch_one(&e.app.db).await.unwrap();
    assert!(held);
    let audited: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM moderation_actions WHERE action='username_claim_release')",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(audited);
    assert_eq!(
        call(
            e,
            "GET",
            "/api/admin/username-claims",
            Some(&new),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "staff only"
    );
}
