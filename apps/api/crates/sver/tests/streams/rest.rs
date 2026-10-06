//! The rest of Module 6 against a synthetic Stripe: creator tiers (every requirement, never down,
//! held by an open integrity case, the split they set, the chat badge), Early Pay (75% cap, once a
//! day, standard and instant with its 1% fee), payday, pooled money in a merged co-stream
//! (tributes, a Valor month, a card first month and its refund), and Shine (charity streams, the
//! label, submission after the stream, staff verify and revoke with audit).
use super::Env;
use super::bans::staff;
use super::chat::{call, id, person};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use std::net::SocketAddr;
use tower::ServiceExt;

async fn event(e: &Env, event: Value) -> StatusCode {
    let body = event.to_string();
    let signature = sver::stripe::sign(
        "whsec_synthetic",
        body.as_bytes(),
        chrono::Utc::now().timestamp(),
    );
    let request = Request::builder()
        .method("POST")
        .uri("/api/stripe/webhook")
        .header("content-type", "application/json")
        .header("stripe-signature", signature)
        .extension(ConnectInfo("127.0.0.1:1".parse::<SocketAddr>().unwrap()))
        .body(Body::from(body))
        .unwrap();
    sver::router(e.app.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}
/// A paid subscription invoice (API version 2025-03-31.basil shape).
fn invoice(id: &str, channel: &str, pool: Option<&str>, cents: i64) -> Value {
    let end = Utc::now().timestamp() + 30 * 86_400;
    let mut meta = json!({"sver_channel": channel, "sver_user": "rt-fan", "sver_tier": "2"});
    if let Some(pool) = pool {
        meta["sver_pool"] = json!(pool);
    }
    json!({"id": format!("evt_{id}"), "type": "invoice.paid", "data": {"object": {
        "id": id, "amount_paid": cents, "billing_reason": "subscription_create",
        "lines": {"data": [{"period": {"end": end}}]},
        "parent": {"subscription_details": {"subscription": format!("sub_{id}"), "metadata": meta}}}}})
}
async fn earnings(e: &Env, user: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(sum(amount),0)::bigint FROM ledger_entries WHERE account=$1",
    )
    .bind(format!("usd:earnings:{user}"))
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}
/// A creator who can earn: verified, authenticator 2FA, Stripe onboarding done.
async fn creator(e: &Env, id: &str, name: &str) -> String {
    let token = person(e, id, name, true).await;
    sqlx::query("UPDATE users SET mfa_enabled=true,mfa_secret='synthetic' WHERE id=$1")
        .bind(id)
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("UPDATE sessions SET mfa_verified=true WHERE user_id=$1")
        .bind(id)
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO payout_accounts(user_id,stripe_account,details_submitted,payouts_enabled) VALUES($1,$2,true,true)")
        .bind(id).bind(format!("acct_{id}")).execute(&e.app.db).await.unwrap();
    token
}
async fn check(e: &Env) -> i16 {
    let mut db = e.app.db.acquire().await.unwrap();
    sver::tiers::check(&mut db, "rt-owner").await.unwrap()
}
fn last_call(e: &Env) -> (String, String) {
    e.fake.lock().unwrap().stripe.last().unwrap().clone()
}

