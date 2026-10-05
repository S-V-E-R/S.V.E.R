//! Reports, reporter feedback, strikes, appeals and the admin review queue (docs/PROFILES.md, "Safety").
// Row tuples from runtime sqlx queries read more clearly inline than as aliases.
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
use crate::{
    App, auth,
    auth::User,
    media,
    profiles::{self, CursorQuery, Fail, Res, make_cursor, new_id, parse_cursor},
    text,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::str::FromStr;
#[path = "safety_take_down.rs"]
pub mod take_down;

pub const REASONS: &[&str] = &[
    "spam",
    "harassment",
    "hate",
    "sexual",
    "violence",
    "impersonation",
    "private_information",
    "copyright",
    "other",
];
pub const FIELDS: &[&str] = &[
    "display_name",
    "username",
    "avatar",
    "banner",
    "bio",
    "status",
    "mood",
    "links",
    "song",
    "war_council",
    "sponsors",
    "setup",
    "blocks",
    "header",
];
const TARGETS: &[&str] = &[
    "faction_post",
    "emote",
    "profile",
    "wall_post",
    "wall_reply",
    "fan_art",
    "setup_photo",
    "chat_message",
    "live_stream",
];
pub const STANDING_MAIL: &str =
    "There's an update to your account standing. See sver.tv/settings/standing.";
pub const REPORT_MAIL: &str =
    "Action was taken on a report you submitted. See sver.tv/settings/reports.";
const LEVEL2_NOTE: &str = "This restriction lasts 72 hours. Appeals are usually decided after it ends. If your appeal succeeds, the strike is removed from your record and no longer counts toward future penalties.";

/// The stored end of an indefinite restriction (a far-future timestamp chrono can decode).
pub fn indefinite() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(9999, 12, 31, 0, 0, 0).unwrap()
}
fn restriction_json(until: Option<DateTime<Utc>>) -> Value {
    match until {
        Some(at) if at > Utc::now() => {
            json!({"until": if at >= indefinite() { Value::Null } else { json!(at) }, "indefinite": at >= indefinite()})
        }
        _ => Value::Null,
    }
}
pub(crate) fn date_in(at: DateTime<Utc>, zone: Option<&str>) -> String {
    let tz = zone
        .and_then(|z| Tz::from_str(z).ok())
        .unwrap_or(chrono_tz::UTC);
    at.with_timezone(&tz).format("%B %-d, %Y").to_string()
}
pub(crate) fn log(action: &str, outcome: &str) {
    // Fixed fields only: no user IDs, content or notes (Login's audit-event rule).
    eprintln!("mod_event={action} outcome={outcome}");
}
pub(crate) fn note(value: Option<&str>, field: &'static str, required: bool) -> Res<String> {
    let value = text::clean(value.unwrap_or(""), field, true)?;
    if required && value.is_empty() {
        return Err(Fail::field(field, "A moderator note is required."));
    }
    if text::count(&value) > 500 {
        return Err(Fail::field(field, "Notes can be up to 500 characters."));
    }
    Ok(value)
}

