//! Raids and hosting (docs/LIVE_STREAMS.md "Raids" and "Hosting"). Who may send viewers to whom is
//! defined once, in the SQL function `viewer_send_refusal` (migration 0022).
use crate::{
    App,
    profiles::{self, Fail, Res, signed_in},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// Viewers see this countdown (with Cancel) before their player moves.
const COUNTDOWN_SECONDS: f64 = 10.0;

fn refusal_message(refusal: &str, hosting: bool) -> Fail {
    let verb = if hosting { "host" } else { "raid" };
    match refusal {
        "self" => Fail::bad(format!("You can't {verb} your own channel.")),
        "closed" => Fail::bad(if hosting {
            "That channel isn't accepting hosts."
        } else {
            "That channel isn't accepting raids."
        }),
        "unavailable" => Fail::channel_missing(),
        _ => Fail::bad(format!("You can't {verb} that channel.")),
    }
}
/// The target's live broadcast, after the shared send rules.
async fn target(
    db: &mut PgConnection,
    from: &str,
    name: &str,
    hosting: bool,
) -> Res<(String, String)> {
    let target = profiles::eligible_by_name(&mut *db, name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let refusal: Option<String> = sqlx::query_scalar("SELECT viewer_send_refusal($1,$2,$3)")
        .bind(from)
        .bind(&target.id)
        .bind(hosting)
        .fetch_one(&mut *db)
        .await?;
    if let Some(refusal) = refusal {
        return Err(refusal_message(&refusal, hosting));
    }
    let broadcast: Option<String> = sqlx::query_scalar(
        "SELECT id FROM broadcasts WHERE owner_id=$1 AND state IN ('LIVE','RECONNECTING')",
    )
    .bind(&target.id)
    .fetch_optional(&mut *db)
    .await?;
    let broadcast = broadcast.ok_or_else(|| Fail::bad("That channel isn't live."))?;
    Ok((target.id, broadcast))
}

#[derive(Deserialize)]
struct Target {
    username: String,
}
/// POST /api/me/raids: a live owner starts a raid (also `/raid username` in their chat).
async fn start(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Target>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let broadcast: Option<String> = sqlx::query_scalar(
        "SELECT id FROM broadcasts WHERE owner_id=$1 AND state='LIVE' FOR UPDATE",
    )
    .bind(&user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let broadcast = broadcast.ok_or_else(|| Fail::bad("Go live before starting a raid."))?;
    let name = input.username.trim().trim_start_matches('@');
    let (target_id, target_broadcast) = target(&mut tx, &user.id, name, false).await?;
    // One raid per broadcast every 10 minutes; a cancelled countdown doesn't count.
    let recent: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM raids WHERE broadcast_id=$1 AND status<>'cancelled' AND created_at>now()-interval '10 minutes')")
        .bind(&broadcast)
        .fetch_one(&mut *tx)
        .await?;
    if recent {
        return Err(Fail::conflict("You can raid once every 10 minutes."));
    }
    let id = profiles::new_id();
    sqlx::query("INSERT INTO raids(id,raider_id,broadcast_id,target_id,target_broadcast_id,execute_at) VALUES($1,$2,$3,$4,$5,now()+make_interval(secs=>$6))")
        .bind(&id).bind(&user.id).bind(&broadcast).bind(&target_id).bind(&target_broadcast).bind(COUNTDOWN_SECONDS)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    let raid = raid_json(&app, &id).await?;
    app.chat
        .publish(&user.id, None, 0, json!({"type":"raid","raid":raid}));
    Ok(Json(json!({"raid":raid})))
}

/// The raid as viewers see it: id, status, target and when the move happens.
async fn raid_json(app: &App, id: &str) -> Res<Value> {
    let (status, execute_at, username, display_name): (String, DateTime<Utc>, String, String) = sqlx::query_as("SELECT r.status,r.execute_at,c.username,c.display_name FROM raids r JOIN channel_users c ON c.id=r.target_id WHERE r.id=$1")
        .bind(id)
        .fetch_one(&app.db)
        .await?;
    Ok(
        json!({"id":id,"status":status,"execute_at":execute_at,"target":{"username":username,"display_name":display_name}}),
    )
}

/// DELETE /api/me/raids: the raider cancels during the countdown.
async fn cancel(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let cancelled: Option<String> = sqlx::query_scalar("UPDATE raids SET status='cancelled' WHERE raider_id=$1 AND status='countdown' AND execute_at>now() RETURNING id")
        .bind(&user.id)
        .fetch_optional(&app.db)
        .await?;
    let id = cancelled.ok_or_else(|| Fail::bad("There's no raid counting down."))?;
    app.chat
        .publish(&user.id, None, 0, json!({"type":"raid_cancelled","id":id}));
    Ok(Json(json!({"cancelled":true})))
}

/// Moves a raid whose countdown has ended, rechecking the rules; idempotent.
async fn execute(app: &App, id: &str) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let due: Option<(String, String, String)> = sqlx::query_as("SELECT raider_id,target_id,target_broadcast_id FROM raids WHERE id=$1 AND status='countdown' AND execute_at<=now() FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some((raider, target, broadcast)) = due else {
        return Ok(());
    };
    let ok: bool = sqlx::query_scalar("SELECT viewer_send_refusal($1,$2,false) IS NULL AND EXISTS(SELECT 1 FROM broadcasts WHERE id=$3 AND state IN ('LIVE','RECONNECTING'))")
        .bind(&raider).bind(&target).bind(&broadcast).fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE raids SET status=$2 WHERE id=$1")
        .bind(id)
        .bind(if ok { "moved" } else { "failed" })
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if !ok {
        app.chat
            .publish(&raider, None, 0, json!({"type":"raid_cancelled","id":id}));
    }
    Ok(())
}

/// GET /api/raids/{id}: a viewer's player asks at the end of the countdown whether to move.
async fn status(State(app): State<App>, Path(id): Path<String>) -> Res<Json<Value>> {
    execute(&app, &id).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM raids WHERE id=$1)")
        .bind(&id)
        .fetch_one(&app.db)
        .await?;
    if !exists {
        return Err(Fail::missing());
    }
    Ok(Json(raid_json(&app, &id).await?))
}

