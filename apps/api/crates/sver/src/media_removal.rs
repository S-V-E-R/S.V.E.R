//! Reversible storage quarantine and exact-copy blocking, owned by the media module.
use super::*;
use crate::security;
use base64::{Engine, engine::general_purpose::STANDARD};

pub fn fingerprints(bytes: &[u8], image: &DynamicImage) -> Vec<String> {
    let rgba = image.to_rgba8();
    let mut pixels = Sha256::new();
    pixels.update(rgba.width().to_be_bytes());
    pixels.update(rgba.height().to_be_bytes());
    pixels.update(rgba.as_raw());
    vec![hex(&Sha256::digest(bytes)), hex(&pixels.finalize())]
}

pub async fn upload_allowed(db: &mut PgConnection, processed: &Processed) -> Res<()> {
    // Serialize publication with quarantine registration. Record checks again in its transaction.
    sqlx::query("SELECT pg_advisory_xact_lock(1414087745)")
        .execute(&mut *db)
        .await?;
    let denied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blocked_media_hashes WHERE hash=ANY($1)) OR EXISTS(SELECT 1 FROM media_removal_holds h LEFT JOIN media_fingerprints f USING(root) WHERE f.hashes && $1 OR h.root=$2)")
        .bind(&processed.fingerprints).bind(stored_prefix(&processed.stored)).fetch_one(&mut *db).await?;
    if denied {
        return Err(Fail::field("file", "This image is unavailable for upload."));
    }
    sqlx::query("INSERT INTO media_fingerprints(root,hashes) VALUES($1,$2) ON CONFLICT(root) DO UPDATE SET hashes=(SELECT ARRAY(SELECT DISTINCT unnest(media_fingerprints.hashes || EXCLUDED.hashes)))")
        .bind(stored_prefix(&processed.stored)).bind(&processed.fingerprints).execute(db).await?;
    Ok(())
}

/// Historical uploads have no source file; index the published files without fetching user URLs.
pub async fn index_existing(app: &App) -> Res<usize> {
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM media_objects WHERE NOT fingerprinted ORDER BY key LIMIT 100",
    )
    .fetch_all(&app.db)
    .await?;
    let count = keys.len();
    for key in keys {
        let root = root_of_key(&key);
        let Some(bytes) = app.config.media.storage.get(&app.http, &key).await? else {
            sqlx::query("UPDATE media_objects SET fingerprinted=true WHERE key=$1")
                .bind(&key)
                .execute(&app.db)
                .await?;
            continue;
        };
        let decoded = decode(&bytes, Kind::Banner)?;
        let hashes = fingerprints(&bytes, &decoded);
        sqlx::query("INSERT INTO media_fingerprints(root,hashes) VALUES($1,$2) ON CONFLICT(root) DO UPDATE SET hashes=(SELECT ARRAY(SELECT DISTINCT unnest(media_fingerprints.hashes || EXCLUDED.hashes)))")
            .bind(root).bind(hashes).execute(&app.db).await?;
        sqlx::query("UPDATE media_objects SET fingerprinted=true WHERE key=$1")
            .bind(key)
            .execute(&app.db)
            .await?;
    }
    Ok(count)
}

pub fn root_of_key(key: &str) -> &str {
    if key.matches('/').count() >= 2 {
        key.rsplit_once('/').unwrap().0
    } else {
        key
    }
}

pub async fn held(db: &mut PgConnection, key: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM media_removal_holds WHERE root=$1 OR $1 LIKE root||'/%')",
    )
    .bind(key)
    .fetch_one(db)
    .await?)
}
/// Batch visibility check for a catalog of stored image roots.
pub async fn held_roots(
    db: &mut PgConnection,
    roots: &[String],
) -> Res<std::collections::HashSet<String>> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT root FROM media_removal_holds WHERE root=ANY($1)")
            .bind(roots)
            .fetch_all(db)
            .await?
            .into_iter()
            .collect(),
    )
}
pub async fn existing_root(db: &mut PgConnection, key: &str) -> Res<Option<String>> {
    if !valid_key(key) {
        return Ok(None);
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_objects WHERE key=$1)")
            .bind(key)
            .fetch_one(db)
            .await?;
    Ok(exists.then(|| root_of_key(key).to_string()))
}

