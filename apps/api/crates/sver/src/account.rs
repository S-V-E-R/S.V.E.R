//! Account changes while signed in (docs/LOGIN.md "Account changes"): password, email address and
//! a download of your data. Changes need the same step-up as other security changes (a sign-in
//! confirmed in the last five minutes plus a fresh code when 2FA is on), and each one sends a
//! security notice to the account's address.
use crate::{App, Error, Result, auth, jobs, security as sec};
use axum::{
    Json, Router,
    extract::State,
    http::header,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// `j***@example.com`, for notices that mention another address.
fn masked(email: &str) -> String {
    match email.split_once('@') {
        Some((name, domain)) => format!("{}***@{domain}", name.chars().next().unwrap_or('*')),
        None => "***".into(),
    }
}
async fn notice(
    app: &App,
    db: &mut PgConnection,
    user: &auth::User,
    subject: &str,
    what: &str,
) -> Result<()> {
    let body = format!(
        "{subject}\n\nHi {}, {what}\n\nIf this wasn't you, reset your password at {}/forgot and review your devices at {}/account.",
        user.username, app.config.origin, app.config.origin
    );
    jobs::queue_address(app, db, Some(&user.id), &user.email, subject, &body).await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct NewPassword {
    password: String,
    #[serde(default)]
    code: String,
}
/// POST /api/auth/password/change: sets a new password (or adds one to a provider-only account)
/// and signs out every other device.
async fn change_password(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewPassword>,
) -> Result<Json<Value>> {
    // Hash first (it can call the breach check), then take the step-up inside one transaction.
    let hash = sec::new_password(&app, input.password).await?;
    let (mut tx, user, session) = auth::sensitive(&app, &jar, &input.code).await?;
    sqlx::query("UPDATE users SET password_hash=$2 WHERE id=$1")
        .bind(&user.id)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    auth::invalidate(&mut tx, &user.id, Some(&session.id)).await?;
    notice(
        &app,
        &mut tx,
        &user,
        "Your S.V.E.R password was changed",
        "your password was just changed and your other devices were signed out.",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"changed": true})))
}

#[derive(Deserialize)]
pub struct NewEmail {
    email: String,
    #[serde(default)]
    code: String,
}
/// POST /api/auth/email/change: sends a confirmation link to the new address (24 hours). The
/// address changes only when that link is opened; the old address is told either way.
async fn change_email(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewEmail>,
) -> Result<Json<Value>> {
    let email = sec::email(&input.email)?;
    let (mut tx, user, _) = auth::sensitive(&app, &jar, &input.code).await?;
    if email == user.email.to_lowercase() {
        return Err(Error::bad("That's already your email address."));
    }
    sec::reserve(&app, vec![format!("email-change:{}", user.id)], 3, 3600).await?;
    // The same answer whether or not another account has it; the confirm step checks again.
    let taken: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE lower(email)=$1)")
            .bind(&email)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM challenges WHERE user_id=$1 AND kind='email'")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    if !taken {
        let token = sec::token();
        sqlx::query("INSERT INTO challenges(token_hash,user_id,kind,auth_version,payload,expires_at) VALUES($1,$2,'email',$3,$4,now()+interval '24 hours')")
            .bind(sec::digest(&token)).bind(&user.id).bind(user.auth_version).bind(&email).execute(&mut *tx).await?;
        let subject = "Confirm your new S.V.E.R email";
        let body = format!(
            "{subject}\n\n{}/verify#token={token}\n\nOpen this link to make this the email address for @{}. It expires in 24 hours. If you did not request this, ignore this email.",
            app.config.origin, user.username
        );
        jobs::queue_address(&app, &mut tx, Some(&user.id), &email, subject, &body).await?;
    }
    let what = format!(
        "someone signed in as you asked to change your email address to {}. It changes only when the link sent there is opened.",
        masked(&email)
    );
    notice(
        &app,
        &mut tx,
        &user,
        "Email change requested on S.V.E.R",
        &what,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"pending": masked(&email)})))
}
/// DELETE /api/auth/email/change: cancels a pending change.
async fn cancel_email(State(app): State<App>, jar: CookieJar) -> Result<Json<Value>> {
    let (mut tx, user, _) = auth::session(&app, &jar, false).await?;
    sqlx::query("DELETE FROM challenges WHERE user_id=$1 AND kind='email'")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"cancelled": true})))
}
/// The pending new address for /api/auth/me, masked.
pub async fn pending_email(db: &mut PgConnection, user: &str) -> Result<Option<String>> {
    let email: Option<String> = sqlx::query_scalar(
        "SELECT payload FROM challenges WHERE user_id=$1 AND kind='email' AND expires_at>now()",
    )
    .bind(user)
    .fetch_optional(db)
    .await?;
    Ok(email.map(|e| masked(&e)))
}
/// Whether `token` is a pending email-change link (checked before `auth::verify_email` consumes it).
pub async fn is_email_change(app: &App, token: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM challenges WHERE token_hash=$1 AND kind='email')",
    )
    .bind(sec::digest(token))
    .fetch_one(&app.db)
    .await?)
}
/// Opens an email-change link: switches the address (verified, as the link proves the mailbox)
/// and tells the old address.
pub async fn confirm_email(app: &App, token: &str) -> Result<()> {
    if token.len() > 128 {
        return Err(Error::bad("Invalid or expired link."));
    }
    // consume() deletes the challenge, so read the new address first.
    let email: String =
        sqlx::query_scalar("SELECT payload FROM challenges WHERE token_hash=$1 AND kind='email'")
            .bind(sec::digest(token))
            .fetch_optional(&app.db)
            .await?
            .ok_or_else(|| Error::bad("Invalid or expired link."))?;
    let (mut tx, user) = auth::consume(app, token, "email").await?;
    let taken: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE lower(email)=$1 AND id<>$2)")
            .bind(&email)
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    if taken {
        return Err(Error::bad(
            "That email address is already used by another account.",
        ));
    }
    sqlx::query("UPDATE users SET email=$2,email_verified=true WHERE id=$1")
        .bind(&user.id)
        .bind(&email)
        .execute(&mut *tx)
        .await?;
    let what = format!(
        "the email address on your account was changed to {}. Notices go there from now on.",
        masked(&email)
    );
    notice(app, &mut tx, &user, "Your S.V.E.R email was changed", &what).await?;
    tx.commit().await?;
    Ok(())
}