/// The raid viewers of `broadcast` should see, unless the viewer is banned from its target.
pub async fn for_viewers(app: &App, broadcast: &str, viewer: Option<&str>) -> Res<Option<Value>> {
    let raid: Option<(String, String)> = sqlx::query_as("SELECT id,target_id FROM raids WHERE broadcast_id=$1 AND (status='countdown' OR (status='moved' AND execute_at>now()-interval '60 seconds')) ORDER BY created_at DESC LIMIT 1")
        .bind(broadcast)
        .fetch_optional(&app.db)
        .await?;
    let Some((id, target)) = raid else {
        return Ok(None);
    };
    // Signed-in viewers banned from the target stay put.
    if let Some(viewer) = viewer
        && crate::moderation::banned(app, &target, viewer).await?
    {
        return Ok(None);
    }
    Ok(Some(raid_json(app, &id).await?))
}
/// Tags a lease that started on the target within 60 seconds of the raid, for the arrival count.
pub async fn arrived(app: &App, broadcast: &str, key: &str, raid: &str) -> Res<()> {
    sqlx::query("UPDATE playback_leases l SET raid_id=r.id FROM raids r WHERE l.broadcast_id=$1 AND l.viewer_key=$2 AND l.raid_id IS NULL AND r.id=$3 AND r.target_broadcast_id=$1 AND r.status IN ('countdown','moved') AND l.created_at>=r.execute_at-interval '15 seconds' AND l.created_at<=r.execute_at+interval '60 seconds'")
        .bind(broadcast).bind(key).bind(raid).execute(&app.db).await?;
    Ok(())
}
/// A raid into this broadcast in the last two minutes explains a burst of arrivals.
pub async fn explains_burst(db: &mut PgConnection, broadcast: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM raids WHERE target_broadcast_id=$1 AND status IN ('countdown','moved') AND execute_at>now()-interval '2 minutes')")
        .bind(broadcast).fetch_one(db).await?)
}

