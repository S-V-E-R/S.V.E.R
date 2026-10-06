//! Co-stream lifecycle and shared-chat authorization. Media remains per broadcaster.
use crate::{
    App, auth, moderation,
    profiles::{self, Fail, Res},
    streams,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};

#[derive(FromRow)]
struct Squad {
    id: String,
    host_id: Option<String>,
    mode: String,
    ended_at: Option<DateTime<Utc>>,
}
async fn lock(db: &mut PgConnection) -> Res<()> {
    // ponytail: infrequent squad changes share one lock; partition if contention is measured.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('squad-membership',0))")
        .execute(db)
        .await?;
    Ok(())
}
async fn load(db: &mut PgConnection, id: &str) -> Res<Squad> {
    sqlx::query_as("SELECT id,host_id,mode,ended_at FROM squads WHERE id=$1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(Fail::missing)
}
async fn roster(db: &mut PgConnection, id: &str) -> Res<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT user_id FROM squad_members WHERE squad_id=$1 ORDER BY joined_at,user_id",
    )
    .bind(id)
    .fetch_all(db)
    .await?)
}
async fn available(db: &mut PgConnection, user: &str) -> Res<String> {
    let u = profiles::channel_user_by_id(db, user)
        .await?
        .ok_or_else(Fail::missing)?;
    if !u.eligible || !u.email_verified {
        return Err(Fail::denied(
            "A verified account in good standing is required.",
        ));
    }
    streams::live_broadcast(db, user)
        .await?
        .ok_or_else(|| Fail::conflict("Start your stream before joining a co-stream."))
}
async fn pair_allowed(app: &App, db: &mut PgConnection, a: &str, b: &str) -> Res<bool> {
    Ok(!profiles::blocked_between(db, a, b).await?
        && !moderation::banned(app, a, b).await?
        && !moderation::banned(app, b, a).await?)
}
async fn end(db: &mut PgConnection, id: &str) -> Res<()> {
    sqlx::query("UPDATE squads SET ended_at=coalesce(ended_at,now()) WHERE id=$1")
        .bind(id)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM squad_members WHERE squad_id=$1")
        .bind(id)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM squad_invites WHERE squad_id=$1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
/// Repairs live membership on every read/mutation as well as in the worker.
async fn reconcile(app: &App, db: &mut PgConnection, s: &Squad) -> Res<()> {
    if s.ended_at.is_some() {
        return Ok(());
    }
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT user_id,broadcast_id FROM squad_members WHERE squad_id=$1 ORDER BY joined_at,user_id").bind(&s.id).fetch_all(&mut *db).await?;
    let mut present = Vec::<String>::new();
    // The host is checked first; a reconnect retains the same broadcast identity.
    let mut rows = rows;
    rows.sort_by_key(|r| s.host_id.as_deref() != Some(&r.0));
    for (user, broadcast) in rows {
        let good = profiles::channel_user_by_id(db, &user)
            .await?
            .is_some_and(|u| u.eligible)
            && streams::broadcast_active(db, &user, &broadcast).await?;
        if !good && s.host_id.as_deref() == Some(&user) {
            return end(db, &s.id).await;
        }
        let mut compatible = good;
        for other in &present {
            if !pair_allowed(app, db, other, &user).await? {
                compatible = false;
                break;
            }
        }
        if compatible {
            present.push(user);
        } else {
            sqlx::query("DELETE FROM squad_members WHERE squad_id=$1 AND user_id=$2")
                .bind(&s.id)
                .bind(user)
                .execute(&mut *db)
                .await?;
        }
    }
    if !s.host_id.as_ref().is_some_and(|h| present.contains(h)) {
        end(db, &s.id).await?;
    }
    Ok(())
}
async fn current(app: &App, id: &str) -> Res<(Squad, Vec<String>)> {
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let s = load(&mut tx, id).await?;
    reconcile(app, &mut tx, &s).await?;
    let s = load(&mut tx, id).await?;
    let members = roster(&mut tx, id).await?;
    tx.commit().await?;
    Ok((s, members))
}
pub async fn tick(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let rows: Vec<Squad> =
        sqlx::query_as("SELECT id,host_id,mode,ended_at FROM squads WHERE ended_at IS NULL")
            .fetch_all(&mut *tx)
            .await?;
    for s in rows {
        reconcile(app, &mut tx, &s).await?;
    }
    sqlx::query("DELETE FROM squad_invites WHERE expires_at<=now()")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
#[derive(Deserialize)]
struct Create {
    mode: String,
}
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Create>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if !["SEPARATE", "MERGED"].contains(&input.mode.as_str()) {
        return Err(Fail::bad("Choose separate or merged chat."));
    }
    profiles::rate(&app, format!("squad-create:{}", user.id), 10, 3600).await?;
    tick(&app).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let broadcast = available(&mut tx, &user.id).await?;
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM squad_members WHERE user_id=$1)")
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(Fail::conflict("Leave your current co-stream first."));
    }
    let id = profiles::new_id();
    sqlx::query("INSERT INTO squads(id,host_id,mode) VALUES($1,$2,$3)")
        .bind(&id)
        .bind(&user.id)
        .bind(input.mode)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO squad_members(squad_id,user_id,broadcast_id) VALUES($1,$2,$3)")
        .bind(&id)
        .bind(&user.id)
        .bind(broadcast)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
