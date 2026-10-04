//! Module 2 maintenance: account erasure steps, purges, hold release and media cleanup.
use crate::{App, Result};
use sqlx::PgConnection;

/// Runs before Login deletes an erased user row. Foreign keys cascade the profile, links, song,
/// schedule, sponsors, setup, blocks, follows, War Council rows, walls, likes, fan art, user
/// blocks, strikes, appeals, interim restrictions and username history.
pub async fn erase(db: &mut PgConnection, user_id: &str) -> Result<()> {
    let username: Option<String> =
        sqlx::query_scalar("SELECT username FROM users WHERE id=$1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *db)
            .await?;
    let Some(username) = username else {
        return Ok(());
    };
    crate::streams::revoke(db, user_id).await?;
    // Their playback sessions on other channels; leases on their own broadcasts cascade.
    sqlx::query("DELETE FROM playback_leases WHERE viewer_key='u:'||$1")
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    // Media they own, and fan art others submitted to their channel, is queued for storage deletion.
    // Setup photos are queued too, except content-addressed keys another user's setup photo still uses.
    sqlx::query("UPDATE media_objects m SET delete_after=now() WHERE (m.owner_id=$1 OR m.key LIKE ANY(SELECT image_key||'%' FROM fan_art WHERE channel_id=$1 OR submitter_id=$1) OR m.key LIKE ANY(SELECT image_key||'/%' FROM setup_photos WHERE user_id=$1)) AND NOT EXISTS(SELECT 1 FROM setup_photos o WHERE o.user_id<>$1 AND m.key LIKE o.image_key||'/%')").bind(user_id).execute(&mut *db).await?;
    // Counts affected by removed follows are recomputed after the cascade (see `refresh`).
    let touched: Vec<String> = sqlx::query_scalar("SELECT following_id FROM follows WHERE follower_id=$1 UNION SELECT follower_id FROM follows WHERE following_id=$1").bind(user_id).fetch_all(&mut *db).await?;
    // War Councils they appear in are compacted after the cascade.
    let councils: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM war_council WHERE member_id=$1")
            .bind(user_id)
            .fetch_all(&mut *db)
            .await?;
    // Reports they filed keep the snapshot without the reporter; their feedback goes with them.
    sqlx::query("UPDATE reports SET reporter_id=NULL,reporter_notice='NONE',reporter_seen_at=NULL WHERE reporter_id=$1").bind(user_id).execute(&mut *db).await?;
    // Reports about them close as ACTIONED with no reporter notice.
    sqlx::query("UPDATE reports SET status='ACTIONED',closed_at=now(),closed_reason='account erased' WHERE target_user_id=$1 AND status='OPEN'").bind(user_id).execute(&mut *db).await?;
    // Rename holds pointing at them stop redirecting; the current name is held 30 days.
    sqlx::query("UPDATE username_holds SET redirect=false,user_id=NULL WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT INTO username_holds(handle_canonical,user_id,released_at,redirect) VALUES(lower($1),NULL,now()+interval '30 days',false) ON CONFLICT (handle_canonical) DO UPDATE SET user_id=NULL,released_at=EXCLUDED.released_at,redirect=false")
        .bind(&username)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    for id in touched {
        sqlx::query("UPDATE profiles SET follower_count=(SELECT count(*) FROM follows WHERE following_id=$1),following_count=(SELECT count(*) FROM follows WHERE follower_id=$1) WHERE user_id=$1").bind(&id).execute(&mut *db).await?;
    }
    for owner in councils {
        compact_council(db, &owner).await?;
    }
    Ok(())
}
pub async fn compact_council(db: &mut PgConnection, owner: &str) -> Result<()> {
    // Positions are constrained to 1..8 with a non-deferrable key, so renumber by re-inserting.
    let members: Vec<String> =
        sqlx::query_scalar("SELECT member_id FROM war_council WHERE user_id=$1 ORDER BY position")
            .bind(owner)
            .fetch_all(&mut *db)
            .await?;
    sqlx::query("DELETE FROM war_council WHERE user_id=$1")
        .bind(owner)
        .execute(&mut *db)
        .await?;
    for (i, member) in members.iter().enumerate() {
        sqlx::query("INSERT INTO war_council(user_id,position,member_id) VALUES($1,$2,$3)")
            .bind(owner)
            .bind(i as i32 + 1)
            .bind(member)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}

pub async fn tick(app: &App) -> Result<()> {
    let due: Vec<String> = sqlx::query_scalar("SELECT id FROM users WHERE deleted_at<=now()-interval '14 days' AND NOT legacy_deletion_hold LIMIT 50").fetch_all(&app.db).await?;
    for id in due {
        let mut tx = app.db.begin().await?;
        erase(&mut tx, &id).await?;
        tx.commit().await?;
        eprintln!("profile_event=erasure outcome=ok");
    }
    let open = "NOT EXISTS(SELECT 1 FROM reports r WHERE r.status='OPEN' AND r.target_id=x.id)";
    // open is a fixed SQL predicate; no user input enters this statement.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM wall_replies x WHERE deleted_at<=now()-interval '30 days' AND {open}"
    )))
    .execute(&app.db)
    .await?;
    // open is a fixed SQL predicate; no user input enters this statement.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM wall_posts x WHERE deleted_at<=now()-interval '30 days' AND {open}"
    )))
    .execute(&app.db)
    .await?;
    sqlx::query("DELETE FROM schedule_events WHERE end_at<=now()-interval '30 days'")
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM username_holds WHERE released_at<=now()")
        .execute(&app.db)
        .await?;
    // Activity feed retention (docs/PROFILES.md, P7).
    sqlx::query("DELETE FROM activity_events WHERE created_at<now()-make_interval(days=>$1)")
        .bind(crate::activity::RETENTION_DAYS)
        .execute(&app.db)
        .await?;
    // Rejected fan art files are deleted from storage after 7 days.
    let mut tx = app.db.begin().await?;
    let rejected: Vec<(String, String)> = sqlx::query_as("SELECT id,image_key FROM fan_art x WHERE status='REJECTED' AND reviewed_at<=now()-interval '7 days' AND NOT EXISTS(SELECT 1 FROM reports r WHERE r.status='OPEN' AND r.target_id=x.id) LIMIT 200").fetch_all(&mut *tx).await?;
    for (id, key) in rejected {
        sqlx::query(
            "UPDATE media_objects SET delete_after=now() WHERE key=$1 OR key LIKE $1||'/%'",
        )
        .bind(crate::media::stored_prefix(&key))
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM fan_art WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    if let Err(e) = crate::safety::tick(app).await {
        eprintln!(
            "profile_event=safety_tick outcome=failed status={}",
            e.status.as_u16()
        );
    }
    // Imported songs: oEmbed metadata and thumbnails, at most one request per second.
    if crate::profile_import::song_job(app, 10).await.is_err() {
        eprintln!("profile_event=import_song outcome=failed");
    }
    if let Err(e) = crate::media::cleanup(app).await {
        eprintln!(
            "profile_event=media_cleanup outcome=failed status={}",
            e.status.as_u16()
        );
    }
    Ok(())
}