/// Runs with the 5-second stream pass: due raids, arrival counts, and host starts and stops.
pub async fn tick(app: &App) -> Res<()> {
    let due: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM raids WHERE status='countdown' AND execute_at<=now() LIMIT 100",
    )
    .fetch_all(&app.db)
    .await?;
    for id in due {
        execute(app, &id).await?;
    }
    // Arrivals: leases that actually started on the target within 60 seconds and came from the
    // raid, never the raider's own viewer count.
    let counted: Vec<(String, i64, String)> = sqlx::query_as("UPDATE raids r SET arrivals=(SELECT count(*) FROM playback_leases l WHERE l.raid_id=r.id AND l.broadcast_id=r.target_broadcast_id),counted_at=now() FROM channel_users c WHERE c.id=r.raider_id AND r.status='moved' AND r.counted_at IS NULL AND r.execute_at<=now()-interval '60 seconds' RETURNING r.target_id,r.arrivals::bigint,c.display_name")
        .fetch_all(&app.db)
        .await?;
    for (target, arrivals, raider) in counted {
        app.chat.publish(
            &target,
            None,
            0,
            json!({"type":"system","text":format!("{raider} is raiding with {arrivals}")}),
        );
    }
    let mut tx = app.db.begin().await?;
    // Hosting stops when the host goes live, the target goes offline or the rules no longer allow it.
    sqlx::query("DELETE FROM host_state h WHERE EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=h.host_id AND b.state<>'ENDED') OR NOT EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=h.target_id AND b.state IN ('LIVE','RECONNECTING')) OR viewer_send_refusal(h.host_id,h.target_id,true) IS NOT NULL")
        .execute(&mut *tx).await?;
    // A raider whose broadcast ended hosts the raid target (once, within a day of the raid).
    sqlx::query("WITH done AS (UPDATE raids r SET hosted=true WHERE r.status='moved' AND NOT r.hosted AND r.execute_at>now()-interval '1 day' AND NOT EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=r.raider_id AND b.state<>'ENDED') RETURNING r.raider_id,r.target_id,r.execute_at) INSERT INTO host_state(host_id,target_id,source) SELECT DISTINCT ON (raider_id) raider_id,target_id,'raid' FROM done WHERE viewer_send_refusal(raider_id,target_id,true) IS NULL AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=target_id AND b.state IN ('LIVE','RECONNECTING')) ORDER BY raider_id,execute_at DESC ON CONFLICT(host_id) DO NOTHING")
        .execute(&mut *tx).await?;
    // Auto-host: an offline channel hosting nothing takes the first live, eligible channel on its list.
    sqlx::query("INSERT INTO host_state(host_id,target_id,source) SELECT s.user_id,t.id,'auto' FROM channel_host_settings s CROSS JOIN LATERAL (SELECT x.id FROM unnest(s.auto_list) WITH ORDINALITY x(id,n) WHERE EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=x.id AND b.state IN ('LIVE','RECONNECTING')) AND viewer_send_refusal(s.user_id,x.id,true) IS NULL ORDER BY x.n LIMIT 1) t WHERE s.auto_host AND NOT EXISTS(SELECT 1 FROM host_state h WHERE h.host_id=s.user_id) AND NOT EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=s.user_id AND b.state<>'ENDED') ON CONFLICT(host_id) DO NOTHING")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// The hosted channel shown on an offline channel's page; the host shows no count of its own.
pub async fn hosting(app: &App, host: &str) -> Res<Option<Value>> {
    let row: Option<(String, String, String)> = sqlx::query_as("SELECT c.username,c.display_name,h.source FROM host_state h JOIN channel_users c ON c.id=h.target_id AND c.eligible WHERE h.host_id=$1")
        .bind(host)
        .fetch_optional(&app.db)
        .await?;
    Ok(row.map(|(username, display_name, source)| {
        json!({"username":username,"display_name":display_name,"source":source})
    }))
}