/// Generic notice through Login's encrypted mail queue; unverified addresses never receive mail.
pub async fn queue_notice(
    app: &App,
    db: &mut PgConnection,
    user_id: &str,
    subject: &str,
    body: &str,
) -> Res<bool> {
    Ok(queue_notice_id(app, db, user_id, subject, body)
        .await?
        .is_some())
}
pub async fn queue_notice_id(
    app: &App,
    db: &mut PgConnection,
    user_id: &str,
    subject: &str,
    body: &str,
) -> Res<Option<String>> {
    let row: Option<(String, bool)> =
        sqlx::query_as("SELECT email,email_verified FROM users WHERE id=$1 AND deleted_at IS NULL")
            .bind(user_id)
            .fetch_optional(&mut *db)
            .await?;
    let Some((email, true)) = row else {
        return Ok(None);
    };
    let payload = json!({"to": [email], "subject": subject, "text": body});
    let id = new_id();
    sqlx::query("INSERT INTO mail_jobs(id,user_id,payload,expires_at) VALUES($1,$2,$3,now()+interval '24 hours')")
        .bind(&id)
        .bind(user_id)
        .bind(crate::security::seal(app, "mail", &payload.to_string())?)
        .execute(&mut *db)
        .await?;
    Ok(Some(id))
}
pub async fn audit(
    db: &mut PgConnection,
    actor: Option<&str>,
    action: &str,
    target_type: &str,
    target_id: &str,
    report_ids: &[String],
    note: &str,
    detail: Value,
    self_review: bool,
) -> Res<()> {
    sqlx::query("INSERT INTO moderation_actions(id,actor_id,action,target_type,target_id,report_ids,note,detail,self_review) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(new_id())
        .bind(actor)
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(report_ids)
        .bind(note)
        .bind(detail)
        .bind(self_review)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Copy of a channel's public fields (one field, or all) for a report or strike snapshot.
pub async fn profile_snapshot(
    db: &mut PgConnection,
    user_id: &str,
    field: Option<&str>,
) -> Res<Value> {
    let base: Value = sqlx::query_scalar("SELECT jsonb_build_object('username',c.username,'display_name',c.display_name,'bio',c.bio,'status',c.status_text,'mood',c.mood_emoji,'avatar',c.avatar_key,'banner',c.banner_key,'song',(SELECT CASE WHEN p.song_url IS NULL THEN NULL ELSE jsonb_build_object('url',p.song_url,'title',p.song_title,'artist',p.song_artist,'thumbnail',p.song_thumb_key) END FROM profiles p WHERE p.user_id=c.id),'links',(SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'platform',platform,'url',url) ORDER BY position),'[]') FROM social_links WHERE user_id=c.id),'war_council',(SELECT coalesce(jsonb_agg(u.username ORDER BY w.position),'[]') FROM war_council w JOIN users u ON u.id=w.member_id WHERE w.user_id=c.id),'sponsors',(SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'name',name,'description',description,'link',link,'discount_code',discount_code,'logo',logo_key) ORDER BY position),'[]') FROM sponsors WHERE user_id=c.id),'setup',(SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'category',category,'name',name,'note',note,'link',link) ORDER BY position),'[]') FROM setup_items WHERE user_id=c.id),'blocks',(SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'type',type,'config',config) ORDER BY position),'[]') FROM profile_blocks WHERE user_id=c.id),'setup_text',(SELECT jsonb_build_object('title',p.setup_title,'description',p.setup_description) FROM profiles p WHERE p.user_id=c.id),'header',(SELECT jsonb_build_object('page_label',p.page_label,'welcome_line',p.welcome_line,'intro_title',p.intro_title,'intro_body',p.intro_body,'page_vibe',p.page_vibe) FROM profiles p WHERE p.user_id=c.id)) FROM channel_users c WHERE c.id=$1")
        .bind(user_id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(Fail::missing)?;
    Ok(match field {
        Some(f) => json!({"field": f, "value": base.get(f).cloned().unwrap_or(Value::Null)}),
        None => json!({"field": null, "value": base}),
    })
}
struct Target {
    owner_id: String,
    owner_name: String,
    target_id: String,
    snapshot: Value,
}
/// Resolves a report target that the reporter can currently see.
async fn target(
    db: &mut PgConnection,
    kind: &str,
    id: &str,
    field: Option<&str>,
    reporter: &str,
) -> Res<Target> {
    let row: Option<(String, String, Value)> = match kind {
        "faction_post" => crate::factions::target(db,id,reporter).await?,
        "emote" => crate::emotes::target(db, id).await?,
        "profile" => {
            let user = profiles::eligible_by_name(db, id).await?.ok_or_else(Fail::missing)?;
            let field = field.filter(|f| FIELDS.contains(f)).ok_or_else(|| Fail::field("field", "Choose what part of the channel you're reporting."))?;
            let snap = profile_snapshot(db, &user.id, Some(field)).await?;
            return Ok(Target { owner_id: user.id.clone(), owner_name: user.username.clone(), target_id: user.id, snapshot: snap });
        }
        "wall_post" => sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('body',p.body) FROM wall_posts p JOIN channel_users c ON c.id=p.author_id JOIN channel_users o ON o.id=p.wall_owner_id WHERE p.id=$1 AND p.status='APPROVED' AND p.deleted_at IS NULL AND o.eligible AND c.deleted_at IS NULL").bind(id).fetch_optional(&mut *db).await?,
        "wall_reply" => sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('body',r.body) FROM wall_replies r JOIN wall_posts p ON p.id=r.post_id JOIN channel_users c ON c.id=r.author_id JOIN channel_users o ON o.id=p.wall_owner_id WHERE r.id=$1 AND r.status='APPROVED' AND r.deleted_at IS NULL AND p.status='APPROVED' AND p.deleted_at IS NULL AND o.eligible AND c.deleted_at IS NULL").bind(id).fetch_optional(&mut *db).await?,
        "fan_art" => sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('image',f.image_key,'artist_name',f.artist_name,'artist_link',f.artist_link,'caption',f.caption) FROM fan_art f JOIN channel_users c ON c.id=f.submitter_id JOIN channel_users o ON o.id=f.channel_id WHERE f.id=$1 AND f.status='APPROVED' AND o.eligible AND c.deleted_at IS NULL").bind(id).fetch_optional(&mut *db).await?,
        "setup_photo" => sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('image',p.image_key||'/400.webp','alt',p.alt) FROM setup_photos p JOIN channel_users c ON c.id=p.user_id WHERE p.id=$1 AND p.status='VISIBLE' AND c.eligible").bind(id).fetch_optional(&mut *db).await?,
        // The snapshot keeps the body, so a chat report outlives the seven-day message expiry.
        "chat_message" => sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('body',m.body,'channel',o.username,'sent_at',m.created_at) FROM chat_messages m JOIN channel_users c ON c.id=m.author_id JOIN channel_users o ON o.id=m.channel_id WHERE m.id=$1 AND m.deleted_at IS NULL AND o.eligible AND c.deleted_at IS NULL").bind(id).fetch_optional(&mut *db).await?,
        // Only a stream that is public now; no video is recorded as evidence.
        "live_stream" => sqlx::query_as("SELECT c.id,c.username,jsonb_build_object('title',coalesce(s.title,c.username||'''s stream'),'category',k.name,'broadcast_id',b.id,'started_at',b.started_at,'reported_at',now()) FROM broadcasts b JOIN channel_users c ON c.id=b.owner_id LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories k ON k.id=s.category_id WHERE b.id=$1 AND b.state IN ('LIVE','RECONNECTING') AND c.eligible").bind(id).fetch_optional(&mut *db).await?,
        _ => return Err(Fail::field("target_type", "Choose what you're reporting.")),
    };
    let (owner_id, owner_name, snapshot) = row.ok_or_else(Fail::missing)?;
    Ok(Target {
        owner_id,
        owner_name,
        target_id: id.to_string(),
        snapshot: json!({"field": null, "value": snapshot}),
    })
}

#[derive(Deserialize)]
pub struct ReportInput {
    target_type: String,
    target_id: String,
    field: Option<String>,
    reason: String,
    note: Option<String>,
}
/// POST /api/reports
pub async fn report(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<ReportInput>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    profiles::ensure_unrestricted(&mut tx, &user.id).await?;
    if !TARGETS.contains(&input.target_type.as_str()) {
        return Err(Fail::field("target_type", "Choose what you're reporting."));
    }
    if !REASONS.contains(&input.reason.as_str()) {
        return Err(Fail::field("reason", "Choose a reason."));
    }
    let note = text::clean(input.note.as_deref().unwrap_or(""), "note", true)?;
    if text::count(&note) > 500 {
        return Err(Fail::field("note", "Notes can be up to 500 characters."));
    }
    let target = target(
        &mut tx,
        &input.target_type,
        &input.target_id,
        input.field.as_deref(),
        &user.id,
    )
    .await?;
    if target.owner_id == user.id {
        return Err(Fail::bad("You can't report your own content."));
    }
    profiles::rate(&app, format!("report:{}", user.id), 10, 3600).await?;
    let field = if input.target_type == "profile" {
        input.field.clone()
    } else {
        None
    };
    // A repeat report on the same open target only updates the note.
    sqlx::query("INSERT INTO reports(id,reporter_id,target_type,target_id,target_user_id,target_username,field,reason,note,snapshot) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT (reporter_id,target_type,target_id) WHERE status='OPEN' DO UPDATE SET note=EXCLUDED.note")
        .bind(new_id())
        .bind(&user.id)
        .bind(&input.target_type)
        .bind(&target.target_id)
        .bind(&target.owner_id)
        .bind(&target.owner_name)
        .bind(field)
        .bind(&input.reason)
        .bind(&note)
        .bind(&target.snapshot)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    log("report_filed", "ok");
    Ok(Json(json!({"message": "Thanks. We'll review this."})))
}

/// GET /api/me/reports: the reporter's own reports from the last 180 days, 20 per page.
pub async fn my_reports(
    State(app): State<App>,
    jar: CookieJar,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let cursor = parse_cursor(&q.cursor)?;
    let mut db = app.db.acquire().await?;
    let rows: Vec<(String, DateTime<Utc>, Value)> = sqlx::query_as("SELECT r.id,r.created_at,jsonb_build_object('id',r.id,'target_type',r.target_type,'username',CASE WHEN u.id IS NULL OR u.deleted_at IS NOT NULL THEN NULL ELSE u.username END,'reason',r.reason,'note',r.note,'created_at',r.created_at,'status',CASE WHEN r.status='OPEN' THEN 'under_review' WHEN r.reporter_notice='ACTION_TAKEN' THEN 'action_taken' ELSE 'closed' END,'notice',CASE WHEN r.reporter_notice='ACTION_TAKEN' THEN 'We reviewed your report and took action. Thanks for helping keep S.V.E.R safe.' END,'unread',r.reporter_notice='ACTION_TAKEN' AND r.reporter_seen_at IS NULL) FROM reports r LEFT JOIN users u ON u.id=r.target_user_id WHERE r.reporter_id=$1 AND r.created_at>now()-interval '180 days' AND ($2::timestamptz IS NULL OR (r.created_at,r.id)<($2,$3)) ORDER BY r.created_at DESC,r.id DESC LIMIT 21")
        .bind(&user.id)
        .bind(cursor.as_ref().map(|c| c.0))
        .bind(cursor.as_ref().map(|c| c.1.clone()).unwrap_or_default())
        .fetch_all(&mut *db)
        .await?;
    let next = (rows.len() > 20).then(|| make_cursor(rows[19].1, &rows[19].0));
    let email: Option<bool> =
        sqlx::query_scalar("SELECT email_report_updates FROM profiles WHERE user_id=$1")
            .bind(&user.id)
            .fetch_optional(&mut *db)
            .await?;
    Ok(Json(
        json!({"reports": rows.into_iter().take(20).map(|r| r.2).collect::<Vec<_>>(), "next_cursor": next, "email_updates": email.unwrap_or(false), "email_available": !app.config.resend_key.is_empty() || !app.config.production}),
    ))
}
pub async fn reports_seen(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query("UPDATE reports SET reporter_seen_at=now() WHERE reporter_id=$1 AND reporter_notice='ACTION_TAKEN' AND reporter_seen_at IS NULL").bind(&user.id).execute(&app.db).await?;
    Ok(Json(json!({"seen": true})))
}
#[derive(Deserialize)]
pub struct Toggle {
    enabled: bool,
}
pub async fn reports_email(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Toggle>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    profiles::ensure_profile(&mut tx, &user.id).await?;
    sqlx::query("UPDATE profiles SET email_report_updates=$2 WHERE user_id=$1")
        .bind(&user.id)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"email_updates": input.enabled})))
}
/// GET /api/me/alerts: unread report notices, unacknowledged strikes and the restriction banner.
pub async fn alerts(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let Some(user) = profiles::viewer(&app, &jar).await? else {
        return Ok(Json(json!({"signed_in": false})));
    };
    let (reports, strikes, until): (i64, i64, Option<DateTime<Utc>>) = sqlx::query_as("SELECT (SELECT count(*) FROM reports WHERE reporter_id=$1 AND reporter_notice='ACTION_TAKEN' AND reporter_seen_at IS NULL),(SELECT count(*) FROM strikes WHERE user_id=$1 AND acknowledged_at IS NULL),(SELECT restricted_until FROM profiles WHERE user_id=$1)")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    let mut db = app.db.acquire().await?;
    let urgent = if user.mfa_enabled && is_staff(&mut db, &user.id).await? {
        crate::take_down::open_count(&mut db).await?
    } else {
        0
    };
    let notifications = crate::alerts::unread(&app, &user.id).await?;
    Ok(Json(
        json!({"signed_in": true, "unread_reports": reports, "new_strikes": strikes, "restriction": restriction_json(until),"urgent_take_down":urgent,"notifications":notifications}),
    ))
}