#[derive(Deserialize)]
struct Invite {
    username: String,
}
async fn invite(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Invite>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::rate(&app, format!("squad-invite:{}", user.id), 20, 3600).await?;
    tick(&app).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let s = load(&mut tx, &id).await?;
    if s.ended_at.is_some() || s.host_id.as_deref() != Some(&user.id) {
        return Err(Fail::denied("Only the live host can invite streamers."));
    }
    available(&mut tx, &user.id).await?;
    let target = profiles::eligible_by_name(&mut tx, input.username.trim().trim_start_matches('@'))
        .await?
        .ok_or_else(Fail::channel_missing)?;
    available(&mut tx, &target.id).await?;
    let busy: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM squad_members WHERE user_id=$1)")
            .bind(&target.id)
            .fetch_one(&mut *tx)
            .await?;
    if busy {
        return Err(Fail::conflict("That streamer is already in a co-stream."));
    }
    let members = roster(&mut tx, &id).await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM squad_invites WHERE squad_id=$1 AND expires_at>now()",
    )
    .bind(&id)
    .fetch_one(&mut *tx)
    .await?;
    if members.len() as i64 + pending >= 4 {
        return Err(Fail::conflict(
            "A co-stream has up to four streams, including pending invitations.",
        ));
    }
    for member in members {
        if !pair_allowed(&app, &mut tx, &member, &target.id).await? {
            return Err(Fail::denied(
                "A block or channel ban prevents this invitation.",
            ));
        }
    }
    let invite = profiles::new_id();
    let added=sqlx::query("INSERT INTO squad_invites(id,squad_id,user_id) VALUES($1,$2,$3) ON CONFLICT(squad_id,user_id) DO NOTHING").bind(&invite).bind(&id).bind(&target.id).execute(&mut *tx).await?.rows_affected();
    if added == 0 {
        return Err(Fail::conflict("An invitation is already pending."));
    }
    crate::alerts::community(&mut tx,&target.id,&user.id,"squad_invite",&invite,None,&json!({"title":"Co-stream invitation","body":format!("{} invited you to a co-stream.",user.username),"url":format!("/squads/{id}")})).await?;
    tx.commit().await?;
    Ok(Json(json!({"invited":true})))
}
#[derive(Deserialize)]
struct Answer {
    accept: bool,
}
async fn answer(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Answer>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    tick(&app).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let s = load(&mut tx, &id).await?;
    let invite:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM squad_invites WHERE squad_id=$1 AND user_id=$2 AND expires_at>now())").bind(&id).bind(&user.id).fetch_one(&mut *tx).await?;
    if !invite || s.ended_at.is_some() {
        return Err(Fail::conflict("This invitation has expired."));
    }
    if input.accept {
        let broadcast = available(&mut tx, &user.id).await?;
        let members = roster(&mut tx, &id).await?;
        if members.len() >= 4
            || sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM squad_members WHERE user_id=$1)",
            )
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?
        {
            return Err(Fail::conflict(
                "The co-stream is full or you already joined one.",
            ));
        }
        for member in members {
            if !pair_allowed(&app, &mut tx, &member, &user.id).await? {
                return Err(Fail::denied("A block or channel ban prevents joining."));
            }
        }
        sqlx::query("INSERT INTO squad_members(squad_id,user_id,broadcast_id) VALUES($1,$2,$3)")
            .bind(&id)
            .bind(&user.id)
            .bind(broadcast)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM squad_invites WHERE squad_id=$1 AND user_id=$2")
        .bind(&id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"accepted":input.accept})))
}
async fn leave(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let s = load(&mut tx, &id).await?;
    if s.host_id.as_deref() == Some(&user.id) {
        end(&mut tx, &id).await?;
    } else {
        sqlx::query("DELETE FROM squad_members WHERE squad_id=$1 AND user_id=$2")
            .bind(&id)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"left":true})))
}
async fn cancel(
    State(app): State<App>,
    jar: CookieJar,
    Path((id, username)): Path<(String, String)>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let s = load(&mut tx, &id).await?;
    if s.host_id.as_deref() != Some(&user.id) {
        return Err(Fail::denied("Only the host can cancel invitations."));
    }
    if let Some(target) = profiles::account_by_name(&mut tx, &username).await? {
        sqlx::query("DELETE FROM squad_invites WHERE squad_id=$1 AND user_id=$2")
            .bind(id)
            .bind(target.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"cancelled":true})))
}
async fn page(State(app): State<App>, jar: CookieJar, Path(id): Path<String>) -> Res<Json<Value>> {
    let viewer = profiles::viewer(&app, &jar).await?;
    let (s, ids) = current(&app, &id).await?;
    let mut db = app.db.acquire().await?;
    let users = profiles::public_channels(&mut db, &ids).await?;
    let mut members = Vec::new();
    for u in users {
        if let Some(v) = &viewer
            && (profiles::blocked_between(&mut db, &v.id, &u.id).await?
                || moderation::banned(&app, &u.id, &v.id).await?)
        {
            continue;
        }
        let mut member = profiles::chip(&app, &u);
        member["host"] = json!(s.host_id.as_deref() == Some(&u.id));
        members.push(member);
    }
    members.sort_by_key(|m| !m["host"].as_bool().unwrap_or(false));
    let (joined, host, invited, pending) = if let Some(v) = &viewer {
        let host = s.host_id.as_deref() == Some(&v.id);
        let invited:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM squad_invites WHERE squad_id=$1 AND user_id=$2 AND expires_at>now())").bind(&id).bind(&v.id).fetch_one(&mut *db).await?;
        let pending: Vec<Value> = if host {
            sqlx::query_scalar("SELECT jsonb_build_object('username',c.username,'expires_at',i.expires_at) FROM squad_invites i JOIN channel_users c ON c.id=i.user_id WHERE i.squad_id=$1 AND i.expires_at>now() ORDER BY i.created_at").bind(&id).fetch_all(&mut *db).await?
        } else {
            vec![]
        };
        (ids.contains(&v.id), host, invited, pending)
    } else {
        (false, false, false, vec![])
    };
    Ok(Json(
        json!({"id":id,"mode":s.mode,"ended":s.ended_at.is_some(),"members":members,"joined":joined,"host":host,"invited":invited,"pending":pending}),
    ))
}
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    tick(&app).await?;
    let current: Option<String> =
        sqlx::query_scalar("SELECT squad_id FROM squad_members WHERE user_id=$1")
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    let invites:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'host',c.username,'mode',s.mode,'expires_at',i.expires_at) FROM squad_invites i JOIN squads s ON s.id=i.squad_id JOIN channel_users c ON c.id=s.host_id WHERE i.user_id=$1 AND i.expires_at>now() AND s.ended_at IS NULL ORDER BY i.created_at").bind(&user.id).fetch_all(&app.db).await?;
    let live = streams::live_broadcast(&mut *app.db.acquire().await?, &user.id)
        .await?
        .is_some();
    Ok(Json(
        json!({"current":current,"invites":invites,"live":live}),
    ))
}

