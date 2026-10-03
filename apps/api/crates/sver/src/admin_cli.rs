//! Operator CLI run on the server:
//! - `sver-admin role grant|revoke <username>`: staff roles are never granted through the web;
//!   every change is audited.
//! - `sver-admin media probe`: writes, reads and deletes one probe object in the configured bucket.
//! - `sver-admin media sync <dir>`: copies every live media object from a filesystem store (the
//!   interim `MEDIA_DIR`) into the configured bucket under the same keys, then verifies sizes.
//!   It only writes keys recorded in `media_objects`, never lists or deletes bucket objects, and
//!   prints aggregate counts only.
use sqlx::Row;
use sver::media::Storage;

async fn media(args: &[String]) -> Result<(), String> {
    let config = sver::Config::from_env()?;
    let storage = &config.media.storage;
    if !matches!(storage, Storage::S3(_)) {
        return Err("MEDIA_STORAGE=s3 must be configured for this command".into());
    }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("SVER/2.0")
        .build()
        .map_err(|_| "HTTP client unavailable")?;
    let fail = |what: &str| format!("{what} failed");
    match args {
        [cmd] if cmd == "probe" => {
            let key = format!("probe/{}.webp", uuid::Uuid::new_v4().simple());
            let body = b"sver-media-probe".to_vec();
            if storage
                .head(&http, &key)
                .await
                .map_err(|_| fail("head"))?
                .is_some()
            {
                return Err("probe key unexpectedly exists".into());
            }
            storage
                .put(&http, &key, body.clone())
                .await
                .map_err(|_| fail("put"))?;
            let size = storage.head(&http, &key).await.map_err(|_| fail("head"))?;
            let read = storage.get(&http, &key).await.map_err(|_| fail("get"))?;
            let public = http
                .get(format!("{}/{key}", config.media.public_base))
                .send()
                .await
                .map(|r| r.status().as_u16())
                .unwrap_or(0);
            storage
                .delete(&http, &key)
                .await
                .map_err(|_| fail("delete"))?;
            let gone = storage
                .head(&http, &key)
                .await
                .map_err(|_| fail("head"))?
                .is_none();
            println!(
                "media_probe put=ok head_size_match={} get_match={} public_status={public} deleted={gone}",
                size == Some(body.len() as u64),
                read.as_deref() == Some(&body[..])
            );
            if size == Some(body.len() as u64)
                && read.as_deref() == Some(&body[..])
                && public == 200
                && gone
            {
                Ok(())
            } else {
                Err("media probe failed".into())
            }
        }
        [cmd, dir] if cmd == "sync" => {
            let dir = std::path::Path::new(dir);
            let db = sver::connect(
                &std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?,
            )
            .await?;
            let rows: Vec<(String, i64)> = sqlx::query_as(
                "SELECT key,bytes FROM media_objects WHERE delete_after IS NULL ORDER BY key",
            )
            .fetch_all(&db)
            .await
            .map_err(|_| "Database unavailable")?;
            let (mut copied, mut present, mut missing, mut mismatched) = (0, 0, 0, 0);
            for (key, bytes) in &rows {
                let Ok(data) = tokio::fs::read(dir.join(key)).await else {
                    missing += 1;
                    continue;
                };
                if data.len() as i64 != *bytes {
                    mismatched += 1;
                    continue;
                }
                if storage.head(&http, key).await.map_err(|_| fail("head"))?
                    == Some(data.len() as u64)
                {
                    present += 1;
                    continue;
                }
                storage
                    .put(&http, key, data.clone())
                    .await
                    .map_err(|_| fail("put"))?;
                if storage.head(&http, key).await.map_err(|_| fail("head"))?
                    != Some(data.len() as u64)
                {
                    return Err("an uploaded object failed size verification".into());
                }
                copied += 1;
            }
            println!(
                "media_sync objects={} copied={copied} already_present={present} missing_locally={missing} size_mismatch={mismatched}",
                rows.len()
            );
            if missing + mismatched > 0 {
                return Err("some objects could not be copied".into());
            }
            Ok(())
        }
        _ => Err("Usage: sver-admin media probe | sver-admin media sync <dir>".into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "media") {
        return media(&args[1..]).await;
    }
    let usage = "Usage: sver-admin role grant|revoke <username> | media probe | media sync <dir>";
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