/// Active-strike level: active strike count capped at 3, or 3 with any active SEVERE strike.
pub async fn level(db: &mut PgConnection, user_id: &str) -> Res<i32> {
    let (count, severe): (i64, bool) = sqlx::query_as("SELECT count(*),coalesce(bool_or(severity='SEVERE'),false) FROM strikes WHERE user_id=$1 AND status='ACTIVE' AND expires_at>now()").bind(user_id).fetch_one(&mut *db).await?;
    Ok(if severe { 3 } else { count.min(3) as i32 })
}
/// Recomputes the cached effective restriction from active strike penalties and open interim restrictions.
pub async fn recompute(db: &mut PgConnection, user_id: &str) -> Res<Option<DateTime<Utc>>> {
    auth::stream_owner(db, user_id).await?;
    profiles::ensure_profile(db, user_id).await?;
    let until: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT max(t) FROM (SELECT CASE WHEN penalty='RESTRICT_INDEFINITE' THEN $2 ELSE penalty_until END AS t FROM strikes WHERE user_id=$1 AND status='ACTIVE' AND penalty_lifted_at IS NULL AND penalty<>'WARNING' UNION ALL SELECT until FROM interim_restrictions WHERE user_id=$1 AND resolution='OPEN' AND until>now() UNION ALL SELECT coalesce(until,$2) FROM account_bans WHERE user_id=$1 AND status='ACTIVE') x WHERE t>now()")
        .bind(user_id)
        .bind(indefinite())
        .fetch_one(&mut *db)
        .await?;
    sqlx::query("UPDATE profiles SET restricted_until=$2 WHERE user_id=$1")
        .bind(user_id)
        .bind(until)
        .execute(&mut *db)
        .await?;
    if until.is_some() {
        crate::streams::revoke(db, user_id).await?;
    }
    Ok(until)
}

fn strike_json(row: &Value, staff: bool) -> Value {
    let mut v = row.clone();
    if let (false, Some(map)) = (staff, v.as_object_mut()) {
        for key in [
            "staff_note",
            "issued_by",
            "report_ids",
            "removed_refs",
            "interim_restriction_id",
            "ban_review_open",
        ] {
            map.remove(key);
        }
    }
    v
}
const STRIKE_SQL: &str = "jsonb_build_object('id',s.id,'reason',s.reason,'severity',s.severity,'content',s.content_snapshot,'penalty',s.penalty,'penalty_starts_at',s.penalty_starts_at,'penalty_until',s.penalty_until,'penalty_lifted_at',s.penalty_lifted_at,'level',s.level,'message_to_user',s.message_to_user,'issued_at',s.issued_at,'expires_at',s.expires_at,'status',CASE WHEN s.status='OVERTURNED' THEN 'overturned' WHEN s.expires_at<=now() THEN 'expired' ELSE 'active' END,'acknowledged',s.acknowledged_at IS NOT NULL,'appeal_closes_at',s.issued_at+interval '14 days','can_appeal',s.status='ACTIVE' AND a.id IS NULL AND s.issued_at+interval '14 days'>now(),'appeal',CASE WHEN a.id IS NULL THEN NULL ELSE jsonb_build_object('status',a.status,'created_at',a.created_at,'body',a.body,'message_to_user',CASE WHEN a.status<>'PENDING' THEN a.message_to_user END,'signed',CASE WHEN a.status<>'PENDING' THEN 'S.V.E.R moderators' END) END,'staff_note',s.staff_note,'issued_by',(SELECT username FROM users WHERE id=s.issued_by),'report_ids',s.report_ids,'removed_refs',s.removed_refs,'interim_restriction_id',s.interim_restriction_id,'ban_review_open',s.ban_review_open)";
async fn strikes(db: &mut PgConnection, user_id: &str, staff: bool) -> Res<Vec<Value>> {
    // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
    let rows: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT {STRIKE_SQL} FROM strikes s LEFT JOIN appeals a ON a.strike_id=s.id WHERE s.user_id=$1 ORDER BY s.issued_at DESC")))
        .bind(user_id)
        .fetch_all(&mut *db)
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let mut v = strike_json(r, staff);
            if v["penalty"] == "RESTRICT_72H" {
                v["note"] = json!(LEVEL2_NOTE);
            }
            v
        })
        .collect())
}
/// GET /api/me/standing (available during a restriction).
pub async fn standing(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let level = level(&mut db, &user.id).await?;
    let until: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT restricted_until FROM profiles WHERE user_id=$1")
            .bind(&user.id)
            .fetch_optional(&mut *db)
            .await?
            .flatten();
    let list = strikes(&mut db, &user.id, false).await?;
    let resets: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('old_username',old_username,'new_username',new_username,'reason',note,'changed_at',changed_at) FROM username_history WHERE user_id=$1 AND reason='staff_reset' ORDER BY changed_at DESC")
        .bind(&user.id)
        .fetch_all(&mut *db)
        .await?;
    Ok(Json(
        json!({"level": level, "restriction": restriction_json(until), "strikes": list, "username_resets": resets}),
    ))
}
pub async fn acknowledge(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let done = sqlx::query("UPDATE strikes SET acknowledged_at=coalesce(acknowledged_at,now()) WHERE id=$1 AND user_id=$2").bind(&id).bind(&user.id).execute(&app.db).await?;
    if done.rows_affected() == 0 {
        return Err(Fail::missing());
    }
    Ok(Json(json!({"acknowledged": true})))
}
#[derive(Deserialize)]
pub struct AppealInput {
    body: String,
    timezone: Option<String>,
}
/// POST /api/me/strikes/{id}/appeal: once per strike, within 14 days of issue.
pub async fn appeal(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<AppealInput>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let strike: Option<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT status,issued_at FROM strikes WHERE id=$1 AND user_id=$2 FOR UPDATE",
    )
    .bind(&id)
    .bind(&user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, issued) = strike.ok_or_else(Fail::missing)?;
    let existing: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM appeals WHERE strike_id=$1)")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
    if existing {
        return Err(Fail::conflict("You've already appealed this strike."));
    }
    if status == "OVERTURNED" {
        return Err(Fail::conflict("This strike can no longer be appealed."));
    }
    let closes = issued + Duration::days(14);
    if closes <= Utc::now() {
        return Err(Fail::conflict(format!(
            "The appeal window for this strike closed on {}.",
            date_in(closes, input.timezone.as_deref())
        )));
    }
    // Plain text, escaped when rendered and deliberately not word-filtered so users can quote.
    let body = text::clean(&input.body, "body", true)?;
    if body.is_empty() || text::count(&body) > 1000 {
        return Err(Fail::field("body", "Appeals can be 1-1000 characters."));
    }
    profiles::rate(&app, format!("appeal:{}", user.id), 5, 86_400).await?;
    sqlx::query("INSERT INTO appeals(id,strike_id,user_id,body) VALUES($1,$2,$3,$4)")
        .bind(new_id())
        .bind(&id)
        .bind(&user.id)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    log("appeal_submitted", "ok");
    Ok(Json(
        json!({"status": "PENDING", "message": "Appeal submitted"}),
    ))
}

// ---- Staff ----

