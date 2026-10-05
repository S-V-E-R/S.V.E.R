//! Module 3 platform account bans. A ban is a staff decision separate from strikes: while it is
//! current the account is restricted (channel hidden, public actions refused, stream revoked), every
//! session is revoked, and a new session can only use security, standing and appeal routes (see
//! `blocked_write`). Each ban gets one appeal within 14 days, reviewed like a strike appeal.
use crate::{
    App, auth,
    profiles::{self, Fail, Res, new_id},
    safety::{self, STANDING_MAIL},
    security as sec, text,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

// A ban is in force while status='ACTIVE' AND (until IS NULL OR until>now()): a timed ban ends
// by database time. The condition is written out in each query so every query stays static SQL.

pub async fn active(db: &mut sqlx::PgConnection, user: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM account_bans WHERE user_id=$1 AND status='ACTIVE' AND (until IS NULL OR until>now()))").bind(user).fetch_one(db).await?)
}

/// Writes a banned account may still make: account security (all of Login), standing and appeals.
fn allowed_while_banned(path: &str) -> bool {
    path.starts_with("/api/auth/")
        || path == "/api/take-it-down"
        || path == "/api/take-it-down/status"
        || path.starts_with("/api/me/strikes/")
        || path.starts_with("/api/me/bans/")
        || path == "/api/me/reports/seen"
        // Turning alerts off is always allowed.
        || path == "/api/notifications/unsubscribe"
        || path == "/api/me/notifications/settings"
        || path == "/api/me/notifications/read"
        || path == "/api/me/push"
}
/// Called by the router for every non-GET request: true when the session belongs to a banned
/// account and the route isn't one it may still use. Password reset or another sign-in method
/// can't get around this, because the check is on the account, not the session.
pub async fn blocked_write(app: &App, path: &str, jar: &CookieJar) -> bool {
    if allowed_while_banned(path) {
        return false;
    }
    let Some(token) = jar.get(app.config.cookie_name()) else {
        return false;
    };
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN account_bans b ON b.user_id=s.user_id WHERE s.token_hash=$1 AND b.status='ACTIVE' AND (b.until IS NULL OR b.until>now()))")
        .bind(sec::digest(token.value()))
        .fetch_one(&app.db)
        .await
        // Fail closed: a database error refuses the write rather than letting a banned account through.
        .unwrap_or(true)
}

fn ban_json(row: &Ban) -> Value {
    json!({"id": row.id, "reason": row.reason, "message_to_user": row.message_to_user,
        "issued_at": row.issued_at, "until": row.until, "status": row.status,
        "appeal_closes_at": row.issued_at + Duration::days(14)})
}
#[derive(sqlx::FromRow)]
struct Ban {
    id: String,
    reason: String,
    message_to_user: String,
    issued_at: DateTime<Utc>,
    until: Option<DateTime<Utc>>,
    status: String,
}

#[derive(Deserialize)]
pub struct Issue {
    reason: String,
    message_to_user: Option<String>,
    staff_note: Option<String>,
    /// Hours for a timed ban; omitted for an indefinite ban.
    hours: Option<i64>,
    /// The strike whose level-three review led to this ban, if any.
    strike_id: Option<String>,
}
/// POST /api/admin/users/{username}/ban
pub async fn issue(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Issue>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    let reason = safety::note(Some(&input.reason), "reason", true)?;
    let message = safety::note(input.message_to_user.as_deref(), "message_to_user", false)?;
    let staff_note = safety::note(input.staff_note.as_deref(), "staff_note", false)?;
    let until = match input.hours {
        None => None,
        Some(h) if (1..=24 * 365).contains(&h) => Some(Utc::now() + Duration::hours(h)),
        Some(_) => {
            return Err(Fail::field(
                "hours",
                "A timed ban lasts 1 hour to 365 days.",
            ));
        }
    };
    let mut tx = app.db.begin().await?;
    let user_id = safety::admin_target(&mut tx, &name).await?;
    // Takes the account lifecycle lock so a concurrent sign-in or stream start can't race the ban.
    auth::stream_owner(&mut tx, &user_id).await?;
    let target = profiles::channel_user_by_id(&mut tx, &user_id)
        .await?
        .ok_or_else(Fail::missing)?;
    if target.internal || user_id == actor.id {
        return Err(Fail::bad("This account can't be banned."));
    }
    let mut conn = app.db.acquire().await?;
    if safety::is_staff(&mut conn, &user_id).await? {
        return Err(Fail::bad(
            "Remove the staff role before banning this account.",
        ));
    }
    let current: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account_bans WHERE user_id=$1 AND status='ACTIVE' AND (until IS NULL OR until>now()))",
    )
    .bind(&user_id)
    .fetch_one(&mut *tx)
    .await?;
    if current {
        return Err(Fail::conflict("This account is already banned."));
    }
    if let Some(strike) = &input.strike_id {
        let owned: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM strikes WHERE id=$1 AND user_id=$2)")
                .bind(strike)
                .bind(&user_id)
                .fetch_one(&mut *tx)
                .await?;
        if !owned {
            return Err(Fail::field(
                "strike_id",
                "That strike isn't on this account.",
            ));
        }
    }
    let id = new_id();
    sqlx::query("INSERT INTO account_bans(id,user_id,reason,message_to_user,staff_note,issued_by,until,strike_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&id).bind(&user_id).bind(&reason).bind(&message).bind(&staff_note).bind(&actor.id).bind(until).bind(&input.strike_id)
        .execute(&mut *tx).await?;
    // Restriction (channel hidden, stream revoked) and every session ends; chat sockets lose their
    // session, so their next send is refused.
    safety::recompute(&mut tx, &user_id).await?;
    auth::invalidate(&mut tx, &user_id, None).await?;
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "account_ban",
        "user",
        &user_id,
        &[],
        &staff_note,
        json!({"ban_id": id, "until": until, "strike_id": input.strike_id}),
        false,
    )
    .await?;
    safety::queue_notice(
        &app,
        &mut tx,
        &user_id,
        "Your S.V.E.R account standing",
        STANDING_MAIL,
    )
    .await?;
    tx.commit().await?;
    safety::log("account_ban", "ok");
    Ok(Json(json!({"id": id, "until": until})))
}