/// Expands known exact matches, then durably registers the hold before touching object storage.
pub async fn hold(db: &mut PgConnection, roots: &[String]) -> Res<Vec<String>> {
    sqlx::query("SELECT pg_advisory_xact_lock(1414087745)")
        .execute(&mut *db)
        .await?;
    let mut all = roots.to_vec();
    loop {
        let copies: Vec<String> = sqlx::query_scalar("SELECT DISTINCT b.root FROM media_fingerprints a JOIN media_fingerprints b ON a.hashes && b.hashes WHERE a.root=ANY($1)")
            .bind(&all).fetch_all(&mut *db).await?;
        let n = all.len();
        all.extend(copies);
        all.sort();
        all.dedup();
        if all.len() == n {
            break;
        }
    }
    for root in &all {
        sqlx::query("INSERT INTO media_removal_holds(root) VALUES($1) ON CONFLICT DO NOTHING")
            .bind(root)
            .execute(&mut *db)
            .await?;
    }
    Ok(all)
}

async fn purge(app: &App, keys: &[String]) -> Res<()> {
    let urls: Vec<String> = keys
        .iter()
        .flat_map(|key| {
            [
                media_url(app, key),
                format!("{}/api/media/{key}", app.config.origin),
            ]
        })
        .collect();
    purge_urls(app, &urls).await
}
pub async fn purge_urls(app: &App, urls: &[String]) -> Res<()> {
    if !app.config.production
        && matches!(app.config.media.storage, Storage::Filesystem(_))
        && app.config.take_down.purge_url.is_empty()
    {
        return Ok(());
    }
    let config = &app.config.take_down;
    if config.purge_url.is_empty() || config.purge_token.is_empty() {
        return Err(Fail::unavailable(
            "Media cache removal is not configured. Staff have been alerted.",
        ));
    }
    for urls in urls.chunks(25) {
        let response = app
            .http
            .post(&config.purge_url)
            .bearer_auth(&config.purge_token)
            .json(&json!({"files":urls}))
            .send()
            .await
            .map_err(|_| Fail::unavailable("Media cache removal will retry."))?;
        if !response.status().is_success()
            || response
                .json::<Value>()
                .await
                .map_err(|_| Fail::internal())?["success"]
                != true
        {
            return Err(Fail::unavailable("Media cache removal will retry."));
        }
    }
    Ok(())
}