/// Staff for admin reads: role plus MFA. Everyone else (signed out included) gets 404.
pub async fn staff(app: &App, jar: &CookieJar) -> Res<User> {
    let Some(user) = profiles::viewer(app, jar).await? else {
        log("admin_access", "denied");
        return Err(Fail::missing());
    };
    let is_staff: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM staff_roles WHERE user_id=$1 AND role='admin')",
    )
    .bind(&user.id)
    .fetch_one(&app.db)
    .await?;
    if !is_staff {
        log("admin_access", "denied");
        return Err(Fail::missing());
    }
    // Staff without MFA get the same 404 as everyone else (docs/PROFILES.md, acceptance 11).
    if !user.mfa_enabled {
        log("admin_access", "denied");
        return Err(Fail::missing());
    }
    Ok(user)
}
pub async fn is_staff(db: &mut PgConnection, user_id: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM staff_roles WHERE user_id=$1 AND role='admin')",
    )
    .bind(user_id)
    .fetch_one(&mut *db)
    .await?)
}
/// Staff for admin mutations: an MFA-verified session inside the staff window. A sign-in,
/// password confirmation or authenticator confirmation unlocks it for 15 minutes; each staff
/// action extends it by 15 more, up to 8 hours from that confirmation (Joe, October 4, 2026).
pub async fn staff_write(app: &App, jar: &CookieJar) -> Res<User> {
    let user = staff(app, jar).await?;
    let (mut tx, _, session) = auth::session(app, jar, false).await?;
    let unlocked: bool = sqlx::query_scalar("SELECT mfa_verified AND greatest(authenticated_at, staff_confirmed_at) > now()-interval '8 hours' AND (greatest(authenticated_at, staff_confirmed_at) > now()-interval '15 minutes' OR coalesce(staff_active_at > now()-interval '15 minutes', false)) FROM sessions WHERE id=$1")
        .bind(&session.id)
        .fetch_one(&mut *tx)
        .await?;
    if !unlocked {
        tx.commit().await?;
        log("admin_step_up", "denied");
        return Err(Fail::denied(
            "Confirm your sign-in method again before using admin tools.",
        ));
    }
    sqlx::query("UPDATE sessions SET staff_active_at=now() WHERE id=$1")
        .bind(&session.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(user)
}

#[derive(Deserialize)]
pub struct StaffConfirm {
    code: String,
}
/// POST /api/admin/confirm: staff unlock admin tools with an authenticator or recovery code. This
/// never counts as a fresh primary sign-in for account-security changes.
pub async fn staff_confirm(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<StaffConfirm>,
) -> Res<Json<Value>> {
    let user = staff(&app, &jar).await?;
    let (mut tx, _, session) = auth::session(&app, &jar, false).await?;
    if !session.mfa_verified {
        return Err(Fail::denied("Sign in with your authenticator first."));
    }
    let permits =
        crate::security::reserve(&app, vec![format!("staff-confirm:{}", user.id)], 5, 900).await?;
    crate::security::prove_mfa(&app, &mut tx, &user, input.code.trim()).await?;
    crate::security::release(&app, permits).await?;
    sqlx::query("UPDATE sessions SET staff_confirmed_at=now(), staff_active_at=now() WHERE id=$1")
        .bind(&session.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    log("admin_step_up", "confirmed");
    Ok(Json(json!({"confirmed": true})))
}

#[derive(Deserialize, Clone)]
pub struct StrikeInput {
    reason: String,
    severity: String,
    #[serde(default)]
    message_to_user: String,
    interim_restriction_id: Option<String>,
}
/// Issues a strike and applies its penalty; returns the strike ID, level and queued notice ID.
pub async fn issue_strike(
    app: &App,
    db: &mut PgConnection,
    actor: &User,
    user_id: &str,
    input: &StrikeInput,
    report_ids: &[String],
    snapshot: Value,
    removed: Value,
    staff_note: &str,
) -> Res<(String, i32, Option<String>)> {
    let target = profiles::channel_user_by_id(db, user_id)
        .await?
        .ok_or_else(Fail::missing)?;
    if target.internal {
        return Err(Fail::bad("Strikes can't be issued to internal accounts."));
    }
    if !REASONS.contains(&input.reason.as_str()) {
        return Err(Fail::field("reason", "Choose a reason."));
    }
    if input.severity != "STANDARD" && input.severity != "SEVERE" {
        return Err(Fail::field("severity", "Choose a severity."));
    }
    let message = text::clean(&input.message_to_user, "message_to_user", true)?;
    if text::count(&message) > 500 {
        return Err(Fail::field(
            "message_to_user",
            "Messages can be up to 500 characters.",
        ));
    }
    // Lock the account's moderation state so concurrent strikes compute levels in order.
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    let current = level(db, user_id).await?;
    let reached = if input.severity == "SEVERE" {
        3
    } else {
        (current + 1).min(3)
    };
    let now = Utc::now();
    let mut start = now;
    if let Some(interim) = &input.interim_restriction_id {
        let starts: Option<DateTime<Utc>> = sqlx::query_scalar("UPDATE interim_restrictions SET resolution='CONVERTED',resolved_at=now(),resolved_by=$3 WHERE id=$1 AND user_id=$2 AND resolution='OPEN' RETURNING starts_at")
            .bind(interim)
            .bind(user_id)
            .bind(&actor.id)
            .fetch_optional(&mut *db)
            .await?;
        start = starts.ok_or_else(|| {
            Fail::field(
                "interim_restriction_id",
                "That interim restriction is no longer open.",
            )
        })?;
    }
    let (penalty, until) = match reached {
        1 => ("WARNING", None),
        2 => ("RESTRICT_72H", Some(start + Duration::hours(72))),
        _ => ("RESTRICT_INDEFINITE", None),
    };
    let expires = now
        + if input.severity == "SEVERE" {
            Duration::days(365)
        } else {
            Duration::days(90)
        };
    let id = new_id();
    sqlx::query("INSERT INTO strikes(id,user_id,reason,severity,content_snapshot,report_ids,removed_refs,penalty,interim_restriction_id,penalty_starts_at,penalty_until,level,message_to_user,staff_note,issued_by,issued_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)")
        .bind(&id)
        .bind(user_id)
        .bind(&input.reason)
        .bind(&input.severity)
        .bind(&snapshot)
        .bind(report_ids)
        .bind(&removed)
        .bind(penalty)
        .bind(&input.interim_restriction_id)
        .bind(start)
        .bind(until)
        .bind(reached)
        .bind(&message)
        .bind(staff_note)
        .bind(&actor.id)
        .bind(now)
        .bind(expires)
        .execute(&mut *db)
        .await?;
    recompute(db, user_id).await?;
    audit(db, Some(&actor.id), "strike_issued", "user", user_id, report_ids, staff_note, json!({"strike_id": id, "level": reached, "penalty": penalty, "severity": input.severity, "converted_interim": input.interim_restriction_id}), false).await?;
    if input.interim_restriction_id.is_some() {
        audit(
            db,
            Some(&actor.id),
            "interim_restriction_converted",
            "user",
            user_id,
            report_ids,
            staff_note,
            json!({"strike_id": id}),
            false,
        )
        .await?;
    }
    let notice = queue_notice_id(
        app,
        db,
        user_id,
        "Your S.V.E.R account standing",
        STANDING_MAIL,
    )
    .await?;
    if notice.is_some() {
        audit(
            db,
            Some(&actor.id),
            "strike_notice_queued",
            "user",
            user_id,
            &[],
            "",
            json!({"strike_id": id}),
            false,
        )
        .await?;
    }
    log("strike_issued", "ok");
    Ok((id, reached, notice))
}

/// Reset a profile field to its default (Reset field action).
async fn reset_field(
    db: &mut PgConnection,
    user_id: &str,
    field: &str,
    item: Option<&str>,
) -> Res<()> {
    profiles::ensure_profile(db, user_id).await?;
    let section = match field {
        "avatar" | "banner" => {
            let column = if field == "avatar" {
                "avatar_key"
            } else {
                "banner_key"
            };
            let old: Option<String> =
                // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT {column} FROM profiles WHERE user_id=$1")))
                    .bind(user_id)
                    .fetch_one(&mut *db)
                    .await?;
            // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE profiles SET {column}=NULL,updated_at=now() WHERE user_id=$1"
            )))
            .bind(user_id)
            .execute(&mut *db)
            .await?;
            media::queue_delete(db, old.as_deref(), None).await?;
            "profile"
        }
        "display_name" => {
            sqlx::query("UPDATE profiles p SET display_name=u.username,updated_at=now() FROM users u WHERE u.id=p.user_id AND p.user_id=$1").bind(user_id).execute(&mut *db).await?;
            "profile"
        }
        "bio" | "status" | "mood" => {
            let column = match field {
                "bio" => "bio",
                "status" => "status_text",
                _ => "mood_emoji",
            };
            // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE profiles SET {column}='',updated_at=now() WHERE user_id=$1"
            )))
            .bind(user_id)
            .execute(&mut *db)
            .await?;
            "profile"
        }
        "song" => {
            let old: Option<String> =
                sqlx::query_scalar("SELECT song_thumb_key FROM profiles WHERE user_id=$1")
                    .bind(user_id)
                    .fetch_one(&mut *db)
                    .await?;
            sqlx::query("UPDATE profiles SET song_provider=NULL,song_media_id=NULL,song_url=NULL,song_title=NULL,song_artist=NULL,song_thumb_key=NULL,song_notice=NULL,song_updated_at=now() WHERE user_id=$1").bind(user_id).execute(&mut *db).await?;
            media::queue_delete(db, old.as_deref(), None).await?;
            crate::activity::forget(db, user_id, "song").await?;
            "song"
        }
        "war_council" => {
            sqlx::query("DELETE FROM war_council WHERE user_id=$1")
                .bind(user_id)
                .execute(&mut *db)
                .await?;
            crate::activity::forget(db, user_id, "war_council").await?;
            "war_council"
        }
        "header" => {
            sqlx::query("UPDATE profiles SET page_label='',welcome_line='',intro_title='',intro_body='',page_vibe='',header_copy_enabled=true,updated_at=now() WHERE user_id=$1")
                .bind(user_id)
                .execute(&mut *db)
                .await?;
            "header"
        }
        "links" | "sponsors" | "setup" | "blocks" => {
            let table = match field {
                "links" => "social_links",
                "sponsors" => "sponsors",
                "setup" => "setup_items",
                _ => "profile_blocks",
            };
            if field == "setup" && item.is_none() {
                // The whole setup section: title, description and photos too (decision P4).
                sqlx::query("UPDATE profiles SET setup_title='',setup_description='',updated_at=now() WHERE user_id=$1")
                    .bind(user_id)
                    .execute(&mut *db)
                    .await?;
                let photos: Vec<String> = sqlx::query_scalar(
                    "DELETE FROM setup_photos WHERE user_id=$1 RETURNING image_key",
                )
                .bind(user_id)
                .fetch_all(&mut *db)
                .await?;
                for key in photos {
                    crate::studio::release_setup_photo(db, &key).await?;
                }
            }
            if field == "sponsors" {
                let logos: Vec<Option<String>> = sqlx::query_scalar("SELECT logo_key FROM sponsors WHERE user_id=$1 AND ($2::text IS NULL OR id=$2)").bind(user_id).bind(item).fetch_all(&mut *db).await?;
                for logo in logos {
                    media::queue_delete(db, logo.as_deref(), None).await?;
                }
            }
            // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {table} WHERE user_id=$1 AND ($2::text IS NULL OR id=$2)"
            )))
            .bind(user_id)
            .bind(item)
            .execute(&mut *db)
            .await?;
            field
        }
        "username" => {
            return Err(Fail::field(
                "field",
                "Username resets arrive with the Module 3 moderation pass.",
            ));
        }
        _ => return Err(Fail::field("field", "Choose a field to reset.")),
    };
    profiles::bump_section(db, user_id, section, None).await?;
    Ok(())
}
/// Hides a wall post, reply or fan art item; returns the reference used to restore it on overturn.
async fn remove_content(db: &mut PgConnection, kind: &str, id: &str) -> Res<Value> {
    if kind == "faction_post" {
        return crate::factions::remove(db, id).await;
    }
    if kind == "emote" {
        return crate::emotes::remove(db, id).await;
    }
    match kind {
        // A chat delete is a tombstone, like a channel moderator's; it is not restored on appeal.
        "chat_message" => {
            let visible: bool = sqlx::query_scalar(
                "SELECT deleted_at IS NULL FROM chat_messages WHERE id=$1 FOR UPDATE",
            )
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(Fail::missing)?;
            if visible {
                sqlx::query("UPDATE chat_messages SET deleted_at=now() WHERE id=$1")
                    .bind(id)
                    .execute(&mut *db)
                    .await?;
            }
            let previous = if visible { "VISIBLE" } else { "REMOVED" };
            return Ok(json!({"type": kind, "id": id, "previous": previous}));
        }
        // Removing a live stream stops it: the key is revoked and the worker disconnects the
        // publisher. Already-delivered video cannot be recalled, and a stop is never undone.
        "live_stream" => {
            let (owner, state): (String, String) =
                sqlx::query_as("SELECT owner_id,state FROM broadcasts WHERE id=$1")
                    .bind(id)
                    .fetch_optional(&mut *db)
                    .await?
                    .ok_or_else(Fail::missing)?;
            crate::streams::revoke(db, &owner).await?;
            return Ok(json!({"type": kind, "id": id, "previous": state}));
        }
        _ => {}
    }
    let table = match kind {
        "wall_post" => "wall_posts",
        "wall_reply" => "wall_replies",
        "fan_art" => "fan_art",
        "setup_photo" => "setup_photos",
        _ => {
            return Err(Fail::bad(
                "Only wall posts, replies, fan art, setup photos, chat messages and live streams can be removed.",
            ));
        }
    };
    // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
    let previous: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT status FROM {table} WHERE id=$1 FOR UPDATE"
    )))
    .bind(id)
    .fetch_optional(&mut *db)
    .await?;
    let previous = previous.ok_or_else(Fail::missing)?;
    if previous != "REMOVED" {
        let stamp = match kind {
            "fan_art" => ",reviewed_at=now()",
            "setup_photo" => "",
            _ => ",moderated_at=now()",
        };
        // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET status='REMOVED'{stamp} WHERE id=$1"
        )))
        .bind(id)
        .execute(&mut *db)
        .await?;
    }
    Ok(json!({"type": kind, "id": id, "previous": previous}))
}
async fn owner_of(db: &mut PgConnection, kind: &str, id: &str) -> Res<String> {
    let owner: Option<String> = match kind {
        "faction_post" => crate::factions::owner(db, id).await?,
        "emote" => crate::emotes::owner(db, id).await?,
        "profile" => {
            sqlx::query_scalar("SELECT id FROM users WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        "wall_post" => {
            sqlx::query_scalar("SELECT author_id FROM wall_posts WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        "wall_reply" => {
            sqlx::query_scalar("SELECT author_id FROM wall_replies WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        "fan_art" => {
            sqlx::query_scalar("SELECT submitter_id FROM fan_art WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        "setup_photo" => {
            sqlx::query_scalar("SELECT user_id FROM setup_photos WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        "chat_message" => {
            sqlx::query_scalar("SELECT author_id FROM chat_messages WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        "live_stream" => {
            sqlx::query_scalar("SELECT owner_id FROM broadcasts WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *db)
                .await?
        }
        _ => None,
    };
    match owner {
        Some(o) => Ok(o),
        // Fall back to the reports' recorded owner (content may be gone).
        None => sqlx::query_scalar(
            "SELECT target_user_id FROM reports WHERE target_type=$1 AND target_id=$2 LIMIT 1",
        )
        .bind(kind)
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(Fail::missing),
    }
}
async fn current_content(
    db: &mut PgConnection,
    kind: &str,
    id: &str,
    field: Option<&str>,
) -> Res<Value> {
    Ok(match kind {
        "emote" => crate::emotes::current(db, id).await?,
        "profile" => match profile_snapshot(db, id, field).await {
            Ok(v) => v,
            Err(_) => Value::Null,
        },
        "faction_post" => crate::factions::snapshot(db,id).await?,
        "wall_post" => sqlx::query_scalar("SELECT jsonb_build_object('body',body,'status',status,'deleted',deleted_at IS NOT NULL) FROM wall_posts WHERE id=$1").bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null),
        "wall_reply" => sqlx::query_scalar("SELECT jsonb_build_object('body',body,'status',status,'deleted',deleted_at IS NOT NULL) FROM wall_replies WHERE id=$1").bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null),
        "setup_photo" => sqlx::query_scalar("SELECT jsonb_build_object('image',image_key||'/400.webp','alt',alt,'status',status) FROM setup_photos WHERE id=$1").bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null),
        "chat_message" => sqlx::query_scalar("SELECT jsonb_build_object('body',body,'deleted',deleted_at IS NOT NULL) FROM chat_messages WHERE id=$1").bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null),
        "live_stream" => sqlx::query_scalar("SELECT jsonb_build_object('state',state,'started_at',started_at,'ended_at',ended_at) FROM broadcasts WHERE id=$1").bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null),
        _ => sqlx::query_scalar("SELECT jsonb_build_object('image',image_key,'artist_name',artist_name,'caption',caption,'status',status) FROM fan_art WHERE id=$1").bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null),
    })
}
fn media_keys(app: &App, value: &mut Value) {
    // Resolve stored image keys in snapshots to staff-visible URLs.
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if let (true, Some(key)) = (
                    matches!(
                        k.as_str(),
                        "avatar" | "banner" | "image" | "logo" | "thumbnail"
                    ),
                    v.as_str(),
                ) {
                    let url = if k == "avatar" {
                        profiles::avatar_json(app, Some(key))
                    } else if k == "banner" {
                        profiles::banner_json(app, Some(key))
                    } else {
                        json!(profiles::media_url(app, key))
                    };
                    *v = url;
                } else {
                    media_keys(app, v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| media_keys(app, v)),
        _ => {}
    }
}

#[derive(Deserialize)]
pub struct QueueQuery {
    cursor: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    reason: Option<String>,
}
/// GET /api/admin/reports: open reports grouped by target, oldest first, plus open interim restrictions.
pub async fn admin_reports(
    State(app): State<App>,
    jar: CookieJar,
    Query(q): Query<QueueQuery>,
) -> Res<Json<Value>> {
    let _staff = staff(&app, &jar).await?;
    let cursor = parse_cursor(&q.cursor)?;
    let mut db = app.db.acquire().await?;
    let groups: Vec<(String, String, DateTime<Utc>, Value)> = sqlx::query_as("SELECT target_type,target_id,min(created_at) AS oldest,jsonb_build_object('target_type',target_type,'target_id',target_id,'username',(array_agg(target_username ORDER BY created_at DESC))[1],'count',count(*),'reasons',jsonb_agg(DISTINCT reason),'reports',jsonb_agg(jsonb_build_object('id',id,'reason',reason,'note',note,'field',field,'snapshot',snapshot,'created_at',created_at) ORDER BY created_at),'oldest',min(created_at)) FROM reports WHERE status='OPEN' AND ($1::text IS NULL OR target_type=$1) GROUP BY target_type,target_id HAVING ($2::text IS NULL OR bool_or(reason=$2)) AND ($3::timestamptz IS NULL OR (min(created_at),target_type||':'||target_id)>($3,$4)) ORDER BY oldest,target_type||':'||target_id LIMIT 21")
        .bind(q.kind.as_deref().filter(|k| !k.is_empty()))
        .bind(q.reason.as_deref().filter(|k| !k.is_empty()))
        .bind(cursor.as_ref().map(|c| c.0))
        .bind(cursor.as_ref().map(|c| c.1.clone()).unwrap_or_default())
        .fetch_all(&mut *db)
        .await?;
    let next = (groups.len() > 20)
        .then(|| make_cursor(groups[19].2, &format!("{}:{}", groups[19].0, groups[19].1)));
    let mut out = Vec::new();
    for (kind, id, _, mut group) in groups.into_iter().take(20) {
        let field = group["reports"][0]["field"].as_str().map(str::to_string);
        group["current"] = current_content(&mut db, &kind, &id, field.as_deref()).await?;
        let owner = owner_of(&mut db, &kind, &id).await.ok();
        group["history"] = sqlx::query_scalar("SELECT coalesce(jsonb_agg(jsonb_build_object('action',m.action,'note',m.note,'created_at',m.created_at,'actor',(SELECT username FROM users WHERE id=m.actor_id)) ORDER BY m.created_at DESC),'[]') FROM (SELECT * FROM moderation_actions WHERE (target_type=$1 AND target_id=$2) OR (target_type='user' AND target_id=$3) ORDER BY created_at DESC LIMIT 10) m")
            .bind(&kind)
            .bind(&id)
            .bind(owner.as_deref().unwrap_or(""))
            .fetch_one(&mut *db)
            .await?;
        media_keys(&app, &mut group);
        out.push(group);
    }
    let interim: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',i.id,'username',u.username,'starts_at',i.starts_at,'until',i.until,'overdue',i.until<=now(),'note',i.note) FROM interim_restrictions i JOIN users u ON u.id=i.user_id WHERE i.resolution='OPEN' ORDER BY i.until")
        .fetch_all(&mut *db)
        .await?;
    let appeals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM appeals WHERE status='PENDING' AND strike_id IS NOT NULL",
    )
    .fetch_one(&mut *db)
    .await?;
    log("admin_queue_read", "ok");
    Ok(Json(
        json!({"groups": out, "next_cursor": next, "interim_restrictions": interim, "pending_appeals": appeals}),
    ))
}

