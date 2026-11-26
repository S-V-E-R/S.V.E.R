use super::*;

/// A full batch of queued keys that are still used must not stop cleanup of the keys behind it.
pub async fn exercise(e: &Env, dir: &std::path::Path) {
    let (owner, _) = e.user("AuditMediaRef", true).await;
    let stuck: Vec<String> = (0..100)
        .map(|n| format!("auditmedia/stuck-{n}.webp"))
        .collect();
    sqlx::query("INSERT INTO overlay_settings(channel_id,settings) VALUES($1,$2)")
        .bind(&owner)
        .bind(json!({ "keys": stuck }))
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO media_objects(key,kind,bytes,delete_after) SELECT k,'avatar',1,now()-interval '10 years' FROM unnest($1::text[]) k")
        .bind(&stuck)
        .execute(&e.app.db)
        .await
        .unwrap();
    let free = "auditmedia/free.webp";
    std::fs::create_dir_all(dir.join("auditmedia")).unwrap();
    std::fs::write(dir.join(free), b"x").unwrap();
    sqlx::query("INSERT INTO media_objects(key,kind,bytes,delete_after) VALUES($1,'avatar',1,now()-interval '9 years')")
        .bind(free)
        .execute(&e.app.db)
        .await
        .unwrap();

    media::cleanup(&e.app).await.unwrap();
    media::cleanup(&e.app).await.unwrap();
    assert!(
        !dir.join(free).exists(),
        "the unused key behind the batch is deleted"
    );
    let postponed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media_objects WHERE key=ANY($1) AND delete_after>now()",
    )
    .bind(&stuck)
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert_eq!(
        postponed, 100,
        "keys still in use are kept and retried later"
    );
}
