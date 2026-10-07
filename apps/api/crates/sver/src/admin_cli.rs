//! Operator CLI run on the server:
//! - `sver-admin role grant|revoke <username>`: staff roles are never granted through the web;
//!   every change is audited.
//! - `sver-admin media probe`: writes, reads and deletes one probe object in the configured bucket.
//! - `sver-admin videos probe`: checks private recording storage and multipart cleanup without DB access.
//! - `sver-admin media sync <dir>`: copies every live media object from a filesystem store (the
//!   interim `MEDIA_DIR`) into the configured bucket under the same keys, then verifies sizes.
//!   It only writes keys recorded in `media_objects`, never lists or deletes bucket objects, and
//!   prints aggregate counts only.
use sqlx::Row;
use sver::media::Storage;

fn storage_http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("SVER/2.0")
        .build()
        .map_err(|_| "HTTP client unavailable".into())
}

async fn video_probe(storage: &Storage) -> Result<(), String> {
    let Storage::S3(s3) = storage else {
        return Err("VOD_STORAGE=s3 must be configured for this command".into());
    };
    let http = storage_http()?;
    let prefix = format!("probe/{}", uuid::Uuid::new_v4().simple());
    let segment = format!("{prefix}.ts");
    let mp4 = format!("{prefix}.mp4");
    let aborted = format!("{prefix}-aborted.mp4");
    let mut uploads = Vec::new();
    let mut owns_keys = false;
    let result: Result<(), String> = async {
        for key in [&segment, &mp4, &aborted] {
            if storage
                .head(&http, key)
                .await
                .map_err(|_| "Probe HEAD failed")?
                .is_some()
            {
                return Err("Probe key unexpectedly exists".into());
            }
        }
        owns_keys = true;
        storage
            .put_typed(
                &http,
                &segment,
                b"synthetic segment".to_vec(),
                "video/mp2t",
                "private, no-store",
            )
            .await
            .map_err(|_| "Segment upload failed")?;
        if storage
            .get(&http, &segment)
            .await
            .map_err(|_| "Segment read failed")?
            .as_deref()
            != Some(b"synthetic segment")
        {
            return Err("Segment read mismatch".into());
        }
        // Cross the native uploader's part boundary; no file or real recording is used.
        let body: Vec<u8> = (0..8 * 1024 * 1024 + 1024)
            .map(|n| (n % 251) as u8)
            .collect();
        let upload = storage
            .begin_mp4(&http, &mp4)
            .await
            .map_err(|_| "Multipart initiation failed")?
            .ok_or("Missing upload ID")?;
        uploads.push((mp4.clone(), upload.clone()));
        let bytes = storage
            .finish_mp4(&http, &mp4, Some(&upload), &body[..])
            .await
            .map_err(|_| "Multipart completion failed")?;
        if bytes != body.len() as u64
            || storage
                .head(&http, &mp4)
                .await
                .map_err(|_| "MP4 HEAD failed")?
                != Some(bytes)
        {
            return Err("Multipart size mismatch".into());
        }
        for (start, end) in [
            (0, 31),
            (8 * 1024 * 1024 - 16, 8 * 1024 * 1024 + 15),
            (bytes - 32, bytes - 1),
        ] {
            let read = storage
                .range(&http, &mp4, start, end)
                .await
                .map_err(|_| "MP4 range failed")?;
            if read != body[start as usize..=end as usize] {
                return Err("MP4 range mismatch".into());
            }
        }
        let public = http
            .get(s3.object_url(&mp4))
            .send()
            .await
            .map_err(|_| "Anonymous S3 request failed")?
            .status();
        // Cloudflare R2 answers unsigned requests with 400 (missing authorization), others 401/403.
        if !matches!(public.as_u16(), 400 | 401 | 403) {
            return Err(format!(
                "Anonymous S3 request was not denied as expected (HTTP {})",
                public.as_u16()
            ));
        }
        let upload = storage
            .begin_mp4(&http, &aborted)
            .await
            .map_err(|_| "Abort probe initiation failed")?
            .ok_or("Missing upload ID")?;
        uploads.push((aborted.clone(), upload.clone()));
        storage
            .abort_mp4(&http, &aborted, &upload)
            .await
            .map_err(|_| "Multipart abort failed")?;
        if storage
            .finish_mp4(&http, &aborted, Some(&upload), &b"aborted probe"[..])
            .await
            .is_ok()
        {
            return Err("Aborted multipart upload was still writable".into());
        }
        Ok(())
    }
    .await;
    // Attempt every cleanup even when one request fails. Never touch existing media keys.
    let mut clean = true;
    for (key, upload) in uploads {
        clean &= storage.abort_mp4(&http, &key, &upload).await.is_ok();
    }
    if owns_keys {
        for key in [&segment, &mp4, &aborted] {
            clean &= storage.delete(&http, key).await.is_ok();
            clean &= matches!(storage.head(&http, key).await, Ok(None));
        }
    }
    if !clean {
        return Err(format!("Recording probe cleanup needs retry for {prefix}"));
    }
    result?;
    println!(
        "video_probe segment=ok multipart=ok ranges=ok anonymous_s3_denied=true abort=ok deleted=true"
    );
    Ok(())
}

async fn media(args: &[String]) -> Result<(), String> {
    let config = sver::Config::from_env()?;
    let storage = &config.media.storage;
    if !matches!(storage, Storage::S3(_)) {
        return Err("MEDIA_STORAGE=s3 must be configured for this command".into());
    }
    let http = storage_http()?;
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
    if args == ["videos", "probe"] {
        let config = sver::Config::from_env()?;
        return video_probe(&config.videos.storage).await;
    }
    if args.first().is_some_and(|a| a == "media") {
        return media(&args[1..]).await;
    }
    let usage = "Usage: sver-admin role grant|revoke <username> | media probe | media sync <dir> | videos probe";
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn recording_probe_preserves_collisions_and_attempts_every_cleanup() {
        let collision = Arc::new(AtomicBool::new(true));
        let deletes = Arc::new(AtomicUsize::new(0));
        let router = axum::Router::new().fallback({
            let collision = collision.clone();
            let deletes = deletes.clone();
            move |method: axum::http::Method| {
                let present = collision.load(Ordering::SeqCst);
                let deletes = deletes.clone();
                async move {
                    use axum::http::{Method, StatusCode};
                    if method == Method::HEAD {
                        if present {
                            StatusCode::OK
                        } else {
                            StatusCode::NOT_FOUND
                        }
                    } else {
                        if method == Method::DELETE {
                            deletes.fetch_add(1, Ordering::SeqCst);
                        }
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let storage = Storage::S3(sver::media::S3 {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            bucket: "synthetic".into(),
            region: "auto".into(),
            access_key: "synthetic".into(),
            secret_key: "synthetic".into(),
            prefix: String::new(),
        });
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        assert_eq!(
            video_probe(&storage).await.unwrap_err(),
            "Probe key unexpectedly exists"
        );
        assert_eq!(deletes.load(Ordering::SeqCst), 0);
        collision.store(false, Ordering::SeqCst);
        assert!(
            video_probe(&storage)
                .await
                .unwrap_err()
                .contains("cleanup needs retry")
        );
        assert_eq!(deletes.load(Ordering::SeqCst), 3);
        server.abort();
    }
}