#[derive(Deserialize)]
pub struct ActionInput {
    action: String,
    note: Option<String>,
    field: Option<String>,
    item_id: Option<String>,
    interim_hours: Option<i64>,
    strike: Option<StrikeInput>,
}
/// POST /api/admin/reports/{target_type}/{target_id}/actions: closes every open report on the target.
pub async fn admin_action(
    State(app): State<App>,
    jar: CookieJar,
    Path((kind, id)): Path<(String, String)>,
    Json(input): Json<ActionInput>,
) -> Res<Json<Value>> {
    let actor = staff_write(&app, &jar).await?;
    if !TARGETS.contains(&kind.as_str()) {
        return Err(Fail::missing());
    }
    let note = note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let reports: Vec<(String, Value)> = sqlx::query_as("SELECT id,snapshot FROM reports WHERE target_type=$1 AND target_id=$2 AND status='OPEN' ORDER BY created_at FOR UPDATE").bind(&kind).bind(&id).fetch_all(&mut *tx).await?;
    let report_ids: Vec<String> = reports.iter().map(|r| r.0.clone()).collect();
    let owner = owner_of(&mut tx, &kind, &id).await?;
    let snapshot = match reports.first() {
        Some((_, snap)) => snap.clone(),
        None => match kind.as_str() {
            "profile" => profile_snapshot(&mut tx, &owner, None).await?,
            _ => json!({"field": null, "value": current_content(&mut tx, &kind, &id, None).await?}),
        },
    };
    let mut removed = json!([]);
    let mut strike_id = None;
    match input.action.as_str() {
        "dismiss" => {
            if input.strike.is_some() {
                return Err(Fail::bad("Dismissing a report never issues a strike."));
            }
        }
        "remove_content" => removed = json!([remove_content(&mut tx, &kind, &id).await?]),
        "reset_field" => {
            if kind != "profile" {
                return Err(Fail::bad("Reset field applies to channel reports."));
            }
            let field = input
                .field
                .as_deref()
                .or_else(|| reports.first().and_then(|r| r.1["field"].as_str()))
                .unwrap_or("")
                .to_string();
            reset_field(&mut tx, &owner, &field, input.item_id.as_deref()).await?;
        }
        "restrict" => {
            if input.strike.is_none() {
                let hours = input.interim_hours.unwrap_or(0);
                set_interim(&mut tx, &actor, &owner, hours, &note).await?;
            }
        }
        _ => return Err(Fail::field("action", "Choose an action.")),
    }
    if let Some(strike) = &input.strike {
        let (sid, _, _) = issue_strike(
            &app,
            &mut tx,
            &actor,
            &owner,
            strike,
            &report_ids,
            snapshot,
            removed.clone(),
            &note,
        )
        .await?;
        strike_id = Some(sid);
    }
    let actioned = input.action != "dismiss";
    if kind == "emote" && matches!(input.action.as_str(), "dismiss" | "remove_content") {
        crate::emotes::reviewed(&mut tx, &id).await?;
    }
    sqlx::query("UPDATE reports SET status=$2,closed_at=now(),closed_reason=$3,reporter_notice=CASE WHEN $4 THEN 'ACTION_TAKEN' ELSE 'NONE' END WHERE id=ANY($1)")
        .bind(&report_ids)
        .bind(if actioned { "ACTIONED" } else { "DISMISSED" })
        .bind(&input.action)
        .bind(actioned)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&actor.id), &input.action, &kind, &id, &report_ids, &note, json!({"field": input.field, "item_id": input.item_id, "strike_id": strike_id, "removed": removed}), false).await?;
    tx.commit().await?;
    // Open chats drop a message staff removed, as they do for a channel moderator's delete.
    if kind == "emote" {
        app.chat.publish(&owner, None, 0, json!({"type":"emotes"}));
    }
    if kind == "chat_message" && input.action == "remove_content" {
        let channel: Option<String> =
            sqlx::query_scalar("SELECT channel_id FROM chat_messages WHERE id=$1")
                .bind(&id)
                .fetch_optional(&app.db)
                .await?;
        if let Some(channel) = channel {
            app.chat
                .publish(&channel, None, 0, json!({"type":"delete","id":id}));
        }
    }
    log(&input.action, "ok");
    Ok(Json(
        json!({"closed_reports": report_ids.len(), "strike_id": strike_id}),
    ))
}