/// A live co-stream: its id, mode and members (host first) with their broadcasts.
pub struct Active {
    pub id: String,
    pub mode: String,
    pub members: Vec<(String, String)>,
}
/// Every live co-stream (MAGNet, docs/MAGNET.md "Co-streams on MAGNet"). Membership is repaired
/// by the worker tick; callers still check each member's own eligibility.
pub async fn active(db: &mut PgConnection) -> Res<Vec<Active>> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as("SELECT s.id,s.mode,m.user_id,m.broadcast_id FROM squads s JOIN squad_members m ON m.squad_id=s.id WHERE s.ended_at IS NULL ORDER BY s.id,m.user_id IS DISTINCT FROM s.host_id,m.joined_at,m.user_id")
        .fetch_all(db)
        .await?;
    let mut out: Vec<Active> = Vec::new();
    for (id, mode, user, broadcast) in rows {
        match out.last_mut() {
            Some(a) if a.id == id => a.members.push((user, broadcast)),
            _ => out.push(Active {
                id,
                mode,
                members: vec![(user, broadcast)],
            }),
        }
    }
    Ok(out)
}
/// The live co-stream a channel is in, if any.
pub async fn active_of(db: &mut PgConnection, user: &str) -> Res<Option<Active>> {
    Ok(active(db)
        .await?
        .into_iter()
        .find(|a| a.members.iter().any(|m| m.0 == user)))
}