pub async fn exercise(e: &Env) {
    let owner = creator(e, "rt-owner", "RtOwner").await;
    let mate = creator(e, "rt-mate", "RtMate").await;
    let fan = person(e, "rt-fan", "RtFan", true).await;

    // ---- Creator tiers ----
    // Ten 2-hour streams on ten days, 6 trusted viewers on average, 149 followers.
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state,ended_at,end_reason)
        SELECT 'rt-b'||g,'rt-owner','rt-b'||g,1,'ENDED','s','v','c',now()-make_interval(days=>g),now(),now(),'skipped',now()-make_interval(days=>g)+interval '2 hours','test' FROM generate_series(1,10) g").await;
    e.sql("INSERT INTO integrity_snapshots(broadcast_id,taken_at,raw,counted,trusted,excluded,pending) SELECT 'rt-b'||g,now()-make_interval(days=>g),6,6,6,0,0 FROM generate_series(1,10) g").await;
    e.sql("INSERT INTO users(id,email,username,email_verified,date_of_birth) SELECT 'rt-f'||g,'rt-f'||g||'@example.test','RtFollower'||g,true,'1990-01-01' FROM generate_series(1,150) g").await;
    e.sql("INSERT INTO follows(follower_id,following_id) SELECT 'rt-f'||g,'rt-owner' FROM generate_series(1,149) g").await;
    assert_eq!(check(e).await, 0, "149 followers is one short");
    e.sql("INSERT INTO follows(follower_id,following_id) VALUES('rt-f150','rt-owner')")
        .await;
    // An open integrity case holds promotion until it's closed.
    e.sql("INSERT INTO integrity_cases(id,broadcast_id,owner_id,evidence) VALUES('rt-case','rt-b1','rt-owner','{}')").await;
    assert_eq!(check(e).await, 0);
    e.sql("UPDATE integrity_cases SET status='DISMISSED',decided_at=now() WHERE id='rt-case'")
        .await;
    assert_eq!(check(e).await, 1);
    let (_, tier) = call(e, "GET", "/api/me/tier", Some(&owner), Value::Null).await;
    assert_eq!(tier["name"], "Trailblazer");
    assert_eq!(tier["split"], 70);
    assert_eq!(tier["next"]["name"], "Pioneer");
    e.sql("DELETE FROM follows WHERE following_id='rt-owner'")
        .await;
    assert_eq!(check(e).await, 1, "tiers never go down");
    let (status, said) = call(
        e,
        "POST",
        "/api/channels/rtowner/chat",
        Some(&owner),
        json!({"id": id(), "body": "Trailblazer here"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{said}");
    assert_eq!(said["message"]["creator_tier"], 1);
    // The tier sets the subscription split: 70% of $9.99.
    assert_eq!(
        event(e, invoice("in_rt_1", "rt-owner", None, 999)).await,
        StatusCode::OK
    );
    assert_eq!(earnings(e, "rt-owner").await, 6_993);

    // ---- Early Pay and payday ----
    let (_, s) = call(
        e,
        "GET",
        "/api/me/payouts/summary",
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(s["available_cents"], 699);
    assert_eq!(s["early"]["limit_cents"], 524, "75% of what was earned");
    let (status, paid) = call(
        e,
        "POST",
        "/api/me/payouts/early",
        Some(&owner),
        json!({"method": "standard"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paid}");
    let (path, form) = last_call(e);
    assert_eq!(path, "/v1/transfers");
    assert!(
        form.contains("amount=524") && form.contains("destination=acct_rt-owner"),
        "{form}"
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/me/payouts/early",
            Some(&owner),
            json!({"method": "standard"})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "once a day"
    );
    e.sql("UPDATE payout_runs SET created_at=now()-interval '1 day' WHERE user_id='rt-owner'")
        .await;
    assert_eq!(
        event(e, invoice("in_rt_2", "rt-owner", None, 999)).await,
        StatusCode::OK
    );
    // 75% of 13,986 tenths earned, less the 524 already taken early; instant costs 1%.
    let (status, paid) = call(
        e,
        "POST",
        "/api/me/payouts/early",
        Some(&owner),
        json!({"method": "instant"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paid}");
    let (path, form) = last_call(e);
    assert_eq!(path, "/v1/payouts");
    assert!(
        form.contains("method=instant")
            && form.contains("amount=518")
            && form.contains("|account=acct_rt-owner"),
        "{form}"
    );
    assert_eq!(paid["history"][0]["kind"], "early_instant");
    assert_eq!(paid["history"][0]["fee_cents"], 6);
    assert_eq!(earnings(e, "rt-owner").await, 13_986 - 5_240 - 5_240);
    // Payday pays the whole available balance, once per period.
    let period = Utc.with_ymd_and_hms(2026, 10, 9, 16, 0, 0).unwrap();
    sver::payouts::payday(&e.app, period).await.unwrap();
    sver::payouts::payday(&e.app, period).await.unwrap();
    let (_, s) = call(
        e,
        "GET",
        "/api/me/payouts/summary",
        Some(&owner),
        Value::Null,
    )
    .await;
    assert_eq!(s["history"][0]["kind"], "payday");
    assert_eq!(s["history"][0]["amount_cents"], 350);
    assert_eq!(s["history"].as_array().unwrap().len(), 3);
    assert_eq!(s["available_cents"], 0);

    // ---- A merged co-stream pools money among its live members ----
    e.sql("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state)
        VALUES('rt-l1','rt-owner','rt-l1',1,'LIVE','s','v','c',now()-interval '5 minutes',now(),now(),'skipped'),('rt-l2','rt-mate','rt-l2',1,'LIVE','s','v','c',now()-interval '5 minutes',now(),now(),'skipped')").await;
    e.sql("INSERT INTO squads(id,host_id,mode) VALUES('sq-rt','rt-owner','MERGED')")
        .await;
    e.sql("INSERT INTO squad_members(squad_id,user_id,broadcast_id,joined_at) VALUES('sq-rt','rt-owner','rt-l1',now()-interval '2 minutes'),('sq-rt','rt-mate','rt-l2',now()-interval '1 minute')").await;
    let mut tx = e.app.db.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO ledger_transactions(id,kind,reference) VALUES('fund-rt','test','fund-rt')",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO ledger_entries(transaction_id,account,unit,amount) VALUES('fund-rt','valor:rt-fan','valor',2000),('fund-rt','valor:issued','valor',-2000)").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let (owner_before, mate_before) = (earnings(e, "rt-owner").await, earnings(e, "rt-mate").await);
    let (status, sent) = call(
        e,
        "POST",
        "/api/squads/sq-rt/chat",
        Some(&fan),
        json!({"id": id(), "body": "For the squad!", "tribute": 100}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(earnings(e, "rt-owner").await - owner_before, 400);
    assert_eq!(earnings(e, "rt-mate").await - mate_before, 400);
    let (status, subbed) = call(
        e,
        "POST",
        "/api/channels/rtmate/subscription",
        Some(&fan),
        json!({"tier": 1, "pay": "valor", "id": id(), "squad": "sq-rt"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{subbed}");
    assert_eq!(earnings(e, "rt-owner").await - owner_before, 400 + 1_996);
    assert_eq!(earnings(e, "rt-mate").await - mate_before, 400 + 1_996);
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/rtowner/subscription",
            Some(&mate),
            json!({"tier": 1, "pay": "valor", "id": id(), "squad": "sq-rt"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "members can't pay their own squad"
    );
    // A card first month: half each, at each member's own split (70% and 65%); a refund reverses
    // exactly that.
    let (owner_now, mate_now) = (earnings(e, "rt-owner").await, earnings(e, "rt-mate").await);
    let pooled = invoice("in_rt_pool", "rt-mate", Some("rt-owner,rt-mate"), 999);
    assert_eq!(event(e, pooled).await, StatusCode::OK);
    assert_eq!(earnings(e, "rt-owner").await - owner_now, 3_496);
    assert_eq!(earnings(e, "rt-mate").await - mate_now, 3_246);
    let refund = json!({"id": "evt_rt_refund", "type": "charge.refunded", "data": {"object": {
        "payment_intent": "pi_in_rt_pool", "amount": 999, "amount_refunded": 999}}});
    assert_eq!(event(e, refund).await, StatusCode::OK);
    assert_eq!(earnings(e, "rt-owner").await, owner_now);
    assert_eq!(earnings(e, "rt-mate").await, mate_now);

    // ---- Shine ----
    let (status, bad) = call(
        e,
        "PUT",
        "/api/me/shine",
        Some(&owner),
        json!({"charity_name": "Red Cross", "charity_url": "http://redcross.example"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(bad["field"], "charity_url");
    let (status, set) = call(
        e,
        "PUT",
        "/api/me/shine",
        Some(&owner),
        json!({"charity_name": "Red Cross", "charity_url": "https://redcross.example/give"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(
        set["streams"][0]["live"], true,
        "the live stream became a charity stream"
    );
    let charity = set["streams"][0]["id"].as_str().unwrap().to_string();
    {
        let mut db = e.app.db.acquire().await.unwrap();
        let live = sver::discovery::live_streams(&mut db).await.unwrap();
        let card = live.iter().find(|s| s.broadcast_id == "rt-l1").unwrap();
        assert_eq!(card.charity.as_deref(), Some("Red Cross"));
        let channel = sver::shine::channel(&mut db, "rt-owner").await.unwrap();
        assert_eq!(channel["live"]["url"], "https://redcross.example/give");
    }
    let submit_path = format!("/api/me/shine/{charity}/submit");
    let raised = json!({"raised_cents": 12_345, "proof_url": "https://redcross.example/receipt/1"});
    assert_eq!(
        call(e, "POST", &submit_path, Some(&owner), raised.clone())
            .await
            .0,
        StatusCode::CONFLICT,
        "after the stream ends"
    );
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test',reconnect_deadline=NULL WHERE id IN ('rt-l1','rt-l2')").await;
    let (status, submitted) = call(e, "POST", &submit_path, Some(&owner), raised).await;
    assert_eq!(status, StatusCode::OK, "{submitted}");
    assert_eq!(submitted["streams"][0]["status"], "submitted");
    let admin = staff(e, "rt-staff", "RtStaff").await;
    let (_, queue) = call(e, "GET", "/api/admin/shine", Some(&admin), Value::Null).await;
    assert!(
        queue["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == charity)
    );
    let review_path = format!("/api/admin/shine/{charity}");
    assert_eq!(
        call(
            e,
            "POST",
            &review_path,
            Some(&admin),
            json!({"action": "verify"})
        )
        .await
        .0,
        StatusCode::OK
    );
    {
        let mut db = e.app.db.acquire().await.unwrap();
        let channel = sver::shine::channel(&mut db, "rt-owner").await.unwrap();
        assert_eq!(channel["badges"][0]["charity"], "Red Cross");
        assert_eq!(channel["badges"][0]["raised_cents"], 12_345);
    }
    assert_eq!(
        call(
            e,
            "POST",
            &review_path,
            Some(&admin),
            json!({"action": "revoke"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "a reason is required"
    );
    assert_eq!(
        call(
            e,
            "POST",
            &review_path,
            Some(&admin),
            json!({"action": "revoke", "note": "Proof was for another fundraiser"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE target_type='charity_stream' AND target_id=$1")
        .bind(&charity).fetch_one(&e.app.db).await.unwrap();
    assert_eq!(audited, 2);
    let unbalanced: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT unit FROM ledger_entries GROUP BY unit HAVING sum(amount)<>0) u",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(unbalanced, 0);
}
