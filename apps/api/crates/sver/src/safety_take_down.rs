//! Profile content operations for the anonymous removal process.
use super::*;
use serde::Serialize;

#[derive(Serialize, Deserialize, Clone)]
pub struct Located {
    pub kind: String,
    pub id: String,
    pub owner: String,
    pub roots: Vec<String>,
    pub snapshot: Value,
}

pub async fn locate(
    db: &mut PgConnection,
    kind: &str,
    id: &str,
    field: Option<&str>,
) -> Res<Option<Located>> {
    if kind == "profile" {
        let Some(user) = profiles::eligible_by_name(db, id).await? else {
            return Ok(None);
        };
        let roots: Vec<String> = sqlx::query_scalar("SELECT key FROM (SELECT avatar_key AS key,'avatar' AS field FROM profiles WHERE user_id=$1 UNION ALL SELECT banner_key,'banner' FROM profiles WHERE user_id=$1 UNION ALL SELECT song_thumb_key,'song' FROM profiles WHERE user_id=$1 UNION ALL SELECT logo_key,'sponsors' FROM sponsors WHERE user_id=$1 UNION ALL SELECT image_key,'setup' FROM setup_photos WHERE user_id=$1) m WHERE key IS NOT NULL AND ($2::text IS NULL OR field=$2)")
            .bind(&user.id).bind(field).fetch_all(db).await?;
        return Ok(Some(Located {
            kind: kind.into(),
            id: user.id.clone(),
            owner: user.id,
            roots: roots
                .iter()
                .map(|s| media::stored_prefix(s).to_string())
                .collect(),
            snapshot: Value::Null,
        }));
    }
    let t = match target(db, kind, id, None).await {
        Ok(t) => t,
        Err(e) if e.status == axum::http::StatusCode::NOT_FOUND => return Ok(None),
        Err(e) => return Err(e),
    };
    let roots = match kind {
        "fan_art" => vec![
            t.snapshot["value"]["image"]
                .as_str()
                .ok_or_else(Fail::internal)?
                .into(),
        ],
        "setup_photo" | "emote" => vec![
            media::removal::root_of_key(
                t.snapshot["value"]["image"]
                    .as_str()
                    .ok_or_else(Fail::internal)?,
            )
            .into(),
        ],
        _ => vec![],
    };
    Ok(Some(Located {
        kind: kind.into(),
        id: id.into(),
        owner: t.owner_id,
        roots,
        snapshot: t.snapshot,
    }))
}

pub async fn hide(db: &mut PgConnection, target: &Located) -> Res<Value> {
    if matches!(
        target.kind.as_str(),
        "profile" | "fan_art" | "setup_photo" | "live_stream" | "emote"
    ) {
        return Ok(Value::Null);
    }
    remove_content(db, &target.kind, &target.id).await
}

pub async fn restore(
    db: &mut PgConnection,
    kind: &str,
    id: &str,
    previous: &Value,
    since: DateTime<Utc>,
) -> Res<()> {
    // A separate staff action after quarantine takes precedence over restoring a mistaken report.
    let superseded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM moderation_actions WHERE target_type=$1 AND target_id=$2 AND created_at>=$3 AND action<>'take_down_received')")
        .bind(kind).bind(id).bind(since).fetch_one(&mut *db).await?;
    if superseded {
        return Ok(());
    }
    match kind {
        "wall_post" | "wall_reply" => {
            let table = if kind == "wall_post" {
                "wall_posts"
            } else {
                "wall_replies"
            };
            if let Some(previous) = previous["previous"].as_str() {
                // table is selected from the two literals above; all values remain bound.
                sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE {table} SET status=$2 WHERE id=$1 AND status='REMOVED' AND deleted_at IS NULL")))
                    .bind(id).bind(previous).execute(db).await?;
            }
        }
        "chat_message" if previous["previous"] == "VISIBLE" => {
            sqlx::query("UPDATE chat_messages SET deleted_at=NULL WHERE id=$1 AND deleted_at=$2 AND expires_at>now()")
                .bind(id).bind(since).execute(db).await?;
        }
        _ => {}
    }
    Ok(())
}

