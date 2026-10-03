//! Channel activity feed (docs/PROFILES.md, "P7. Activity feed"). Events are recorded when an
//! action succeeds and filtered at read time, so unfollows, deletions, blocks, restrictions and
//! erasure take effect at once. Later modules add kinds to `KINDS`; no migration is needed.
// Row tuples from runtime sqlx queries read more clearly inline than as aliases.
#![allow(clippy::type_complexity)]
use crate::{
    App,
    profiles::{
        CursorQuery, Fail, Res, chip_sql, eligible_by_name, hydrate, make_cursor, new_id,
        parse_cursor, viewer,
    },
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgConnection;

/// One registered event kind. `coalesce` merges repeated saves within 60 minutes into one row.
pub struct EventKind {
    pub name: &'static str,
    pub coalesce: bool,
}
/// The registry of kinds that are recorded and shown. Unknown kinds in the table are skipped.
pub const KINDS: &[EventKind] = &[
    EventKind {
        name: "follow",
        coalesce: false,
    },
    EventKind {
        name: "wall_post",
        coalesce: false,
    },
    EventKind {
        name: "war_council",
        coalesce: true,
    },
    EventKind {
        name: "song",
        coalesce: true,
    },
    EventKind {
        name: "schedule",
        coalesce: true,
    },
];
pub const RETENTION_DAYS: i32 = 90;
const PAGE: usize = 20;

fn kind(name: &str) -> Option<&'static EventKind> {
    KINDS.iter().find(|k| k.name == name)
}
pub fn known_kinds() -> Vec<&'static str> {
    KINDS.iter().map(|k| k.name).collect()
}

/// Records an event for `actor`. Internal accounts record nothing. A follow keeps one row per
/// pair (a refollow moves it to now); coalescing kinds update the actor's latest row of that
/// kind when it is under 60 minutes old.
pub async fn record(
    db: &mut PgConnection,
    actor: &str,
    name: &str,
    subject: Option<&str>,
    ref_id: Option<&str>,
    data: Value,
) -> Res<()> {
    let Some(kind) = kind(name) else {
        return Err(Fail::internal());
    };
    let internal: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT internal FROM channel_users WHERE id=$1),true)",
    )
    .bind(actor)
    .fetch_one(&mut *db)
    .await?;
    if internal {
        return Ok(());
    }
    if kind.name == "follow" {
        sqlx::query("INSERT INTO activity_events(id,actor_id,kind,subject_id) VALUES($1,$2,'follow',$3) ON CONFLICT (actor_id,subject_id) WHERE kind='follow' DO UPDATE SET created_at=now()")
            .bind(new_id())
            .bind(actor)
            .bind(subject)
            .execute(&mut *db)
            .await?;
        return Ok(());
    }
    if kind.coalesce {
        let updated = sqlx::query("UPDATE activity_events SET data=$3,ref_id=$4,subject_id=$5,created_at=now() WHERE id=(SELECT id FROM activity_events WHERE actor_id=$1 AND kind=$2 AND created_at>now()-interval '60 minutes' ORDER BY created_at DESC LIMIT 1)")
            .bind(actor)
            .bind(kind.name)
            .bind(&data)
            .bind(ref_id)
            .bind(subject)
            .execute(&mut *db)
            .await?
            .rows_affected();
        if updated > 0 {
            return Ok(());
        }
    }
    sqlx::query("INSERT INTO activity_events(id,actor_id,kind,subject_id,ref_id,data) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(new_id())
        .bind(actor)
        .bind(kind.name)
        .bind(subject)
        .bind(ref_id)
        .bind(&data)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Deletes the actor's events of one kind (song removed, or a staff reset of that section).
pub async fn forget(db: &mut PgConnection, actor: &str, name: &str) -> Res<()> {
    sqlx::query("DELETE FROM activity_events WHERE actor_id=$1 AND kind=$2")
        .bind(actor)
        .bind(name)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// GET /api/channels/{username}/activity?cursor=: the channel owner's events, newest first.
pub async fn channel_activity(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let viewer = viewer(&app, &jar).await?;
    let viewer_id = viewer.as_ref().map(|v| v.id.clone());
    let mut db = app.db.acquire().await?;
    let channel = eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let after = parse_cursor(&q.cursor)?;
    // The subject must be eligible and outside any block with the viewer or the actor; follows
    // and wall posts must still exist and be visible.
    let rows: Vec<(String, String, DateTime<Utc>, Option<Value>, Value)> = sqlx::query_as(&format!(
        "SELECT e.id,e.kind,e.created_at,CASE WHEN s.id IS NULL THEN NULL ELSE {chip} END,e.data FROM activity_events e LEFT JOIN channel_users s ON s.id=e.subject_id \
         WHERE e.actor_id=$1 AND e.kind=ANY($2) \
         AND (e.subject_id IS NULL OR (s.eligible AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE (b.blocker_id=e.subject_id AND b.blocked_id IN (e.actor_id,$3)) OR (b.blocked_id=e.subject_id AND b.blocker_id IN (e.actor_id,$3))))) \
         AND (e.kind<>'follow' OR EXISTS(SELECT 1 FROM follows f WHERE f.follower_id=e.actor_id AND f.following_id=e.subject_id)) \
         AND (e.kind<>'wall_post' OR EXISTS(SELECT 1 FROM wall_posts w WHERE w.id=e.ref_id AND w.status='APPROVED' AND w.deleted_at IS NULL)) \
         AND ($4::timestamptz IS NULL OR (e.created_at,e.id)<($4,$5)) ORDER BY e.created_at DESC,e.id DESC LIMIT {limit}",
        chip = chip_sql("s"),
        limit = PAGE + 1
    ))
    .bind(&channel.id)
    .bind(known_kinds())
    .bind(viewer_id.clone().unwrap_or_default())
    .bind(after.as_ref().map(|a| a.0))
    .bind(after.as_ref().map(|a| a.1.clone()).unwrap_or_default())
    .fetch_all(&mut *db)
    .await?;
    let more = rows.len() > PAGE;
    let rows = &rows[..rows.len().min(PAGE)];
    let mut items: Vec<Value> = rows
        .iter()
        .map(|r| json!({"id": r.0, "kind": r.1, "created_at": r.2, "subject": r.3, "data": r.4}))
        .collect();
    items.iter_mut().for_each(|v| hydrate(&app, v));
    Ok(Json(json!({
        "items": items,
        "next_cursor": if more { rows.last().map(|r| make_cursor(r.2, &r.0)) } else { None },
        "is_owner": viewer_id.as_deref() == Some(channel.id.as_str()),
    })))
}
