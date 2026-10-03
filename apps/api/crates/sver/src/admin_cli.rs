//! Operator CLI run on the server: `sver-admin role grant|revoke <username>`.
//! Staff roles are never granted through the web; every change is audited.
use sqlx::Row;

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "Usage: sver-admin role grant|revoke <username>";
    let [scope, verb, username] = args.as_slice() else {
        return Err(usage.into());
    };
    if scope != "role" || (verb != "grant" && verb != "revoke") {
        return Err(usage.into());
    }
    let db = sver::connect(&std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?)
        .await?;
    let mut tx = db.begin().await.map_err(|_| "Database unavailable")?;
    let row = sqlx::query(
        "SELECT id,mfa_enabled FROM users WHERE lower(username)=lower($1) AND deleted_at IS NULL",
    )
    .bind(username)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| "Database unavailable")?
    .ok_or("No active account has that username.")?;
    let id: String = row.get(0);
    let mfa: bool = row.get(1);
    let changed = if verb == "grant" {
        sqlx::query(
            "INSERT INTO staff_roles(user_id,role) VALUES($1,'admin') ON CONFLICT DO NOTHING",
        )
        .bind(&id)
        .execute(&mut *tx)
        .await
    } else {
        sqlx::query("DELETE FROM staff_roles WHERE user_id=$1 AND role='admin'")
            .bind(&id)
            .execute(&mut *tx)
            .await
    }
    .map_err(|_| "Could not change the role")?
    .rows_affected();
    if changed > 0 {
        sqlx::query("INSERT INTO moderation_actions(id,actor_id,action,target_type,target_id,note) VALUES($1,NULL,$2,'user',$3,'operator CLI')")
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(if verb == "grant" { "role_granted" } else { "role_revoked" })
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(|_| "Could not record the change")?;
    }
    tx.commit().await.map_err(|_| "Could not save the change")?;
    println!(
        "mod_event=role_{verb} outcome={}",
        if changed > 0 { "ok" } else { "unchanged" }
    );
    if verb == "grant" && !mfa {
        println!(
            "Note: admin routes stay unavailable until this account enables two-factor authentication."
        );
    }
    Ok(())
}
