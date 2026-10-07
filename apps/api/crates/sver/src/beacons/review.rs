//! Reports, staff hiding, Take It Down and the evidence player. A Beacon with an open report keeps
//! playing unless staff hide it; a removal request hides it at once and a valid one deletes every
//! copy and blocks exact re-uploads.
use super::*;
use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post},
};

pub async fn owner(db: &mut PgConnection, id: &str) -> Res<Option<String>> {
    Ok(load(db, id).await.ok().and_then(|b| b.owner_id))
}
pub async fn snapshot(db: &mut PgConnection, id: &str) -> Res<Value> {
    serde_json::to_value(load(db, id).await?).map_err(|_| Fail::internal())
}
/// A report target: any Beacon that still exists, with its owner and a snapshot.
pub async fn target(db: &mut PgConnection, id: &str) -> Res<Option<(String, String, Value)>> {
    let Ok(beacon) = load(db, id).await else {
        return Ok(None);
    };
    if matches!(beacon.status.as_str(), "DELETING" | "DELETED") {
        return Ok(None);
    }
    let Some(owner) = beacon.owner_id.clone() else {
        return Ok(None);
    };
    let Some(channel) = profiles::channel_user_by_id(db, &owner).await? else {
        return Ok(None);
    };
    Ok(Some((
        owner,
        channel.username,
        serde_json::to_value(&beacon).map_err(|_| Fail::internal())?,
    )))
}
/// Staff removal from the report queue. Files are kept privately so an overturned decision can
/// restore it; the creator can still delete it for good.
pub async fn remove(db: &mut PgConnection, id: &str) -> Res<Value> {
    let previous: Option<String> =
        sqlx::query_scalar("SELECT status FROM beacons WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .filter(|s: &String| matches!(s.as_str(), "PUBLISHED" | "READY"));
    if previous.is_some() {
        sqlx::query("UPDATE beacons SET status='REMOVED',revision=revision+1 WHERE id=$1")
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(json!({"type":"beacon","id":id,"previous":previous}))
}
pub async fn restore(db: &mut PgConnection, id: &str, previous: &str) -> Res<()> {
    if matches!(previous, "PUBLISHED" | "READY") {
        sqlx::query(
            "UPDATE beacons SET status=$2,revision=revision+1 WHERE id=$1 AND status='REMOVED'",
        )
        .bind(id)
        .bind(previous)
        .execute(db)
        .await?;
    }
    Ok(())
}
/// Take It Down: hidden while the request is reviewed; returns what to restore if it isn't removed.
pub async fn hide(db: &mut PgConnection, id: &str) -> Res<Value> {
    let was: Option<bool> = sqlx::query_scalar("SELECT hidden FROM beacons WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *db)
        .await?;
    sqlx::query("UPDATE beacons SET hidden=true,revision=revision+1 WHERE id=$1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(json!({"previous": was.unwrap_or(true)}))
}
pub async fn unhide(db: &mut PgConnection, id: &str, previous: &Value) -> Res<()> {
    if previous["previous"] == false {
        sqlx::query("UPDATE beacons SET hidden=false,revision=revision+1 WHERE id=$1 AND status NOT IN ('DELETING','DELETED')")
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(())
}
/// A valid removal request: every copy goes and exact copies are blocked from publishing again.
pub async fn remove_legal(db: &mut PgConnection, id: &str) -> Res<()> {
    sqlx::query("SELECT id FROM beacons WHERE id=$1 FOR UPDATE")
        .bind(id)
        .execute(&mut *db)
        .await?;
    request_delete(db, id, true).await
}
pub async fn removed(db: &mut PgConnection, ids: &[String]) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM beacons WHERE (id=ANY($1) OR clip_id=ANY($1)) AND status<>'DELETED')",
    )
    .bind(ids)
    .fetch_one(db)
    .await?)
}

#[derive(Deserialize)]
struct Hide {
    hidden: bool,
    reason: String,
}
/// POST /api/admin/beacons/{id}/hide: staff take a reported Beacon out of every feed (or return it).
async fn staff_hide(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Hide>,
) -> Res<Json<Value>> {
    let actor = crate::safety::staff_write(&app, &jar).await?;
    let reason = crate::text::plain(&input.reason, "reason", 1, 500, 5, false)?;
    let mut tx = app.db.begin().await?;
    let beacon = load(&mut tx, &id).await?;
    if matches!(beacon.status.as_str(), "DELETING" | "DELETED") {
        return Err(Fail::missing());
    }
    sqlx::query("UPDATE beacons SET hidden=$2,revision=revision+1 WHERE id=$1")
        .bind(&id)
        .bind(input.hidden)
        .execute(&mut *tx)
        .await?;
    crate::safety::audit(
        &mut tx,
        Some(&actor.id),
        if input.hidden {
            "hide_beacon"
        } else {
            "unhide_beacon"
        },
        "beacon",
        &id,
        &[],
        &reason,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"hidden":input.hidden})))
}
/// GET /api/admin/beacons/{id}/review: evidence playback for staff, logged.
async fn evidence(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = crate::safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let beacon = load(&mut tx, &id).await?;
    if matches!(beacon.status.as_str(), "DELETING" | "DELETED") {
        return Err(Fail::missing());
    }
    crate::safety::audit(
        &mut tx,
        Some(&user.id),
        "beacon_evidence_opened",
        "beacon",
        &id,
        &[],
        "Reviewed Beacon evidence",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    let token = ticket(&app, &beacon, Some(&user), "review", true)?;
    Ok(Json(json!({
        "beacon": beacon,
        "playback": beacon.mp4_key.as_ref().map(|_| format!("/api/beacons/{id}/file?q=hd&ticket={token}")),
        "thumbnail": beacon.thumbnail_key.as_ref().map(|_| format!("/api/beacons/{id}/thumbnail?ticket={token}")),
    })))
}
pub fn routes() -> axum::Router<App> {
    axum::Router::new()
        .route("/api/admin/beacons/{id}/hide", post(staff_hide))
        .route("/api/admin/beacons/{id}/review", get(evidence))
}