#[derive(Deserialize)]
pub struct Lift {
    staff_note: Option<String>,
}
/// POST /api/admin/bans/{id}/lift: ends a ban without deciding an appeal. Old sessions and stream
/// keys are not restored.
pub async fn lift(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Lift>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    let staff_note = safety::note(input.staff_note.as_deref(), "staff_note", true)?;
    let mut tx = app.db.begin().await?;
    let user_id: String = sqlx::query_scalar(
        "UPDATE account_bans SET status='LIFTED',ended_at=now(),ended_by=$2 WHERE id=$1 AND status='ACTIVE' RETURNING user_id",
    )
    .bind(&id)
    .bind(&actor.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| Fail::conflict("This ban isn't active."))?;
    safety::recompute(&mut tx, &user_id).await?;
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "account_ban_lifted",
        "user",
        &user_id,
        &[],
        &staff_note,
        json!({"ban_id": id}),
        false,
    )
    .await?;
    safety::queue_notice(
        &app,
        &mut tx,
        &user_id,
        "Your S.V.E.R account standing",
        STANDING_MAIL,
    )
    .await?;
    tx.commit().await?;
    safety::log("account_ban_lifted", "ok");
    Ok(Json(json!({"status": "LIFTED"})))
}

/// GET /api/me/bans: the signed-in account's bans, newest first, with appeal state.
pub async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let rows: Vec<Ban> = sqlx::query_as("SELECT id,reason,message_to_user,issued_at,until,status FROM account_bans WHERE user_id=$1 ORDER BY issued_at DESC")
        .bind(&user.id).fetch_all(&app.db).await?;
    let mut out = Vec::new();
    for row in &rows {
        let appeal: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('status',status,'created_at',created_at,'message_to_user',message_to_user) FROM appeals WHERE ban_id=$1")
            .bind(&row.id).fetch_optional(&app.db).await?;
        let mut v = ban_json(row);
        v["current"] = json!(row.status == "ACTIVE" && row.until.is_none_or(|u| u > Utc::now()));
        v["appeal"] = appeal.unwrap_or(Value::Null);
        out.push(v);
    }
    Ok(Json(json!({"bans": out})))
}

#[derive(Deserialize)]
pub struct AppealInput {
    body: String,
    timezone: Option<String>,
}
/// POST /api/me/bans/{id}/appeal: once per ban, within 14 days of issue.
pub async fn appeal(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<AppealInput>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let ban: Ban = sqlx::query_as("SELECT id,reason,message_to_user,issued_at,until,status FROM account_bans WHERE id=$1 AND user_id=$2 FOR UPDATE")
        .bind(&id).bind(&user.id).fetch_optional(&mut *tx).await?.ok_or_else(Fail::missing)?;
    let existing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM appeals WHERE ban_id=$1)")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    if existing {
        return Err(Fail::conflict("You've already appealed this ban."));
    }
    if ban.status != "ACTIVE" {
        return Err(Fail::conflict("This ban is no longer active."));
    }
    let closes = ban.issued_at + Duration::days(14);
    if closes <= Utc::now() {
        return Err(Fail::conflict(format!(
            "The appeal window for this ban closed on {}.",
            safety::date_in(closes, input.timezone.as_deref())
        )));
    }
    let body = text::clean(&input.body, "body", true)?;
    if body.is_empty() || text::count(&body) > 1000 {
        return Err(Fail::field("body", "Appeals can be 1-1000 characters."));
    }
    profiles::rate(&app, format!("appeal:{}", user.id), 5, 86_400).await?;
    sqlx::query("INSERT INTO appeals(id,ban_id,user_id,body) VALUES($1,$2,$3,$4)")
        .bind(new_id())
        .bind(&id)
        .bind(&user.id)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    safety::log("ban_appeal_submitted", "ok");
    Ok(Json(
        json!({"status": "PENDING", "message": "Appeal submitted"}),
    ))
}