/// GET /api/me/hosting: settings, auto-host list, raid blocks, current host and raid.
async fn settings(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    settings_json(&app, &user.id).await
}
async fn settings_json(app: &App, user: &str) -> Res<Json<Value>> {
    let (accept_raids, accept_hosts, auto_host): (bool, bool, bool) = sqlx::query_as("SELECT coalesce(s.accept_raids,true),coalesce(s.accept_hosts,true),coalesce(s.auto_host,false) FROM (SELECT 1) one LEFT JOIN channel_host_settings s ON s.user_id=$1")
        .bind(user)
        .fetch_one(&app.db)
        .await?;
    let auto_list: Vec<String> = sqlx::query_scalar("SELECT c.username FROM channel_host_settings s CROSS JOIN LATERAL unnest(s.auto_list) WITH ORDINALITY x(id,n) JOIN channel_users c ON c.id=x.id WHERE s.user_id=$1 AND c.deleted_at IS NULL ORDER BY x.n")
        .bind(user)
        .fetch_all(&app.db)
        .await?;
    let blocks: Vec<String> = sqlx::query_scalar("SELECT c.username FROM raid_blocks b JOIN channel_users c ON c.id=b.blocked_id WHERE b.channel_id=$1 AND c.deleted_at IS NULL ORDER BY lower(c.username)")
        .bind(user)
        .fetch_all(&app.db)
        .await?;
    let (live, broadcast): (bool, Option<String>) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1 AND state<>'ENDED'),(SELECT id FROM broadcasts WHERE owner_id=$1 AND state='LIVE')")
        .bind(user)
        .fetch_one(&app.db)
        .await?;
    let raid = match broadcast {
        Some(b) => for_viewers(app, &b, None).await?,
        None => None,
    };
    Ok(Json(
        json!({"accept_raids":accept_raids,"accept_hosts":accept_hosts,"auto_host":auto_host,
        "auto_list":auto_list,"raid_blocks":blocks,"live":live,"hosting":hosting(app, user).await?,"raid":raid}),
    ))
}
#[derive(Deserialize)]
struct Settings {
    accept_raids: bool,
    accept_hosts: bool,
    auto_host: bool,
    auto_list: Vec<String>,
}
/// PUT /api/me/hosting
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if input.auto_list.len() > 10 {
        return Err(Fail::field(
            "auto_list",
            "Auto-host lists up to 10 channels.",
        ));
    }
    let mut db = app.db.acquire().await?;
    let mut ids: Vec<String> = Vec::new();
    for name in &input.auto_list {
        let name = name.trim().trim_start_matches('@');
        let channel = profiles::eligible_by_name(&mut db, name)
            .await?
            .ok_or_else(|| Fail::field_owned("auto_list", format!("@{name} isn't a channel.")))?;
        if channel.id == user.id {
            return Err(Fail::field(
                "auto_list",
                "You can't auto-host your own channel.",
            ));
        }
        if !ids.contains(&channel.id) {
            ids.push(channel.id);
        }
    }
    sqlx::query("INSERT INTO channel_host_settings(user_id,accept_raids,accept_hosts,auto_host,auto_list) VALUES($1,$2,$3,$4,$5) ON CONFLICT(user_id) DO UPDATE SET accept_raids=$2,accept_hosts=$3,auto_host=$4,auto_list=$5")
        .bind(&user.id).bind(input.accept_raids).bind(input.accept_hosts).bind(input.auto_host).bind(&ids)
        .execute(&mut *db).await?;
    drop(db);
    settings_json(&app, &user.id).await
}
#[derive(Deserialize)]
struct HostTarget {
    username: Option<String>,
}
/// PUT /api/me/hosting/target: host a live channel while offline, or stop (null).
async fn host(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<HostTarget>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    match input
        .username
        .as_deref()
        .map(|n| n.trim().trim_start_matches('@'))
    {
        None | Some("") => {
            sqlx::query("DELETE FROM host_state WHERE host_id=$1")
                .bind(&user.id)
                .execute(&mut *tx)
                .await?;
        }
        Some(name) => {
            let live: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM broadcasts WHERE owner_id=$1 AND state<>'ENDED')",
            )
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
            if live {
                return Err(Fail::bad(
                    "You can host another channel while you're offline.",
                ));
            }
            let (target_id, _) = target(&mut tx, &user.id, name, true).await?;
            sqlx::query("INSERT INTO host_state(host_id,target_id,source) VALUES($1,$2,'manual') ON CONFLICT(host_id) DO UPDATE SET target_id=EXCLUDED.target_id,source='manual',started_at=now()")
                .bind(&user.id).bind(&target_id).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    settings_json(&app, &user.id).await
}
/// POST /api/me/raid-blocks: refuse raids and hosts from a channel.
async fn block(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Target>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let channel =
        profiles::eligible_by_name(&mut db, input.username.trim().trim_start_matches('@'))
            .await?
            .ok_or_else(Fail::channel_missing)?;
    if channel.id == user.id {
        return Err(Fail::bad("You can't block your own channel."));
    }
    sqlx::query(
        "INSERT INTO raid_blocks(channel_id,blocked_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(&user.id)
    .bind(&channel.id)
    .execute(&mut *db)
    .await?;
    drop(db);
    settings_json(&app, &user.id).await
}
/// DELETE /api/me/raid-blocks/{username}
async fn unblock(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    sqlx::query("DELETE FROM raid_blocks b USING users u WHERE b.channel_id=$1 AND b.blocked_id=u.id AND lower(u.username)=lower($2)")
        .bind(&user.id)
        .bind(&name)
        .execute(&app.db)
        .await?;
    settings_json(&app, &user.id).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/raids", post(start).delete(cancel))
        .route("/api/raids/{id}", get(status))
        .route("/api/me/hosting", get(settings).put(save))
        .route("/api/me/hosting/target", put(host))
        .route("/api/me/raid-blocks", post(block))
        .route("/api/me/raid-blocks/{username}", delete(unblock))
}
