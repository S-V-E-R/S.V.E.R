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
}
pub async fn tick(app: &App) -> Result<()> {
    // Only this rebuild database is touched. Future modules extend the user FK erasure policy.
    // Module 2 erasure steps run first so holds, report closures and counts are kept consistent.
    crate::profile_jobs::tick(app).await?;
    sqlx::query(
        "DELETE FROM users WHERE deleted_at<=now()-interval '14 days' AND NOT legacy_deletion_hold",
    )
    .execute(&app.db)
    .await?;
    for table in [
        "challenges",
        "oauth_states",
        "oauth_signups",
        "sessions",
        "rate_limits",
        "mail_jobs",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE expires_at<=now()"))
            .execute(&app.db)
            .await?;
    }
    if app.config.resend_key.is_empty() {
        return Ok(());
    }
    for _ in 0..20 {
        let mut tx = app.db.begin().await?;
        let mail:Option<Mail>=sqlx::query_as("SELECT * FROM mail_jobs WHERE available_at<=now() AND attempts<10 AND expires_at>now() ORDER BY available_at FOR UPDATE SKIP LOCKED LIMIT 1").fetch_optional(&mut *tx).await?;
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
        if delivered {
            sqlx::query("DELETE FROM mail_jobs WHERE id=$1")
                .bind(mail.id)
                .execute(&mut *tx)
                .await?;
        } else {
            let delay = (30_i32 * 2_i32.pow(mail.attempts as u32)).min(1800);
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