/// GET /api/admin/bans: bans in force, pending ban appeals and bans awaiting re-review.
pub async fn admin_list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let _staff = safety::staff(&app, &jar).await?;
    let current: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',b.id,'username',u.username,'reason',b.reason,'issued_at',b.issued_at,'until',b.until,'strike_id',b.strike_id,'review_requested_at',b.review_requested_at,'issued_by',(SELECT username FROM users WHERE id=b.issued_by)) FROM account_bans b JOIN users u ON u.id=b.user_id WHERE b.status='ACTIVE' AND (b.until IS NULL OR b.until>now()) ORDER BY b.review_requested_at DESC NULLS LAST, b.issued_at DESC LIMIT 200")
        .fetch_all(&app.db).await?;
    let appeals: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',a.id,'body',a.body,'created_at',a.created_at,'username',u.username,'ban',jsonb_build_object('id',b.id,'reason',b.reason,'issued_at',b.issued_at,'until',b.until,'staff_note',b.staff_note)) FROM appeals a JOIN account_bans b ON b.id=a.ban_id JOIN users u ON u.id=a.user_id WHERE a.status='PENDING' ORDER BY a.created_at LIMIT 200")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"bans": current, "appeals": appeals})))
}

#[derive(Deserialize)]
pub struct Decision {
    outcome: String,
    staff_note: Option<String>,
    message_to_user: Option<String>,
}
/// POST /api/admin/ban-appeals/{id}/decision: the issuer can't decide their own ban while another
/// MFA admin exists (the same review separation as strike appeals).
pub async fn decide(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    if input.outcome != "upheld" && input.outcome != "overturned" {
        return Err(Fail::field("outcome", "Choose upheld or overturned."));
    }
    let staff_note = safety::note(input.staff_note.as_deref(), "staff_note", true)?;
    let message = safety::note(input.message_to_user.as_deref(), "message_to_user", false)?;
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String, String, String, String)> = sqlx::query_as("SELECT a.status,b.id,b.user_id,b.issued_by,b.status FROM appeals a JOIN account_bans b ON b.id=a.ban_id WHERE a.id=$1 FOR UPDATE OF a,b")
        .bind(&id).fetch_optional(&mut *tx).await?;
    let (status, ban_id, user_id, issued_by, ban_status) = row.ok_or_else(Fail::missing)?;
    if status != "PENDING" {
        return Err(Fail::conflict("This appeal has already been decided."));
    }
    let mut self_review = false;
    if issued_by == actor.id {
        let other: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM staff_roles r JOIN users u ON u.id=r.user_id WHERE r.role='admin' AND u.mfa_enabled AND u.deleted_at IS NULL AND u.id<>$1)").bind(&actor.id).fetch_one(&mut *tx).await?;
        if other {
            safety::log("ban_appeal_decided", "denied");
            return Err(Fail::denied("Another moderator must review this appeal."));
        }
        self_review = true;
    }
    let final_status = if input.outcome == "overturned" {
        "OVERTURNED"
    } else {
        "UPHELD"
    };
    sqlx::query("UPDATE appeals SET status=$2,reviewed_by=$3,reviewed_at=now(),message_to_user=$4,staff_note=$5,self_review=$6 WHERE id=$1")
        .bind(&id).bind(final_status).bind(&actor.id).bind(&message).bind(&staff_note).bind(self_review).execute(&mut *tx).await?;
    if final_status == "OVERTURNED" && ban_status == "ACTIVE" {
        sqlx::query(
            "UPDATE account_bans SET status='OVERTURNED',ended_at=now(),ended_by=$2 WHERE id=$1",
        )
        .bind(&ban_id)
        .bind(&actor.id)
        .execute(&mut *tx)
        .await?;
        safety::recompute(&mut tx, &user_id).await?;
    }
    safety::audit(
        &mut tx,
        Some(&actor.id),
        "ban_appeal_decided",
        "user",
        &user_id,
        &[],
        &staff_note,
        json!({"appeal_id": id, "ban_id": ban_id, "outcome": input.outcome}),
        self_review,
    )
    .await?;
    safety::queue_notice(
        &app,
        &mut tx,
        &user_id,
        "Your S.V.E.R account standing",
        STANDING_MAIL,
    )
    .await?;
    tx.commit().await?;
    safety::log("ban_appeal_decided", &input.outcome);
    Ok(Json(
        json!({"status": final_status, "self_review": self_review}),
    ))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/admin/users/{username}/ban", post(issue))
        .route("/api/admin/bans", get(admin_list))
        .route("/api/admin/bans/{id}/lift", post(lift))
        .route("/api/admin/ban-appeals/{id}/decision", post(decide))
        .route("/api/me/bans", get(mine))
        .route("/api/me/bans/{id}/appeal", post(appeal))
}
