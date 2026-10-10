//! Username claims (Phase 4 "username negotiation", Joe's decision October 10, 2026): a verified
//! user asks for a username held by an account inactive for 12 months. The holder is emailed and
//! keeps the name by signing in within 30 days; otherwise staff release it, the holder gets a
//! generated name and the requester is renamed (rename rules and holds apply). No money.
use crate::{
    App, jobs,
    profiles::{self, Fail, Res},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};

const WAIT_DAYS: i32 = 30;
/// Inactive: no session seen for 12 months and the account is at least that old.
const INACTIVE: &str = "u.created_at<now()-interval '12 months' AND NOT EXISTS(SELECT 1 FROM sessions s WHERE s.user_id=u.id AND s.last_seen_at>now()-interval '12 months')
    AND NOT EXISTS(SELECT 1 FROM staff_roles r WHERE r.user_id=u.id) AND NOT EXISTS(SELECT 1 FROM profiles p WHERE p.user_id=u.id AND p.internal) AND u.deleted_at IS NULL";

#[derive(Deserialize)]
pub struct Ask {
    username: String,
}
/// POST /api/me/username-claims: asks for an inactive account's username.
async fn ask(State(app): State<App>, jar: CookieJar, Json(input): Json<Ask>) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    if !me.email_verified {
        return Err(Fail::denied("Verify your email address first."));
    }
    let name = input.username.trim().to_string();
    crate::security::validate_username(&name).map_err(|e| Fail::field("username", e.1))?;
    let mut tx = app.db.begin().await?;
    profiles::ensure_unrestricted(&mut tx, &me.id).await?;
    let renamed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM username_history WHERE user_id=$1 AND reason IN ('rename','claim') AND changed_at>now()-interval '60 days')")
        .bind(&me.id).fetch_one(&mut *tx).await?;
    if renamed {
        return Err(Fail::conflict(
            "You changed your username in the last 60 days.",
        ));
    }
    let holder: Option<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT u.id,u.email FROM users u WHERE lower(u.username)=lower($1) AND u.id<>$2 AND {INACTIVE}"
    )))
    .bind(&name).bind(&me.id).fetch_optional(&mut *tx).await?;
    // The same answer whether the name is free, active or not claimable, so accounts can't be probed.
    let Some((holder, email)) = holder else {
        return Err(Fail::conflict(
            "That username can't be claimed. Names held by accounts inactive for 12 months can be.",
        ));
    };
    let id = profiles::new_id();
    sqlx::query(
        "INSERT INTO username_claims(id,handle,holder_id,requester_id) VALUES($1,$2,$3,$4)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&holder)
    .bind(&me.id)
    .execute(&mut *tx)
    .await
    .map_err(|_| Fail::conflict("That name, or you, already has an open claim."))?;
    let body = format!(
        "Someone asked for your S.V.E.R username \"{name}\" because the account hasn't been used in a year.\n\nTo keep it, just sign in at {}/login within {WAIT_DAYS} days. If you don't, the username may be given to them and your account will get a new one; nothing else about your account changes.",
        app.config.origin
    );
    jobs::queue_address(
        &app,
        &mut tx,
        Some(&holder),
        &email,
        "Your S.V.E.R username",
        &body,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id": id, "status": "WAITING"})))
}
/// GET /api/me/username-claims
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let claims: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'username',handle,'status',status,'created_at',created_at,'reason',reason) FROM username_claims WHERE requester_id=$1 ORDER BY created_at DESC LIMIT 20")
        .bind(&me.id).fetch_all(&app.db).await?;
    Ok(Json(json!({"claims": claims})))
}
/// DELETE /api/me/username-claims/{id}
async fn withdraw(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    sqlx::query("UPDATE username_claims SET status='WITHDRAWN',decided_at=now() WHERE id=$1 AND requester_id=$2 AND status IN ('WAITING','READY')")
        .bind(&id).bind(&me.id).execute(&app.db).await?;
    mine(State(app), jar).await
}

/// Hourly: a holder who signed in keeps the name; after 30 days the claim waits for staff.
pub async fn tick(app: &App) -> Res<()> {
    sqlx::query("UPDATE username_claims c SET status='KEPT',decided_at=now(),reason='The holder signed in.' WHERE status IN ('WAITING','READY') AND EXISTS(SELECT 1 FROM sessions s WHERE s.user_id=c.holder_id AND s.last_seen_at>c.created_at)")
        .execute(&app.db).await?;
    sqlx::query("UPDATE username_claims SET status='READY' WHERE status='WAITING' AND created_at<now()-make_interval(days=>$1)")
        .bind(WAIT_DAYS).execute(&app.db).await?;
    Ok(())
}

