//! Anonymous intimate-image removal requests. Contact details and quarantine records are encrypted.
#![allow(clippy::type_complexity)]
use crate::{
    App, jobs, media,
    profiles::{Fail, Res},
    safety, security as sec, text,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, NaiveDate, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::net::SocketAddr;

#[derive(Clone, Default)]
pub struct Config {
    pub purge_url: String,
    pub purge_token: String,
}
impl Config {
    pub fn from_env() -> Self {
        let zone = std::env::var("MEDIA_CLOUDFLARE_ZONE_ID").unwrap_or_default();
        Self {
            purge_url: if zone.is_empty() {
                String::new()
            } else {
                format!("https://api.cloudflare.com/client/v4/zones/{zone}/purge_cache")
            },
            purge_token: std::env::var("MEDIA_CLOUDFLARE_PURGE_TOKEN").unwrap_or_default(),
        }
    }
}
#[derive(Deserialize, Serialize)]
pub struct Submission {
    name: String,
    email: String,
    capacity: String,
    #[serde(default)]
    authority: String,
    locations: Vec<String>,
    #[serde(default)]
    description: String,
    good_faith: bool,
    signature: String,
    signed_on: NaiveDate,
    #[serde(default)]
    extra: String,
    #[serde(default, skip_serializing)]
    turnstile_token: String,
}
impl Submission {
    fn validate(&mut self, app: &App) -> Res<()> {
        self.email = sec::email(&self.email)?;
        self.name = text::plain(&self.name, "name", 1, 150, 0, false)?;
        self.signature = text::plain(&self.signature, "signature", 1, 150, 0, false)?;
        self.authority = text::plain(&self.authority, "authority", 0, 1000, 0, false)?;
        self.description = text::plain(&self.description, "description", 0, 4000, 20, false)?;
        self.extra = text::plain(&self.extra, "extra", 0, 4000, 20, false)?;
        if !matches!(self.capacity.as_str(), "shown" | "authorized")
            || (self.capacity == "authorized" && self.authority.is_empty())
        {
            return Err(Fail::bad(
                "Tell us whether you are shown or authorized to act for the person shown, and explain your authority.",
            ));
        }
        if !self.good_faith {
            return Err(Fail::bad("Confirm the good-faith statement."));
        }
        if self.signed_on > Utc::now().date_naive() + chrono::Duration::days(1) {
            return Err(Fail::bad("The signature date cannot be in the future."));
        }
        if self.locations.is_empty() || self.locations.len() > 20 {
            return Err(Fail::bad("Give between 1 and 20 S.V.E.R links."));
        }
        for location in &mut self.locations {
            *location = normalize_location(&app.config.origin, location);
            parse_location(app, location)?;
        }
        Ok(())
    }
}
/// Accepts the forms people paste: a full link, `sver.tv/...` or `www.`/`media.` without a scheme,
/// or a path on this site (`/username/live`).
fn normalize_location(origin: &str, location: &str) -> String {
    let location = location.trim();
    if location.starts_with('/') && !location.starts_with("//") {
        format!("{}{location}", origin.trim_end_matches('/'))
    } else if !location.contains("://") && !location.is_empty() {
        format!("https://{}", location.trim_start_matches('/'))
    } else {
        location.to_string()
    }
}
fn parse_location(app: &App, location: &str) -> Res<url::Url> {
    let url = url::Url::parse(location)
        .map_err(|_| Fail::bad("Use a S.V.E.R link, like sver.tv/username."))?;
    let origin = url::Url::parse(&app.config.origin).map_err(|_| Fail::internal())?;
    let media = url::Url::parse(&app.config.media.public_base).map_err(|_| Fail::internal())?;
    if location.len() > 2048
        || !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || !(url.origin() == origin.origin()
            || url.origin() == media.origin()
            || (url.scheme() == "https"
                && matches!(
                    url.host_str(),
                    Some("sver.tv" | "www.sver.tv" | "media.sver.tv")
                )))
    {
        return Err(Fail::bad(
            "Give a link to content on S.V.E.R. Describe other locations in the optional details.",
        ));
    }
    Ok(url)
}
fn email_hash(app: &App, email: &str) -> String {
    let mut h =
        Hmac::<sha2::Sha256>::new_from_slice(&app.config.key).expect("HMAC accepts any key length");
    h.update(b"take-down-email:");
    h.update(email.as_bytes());
    h.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
async fn event(
    app: &App,
    db: &mut PgConnection,
    id: i64,
    actor: Option<&str>,
    action: &str,
    detail: &str,
) -> Res<()> {
    sqlx::query(
        "INSERT INTO take_down_events(request_id,actor_id,action,detail) VALUES($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(actor)
    .bind(action)
    .bind(sec::seal(app, "take-down-event", detail)?)
    .execute(db)
    .await?;
    Ok(())
}
async fn requester_notice(
    app: &App,
    db: &mut PgConnection,
    id: i64,
    number: &str,
    email: &str,
    status: &str,
) -> Res<()> {
    let body = format!(
        "Take It Down request {number}: {status}.\n\nCheck your request with its number and the email you used: {}/take-it-down#status\n\nDo not send a copy of the image. If you did not submit this request, contact safety@sver.tv.",
        app.config.origin
    );
    let mail_id =
        jobs::queue_address(app, db, None, email, "Your S.V.E.R removal request", &body).await?;
    track_mail(db, id, &mail_id, "requester").await?;
    event(app, db, id, None, "requester_email_queued", &mail_id).await
}
async fn alert_staff(app: &App, db: &mut PgConnection, id: i64) -> Res<()> {
    for user in safety::take_down::staff_ids(db).await? {
        let pushes = crate::staff_push::enqueue(db, &user).await?;
        if pushes.is_empty() {
            event(app, db, id, None, "staff_push_unavailable", &user).await?;
        }
        for job in pushes {
            sqlx::query("INSERT INTO take_down_deliveries(mail_id,request_id,audience,channel) VALUES($1,$2,'staff','push')")
                .bind(&job).bind(id).execute(&mut *db).await?;
            event(app, db, id, None, "staff_push_queued", &job).await?;
        }
        if let Some(mail)=safety::queue_notice_id(app,db,&user,"Urgent S.V.E.R removal request",&format!("A Take It Down request needs review. The 48-hour deadline runs continuously. Sign in to review: {}/admin/take-it-down",app.config.origin)).await? {
            jobs::removal_notice(db,&mail).await?;
            track_mail(db,id,&mail,"staff").await?;
        } else {
            event(app, db, id, None, "staff_email_unavailable", &user).await?;
        }
    }
    event(app, db, id, None, "staff_alert_queued", "").await
}
async fn locate(
    app: &App,
    db: &mut PgConnection,
    location: &str,
) -> Res<(Vec<String>, Option<safety::take_down::Located>)> {
    let url = parse_location(app, location)?;
    let media_base = format!("{}/", app.config.media.public_base.trim_end_matches('/'));
    let key = location
        .strip_prefix(&media_base)
        .or_else(|| url.path().strip_prefix("/api/media/"));
    if let Some(key) = key {
        let root =
            media::removal::existing_root(db, key.split(['?', '#']).next().unwrap_or("")).await?;
        return Ok((root.into_iter().collect(), None));
    }
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let name = url.path().trim_matches('/').split('/').next().unwrap_or("");
    let kind = query.get("report").map(String::as_str).unwrap_or("profile");
    if ![
        "profile",
        "wall_post",
        "wall_reply",
        "fan_art",
        "setup_photo",
        "chat_message",
        "live_stream",
    ]
    .contains(&kind)
    {
        return Ok((vec![], None));
    }
    let id = query.get("id").map(String::as_str).unwrap_or(name);
    let mut target =
        safety::take_down::locate(db, kind, id, query.get("field").map(String::as_str)).await?;
    if target.is_none() && kind != "profile" {
        let held:Option<(String,String)>=sqlx::query_as("SELECT t.owner_id,t.snapshot FROM take_down_targets t JOIN take_down_requests r ON r.id=t.request_id WHERE t.kind=$1 AND t.target_id=$2 AND r.status<>'not_removed' ORDER BY t.request_id LIMIT 1")
            .bind(kind).bind(id).fetch_optional(&mut *db).await?;
        if let Some((owner, snapshot)) = held {
            let saved: Value =
                serde_json::from_str(&sec::unseal(app, "take-down-target", &snapshot)?)
                    .map_err(|_| Fail::internal())?;
            target = Some(safety::take_down::Located {
                kind: kind.into(),
                id: id.into(),
                owner,
                roots: vec![],
                snapshot: saved["content"].clone(),
            });
        }
    }
    Ok((
        target.as_ref().map(|t| t.roots.clone()).unwrap_or_default(),
        target,
    ))
}

async fn attach(
    app: &App,
    db: &mut PgConnection,
    id: i64,
    locations: &[String],
) -> Res<Vec<String>> {
    let held_at: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *db)
        .await?;
    let mut roots = vec![];
    for location in locations {
        let (media, target) = locate(app, db, location).await?;
        roots.extend(media);
        if let Some(target) = target {
            let previous: Option<String> = sqlx::query_scalar("SELECT t.snapshot FROM take_down_targets t JOIN take_down_requests r ON r.id=t.request_id WHERE t.kind=$1 AND t.target_id=$2 AND r.status<>'not_removed' ORDER BY t.request_id LIMIT 1")
                .bind(&target.kind).bind(&target.id).fetch_optional(&mut *db).await?;
            let snapshot = match previous {
                Some(saved) => saved,
                None => {
                    let hidden = safety::take_down::hide(db, &target).await?;
                    sec::seal(
                        app,
                        "take-down-target",
                        &json!({"content":target.snapshot,"previous":hidden,"held_at":held_at})
                            .to_string(),
                    )?
                }
            };
            sqlx::query("INSERT INTO take_down_targets(request_id,kind,target_id,owner_id,snapshot) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
                .bind(id).bind(&target.kind).bind(&target.id).bind(&target.owner).bind(snapshot).execute(&mut *db).await?;
        }
    }
    roots = media::removal::hold(db, &roots).await?;
    for root in &roots {
        sqlx::query(
            "INSERT INTO take_down_media(request_id,root) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(root)
        .execute(&mut *db)
        .await?;
    }
    Ok(roots)
}

async fn refresh_chat(app: &App, id: i64) {
    let messages = sqlx::query_scalar::<_, String>(
        "SELECT target_id FROM take_down_targets WHERE request_id=$1 AND kind='chat_message'",
    )
    .bind(id)
    .fetch_all(&app.db)
    .await;
    if let Ok(messages) = messages {
        for message in messages {
            if crate::chat::notify_changed(app, &message).await.is_err() {
                eprintln!("take_down_event=chat_refresh outcome=retry");
            }
        }
    }
}

async fn submit(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(mut input): Json<Submission>,
) -> Res<Json<Value>> {
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("take-down:{ip}")], 10, 3600).await?;
    input.validate(&app)?;
    sec::turnstile(&app, &input.turnstile_token, "take_down", ip).await?;
    let mut guard = app.db.begin().await?;
    // ponytail: serialize removal mutations across workers; use per-content locks if queue volume warrants it.
    sqlx::query("SELECT pg_advisory_xact_lock(1414087746)")
        .execute(&mut *guard)
        .await?;
    let mut tx = app.db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO take_down_requests(email_hash,details) VALUES($1,$2) RETURNING id",
    )
    .bind(email_hash(&app, &input.email))
    .bind(sec::seal(
        &app,
        "take-down-request",
        &serde_json::to_string(&input).map_err(|_| Fail::internal())?,
    )?)
    .fetch_one(&mut *tx)
    .await?;
    let number = format!("TID-{}-{id:06}", Utc::now().format("%Y"));
    sqlx::query("UPDATE take_down_requests SET number=$2 WHERE id=$1")
        .bind(id)
        .bind(&number)
        .execute(&mut *tx)
        .await?;
    let roots = attach(&app, &mut tx, id, &input.locations).await?;
    event(&app, &mut tx, id, None, "received", "").await?;
    requester_notice(&app, &mut tx, id, &number, &input.email, "received").await?;
    alert_staff(&app, &mut tx, id).await?;
    tx.commit().await?;
    refresh_chat(&app, id).await;
    let mut hidden = true;
    for root in &roots {
        if media::removal::quarantine(&app, root).await.is_err() {
            hidden = false;
        }
    }
    guard.commit().await?;
    Ok(Json(
        json!({"number":number,"status":"received","media_hidden":hidden,"message":"Your request has been received. Keep your request number to check its status."}),
    ))
}

#[derive(Deserialize)]
struct Lookup {
    number: String,
    email: String,
    turnstile_token: String,
}
async fn status(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Lookup>,
) -> Res<Json<Value>> {
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("take-down-status:{ip}")], 30, 3600).await?;
    sec::turnstile(&app, &input.turnstile_token, "take_down_status", ip).await?;
    let email = sec::email(&input.email)?;
    if input.number.len() > 40 {
        return Err(Fail::missing());
    }
    let row: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('number',number,'status',CASE WHEN status='not_removed' AND resolved_at IS NULL THEN 'under_review' ELSE status END,'received_at',received_at,'resolved_at',resolved_at,'reason',CASE WHEN resolved_at IS NOT NULL THEN reason ELSE '' END) FROM take_down_requests WHERE number=$1 AND email_hash=$2")
        .bind(input.number.trim().to_uppercase()).bind(email_hash(&app,&email)).fetch_optional(&app.db).await?;
    Ok(Json(row.ok_or_else(|| {
        Fail::bad("No request matches that number and email.")
    })?))
}

