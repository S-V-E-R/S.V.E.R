//! Public interfaces for reports, copyright and the Take It Down workflow.
use super::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};

async fn playback(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = crate::safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let video = load(&mut tx, &id).await?;
    if matches!(video.status.as_str(), "DELETED" | "EXPIRED") {
        return Err(Fail::missing());
    }
    crate::safety::audit(
        &mut tx,
        Some(&user.id),
        "video_evidence_opened",
        &video.kind.to_ascii_lowercase(),
        &id,
        &[],
        "Reviewed recording evidence",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    let token = ticket(&app, &video, Some(&user), "review", true)?;
    Ok(Json(
        json!({"video":video,"playback":format!("/api/videos/{id}/{}?ticket={token}",if video.kind=="CLIP"{"file"}else{"playlist"}),"thumbnail":video.thumbnail_key.as_ref().map(|_|format!("/api/videos/{id}/thumbnail?ticket={token}"))}),
    ))
}
pub fn routes() -> Router<App> {
    Router::new().route("/api/admin/videos/{id}/review", get(playback))
}

pub fn is_video(kind: &str) -> bool {
    matches!(kind, "vod" | "highlight" | "clip")
}
pub async fn owner(db: &mut PgConnection, id: &str) -> Res<Option<String>> {
    Ok(
        sqlx::query_scalar::<_, Option<String>>("SELECT owner_id FROM videos WHERE id=$1")
            .bind(id)
            .fetch_optional(db)
            .await?
            .flatten(),
    )
}
pub async fn snapshot(db: &mut PgConnection, id: &str) -> Res<Value> {
    serde_json::to_value(load(db, id).await?).map_err(|_| Fail::internal())
}
pub async fn target(
    db: &mut PgConnection,
    kind: &str,
    id: &str,
) -> Res<Option<(String, String, Value)>> {
    let video = load(db, id).await?;
    if video.kind.to_ascii_lowercase() != kind
        || matches!(video.status.as_str(), "DELETED" | "EXPIRED")
    {
        return Ok(None);
    };
    let Some(owner) = video.owner_id.as_deref() else {
        return Ok(None);
    };
    let Some(channel) = profiles::channel_user_by_id(db, owner).await? else {
        return Ok(None);
    };
    Ok(Some((
        owner.into(),
        channel.username,
        snapshot(db, id).await?,
    )))
}
async fn tree(db: &mut PgConnection, id: &str, descendants: bool) -> Res<Vec<String>> {
    // Cut creation takes this short lock too, so no descendant can appear between the walk
    // and the hold. It never covers storage or FFmpeg work.
    sqlx::query("SELECT pg_advisory_xact_lock(80742)")
        .execute(&mut *db)
        .await?;
    let ids:Vec<String>=sqlx::query_scalar("WITH RECURSIVE family AS (SELECT id FROM videos WHERE id=$1 UNION ALL SELECT v.id FROM videos v JOIN family f ON v.parent_id=f.id WHERE $2) SELECT id FROM videos WHERE id IN (SELECT id FROM family) ORDER BY id FOR UPDATE")
        .bind(id).bind(descendants).fetch_all(&mut *db).await?;
    if ids.is_empty() {
        return Err(Fail::missing());
    }
    Ok(ids)
}
pub async fn hold(
    db: &mut PgConnection,
    id: &str,
    kind: &str,
    reference: &str,
    descendants: bool,
) -> Res<()> {
    let ids = tree(db, id, descendants).await?;
    for id in ids {
        sqlx::query("INSERT INTO video_holds(video_id,kind,reference) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(&id).bind(kind).bind(reference).execute(&mut *db).await?;
        sqlx::query("UPDATE videos SET revision=revision+1 WHERE id=$1")
            .bind(id)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}
pub async fn release(db: &mut PgConnection, kind: &str, references: &[String]) -> Res<()> {
    sqlx::query("DELETE FROM video_holds WHERE kind=$1 AND reference=ANY($2) AND NOT permanent")
        .bind(kind)
        .bind(references)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn remove(db: &mut PgConnection, id: &str, descendants: bool, legal: bool) -> Res<Value> {
    let ids = tree(db, id, descendants).await?;
    for video in ids {
        let status: String = sqlx::query_scalar("SELECT status FROM videos WHERE id=$1")
            .bind(&video)
            .fetch_one(&mut *db)
            .await?;
        if matches!(status.as_str(), "DELETED" | "EXPIRED") {
            continue;
        }
        if legal {
            sqlx::query(
                "UPDATE video_holds SET permanent=true WHERE video_id=$1 AND kind='TAKE_DOWN'",
            )
            .bind(&video)
            .execute(&mut *db)
            .await?;
        } else {
            sqlx::query("DELETE FROM video_holds WHERE video_id=$1 AND kind='REPORT'")
                .bind(&video)
                .execute(&mut *db)
                .await?;
        }
        sqlx::query("UPDATE videos SET status='DELETING',revision=revision+1 WHERE id=$1 AND status NOT IN ('DELETED','EXPIRED')").bind(&video).execute(&mut *db).await?;
        enqueue(db, &video, "DELETE", Some(&video), json!({"legal":legal})).await?;
        crate::beacons::remove_made_from(db, std::slice::from_ref(&video), legal).await?;
    }
    Ok(json!({"type":"video","id":id,"previous":"REMOVED"}))
}
pub async fn removed(db: &mut PgConnection, id: &str) -> Res<bool> {
    let family: Vec<(String, String)> = sqlx::query_as("WITH RECURSIVE family AS (SELECT id,status FROM videos WHERE id=$1 UNION ALL SELECT v.id,v.status FROM videos v JOIN family f ON v.parent_id=f.id) SELECT id,status FROM family").bind(id).fetch_all(&mut *db).await?;
    if family
        .iter()
        .any(|(_, status)| !matches!(status.as_str(), "DELETED" | "EXPIRED"))
    {
        return Ok(false);
    }
    let ids: Vec<String> = family.into_iter().map(|(id, _)| id).collect();
    crate::beacons::review::removed(db, &ids).await
}
/// SQL: whether the clip whose id is the expression `clip` is held for a removal request or a
/// copyright notice. Beacons made from it stay out of public view meanwhile (docs/BEACONS.md).
pub fn source_held_sql(clip: &str) -> String {
    format!(
        "EXISTS(SELECT 1 FROM video_holds h WHERE h.video_id={clip} AND h.kind IN ('TAKE_DOWN','COPYRIGHT'))"
    )
}
pub async fn source_held(db: &mut PgConnection, clip: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {}",
        source_held_sql("$1")
    )))
    .bind(clip)
    .fetch_one(db)
    .await?)
}