/// GET /api/admin/username-claims: claims ready for a decision first.
async fn queue(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    crate::safety::staff(&app, &jar).await?;
    let claims: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',c.id,'username',c.handle,'status',c.status,'created_at',c.created_at,'requester',r.username,'reason',c.reason) FROM username_claims c LEFT JOIN users r ON r.id=c.requester_id ORDER BY c.status='READY' DESC,c.created_at DESC LIMIT 100")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"claims": claims})))
}
#[derive(Deserialize)]
pub struct Decision {
    action: String,
    reason: String,
}
/// POST /api/admin/username-claims/{id}: release (rename both accounts) or reject; audited.
async fn decide(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Res<Json<Value>> {
    let actor = crate::safety::staff_write(&app, &jar).await?;
    let reason = crate::text::plain(&input.reason, "reason", 1, 500, 2, false)?;
    let mut tx = app.db.begin().await?;
    let claim: Option<(String, Option<String>, String)> = sqlx::query_as("SELECT handle,holder_id,requester_id FROM username_claims WHERE id=$1 AND status='READY' FOR UPDATE")
        .bind(&id).fetch_optional(&mut *tx).await?;
    let (handle, holder, requester) =
        claim.ok_or_else(|| Fail::conflict("Only a claim past its 30 days can be decided."))?;
    let status = match input.action.as_str() {
        "reject" => "REJECTED",
        "release" => {
            // Re-check under the lock: still the holder's name and still inactive.
            let holder = holder.ok_or_else(Fail::missing)?;
            let still: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT u.username FROM users u WHERE u.id=$1 AND lower(u.username)=lower($2) AND {INACTIVE} FOR UPDATE")))
                .bind(&holder).bind(&handle).fetch_optional(&mut *tx).await?;
            let old = still.ok_or_else(|| {
                Fail::conflict("The holder is no longer inactive or no longer has the name.")
            })?;
            let replacement = format!("user{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
            sqlx::query("UPDATE users SET username=$2 WHERE id=$1")
                .bind(&holder)
                .bind(&replacement)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO username_history(id,user_id,old_username,new_username,reason,note) VALUES($1,$2,$3,$4,'released',$5)")
                .bind(profiles::new_id()).bind(&holder).bind(&old).bind(&replacement).bind(format!("claim {id}")).execute(&mut *tx).await?;
            let mine: String =
                sqlx::query_scalar("SELECT username FROM users WHERE id=$1 FOR UPDATE")
                    .bind(&requester)
                    .fetch_one(&mut *tx)
                    .await?;
            sqlx::query("DELETE FROM username_holds WHERE handle_canonical=lower($1)")
                .bind(&handle)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE users SET username=$2 WHERE id=$1")
                .bind(&requester)
                .bind(&handle)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO username_history(id,user_id,old_username,new_username,reason,note) VALUES($1,$2,$3,$4,'claim',$5)")
                .bind(profiles::new_id()).bind(&requester).bind(&mine).bind(&handle).bind(format!("claim {id}")).execute(&mut *tx).await?;
            // The requester's old name redirects for 30 days, as after any rename.
            sqlx::query("INSERT INTO username_holds(handle_canonical,user_id,released_at,redirect) VALUES(lower($1),$2,now()+interval '30 days',true) ON CONFLICT (handle_canonical) DO UPDATE SET user_id=EXCLUDED.user_id,released_at=EXCLUDED.released_at,redirect=true,created_at=now()")
                .bind(&mine).bind(&requester).execute(&mut *tx).await?;
            "RELEASED"
        }
        _ => return Err(Fail::field("action", "Choose release or reject.")),
    };
    sqlx::query(
        "UPDATE username_claims SET status=$2,decided_at=now(),decided_by=$3,reason=$4 WHERE id=$1",
    )
    .bind(&id)
    .bind(status)
    .bind(&actor.id)
    .bind(&reason)
    .execute(&mut *tx)
    .await?;
    crate::safety::audit(
        &mut tx,
        Some(&actor.id),
        &format!("username_claim_{}", input.action),
        "username_claim",
        &id,
        &[],
        &reason,
        json!({"handle": handle}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"status": status})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/username-claims", get(mine).post(ask))
        .route(
            "/api/me/username-claims/{id}",
            axum::routing::delete(withdraw),
        )
        .route("/api/admin/username-claims", get(queue))
        .route("/api/admin/username-claims/{id}", post(decide))
}
