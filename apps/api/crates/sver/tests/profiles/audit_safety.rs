//! Audit regressions: staff never act on their own standing (docs/ADMIN.md), and the data export
//! leaves out staff internals (docs/PROFILES.md "Reporter feedback"). Call `exercise` from the suite.
use super::*;

pub async fn exercise(e: &Env) {
    e.reset_limits().await;
    let (a_id, _, a) = staff(e, "SelfCheckA").await;
    let (b_id, _, b) = staff(e, "SelfCheckB").await;
    let (_, fan) = e.user("SelfCheckFan", true).await;
    b.ok(
        "PATCH",
        "/api/me/profile",
        json!({"bio": "Synthetic staff bio"}),
    )
    .await;
    fan.ok("POST", "/api/reports", json!({"target_type": "profile", "target_id": "SelfCheckB", "field": "bio", "reason": "spam"}))
        .await;
    let actions = format!("/api/admin/reports/profile/{b_id}/actions");
    // B can't close reports against their own channel.
    assert_eq!(
        b.status(
            "POST",
            &actions,
            json!({"action": "dismiss", "note": "Mine"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    // Another admin may strike B; a severe strike opens a ban review.
    let result = a.ok("POST", &actions, json!({"action": "restrict", "note": "Synthetic restriction", "strike": {"reason": "spam", "severity": "SEVERE"}})).await;
    let strike = result["strike_id"].as_str().unwrap().to_string();

    // Exports: the reporter never sees the action taken; B never sees staff-only strike fields.
    let text = fan
        .ok("GET", "/api/auth/export", Value::Null)
        .await
        .to_string();
    assert!(text.contains("ACTIONED"), "{text}");
    assert!(!text.contains("closed_reason"), "{text}");
    let text = b
        .ok("GET", "/api/auth/export", Value::Null)
        .await
        .to_string();
    assert!(text.contains(&strike), "{text}");
    for hidden in [
        "report_ids",
        "removed_refs",
        "ban_review_open",
        "closed_reason",
    ] {
        assert!(!text.contains(hidden), "export leaks {hidden}");
    }

    // B can't clear their own penalty, strike themselves or decide their own appeal.
    for (method, path, body) in [
        (
            "POST",
            "/api/admin/users/SelfCheckB/restriction/lift",
            json!({"note": "Mine"}),
        ),
        (
            "DELETE",
            "/api/admin/users/SelfCheckB/interim-restriction",
            json!({"note": "Mine"}),
        ),
        (
            "POST",
            "/api/admin/users/SelfCheckB/interim-restriction",
            json!({"hours": 1, "note": "Mine"}),
        ),
        (
            "POST",
            "/api/admin/users/SelfCheckB/strikes",
            json!({"reason": "spam", "severity": "STANDARD", "note": "Mine"}),
        ),
    ] {
        assert_eq!(
            b.status(method, path, body).await,
            StatusCode::FORBIDDEN,
            "{method} {path}"
        );
    }
    b.ok(
        "POST",
        &format!("/api/me/strikes/{strike}/appeal"),
        json!({"body": "Synthetic appeal"}),
    )
    .await;
    let appeal: String = sqlx::query_scalar("SELECT id FROM appeals WHERE strike_id=$1")
        .bind(&strike)
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    let decision = format!("/api/admin/appeals/{appeal}/decision");
    assert_eq!(
        b.status(
            "POST",
            &decision,
            json!({"outcome": "overturned", "staff_note": "Mine"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    // With only the issuer and the appellant left, the issuer may self-review instead of the
    // appeal waiting forever. Other admins are switched off for this step and back on after.
    let others: Vec<String> = sqlx::query_scalar("UPDATE users SET mfa_enabled=false WHERE mfa_enabled AND id IN (SELECT user_id FROM staff_roles WHERE role='admin') AND id<>$1 AND id<>$2 RETURNING id")
        .bind(&a_id)
        .bind(&b_id)
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    let (status, v) = a
        .call(
            "POST",
            &decision,
            json!({"outcome": "upheld", "staff_note": "Only reviewer left"}),
        )
        .await;
    sqlx::query("UPDATE users SET mfa_enabled=true WHERE id=ANY($1)")
        .bind(&others)
        .execute(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        (status, &v["self_review"]),
        (StatusCode::OK, &json!(true)),
        "{v}"
    );
    e.reset_limits().await;
    println!(
        "Passed: staff self-action refusals, appellant excluded from review, export hides staff internals."
    );
}
