use crate::{App, Error, Result, auth::User, security as sec};
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};

pub async fn queue_email(app: &App, db: &mut PgConnection, user: &User, kind: &str) -> Result<()> {
    let token = sec::token();
    let hours = if kind == "verify" { 24.0 } else { 1.0 };
    let page = if kind == "verify" { "verify" } else { "reset" };
    let subject = if kind == "verify" {
        "Verify your S.V.E.R email"
    } else {
        "Reset your S.V.E.R password"
    };
    sqlx::query("DELETE FROM challenges WHERE user_id=$1 AND kind=$2")
        .bind(&user.id)
        .bind(kind)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT INTO challenges(token_hash,user_id,kind,auth_version,expires_at) VALUES($1,$2,$3,$4,now()+make_interval(hours=>$5))")
        .bind(sec::digest(&token)).bind(&user.id).bind(kind).bind(user.auth_version).bind(hours as i32).execute(&mut *db).await?;
    let payload = json!({"to":[user.email],"subject":subject,"text":format!("{subject}\n\n{}/{page}#token={token}\n\nThis link expires in {hours} hour(s). If you did not request this, ignore this email.",app.config.origin)});
    sqlx::query("INSERT INTO mail_jobs(id,user_id,payload,expires_at) VALUES($1,$2,$3,now()+make_interval(hours=>$4))")
        .bind(uuid::Uuid::new_v4().to_string()).bind(&user.id).bind(sec::seal(app,"mail",&payload.to_string())?).bind(hours as i32).execute(db).await?;
    Ok(())
}
#[derive(FromRow)]
struct Mail {
    id: String,
    payload: String,
    attempts: i32,
    retry_until_expiry: bool,
}
/// Transactional mail for a contact who may not have a S.V.E.R account.
pub async fn queue_address(
    app: &App,
    db: &mut PgConnection,
    user_id: Option<&str>,
    email: &str,
    subject: &str,
    body: &str,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let payload = json!({"to":[email],"subject":subject,"text":body});
    sqlx::query("INSERT INTO mail_jobs(id,user_id,payload,expires_at,retry_until_expiry) VALUES($1,$2,$3,now()+interval '7 days',true)")
        .bind(&id).bind(user_id).bind(sec::seal(app,"mail",&payload.to_string())?).execute(db).await?;
    Ok(id)
}
/// Existing account notices keep their normal retry budget unless a removal case owns them.
pub async fn removal_notice(db: &mut PgConnection, id: &str) -> Result<()> {
    sqlx::query("UPDATE mail_jobs SET retry_until_expiry=true,expires_at=greatest(expires_at,created_at+interval '7 days') WHERE id=$1")
        .bind(id).execute(db).await?;
    Ok(())
}
pub async fn tick(app: &App) -> Result<()> {
    if crate::factions::tick(app).await.is_err() {
        eprintln!("faction_event=checkpoint outcome=retry");
    }
    // Deliver urgent notices before historical media indexing or other maintenance work.
    if crate::staff_push::tick(app).await.is_err() {
        eprintln!("staff_push_event=delivery outcome=retry");
    }
    if crate::alerts::fan_out(app).await.is_err() {
        eprintln!("alerts_event=fan_out outcome=retry");
    }
    deliver_mail(app).await?;
    if crate::videos::copyright::tick(app).await.is_err() {
        eprintln!("copyright_event=maintenance outcome=retry");
    }
    if crate::alerts::deliver(app).await.is_err() {
        eprintln!("alerts_event=push outcome=retry");
    }
    if crate::take_down::tick(app).await.is_err() {
        eprintln!("take_down_event=maintenance outcome=retry");
    }
    if crate::integrity::tick(app).await.is_err() {
        eprintln!("integrity_event=maintenance outcome=retry");
    }
    if crate::subs::remind(app).await.is_err() {
        eprintln!("subs_event=reminder outcome=retry");
    }
    if crate::boards::tick(app).await.is_err() {
        eprintln!("board_event=release outcome=retry");
    }
    if crate::plays::tick(app).await.is_err() {
        eprintln!("plays_event=health outcome=retry");
    }
    // Only this rebuild database is touched. Future modules extend the user FK erasure policy.
    // Module 2 erasure steps run first so holds, report closures and counts are kept consistent.
    if crate::squads::tick(app).await.is_err() {
        eprintln!("squad maintenance failed; retrying next pass");
    }
    if crate::guilds::tick(app).await.is_err() {
        eprintln!("guild_event=maintenance outcome=retry");
    }
    crate::profile_jobs::tick(app).await?;
    sqlx::query(
        "DELETE FROM users WHERE deleted_at<=now()-interval '14 days' AND NOT legacy_deletion_hold",
    )
    .execute(&app.db)
    .await?;
    let mut tx = app.db.begin().await?;
    let expired: Vec<String> =
        sqlx::query_scalar("DELETE FROM mail_jobs WHERE expires_at<=now() RETURNING id")
            .fetch_all(&mut *tx)
            .await?;
    for id in expired {
        crate::take_down::notice_result(&mut tx, &id, "expired", false).await?;
        crate::videos::copyright::mail_result(&mut tx, &id, "expired").await?;
    }
    tx.commit().await?;
    for table in [
        "challenges",
        "oauth_states",
        "oauth_signups",
        "sessions",
        "rate_limits",
    ] {
        // The table comes from the fixed maintenance list above.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE expires_at<=now()"
        )))
        .execute(&app.db)
        .await?;
    }
    crate::chat::expire(app).await?;
    Ok(())
}

async fn deliver_mail(app: &App) -> Result<()> {
    if app.config.resend_key.is_empty() {
        return Ok(());
    }
    for _ in 0..20 {
        let mut tx = app.db.begin().await?;
        let mail:Option<Mail>=sqlx::query_as("SELECT * FROM mail_jobs WHERE available_at<=now() AND (attempts<10 OR retry_until_expiry) AND expires_at>now() ORDER BY retry_until_expiry DESC,available_at FOR UPDATE SKIP LOCKED LIMIT 1").fetch_optional(&mut *tx).await?;
        let Some(mail) = mail else {
            return Ok(());
        };
        let mut payload: Value = serde_json::from_str(&sec::unseal(app, "mail", &mail.payload)?)
            .map_err(|_| Error::internal())?;
        payload["from"] = json!(app.config.mail_from);
        let delivered = app
            .http
            .post(&app.config.resend_url)
            .bearer_auth(&app.config.resend_key)
            .header("Idempotency-Key", &mail.id)
            .json(&payload)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
        let state = if delivered {
            "accepted"
        } else if !mail.retry_until_expiry && mail.attempts >= 9 {
            "failed"
        } else {
            "retrying"
        };
        crate::take_down::notice_result(&mut tx, &mail.id, state, true).await?;
        crate::videos::copyright::mail_result(&mut tx, &mail.id, state).await?;
        if delivered {
            sqlx::query("DELETE FROM mail_jobs WHERE id=$1")
                .bind(mail.id)
                .execute(&mut *tx)
                .await?;
        } else {
            let delay = (30_i32 * 2_i32.pow(mail.attempts.min(6) as u32)).min(1800);
            sqlx::query("UPDATE mail_jobs SET attempts=attempts+1,available_at=now()+make_interval(secs=>$2) WHERE id=$1").bind(mail.id).bind(delay as f64).execute(&mut *tx).await?;
            eprintln!(
                "Email delivery failed; queued for retry (attempt {}).",
                mail.attempts + 1
            );
        }
        tx.commit().await?;
    }
    Ok(())
}