pub async fn remove(db: &mut PgConnection, kind: &str, id: &str) -> Res<()> {
    match kind {
        "live_stream" => {
            remove_content(db, kind, id).await?;
        }
        "wall_post" | "wall_reply" | "chat_message" => {
            let table = match kind {
                "wall_post" => "wall_posts",
                "wall_reply" => "wall_replies",
                _ => "chat_messages",
            };
            // table is a fixed literal above, never request SQL.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {table} WHERE id=$1"
            )))
            .bind(id)
            .execute(db)
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// Remove every current reference to the stored image, including other uploaders' copies.
pub async fn remove_media(db: &mut PgConnection, root: &str) -> Res<Vec<String>> {
    let mut owners: Vec<String> = sqlx::query_scalar("SELECT user_id FROM profiles WHERE split_part(avatar_key,'@',1)=$1 OR split_part(banner_key,'@',1)=$1 OR song_thumb_key=$1 UNION SELECT user_id FROM sponsors WHERE logo_key=$1 UNION SELECT user_id FROM setup_photos WHERE image_key=$1 UNION SELECT submitter_id FROM fan_art WHERE image_key=$1")
        .bind(root).fetch_all(&mut *db).await?;
    owners.extend(crate::emotes::remove_media(db, root).await?);
    owners.sort();
    owners.dedup();
    sqlx::query("UPDATE profiles SET avatar_key=CASE WHEN avatar_key=$1 THEN NULL ELSE avatar_key END,banner_key=CASE WHEN split_part(banner_key,'@',1)=$1 THEN NULL ELSE banner_key END,song_thumb_key=CASE WHEN song_thumb_key=$1 THEN NULL ELSE song_thumb_key END WHERE avatar_key=$1 OR split_part(banner_key,'@',1)=$1 OR song_thumb_key=$1")
        .bind(root).execute(&mut *db).await?;
    sqlx::query("UPDATE sponsors SET logo_key=NULL WHERE logo_key=$1")
        .bind(root)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM setup_photos WHERE image_key=$1")
        .bind(root)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM fan_art WHERE image_key=$1")
        .bind(root)
        .execute(db)
        .await?;
    Ok(owners)
}

pub async fn sanction(
    app: &App,
    db: &mut PgConnection,
    actor: &User,
    owner: &str,
    number: &str,
) -> Res<Option<String>> {
    let Some(target) = profiles::channel_user_by_id(db, owner).await? else {
        return Ok(None);
    };
    if target.internal {
        // Existing moderation rules exempt internal accounts from strikes, never from removal.
        audit(
            db,
            Some(&actor.id),
            "take_down_internal_account_review",
            "user",
            owner,
            &[],
            "Removed content from an internal account; staff must review access.",
            json!({"removal_request":number}),
            false,
        )
        .await?;
        return Ok(None);
    }
    let input = StrikeInput { reason:"sexual".into(),severity:"SEVERE".into(),message_to_user:"Content was removed under the Take It Down process. You may appeal this strike; a valid content removal is permanent.".into(),interim_restriction_id:None };
    // No image or requester identity enters the account-facing strike or appeal restoration list.
    let (strike, _, notice) = issue_strike(
        app,
        db,
        actor,
        owner,
        &input,
        &[],
        json!({"removal_request":number}),
        json!([]),
        "Validated Take It Down removal",
    )
    .await?;
    sqlx::query("UPDATE strikes SET ban_review_open=true WHERE id=$1")
        .bind(strike)
        .execute(db)
        .await?;
    Ok(notice)
}

pub async fn staff_ids(db: &mut PgConnection) -> Res<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT s.user_id FROM staff_roles s JOIN users u ON u.id=s.user_id WHERE s.role='admin' AND u.deleted_at IS NULL")
        .fetch_all(db).await?)
}