/// Columns that identify the account holder in a row (the export reads rows keyed by these).
const SUBJECT_COLUMNS: [&str; 10] = [
    "user_id",
    "author_id",
    "follower_id",
    "owner_id",
    "raider_id",
    "reporter_id",
    "blocker_id",
    "member_id",
    "actor_id",
    "voter_id",
];
/// Left out: credentials and sessions, anti-abuse signals, staff internals, and tables that hold
/// other people's personal details (copyright claimants, guardians, Take It Down requesters).
const EXCLUDED_TABLES: [&str; 18] = [
    "sessions",
    "challenges",
    "recovery_codes",
    "mail_jobs",
    "oauth_states",
    "rate_limits",
    "stream_credentials",
    "push_subscriptions",
    "staff_push_subscriptions",
    "integrity_cases",
    "magnet_flags",
    "moderation_actions",
    "take_down_events",
    "take_down_targets",
    "copyright_cases",
    "guardian_consents",
    "legacy_account_data",
    "staff_roles",
];
/// Column names never exported (secrets, keys, network data).
const SECRET_COLUMNS: &str = "(hash|secret|token|sealed|password|mfa|seed|fingerprint|payload|endpoint|network|subscriptions|_key$|^key$|^ip$|^ip_|_ip$)";

/// GET /api/auth/export: a JSON file of the account and every row keyed to it (a sign-in confirmed
/// in the last five minutes; three an hour).
async fn export(State(app): State<App>, jar: CookieJar) -> Result<impl IntoResponse> {
    let (tx, user, session) = auth::session(&app, &jar, true).await?;
    tx.commit().await?;
    auth::recent(&session)?;
    sec::reserve(&app, vec![format!("export:{}", user.id)], 3, 3600).await?;
    let mut db = app.db.acquire().await?;
    let account: Value = sqlx::query_scalar("SELECT to_jsonb(u) - ARRAY(SELECT column_name::text FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='users' AND column_name ~ $2) FROM users u WHERE id=$1")
        .bind(&user.id).bind(SECRET_COLUMNS).fetch_one(&mut *db).await?;
    let tables: Vec<(String, String, Vec<String>)> = sqlx::query_as("SELECT c.table_name::text,c.column_name::text,ARRAY(SELECT x.column_name::text FROM information_schema.columns x WHERE x.table_schema=c.table_schema AND x.table_name=c.table_name AND x.column_name ~ $3)
        FROM information_schema.columns c JOIN information_schema.tables t ON t.table_schema=c.table_schema AND t.table_name=c.table_name AND t.table_type='BASE TABLE'
        WHERE c.table_schema=current_schema() AND c.column_name=ANY($1) AND c.table_name<>ALL($2) AND c.table_name NOT LIKE '\\_%' ORDER BY 1,2")
        .bind(&SUBJECT_COLUMNS[..]).bind(&EXCLUDED_TABLES[..]).bind(SECRET_COLUMNS).fetch_all(&mut *db).await?;
    let mut data = serde_json::Map::new();
    for (table, column, hidden) in tables {
        // ponytail: 10,000 rows per table and column; a background export job if anyone exceeds it.
        // Identifiers come from information_schema and are quoted, never from the request.
        let rows: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT coalesce(jsonb_agg(to_jsonb(t) - $2::text[]),'[]') FROM (SELECT * FROM \"{}\" WHERE \"{}\"=$1 LIMIT 10000) t",
            table.replace('"', "\"\""),
            column.replace('"', "\"\"")
        )))
        .bind(&user.id)
        .bind(&hidden)
        .fetch_one(&mut *db)
        .await?;
        if rows.as_array().is_some_and(|r| !r.is_empty()) {
            let key = if data.contains_key(&table) {
                format!("{table}.{column}")
            } else {
                table
            };
            data.insert(key, rows);
        }
    }
    let file = json!({"exported_at": chrono::Utc::now(), "account": account, "data": data});
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"sver-{}.json\"", user.username),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        serde_json::to_string_pretty(&file).map_err(|_| Error::internal())?,
    ))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/auth/password/change", post(change_password))
        .route(
            "/api/auth/email/change",
            post(change_email).delete(cancel_email),
        )
        .route("/api/auth/export", get(export))
}