async fn set_interim(
    db: &mut PgConnection,
    actor: &User,
    user_id: &str,
    hours: i64,
    note: &str,
) -> Res<String> {
    if !(1..=24).contains(&hours) {
        return Err(Fail::field(
            "interim_hours",
            "An interim restriction can last up to 24 hours.",
        ));
    }
    let target = profiles::channel_user_by_id(db, user_id)
        .await?
        .ok_or_else(Fail::missing)?;
    if target.internal {
        return Err(Fail::bad("Internal accounts can't be restricted."));
    }
    let open: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM interim_restrictions WHERE user_id=$1 AND resolution='OPEN')",
    )
    .bind(user_id)
    .fetch_one(&mut *db)
    .await?;
    if open {
        return Err(Fail::conflict(
            "This channel already has an open interim restriction.",
        ));
    }
    let id = new_id();
    sqlx::query("INSERT INTO interim_restrictions(id,user_id,until,created_by,note) VALUES($1,$2,now()+make_interval(hours=>$3),$4,$5)").bind(&id).bind(user_id).bind(hours as i32).bind(&actor.id).bind(note).execute(&mut *db).await?;
    recompute(db, user_id).await?;
    audit(
        db,
        Some(&actor.id),
        "interim_restriction_set",
        "user",
        user_id,
        &[],
        note,
        json!({"interim_id": id, "hours": hours}),
        false,
    )
    .await?;
    log("interim_restriction_set", "ok");
    Ok(id)
}
pub(crate) async fn admin_target(db: &mut PgConnection, name: &str) -> Res<String> {
    sqlx::query_scalar("SELECT id FROM users WHERE lower(username)=lower($1)")
        .bind(name)
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(Fail::missing)
}
/// GET /api/admin/users/{username}/standing
pub async fn admin_standing(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let _staff = staff(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let user_id = admin_target(&mut db, &name).await?;
    let user = profiles::channel_user_by_id(&mut db, &user_id)
        .await?
        .ok_or_else(Fail::missing)?;
    let level = level(&mut db, &user_id).await?;
    let mut list = strikes(&mut db, &user_id, true).await?;
    list.iter_mut().for_each(|v| media_keys(&app, v));
    let interim: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'starts_at',starts_at,'until',until,'resolution',resolution,'overdue',resolution='OPEN' AND until<=now(),'note',note,'resolved_at',resolved_at) FROM interim_restrictions WHERE user_id=$1 ORDER BY starts_at DESC").bind(&user_id).fetch_all(&mut *db).await?;
    let history: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('action',m.action,'target_type',m.target_type,'note',m.note,'detail',m.detail,'self_review',m.self_review,'created_at',m.created_at,'actor',(SELECT username FROM users WHERE id=m.actor_id)) FROM moderation_actions m WHERE (m.target_type='user' AND m.target_id=$1) OR m.target_id IN (SELECT DISTINCT target_id FROM reports WHERE target_user_id=$1) ORDER BY m.created_at DESC LIMIT 100").bind(&user_id).fetch_all(&mut *db).await?;
    let until: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT restricted_until FROM profiles WHERE user_id=$1")
            .bind(&user_id)
            .fetch_optional(&mut *db)
            .await?
            .flatten();
    Ok(Json(
        json!({"username": user.username, "internal": user.internal, "deleted": user.deleted_at.is_some(), "level": level, "restriction": restriction_json(until), "strikes": list, "interim_restrictions": interim, "history": history}),
    ))
}
#[derive(Deserialize)]
pub struct DirectStrike {
    #[serde(flatten)]
    strike: StrikeInput,
    note: Option<String>,
}
/// POST /api/admin/users/{username}/strikes: a strike without a report.
pub async fn admin_strike(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<DirectStrike>,
) -> Res<Json<Value>> {
    let actor = staff_write(&app, &jar).await?;
    let note = note(input.note.as_deref(), "note", false)?;
    let mut tx = app.db.begin().await?;
    let user_id = admin_target(&mut tx, &name).await?;
    let snapshot = profile_snapshot(&mut tx, &user_id, None)
        .await
        .unwrap_or(json!({"field": null, "value": null}));
    let (id, level, _) = issue_strike(
        &app,
        &mut tx,
        &actor,
        &user_id,
        &input.strike,
        &[],
        snapshot,
        json!([]),
        &note,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"strike_id": id, "level": level})))
}
#[derive(Deserialize)]
pub struct InterimInput {
    hours: i64,
    note: Option<String>,
}
pub async fn admin_interim(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<InterimInput>,
) -> Res<Json<Value>> {
    let actor = staff_write(&app, &jar).await?;
    let note = note(input.note.as_deref(), "note", true)?;
    let mut tx = app.db.begin().await?;
    let user_id = admin_target(&mut tx, &name).await?;
    let id = set_interim(&mut tx, &actor, &user_id, input.hours, &note).await?;
    tx.commit().await?;
    Ok(Json(json!({"interim_restriction_id": id})))
}
#[derive(Deserialize, Default)]
pub struct NoteInput {
    note: Option<String>,
}
/// DELETE /api/admin/users/{username}/interim-restriction: records the "lift" outcome.
pub async fn admin_interim_lift(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    input: Option<Json<NoteInput>>,
) -> Res<Json<Value>> {
    let actor = staff_write(&app, &jar).await?;
    let note = note(
        input.as_ref().and_then(|i| i.note.as_deref()),
        "note",
        false,
    )?;
    let mut tx = app.db.begin().await?;
    let user_id = admin_target(&mut tx, &name).await?;
    let lifted = sqlx::query("UPDATE interim_restrictions SET resolution='LIFTED',resolved_at=now(),resolved_by=$2 WHERE user_id=$1 AND resolution='OPEN'").bind(&user_id).bind(&actor.id).execute(&mut *tx).await?;
    if lifted.rows_affected() == 0 {
        return Err(Fail::missing());
    }
    recompute(&mut tx, &user_id).await?;
    audit(
        &mut tx,
        Some(&actor.id),
        "interim_restriction_lifted",
        "user",
        &user_id,
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    log("interim_restriction_lifted", "ok");
    Ok(Json(json!({"lifted": true})))
}
/// POST /api/admin/users/{username}/restriction/lift: lifts every active penalty without overturning.
pub async fn admin_lift(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    input: Option<Json<NoteInput>>,
) -> Res<Json<Value>> {
    let actor = staff_write(&app, &jar).await?;
    let note = note(input.as_ref().and_then(|i| i.note.as_deref()), "note", true)?;
    let mut tx = app.db.begin().await?;
    let user_id = admin_target(&mut tx, &name).await?;
    sqlx::query("UPDATE strikes SET penalty_lifted_at=now() WHERE user_id=$1 AND status='ACTIVE' AND penalty<>'WARNING' AND penalty_lifted_at IS NULL").bind(&user_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE interim_restrictions SET resolution='LIFTED',resolved_at=now(),resolved_by=$2 WHERE user_id=$1 AND resolution='OPEN'").bind(&user_id).bind(&actor.id).execute(&mut *tx).await?;
    recompute(&mut tx, &user_id).await?;
    audit(
        &mut tx,
        Some(&actor.id),
        "restriction_lifted",
        "user",
        &user_id,
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    log("restriction_lifted", "ok");
    Ok(Json(json!({"lifted": true})))
}
/// GET /api/admin/appeals: pending appeals, oldest first.
pub async fn admin_appeals(
    State(app): State<App>,
    jar: CookieJar,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let _staff = staff(&app, &jar).await?;
    let cursor = parse_cursor(&q.cursor)?;
    let mut db = app.db.acquire().await?;
    // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
    let rows: Vec<(String, DateTime<Utc>, String, Value)> = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT a.id,a.created_at,s.user_id,jsonb_build_object('id',a.id,'body',a.body,'created_at',a.created_at,'username',u.username,'strike',{STRIKE_SQL},'reports',(SELECT coalesce(jsonb_agg(jsonb_build_object('reason',r.reason,'note',r.note)),'[]') FROM reports r WHERE r.id=ANY(s.report_ids))) FROM appeals a JOIN strikes s ON s.id=a.strike_id JOIN users u ON u.id=a.user_id WHERE a.status='PENDING' AND ($1::timestamptz IS NULL OR (a.created_at,a.id)>($1,$2)) ORDER BY a.created_at,a.id LIMIT 21")))
        .bind(cursor.as_ref().map(|c| c.0))
        .bind(cursor.as_ref().map(|c| c.1.clone()).unwrap_or_default())
        .fetch_all(&mut *db)
        .await?;
    let next = (rows.len() > 20).then(|| make_cursor(rows[19].1, &rows[19].0));
    let mut out = Vec::new();
    for (_, _, user_id, mut v) in rows.into_iter().take(20) {
        v["history"] = json!(strikes(&mut db, &user_id, true).await?);
        media_keys(&app, &mut v);
        out.push(v);
    }
    Ok(Json(json!({"appeals": out, "next_cursor": next})))
}
#[derive(Deserialize)]
pub struct Decision {
    outcome: String,
    staff_note: Option<String>,
    message_to_user: Option<String>,
}
/// POST /api/admin/appeals/{id}/decision
pub async fn decide(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Res<Json<Value>> {
    let actor = staff_write(&app, &jar).await?;
    if input.outcome != "upheld" && input.outcome != "overturned" {
        return Err(Fail::field("outcome", "Choose upheld or overturned."));
    }
    let staff_note = note(input.staff_note.as_deref(), "staff_note", true)?;
    let message = note(input.message_to_user.as_deref(), "message_to_user", false)?;
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String, String, String, Value)> = sqlx::query_as("SELECT a.status,s.id,s.user_id,s.issued_by,s.removed_refs FROM appeals a JOIN strikes s ON s.id=a.strike_id WHERE a.id=$1 FOR UPDATE OF a,s").bind(&id).fetch_optional(&mut *tx).await?;
    let (status, strike_id, user_id, issuer, removed) = row.ok_or_else(Fail::missing)?;
    if status != "PENDING" {
        return Err(Fail::conflict("This appeal has already been decided."));
    }
    let mut self_review = false;
    if issuer == actor.id {
        let other: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM staff_roles r JOIN users u ON u.id=r.user_id WHERE r.role='admin' AND u.mfa_enabled AND u.deleted_at IS NULL AND u.id<>$1)").bind(&actor.id).fetch_one(&mut *tx).await?;
        if other {
            log("appeal_decided", "denied");
            return Err(Fail::denied("Another moderator must review this appeal."));
        }
        self_review = true;
    }
    let final_status = if input.outcome == "overturned" {
        "OVERTURNED"
    } else {
        "UPHELD"
    };
    sqlx::query("UPDATE appeals SET status=$2,reviewed_by=$3,reviewed_at=now(),message_to_user=$4,staff_note=$5,self_review=$6 WHERE id=$1").bind(&id).bind(final_status).bind(&actor.id).bind(&message).bind(&staff_note).bind(self_review).execute(&mut *tx).await?;
    if final_status == "OVERTURNED" {
        sqlx::query("UPDATE strikes SET status='OVERTURNED',overturned_at=now() WHERE id=$1")
            .bind(&strike_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE account_bans SET review_requested_at=now() WHERE strike_id=$1 AND status='ACTIVE'")
            .bind(&strike_id)
            .execute(&mut *tx)
            .await?;
        // Restore removed wall posts, replies, fan art and setup photos that still exist; reset fields stay cleared.
        for item in removed.as_array().into_iter().flatten() {
            let (Some(kind), Some(item_id), Some(previous)) = (
                item["type"].as_str(),
                item["id"].as_str(),
                item["previous"].as_str(),
            ) else {
                continue;
            };
            let table = match kind {
                "faction_post" => {
                    crate::factions::restore(&mut tx, item_id, previous).await?;
                    continue;
                }
                "emote" => {
                    crate::emotes::restore(&mut tx, item_id, previous).await?;
                    continue;
                }
                "wall_post" => "wall_posts",
                "wall_reply" => "wall_replies",
                "fan_art" => "fan_art",
                "setup_photo" => "setup_photos",
                _ => continue,
            };
            // Only fixed SQL fragments or allowlisted table/column literals are interpolated; values are bound.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET status=$2 WHERE id=$1 AND status='REMOVED'"
            )))
            .bind(item_id)
            .bind(previous)
            .execute(&mut *tx)
            .await?;
        }
        recompute(&mut tx, &user_id).await?;
    }
    audit(
        &mut tx,
        Some(&actor.id),
        "appeal_decided",
        "user",
        &user_id,
        &[],
        &staff_note,
        json!({"appeal_id": id, "strike_id": strike_id, "outcome": input.outcome}),
        self_review,
    )
    .await?;
    queue_notice(
        &app,
        &mut tx,
        &user_id,
        "Your S.V.E.R account standing",
        STANDING_MAIL,
    )
    .await?;
    tx.commit().await?;
    log("appeal_decided", &input.outcome);
    Ok(Json(
        json!({"status": final_status, "self_review": self_review}),
    ))
}
/// GET /api/admin/moderation-actions
pub async fn admin_actions(
    State(app): State<App>,
    jar: CookieJar,
    Query(q): CursorQuery,
) -> Res<Json<Value>> {
    let _staff = staff(&app, &jar).await?;
    let cursor = parse_cursor(&q.cursor)?;
    let rows: Vec<(String, DateTime<Utc>, Value)> = sqlx::query_as("SELECT m.id,m.created_at,jsonb_build_object('id',m.id,'action',m.action,'target_type',m.target_type,'target_id',m.target_id,'report_ids',m.report_ids,'note',m.note,'detail',m.detail,'self_review',m.self_review,'created_at',m.created_at,'actor',(SELECT username FROM users WHERE id=m.actor_id)) FROM moderation_actions m WHERE ($1::timestamptz IS NULL OR (m.created_at,m.id)<($1,$2)) ORDER BY m.created_at DESC,m.id DESC LIMIT 51")
        .bind(cursor.as_ref().map(|c| c.0))
        .bind(cursor.as_ref().map(|c| c.1.clone()).unwrap_or_default())
        .fetch_all(&app.db)
        .await?;
    let next = (rows.len() > 50).then(|| make_cursor(rows[49].1, &rows[49].0));
    Ok(Json(
        json!({"actions": rows.into_iter().take(50).map(|r| r.2).collect::<Vec<_>>(), "next_cursor": next}),
    ))
}