/// Each original is encrypted in Postgres before deleting its public object. A crash retries the
/// delete and purge, never overwriting a saved original with an empty result.
pub async fn quarantine(app: &App, root: &str) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let row: Option<(Option<String>, Option<chrono::DateTime<Utc>>)> =
        sqlx::query_as("SELECT saved,hidden_at FROM media_removal_holds WHERE root=$1 FOR UPDATE")
            .bind(root)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((saved, hidden)) = row else {
        return Ok(());
    };
    if hidden.is_some() {
        return Ok(());
    }
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM media_objects WHERE key=$1 OR key LIKE $1||'/%' ORDER BY key",
    )
    .bind(root)
    .fetch_all(&mut *tx)
    .await?;
    if saved.is_none() {
        let mut originals = serde_json::Map::new();
        for key in &keys {
            if let Some(bytes) = app.config.media.storage.get(&app.http, key).await? {
                let decoded = decode(&bytes, Kind::Banner)?;
                sqlx::query("INSERT INTO media_fingerprints(root,hashes) VALUES($1,$2) ON CONFLICT(root) DO UPDATE SET hashes=(SELECT ARRAY(SELECT DISTINCT unnest(media_fingerprints.hashes || EXCLUDED.hashes)))")
                    .bind(root).bind(fingerprints(&bytes,&decoded)).execute(&mut *tx).await?;
                originals.insert(key.clone(), json!(STANDARD.encode(bytes)));
            }
        }
        sqlx::query(
            "UPDATE media_objects SET fingerprinted=true WHERE key=$1 OR key LIKE $1||'/%'",
        )
        .bind(root)
        .execute(&mut *tx)
        .await?;
        let sealed = security::seal(
            app,
            "media-quarantine",
            &Value::Object(originals).to_string(),
        )?;
        sqlx::query("UPDATE media_removal_holds SET saved=$2 WHERE root=$1")
            .bind(root)
            .bind(sealed)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let mut tx = app.db.begin().await?;
    let exists: Option<String> =
        sqlx::query_scalar("SELECT root FROM media_removal_holds WHERE root=$1 FOR UPDATE")
            .bind(root)
            .fetch_optional(&mut *tx)
            .await?;
    if exists.is_none() {
        return Ok(());
    }
    for key in &keys {
        app.config.media.storage.delete(&app.http, key).await?;
    }
    purge(app, &keys).await?;
    sqlx::query("UPDATE media_removal_holds SET hidden_at=now() WHERE root=$1")
        .bind(root)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn make_permanent(db: &mut PgConnection, root: &str, minor: bool) -> Res<()> {
    sqlx::query("INSERT INTO blocked_media_hashes(hash) SELECT unnest(hashes) FROM media_fingerprints WHERE root=$1 ON CONFLICT DO NOTHING")
        .bind(root).execute(&mut *db).await?;
    sqlx::query("UPDATE media_removal_holds SET permanent=true,legal_hold=legal_hold OR $2,saved=CASE WHEN legal_hold OR $2 THEN saved ELSE NULL END WHERE root=$1 AND hidden_at IS NOT NULL")
        .bind(root).bind(minor).execute(&mut *db).await?;
    sqlx::query("DELETE FROM media_objects WHERE key=$1 OR key LIKE $1||'/%'")
        .bind(root)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn restore(app: &App, root: &str) -> Res<()> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(1414087745)")
        .execute(&mut *tx)
        .await?;
    let saved: Option<Option<String>> = sqlx::query_scalar(
        "SELECT saved FROM media_removal_holds WHERE root=$1 AND NOT permanent FOR UPDATE",
    )
    .bind(root)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(saved) = saved else { return Ok(()) };
    if let Some(saved) = saved {
        let originals: serde_json::Map<String, Value> =
            serde_json::from_str(&security::unseal(app, "media-quarantine", &saved)?)
                .map_err(|_| Fail::internal())?;
        let keys: Vec<String> = originals.keys().cloned().collect();
        for (key, value) in originals {
            let bytes = STANDARD
                .decode(value.as_str().ok_or_else(Fail::internal)?)
                .map_err(|_| Fail::internal())?;
            app.config.media.storage.put(&app.http, &key, bytes).await?;
        }
        purge(app, &keys).await?;
    }
    sqlx::query("DELETE FROM media_removal_holds WHERE root=$1")
        .bind(root)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn evidence_keys(app: &App, root: &str) -> Res<Vec<String>> {
    let sealed: Option<String> =
        sqlx::query_scalar("SELECT saved FROM media_removal_holds WHERE root=$1")
            .bind(root)
            .fetch_optional(&app.db)
            .await?
            .flatten();
    let Some(sealed) = sealed else {
        return Ok(vec![]);
    };
    let saved: serde_json::Map<String, Value> =
        serde_json::from_str(&security::unseal(app, "media-quarantine", &sealed)?)
            .map_err(|_| Fail::internal())?;
    Ok(saved.into_iter().map(|(key, _)| key).collect())
}
pub async fn evidence(app: &App, key: &str) -> Res<Vec<u8>> {
    let sealed: Option<String> =
        sqlx::query_scalar("SELECT saved FROM media_removal_holds WHERE root=$1")
            .bind(root_of_key(key))
            .fetch_optional(&app.db)
            .await?
            .flatten();
    let sealed = sealed.ok_or_else(Fail::missing)?;
    let saved: Value = serde_json::from_str(&security::unseal(app, "media-quarantine", &sealed)?)
        .map_err(|_| Fail::internal())?;
    STANDARD
        .decode(saved[key].as_str().ok_or_else(Fail::missing)?)
        .map_err(|_| Fail::internal())
}
pub async fn ready(db: &mut PgConnection) -> Res<bool> {
    Ok(!sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM media_objects WHERE NOT fingerprinted)",
    )
    .fetch_one(db)
    .await?)
}