async fn queue(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('number',number,'status',status,'received_at',received_at,'deadline',deadline,'resolved_at',resolved_at,'reason',reason,'media_pending',(SELECT count(*) FROM take_down_media m JOIN media_removal_holds h USING(root) WHERE m.request_id=r.id AND h.hidden_at IS NULL),'target_count',(SELECT count(*) FROM take_down_targets WHERE request_id=r.id)) FROM take_down_requests r ORDER BY resolved_at IS NULL DESC,deadline ASC LIMIT 200")
        .fetch_all(&app.db).await?;
    let compliance: Value = sqlx::query_scalar("SELECT jsonb_build_object('received',count(*),'removed',count(*) FILTER(WHERE status='removed'),'overdue',count(*) FILTER(WHERE coalesce(resolved_at,now())>deadline),'median_hours',percentile_cont(0.5) WITHIN GROUP(ORDER BY extract(epoch FROM resolved_at-received_at)/3600) FILTER(WHERE status='removed'),'longest_hours',max(extract(epoch FROM resolved_at-received_at)/3600) FILTER(WHERE status='removed')) FROM take_down_requests WHERE received_at>=date_trunc('month',now())")
        .fetch_one(&app.db).await?;
    Ok(Json(json!({"requests":rows,"monthly":compliance})))
}
async fn detail(
    State(app): State<App>,
    jar: CookieJar,
    Path(number): Path<String>,
) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let (id, sealed, minor, preservation): (i64, String, bool, String) = sqlx::query_as(
        "SELECT id,details,minor,preservation_reference FROM take_down_requests WHERE number=$1",
    )
    .bind(&number)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Fail::missing)?;
    let details: Value = serde_json::from_str(&sec::unseal(&app, "take-down-request", &sealed)?)
        .map_err(|_| Fail::internal())?;
    let mut events: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('at',at,'action',action,'actor',actor_id,'detail',detail) FROM take_down_events WHERE request_id=$1 ORDER BY id DESC LIMIT 200")
        .bind(id).fetch_all(&app.db).await?;
    for entry in &mut events {
        let sealed = entry["detail"].as_str().unwrap_or("");
        if !sealed.is_empty() {
            entry["detail"] = json!(sec::unseal(&app, "take-down-event", sealed)?);
        }
    }
    let preservation = if preservation.is_empty() {
        String::new()
    } else {
        sec::unseal(&app, "take-down-preservation", &preservation)?
    };
    let notices: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('channel',channel,'audience',audience,'state',state,'queued_at',queued_at,'last_attempt_at',last_attempt_at,'attempts',attempts,'accepted_at',sent_at) FROM take_down_deliveries WHERE request_id=$1 ORDER BY queued_at DESC,mail_id LIMIT 200")
        .bind(id).fetch_all(&app.db).await?;
    let targets: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('kind',kind,'id',target_id) FROM take_down_targets WHERE request_id=$1")
        .bind(id).fetch_all(&app.db).await?;
    let roots: Vec<String> =
        sqlx::query_scalar("SELECT root FROM take_down_media WHERE request_id=$1")
            .bind(id)
            .fetch_all(&app.db)
            .await?;
    let mut evidence = vec![];
    for root in roots {
        evidence.extend(media::removal::evidence_keys(&app, &root).await?);
    }
    Ok(Json(
        json!({"number":number,"details":details,"minor":minor,"preservation_reference":preservation,"events":events,"notices":notices,"targets":targets,"evidence":evidence}),
    ))
}
#[derive(Deserialize)]
struct Decision {
    action: String,
    reason: String,
    #[serde(default)]
    minor: bool,
    #[serde(default)]
    preservation_reference: String,
}
async fn decide(
    State(app): State<App>,
    jar: CookieJar,
    Path(number): Path<String>,
    Json(input): Json<Decision>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    let mut guard = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(1414087746)")
        .execute(&mut *guard)
        .await?;
    let reason = text::plain(&input.reason, "reason", 1, 1000, 10, false)?;
    if !["review", "remove", "dismiss"].contains(&input.action.as_str()) {
        return Err(Fail::bad("Choose review, remove or dismiss."));
    }
    let mut preservation = text::plain(
        &input.preservation_reference,
        "preservation_reference",
        0,
        300,
        0,
        false,
    )?;
    let mut tx = app.db.begin().await?;
    let (id, status, sealed, existing_minor, existing_preservation): (i64, String, String, bool, String) = sqlx::query_as(
        "SELECT id,status,details,minor,preservation_reference FROM take_down_requests WHERE number=$1 FOR UPDATE",
    )
    .bind(&number)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Fail::missing)?;
    if matches!(status.as_str(), "removed" | "not_removed") {
        return Err(Fail::conflict("This request already has a decision."));
    }
    let minor = input.minor || existing_minor;
    if preservation.is_empty() && !existing_preservation.is_empty() {
        preservation = sec::unseal(&app, "take-down-preservation", &existing_preservation)?;
    }
    if minor && input.action == "remove" && preservation.is_empty() {
        return Err(Fail::bad(
            "Record the CyberTipline report or preservation reference before closing a request involving a minor.",
        ));
    }
    let details: Submission =
        serde_json::from_str(&sec::unseal(&app, "take-down-request", &sealed)?)
            .map_err(|_| Fail::internal())?;
    let mut roots: Vec<String> =
        sqlx::query_scalar("SELECT root FROM take_down_media WHERE request_id=$1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let targets: Vec<(String, String, String)> =
        sqlx::query_as("SELECT kind,target_id,owner_id FROM take_down_targets WHERE request_id=$1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    if input.action == "remove"
        && roots.is_empty()
        && targets.iter().all(|(kind, _, _)| kind == "profile")
    {
        return Err(Fail::bad(
            "Locate the content before recording its removal.",
        ));
    }
    if input.action == "remove" {
        if !media::removal::ready(&mut tx).await? {
            return Err(Fail::unavailable(
                "Existing uploads are still being fingerprinted. Keep this request open and retry shortly.",
            ));
        }
        roots = media::removal::hold(&mut tx, &roots).await?;
        for root in &roots {
            sqlx::query(
                "INSERT INTO take_down_media(request_id,root) VALUES($1,$2) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(root)
            .execute(&mut *tx)
            .await?;
        }
        // Publish new copy holds before the storage worker can lock them.
        for (kind, target, _) in &targets {
            if kind == "live_stream" {
                safety::take_down::remove(&mut tx, kind, target).await?;
            }
        }
        sqlx::query("UPDATE take_down_requests SET status='under_review' WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        for root in &roots {
            media::removal::quarantine(&app, root).await?;
        }
        for (kind, _, owner) in &targets {
            if kind == "live_stream" && !crate::streams::confirm_stopped(&app, owner).await? {
                return Err(Fail::unavailable(
                    "The stream disconnect is still pending. Keep the request open and retry shortly.",
                ));
            }
        }
        tx = app.db.begin().await?;
        let current: String =
            sqlx::query_scalar("SELECT status FROM take_down_requests WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if matches!(current.as_str(), "removed" | "not_removed") {
            return Err(Fail::conflict("This request already has a decision."));
        }
        let mut owners = vec![];
        for root in &roots {
            media::removal::make_permanent(&mut tx, root, minor).await?;
            owners.extend(safety::take_down::remove_media(&mut tx, root).await?);
        }
        for (kind, target, owner) in &targets {
            if kind != "live_stream" {
                safety::take_down::remove(&mut tx, kind, target).await?;
            }
            owners.push(owner.clone());
        }
        owners.sort();
        owners.dedup();
        for owner in owners {
            if let Some(mail) =
                safety::take_down::sanction(&app, &mut tx, &actor, &owner, &number).await?
            {
                jobs::removal_notice(&mut tx, &mail).await?;
                track_mail(&mut tx, id, &mail, "uploader").await?;
                event(
                    &app,
                    &mut tx,
                    id,
                    Some(&actor.id),
                    "uploader_email_queued",
                    &mail,
                )
                .await?;
            } else {
                event(
                    &app,
                    &mut tx,
                    id,
                    Some(&actor.id),
                    "uploader_email_unavailable",
                    &owner,
                )
                .await?;
            }
        }
    }
    // A staff review immediately stops named live streams, even while details are being clarified.
    if input.action == "review" {
        for (kind, target, _) in &targets {
            if kind == "live_stream" {
                safety::take_down::remove(&mut tx, kind, target).await?;
            }
        }
    }
    let new_status = match input.action.as_str() {
        "remove" => "removed",
        "dismiss" => "not_removed",
        _ => "under_review",
    };
    sqlx::query("UPDATE take_down_requests SET status=$2,reason=$3,resolved_at=CASE WHEN $2='removed' THEN now() ELSE NULL END,minor=$4,preservation_reference=$5 WHERE id=$1")
        .bind(id).bind(new_status).bind(&reason).bind(minor).bind(sec::seal(&app,"take-down-preservation",&preservation)?).execute(&mut *tx).await?;
    event(&app, &mut tx, id, Some(&actor.id), new_status, &reason).await?;
    if input.action != "dismiss" {
        requester_notice(&app, &mut tx, id, &number, &details.email, new_status).await?;
    }
    tx.commit().await?;
    if input.action == "review" {
        for (kind, _, owner) in &targets {
            if kind == "live_stream" {
                let _ = crate::streams::confirm_stopped(&app, owner).await;
            }
        }
    }
    if input.action == "dismiss" {
        restore_request(&app, id).await?;
    }
    guard.commit().await?;
    Ok(Json(json!({"saved":true})))
}

async fn restore_request(app: &App, id: i64) -> Res<()> {
    let roots: Vec<String> = sqlx::query_scalar("SELECT m.root FROM take_down_media m WHERE m.request_id=$1 AND NOT EXISTS(SELECT 1 FROM take_down_media other JOIN take_down_requests r ON r.id=other.request_id WHERE other.root=m.root AND r.status<>'not_removed')").bind(id).fetch_all(&app.db).await?;
    for root in roots {
        media::removal::restore(app, &root).await?;
    }
    let mut tx = app.db.begin().await?;
    let row: Option<(String,String,DateTime<Utc>)> = sqlx::query_as("SELECT number,details,received_at FROM take_down_requests WHERE id=$1 AND status='not_removed' AND resolved_at IS NULL FOR UPDATE")
        .bind(id).fetch_optional(&mut *tx).await?;
    let Some((number, sealed, received)) = row else {
        return Ok(());
    };
    let targets: Vec<(String,String,String)> = sqlx::query_as("SELECT t.kind,t.target_id,t.snapshot FROM take_down_targets t WHERE t.request_id=$1 AND NOT EXISTS(SELECT 1 FROM take_down_targets other JOIN take_down_requests r ON r.id=other.request_id WHERE other.kind=t.kind AND other.target_id=t.target_id AND r.status<>'not_removed')")
        .bind(id).fetch_all(&mut *tx).await?;
    for (kind, target, snapshot) in targets {
        let saved: Value = serde_json::from_str(&sec::unseal(app, "take-down-target", &snapshot)?)
            .map_err(|_| Fail::internal())?;
        let held_at = saved["held_at"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or(received);
        safety::take_down::restore(&mut tx, &kind, &target, &saved["previous"], held_at).await?;
    }
    sqlx::query("UPDATE take_down_requests SET resolved_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let details: Submission =
        serde_json::from_str(&sec::unseal(app, "take-down-request", &sealed)?)
            .map_err(|_| Fail::internal())?;
    requester_notice(app, &mut tx, id, &number, &details.email, "not removed").await?;
    event(app, &mut tx, id, None, "restoration_completed", "").await?;
    tx.commit().await?;
    refresh_chat(app, id).await;
    Ok(())
}

pub async fn tick(app: &App) -> Res<()> {
    if media::removal::index_existing(app).await.is_err() {
        eprintln!("take_down_event=fingerprint_index outcome=retry");
    }
    let pending: Vec<String> = sqlx::query_scalar(
        "SELECT root FROM media_removal_holds WHERE hidden_at IS NULL ORDER BY created_at LIMIT 50",
    )
    .fetch_all(&app.db)
    .await?;
    for root in pending {
        if media::removal::quarantine(app, &root).await.is_err() {
            eprintln!("take_down_event=media_hide outcome=retry");
        }
    }
    let restoring: Vec<i64> = sqlx::query_scalar("SELECT id FROM take_down_requests WHERE status='not_removed' AND resolved_at IS NULL LIMIT 50").fetch_all(&app.db).await?;
    for id in restoring {
        let mut guard = app.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(1414087746)")
            .execute(&mut *guard)
            .await?;
        if restore_request(app, id).await.is_err() {
            eprintln!("take_down_event=restore outcome=retry");
        }
        guard.commit().await?;
    }
    let mut tx = app.db.begin().await?;
    // Account erasure may remove queued jobs; the case still keeps an honest final notice state.
    let expired: Vec<String> = sqlx::query_scalar("SELECT mail_id FROM take_down_deliveries WHERE state IN ('queued','retrying') AND queued_at<=now()-CASE WHEN channel='push' THEN interval '2 days' ELSE interval '7 days' END FOR UPDATE")
        .fetch_all(&mut *tx).await?;
    for id in expired {
        notice_result(&mut tx, &id, "expired", false).await?;
    }
    let overdue: Vec<i64> = sqlx::query_scalar("UPDATE take_down_requests SET last_alert_at=now() WHERE resolved_at IS NULL AND received_at<=now()-interval '24 hours' AND last_alert_at<=now()-interval '1 hour' RETURNING id")
        .fetch_all(&mut *tx).await?;
    for id in overdue {
        alert_staff(app, &mut tx, id).await?;
    }
    sqlx::query("DELETE FROM take_down_requests WHERE resolved_at IS NOT NULL AND received_at<now()-interval '3 years' AND NOT (minor AND status='removed')").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn open_count(db: &mut PgConnection) -> Res<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM take_down_requests WHERE resolved_at IS NULL")
            .fetch_one(db)
            .await?,
    )
}
async fn track_mail(db: &mut PgConnection, id: i64, mail: &str, audience: &str) -> Res<()> {
    sqlx::query("INSERT INTO take_down_deliveries(mail_id,request_id,audience) VALUES($1,$2,$3)")
        .bind(mail)
        .bind(id)
        .bind(audience)
        .execute(db)
        .await?;
    Ok(())
}
pub async fn notice_result(
    db: &mut PgConnection,
    job: &str,
    state: &str,
    attempted: bool,
) -> crate::Result<()> {
    let row:Option<(i64,String,String)>=sqlx::query_as("UPDATE take_down_deliveries SET state=$2,attempts=attempts+CASE WHEN $3 THEN 1 ELSE 0 END,last_attempt_at=CASE WHEN $3 THEN now() ELSE last_attempt_at END,sent_at=CASE WHEN $2='accepted' THEN now() ELSE sent_at END WHERE mail_id=$1 AND state IN ('queued','retrying') RETURNING request_id,audience,channel")
        .bind(job).bind(state).bind(attempted).fetch_optional(&mut *db).await?;
    if let Some((id, audience, channel)) = row {
        sqlx::query("INSERT INTO take_down_events(request_id,action) VALUES($1,$2)")
            .bind(id)
            .bind(format!("{audience}_{channel}_{state}"))
            .execute(db)
            .await?;
    }
    Ok(())
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/take-it-down", post(submit))
        .route("/api/take-it-down/status", post(status))
        .route("/api/admin/take-it-down", get(queue))
        .route("/api/admin/take-it-down/{number}", get(detail).post(decide))
        .route(
            "/api/admin/take-it-down/{number}/locations",
            post(add_location),
        )
        .route(
            "/api/admin/take-it-down/{number}/media/{*key}",
            get(evidence),
        )
}

#[derive(Deserialize)]
struct AddLocation {
    location: String,
}
async fn add_location(
    State(app): State<App>,
    jar: CookieJar,
    Path(number): Path<String>,
    Json(mut input): Json<AddLocation>,
) -> Res<Json<Value>> {
    let actor = safety::staff_write(&app, &jar).await?;
    input.location = normalize_location(&app.config.origin, &input.location);
    parse_location(&app, &input.location)?;
    let mut guard = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(1414087746)")
        .execute(&mut *guard)
        .await?;
    let mut tx = app.db.begin().await?;
    let id:Option<i64>=sqlx::query_scalar("SELECT id FROM take_down_requests WHERE number=$1 AND status IN ('received','under_review') FOR UPDATE").bind(&number).fetch_optional(&mut *tx).await?;
    let id = id.ok_or_else(|| Fail::conflict("This request is no longer open."))?;
    let (media, target) = locate(&app, &mut tx, &input.location).await?;
    if media.is_empty() && target.is_none() {
        return Err(Fail::bad("No content could be identified from that link."));
    }
    let roots = attach(&app, &mut tx, id, std::slice::from_ref(&input.location)).await?;
    event(
        &app,
        &mut tx,
        id,
        Some(&actor.id),
        "content_located",
        &input.location,
    )
    .await?;
    tx.commit().await?;
    refresh_chat(&app, id).await;
    for root in roots {
        media::removal::quarantine(&app, &root).await?;
    }
    guard.commit().await?;
    Ok(Json(json!({"saved":true})))
}

async fn evidence(
    State(app): State<App>,
    jar: CookieJar,
    Path((number, key)): Path<(String, String)>,
) -> Res<axum::response::Response> {
    use axum::response::IntoResponse;
    let actor = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let id:Option<i64>=sqlx::query_scalar("SELECT r.id FROM take_down_requests r JOIN take_down_media m ON m.request_id=r.id WHERE r.number=$1 AND m.root=$2")
        .bind(number).bind(media::removal::root_of_key(&key)).fetch_optional(&mut *tx).await?;
    let id = id.ok_or_else(Fail::missing)?;
    let bytes = media::removal::evidence(&app, &key).await?;
    event(&app, &mut tx, id, Some(&actor.id), "evidence_viewed", &key).await?;
    tx.commit().await?;
    Ok((
        [
            ("content-type", "image/webp"),
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod location_tests {
    use super::normalize_location;

    #[test]
    fn pasted_forms_become_full_links() {
        let n = |raw: &str| normalize_location("https://sver.tv", raw);
        assert_eq!(n(" sver.tv/joe "), "https://sver.tv/joe");
        assert_eq!(n("www.sver.tv/joe/live"), "https://www.sver.tv/joe/live");
        assert_eq!(n("/joe/fan-art"), "https://sver.tv/joe/fan-art");
        assert_eq!(
            n("https://media.sver.tv/a.webp"),
            "https://media.sver.tv/a.webp"
        );
        // Other hosts still get a scheme here and are then refused by parse_location.
        assert_eq!(n("//evil.example/x"), "https://evil.example/x");
    }
}