/// Background work: overdue interim restrictions, restriction cache refresh and the reporter digest.
pub async fn tick(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    let overdue: Vec<(String, String)> = sqlx::query_as("UPDATE interim_restrictions SET overdue_at=now() WHERE resolution='OPEN' AND until<=now() AND overdue_at IS NULL RETURNING id,user_id").fetch_all(&mut *tx).await?;
    for (id, user_id) in &overdue {
        audit(
            &mut tx,
            None,
            "interim_restriction_overdue",
            "user",
            user_id,
            &[],
            "",
            json!({"interim_id": id}),
            false,
        )
        .await?;
        recompute(&mut tx, user_id).await?;
        log("interim_restriction_overdue", "ok");
    }
    // Expired restrictions clear from the cache so lists stop treating the account as restricted.
    sqlx::query("UPDATE profiles SET restricted_until=NULL WHERE restricted_until<=now()")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    // At most one generic digest per opted-in, verified reporter per 24 hours.
    let reporters: Vec<String> = sqlx::query_scalar("SELECT DISTINCT r.reporter_id FROM reports r JOIN profiles p ON p.user_id=r.reporter_id JOIN users u ON u.id=r.reporter_id WHERE r.reporter_notice='ACTION_TAKEN' AND NOT r.reporter_emailed AND r.reporter_seen_at IS NULL AND p.email_report_updates AND u.email_verified AND u.deleted_at IS NULL AND (p.report_digest_at IS NULL OR p.report_digest_at<=now()-interval '24 hours') LIMIT 100")
        .fetch_all(&app.db)
        .await?;
    for reporter in reporters {
        let mut tx = app.db.begin().await?;
        sqlx::query("UPDATE profiles SET report_digest_at=now() WHERE user_id=$1")
            .bind(&reporter)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE reports SET reporter_emailed=true WHERE reporter_id=$1 AND reporter_notice='ACTION_TAKEN' AND NOT reporter_emailed").bind(&reporter).execute(&mut *tx).await?;
        queue_notice(
            app,
            &mut tx,
            &reporter,
            "An update on your S.V.E.R reports",
            REPORT_MAIL,
        )
        .await?;
        tx.commit().await?;
    }
    // Snapshots are purged 90 days after the report closes.
    sqlx::query("UPDATE reports SET snapshot='{\"purged\":true}' WHERE status<>'OPEN' AND closed_at<=now()-interval '90 days' AND NOT snapshot ? 'purged'").execute(&app.db).await?;
    Ok(())
}