/// Chat calls this on reads, sends and socket refreshes. Channel bans apply across all members.
pub async fn lock_chat(db: &mut PgConnection, id: &str, expected: &[String]) -> Res<()> {
    lock(db).await?;
    let s = load(db, id).await?;
    let members = roster(db, id).await?;
    if s.ended_at.is_some()
        || s.mode != "MERGED"
        || members.len() != expected.len()
        || !members.iter().all(|m| expected.contains(m))
    {
        return Err(Fail::conflict("The co-stream changed. Please try again."));
    }
    Ok(())
}
/// Channel bans and personal blocks restrict shared-chat access immediately.
pub async fn chat_context(app: &App, id: &str, viewer: Option<&str>) -> Res<Vec<String>> {
    let (s, members) = current(app, id).await?;
    if s.ended_at.is_some() || s.mode != "MERGED" {
        return Err(Fail::conflict(
            "This shared chat has ended or is unavailable.",
        ));
    }
    if let Some(v) = viewer {
        let mut db = app.db.acquire().await?;
        let restricted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM squad_restrictions WHERE squad_id=$1 AND user_id=$2 AND (kind='ban' OR until>now()))").bind(id).bind(v).fetch_one(&mut *db).await?;
        if restricted {
            return Err(Fail::denied("You're restricted from this shared chat."));
        }
        for member in &members {
            if moderation::banned(app, member, v).await?
                || profiles::blocked_between(&mut db, member, v).await?
            {
                return Err(Fail::denied(
                    "A member's channel ban or block prevents shared chat access.",
                ));
            }
        }
    }
    let host = s.host_id.ok_or_else(Fail::missing)?;
    let mut members = members;
    members.sort_by_key(|m| m != &host);
    Ok(members)
}
pub async fn chat_role(
    app: &App,
    members: &[String],
    user: &auth::User,
) -> Res<Option<moderation::Role>> {
    if profiles::channel_user_by_id(&mut *app.db.acquire().await?, &user.id)
        .await?
        .is_none_or(|u| !u.eligible || !u.email_verified)
    {
        return Ok(None);
    }
    if members.contains(&user.id) {
        return Ok(Some(moderation::Role::Owner));
    }
    for member in members {
        if let Some(role) = moderation::role_of(app, member, user).await? {
            return Ok(Some(role));
        }
    }
    Ok(None)
}
async fn chat_actor(app: &App, jar: &CookieJar, id: &str) -> Res<(auth::User, Vec<String>)> {
    let user = profiles::signed_in(app, jar).await?;
    let members = chat_context(app, id, Some(&user.id)).await?;
    if chat_role(app, &members, &user).await?.is_none() {
        return Err(Fail::denied("You can't moderate this shared chat."));
    }
    Ok((user, members))
}
async fn chat_moderation(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let (user, members) = chat_actor(&app, &jar, &id).await?;
    let mut restrictions:Vec<Value>=sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('user',{},'kind',r.kind,'until',r.until) FROM squad_restrictions r JOIN channel_users c ON c.id=r.user_id WHERE r.squad_id=$1 AND (r.kind='ban' OR r.until>now())",profiles::chip_sql("c")))).bind(&id).fetch_all(&app.db).await?;
    for r in &mut restrictions {
        profiles::hydrate(&app, r);
    }
    Ok(Json(
        json!({"role":chat_role(&app,&members,&user).await?.map(|r|r.name()),"restrictions":restrictions}),
    ))
}
#[derive(Deserialize)]
struct Restrict {
    username: String,
    kind: String,
    seconds: Option<i64>,
    reason: String,
}
async fn restrict(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Restrict>,
) -> Res<Json<Value>> {
    let (user, members) = chat_actor(&app, &jar, &id).await?;
    let reason = moderation::reason(&input.reason)?;
    let until = match (input.kind.as_str(), input.seconds) {
        ("ban", None) => None,
        ("timeout", Some(s)) if (60..=1_209_600).contains(&s) => {
            Some(Utc::now() + Duration::seconds(s))
        }
        _ => {
            return Err(Fail::bad(
                "Choose a ban or timeout of one minute to 14 days.",
            ));
        }
    };
    let mut tx = app.db.begin().await?;
    let target = profiles::account_by_name(&mut tx, &input.username)
        .await?
        .ok_or_else(Fail::missing)?;
    lock_chat(&mut tx, &id, &members).await?;
    // Every channel's protected roles remain protected in the shared room.
    for channel in &members {
        if moderation::protected(&app, channel, &user.id, &target.id).await? {
            return Err(Fail::denied("You can't restrict this person here."));
        }
    }
    sqlx::query("INSERT INTO squad_restrictions(squad_id,user_id,kind,until) VALUES($1,$2,$3,$4) ON CONFLICT(squad_id,user_id,kind) DO UPDATE SET until=$4").bind(&id).bind(&target.id).bind(&input.kind).bind(until).execute(&mut *tx).await?;
    crate::safety::audit(
        &mut tx,
        Some(&user.id),
        &input.kind,
        "squad",
        &id,
        &[],
        &reason,
        json!({"target":target.id,"until":until}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
struct Reason {
    reason: String,
}
async fn lift(
    State(app): State<App>,
    jar: CookieJar,
    Path((id, name, kind)): Path<(String, String, String)>,
    Json(input): Json<Reason>,
) -> Res<Json<Value>> {
    let (user, members) = chat_actor(&app, &jar, &id).await?;
    let reason = moderation::reason(&input.reason)?;
    if !matches!(kind.as_str(), "ban" | "timeout") {
        return Err(Fail::bad("Choose a ban or timeout."));
    }
    let mut tx = app.db.begin().await?;
    lock_chat(&mut tx, &id, &members).await?;
    let target = profiles::account_by_name(&mut tx, &name)
        .await?
        .ok_or_else(Fail::missing)?;
    sqlx::query("DELETE FROM squad_restrictions WHERE squad_id=$1 AND user_id=$2 AND kind=$3")
        .bind(&id)
        .bind(&target.id)
        .bind(&kind)
        .execute(&mut *tx)
        .await?;
    crate::safety::audit(
        &mut tx,
        Some(&user.id),
        "lift",
        "squad",
        &id,
        &[],
        &reason,
        json!({"target":target.id,"kind":kind}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
async fn delete_message(
    State(app): State<App>,
    jar: CookieJar,
    Path((id, message)): Path<(String, String)>,
    Json(input): Json<Reason>,
) -> Res<Json<Value>> {
    let (user, members) = chat_actor(&app, &jar, &id).await?;
    crate::chat::remove_shared(&app, &id, &message, &user, &members, &input.reason).await?;
    Ok(Json(json!({"deleted":true})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/squads", get(mine).post(create))
        .route("/api/squads/{id}", get(page))
        .route("/api/squads/{id}/invites", post(invite))
        .route("/api/squads/{id}/invites/{username}", delete(cancel))
        .route("/api/squads/{id}/answer", post(answer))
        .route("/api/squads/{id}/leave", post(leave))
        .route(
            "/api/squads/{id}/chat",
            get(crate::chat::squad_read).post(crate::chat::squad_post),
        )
        .route("/api/squads/{id}/chat/moderation", get(chat_moderation))
        .route(
            "/api/squads/{id}/chat/messages/{message}",
            delete(delete_message),
        )
        .route("/api/squads/{id}/chat/restrictions", post(restrict))
        .route(
            "/api/squads/{id}/chat/restrictions/{username}/{kind}",
            delete(lift),
        )
}
