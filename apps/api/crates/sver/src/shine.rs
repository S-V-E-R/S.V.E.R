//! Shine, part 1 (docs/SUPPORT.md "Shine"): charity streams and Good Works badges. A streamer sets
//! a charity and its donation page in Creator Studio; while live, the channel shows a Shine banner
//! with a donate link, and browse and MAGNet label it. S.V.E.R never collects or holds charity
//! money. Afterwards the streamer can submit the amount raised with proof; staff verify it into a
//! permanent, audited Good Works badge (or reject it, or later revoke it).
use crate::{
    App,
    profiles::{self, Fail, Res},
    safety, security as sec, text,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// A broadcast that is publicly live: LIVE, or RECONNECTING inside its grace window.
const LIVE: &str = "(b.state='LIVE' OR (b.state='RECONNECTING' AND b.reconnect_deadline>now()))";
/// A charity link staff approved (Joe, October 17, 2026). A new link shows nothing to viewers until
/// staff approve it once; after that it works at once, for anyone who uses it. `url` is the
/// column holding the link.
fn approved(url: &str) -> String {
    format!("EXISTS(SELECT 1 FROM charity_links k WHERE k.url={url} AND k.status='approved')")
}

/// An https link to a charity's page (no credentials), at most 500 characters.
fn link(value: &str, field: &'static str) -> Res<String> {
    let value = value.trim();
    let parsed =
        url::Url::parse(value).map_err(|_| Fail::field(field, "Enter a full https:// link."))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || value.len() > 500
    {
        return Err(Fail::field(field, "Enter a full https:// link."));
    }
    Ok(value.to_string())
}

/// Live broadcasts whose owner set a charity with an approved link get their charity stream row
/// (once per broadcast). A link approved mid-stream turns the stream into a charity stream here.
pub async fn sync(db: &mut PgConnection, owner: Option<&str>) -> Res<()> {
    // LIVE and the approval check are fixed SQL; the owner is bound.
    sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO charity_streams(id,owner_id,broadcast_id,charity_name,donate_url,started_at)
        SELECT gen_random_uuid()::text,b.owner_id,b.id,s.charity_name,s.charity_url,coalesce(b.started_at,now())
        FROM broadcasts b JOIN stream_settings s ON s.owner_id=b.owner_id
        WHERE {LIVE} AND s.charity_name IS NOT NULL AND s.charity_url IS NOT NULL AND {} AND ($1::text IS NULL OR b.owner_id=$1)
        ON CONFLICT (broadcast_id) DO NOTHING", approved("s.charity_url"))))
        .bind(owner).execute(db).await?;
    Ok(())
}

/// The channel's Shine: the live charity banner, Good Works badges and past charity streams.
pub async fn channel(db: &mut PgConnection, owner: &str) -> Res<Value> {
    // LIVE is fixed SQL; the owner is bound.
    let live: Option<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('name',cs.charity_name,'url',cs.donate_url) FROM charity_streams cs JOIN broadcasts b ON b.id=cs.broadcast_id WHERE cs.owner_id=$1 AND {LIVE} AND {} LIMIT 1", approved("cs.donate_url"))))
        .bind(owner).fetch_optional(&mut *db).await?;
    let badges: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('charity',charity_name,'raised_cents',raised_cents,'verified_at',reviewed_at) FROM charity_streams WHERE owner_id=$1 AND status='verified' ORDER BY reviewed_at DESC")
        .bind(owner).fetch_all(&mut *db).await?;
    // LIVE is fixed SQL; the owner is bound.
    let history: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('charity',cs.charity_name,'url',cs.donate_url,'started_at',cs.started_at,'raised_cents',CASE WHEN cs.status='verified' THEN cs.raised_cents END)
        FROM charity_streams cs LEFT JOIN broadcasts b ON b.id=cs.broadcast_id WHERE cs.owner_id=$1 AND NOT coalesce({LIVE},false) ORDER BY cs.started_at DESC LIMIT 10")))
        .bind(owner).fetch_all(&mut *db).await?;
    Ok(json!({"live": live, "badges": badges, "history": history}))
}

/// The owner's current live broadcast, if any.
async fn live_broadcast(db: &mut PgConnection, owner: &str) -> Res<Option<String>> {
    // LIVE is fixed SQL; the owner is bound.
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT b.id FROM broadcasts b WHERE b.owner_id=$1 AND {LIVE} LIMIT 1"
    )))
    .bind(owner)
    .fetch_optional(db)
    .await?)
}
async fn studio_json(app: &App, owner: &str) -> Res<Value> {
    let mut db = app.db.acquire().await?;
    let settings: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT charity_name,charity_url FROM stream_settings WHERE owner_id=$1")
            .bind(owner)
            .fetch_optional(&mut *db)
            .await?;
    let (name, url) = settings.unwrap_or_default();
    // LIVE is fixed SQL; the owner is bound.
    let streams: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('id',cs.id,'charity',cs.charity_name,'url',cs.donate_url,'started_at',cs.started_at,'live',coalesce({LIVE},false),
            'raised_cents',cs.raised_cents,'proof_url',cs.proof_url,'status',cs.status,'review_note',cs.review_note)
        FROM charity_streams cs LEFT JOIN broadcasts b ON b.id=cs.broadcast_id WHERE cs.owner_id=$1 ORDER BY cs.started_at DESC LIMIT 50")))
        .bind(owner).fetch_all(&mut *db).await?;
    // Where the link stands with staff: approved, pending or rejected (with the staff note).
    let link: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('status',status,'note',note) FROM charity_links WHERE url=$1",
    )
    .bind(&url)
    .fetch_optional(&mut *db)
    .await?;
    Ok(json!({"charity_name": name, "charity_url": url, "link": link, "streams": streams}))
}
/// GET /api/me/shine
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    Ok(Json(studio_json(&app, &user.id).await?))
}

#[derive(Deserialize)]
pub struct Charity {
    charity_name: Option<String>,
    charity_url: Option<String>,
}
/// PUT /api/me/shine: the charity for the next (or current) stream; empty clears it. While live,
/// the current stream becomes (or stops being) a charity stream at once.
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Charity>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let name = input
        .charity_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let url = input
        .charity_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    let (name, url) = match (name, url) {
        (None, None) => (None, None),
        (Some(name), Some(url)) => {
            if name.chars().count() > 80 {
                return Err(Fail::field(
                    "charity_name",
                    "Charity names are up to 80 characters.",
                ));
            }
            text::filter(name, "charity_name")?;
            (Some(name.to_string()), Some(link(url, "charity_url")?))
        }
        (None, Some(_)) => return Err(Fail::field("charity_name", "Name the charity.")),
        (Some(_), None) => {
            return Err(Fail::field(
                "charity_url",
                "Add the link to the charity's donation page.",
            ));
        }
    };
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO stream_settings(owner_id,title,charity_name,charity_url) SELECT id,left(username||'''s stream',140),$2,$3 FROM users WHERE id=$1
        ON CONFLICT(owner_id) DO UPDATE SET charity_name=EXCLUDED.charity_name,charity_url=EXCLUDED.charity_url,updated_at=now()")
        .bind(&user.id).bind(&name).bind(&url).execute(&mut *tx).await?;
    // A link staff haven't seen goes to them; until they approve it, viewers see nothing.
    let mut usable = false;
    if let (Some(name), Some(url)) = (&name, &url) {
        sqlx::query("INSERT INTO charity_links(url,charity_name,requested_by) VALUES($1,$2,$3) ON CONFLICT (url) DO NOTHING")
            .bind(url)
            .bind(name)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?;
        usable = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM charity_links WHERE url=$1 AND status='approved')",
        )
        .bind(url)
        .fetch_one(&mut *tx)
        .await?;
    }
    if let Some(broadcast) = live_broadcast(&mut tx, &user.id).await? {
        match (&name, &url) {
            (Some(name), Some(url)) if usable => {
                sqlx::query("UPDATE charity_streams SET charity_name=$2,donate_url=$3 WHERE broadcast_id=$1 AND status='none'")
                    .bind(&broadcast).bind(name).bind(url).execute(&mut *tx).await?;
                sync(&mut tx, &user.id).await?;
            }
            _ => {
                sqlx::query("DELETE FROM charity_streams WHERE broadcast_id=$1 AND status='none'")
                    .bind(&broadcast)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(Json(studio_json(&app, &user.id).await?))
}

#[derive(Deserialize)]
pub struct Raised {
    raised_cents: i64,
    proof_url: String,
}
/// POST /api/me/shine/{id}/submit: after the stream ends, the amount raised and a link to the
/// charity's receipt or fundraiser page, for staff to verify.
async fn submit(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Raised>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if !(1..=100_000_000_000).contains(&input.raised_cents) {
        return Err(Fail::field("raised_cents", "Enter the amount raised."));
    }
    let proof = link(&input.proof_url, "proof_url")?;
    sec::reserve(&app, vec![format!("shine-submit:{}", user.id)], 10, 3600).await?;
    // LIVE is fixed SQL; values are bound.
    let updated = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE charity_streams cs SET raised_cents=$3,proof_url=$4,status='submitted',submitted_at=now(),review_note=NULL
        WHERE cs.id=$1 AND cs.owner_id=$2 AND cs.status IN ('none','rejected')
            AND NOT EXISTS(SELECT 1 FROM broadcasts b WHERE b.id=cs.broadcast_id AND {LIVE})")))
        .bind(&id).bind(&user.id).bind(input.raised_cents).bind(&proof)
        .execute(&app.db).await?.rows_affected();
    if updated == 0 {
        return Err(Fail::conflict(
            "Submit after the stream ends, once per charity stream.",
        ));
    }
    Ok(Json(studio_json(&app, &user.id).await?))
}

/// GET /api/admin/shine: submissions waiting for staff, and recent decisions.
async fn queue(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    // Only chip_sql with a literal alias is interpolated.
    let mut items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('id',cs.id,'charity',cs.charity_name,'url',cs.donate_url,'started_at',cs.started_at,'raised_cents',cs.raised_cents,
            'proof_url',cs.proof_url,'status',cs.status,'submitted_at',cs.submitted_at,'reviewed_at',cs.reviewed_at,'review_note',cs.review_note,'owner',{})
        FROM charity_streams cs JOIN channel_users c ON c.id=cs.owner_id
        WHERE cs.status='submitted' OR (cs.status IN ('verified','rejected','revoked') AND cs.reviewed_at>now()-interval '90 days')
        ORDER BY cs.status<>'submitted', coalesce(cs.reviewed_at,cs.submitted_at) DESC LIMIT 200", profiles::chip_sql("c"))))
        .fetch_all(&app.db).await?;
    for item in &mut items {
        profiles::hydrate(&app, &mut item["owner"]);
    }
    // Charity links waiting for a first check (oldest first), then recent decisions.
    let mut links: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('url',k.url,'charity',k.charity_name,'status',k.status,'note',k.note,'requested_at',k.requested_at,'reviewed_at',k.reviewed_at,
            'requested_by',CASE WHEN c.id IS NULL THEN NULL ELSE {} END,'used_by',(SELECT count(*) FROM stream_settings s WHERE s.charity_url=k.url))
        FROM charity_links k LEFT JOIN channel_users c ON c.id=k.requested_by
        WHERE k.status='pending' OR k.reviewed_at>now()-interval '90 days'
        ORDER BY k.status<>'pending', coalesce(k.reviewed_at,k.requested_at) LIMIT 200", profiles::chip_sql("c"))))
        .fetch_all(&app.db).await?;
    for link in &mut links {
        profiles::hydrate(&app, &mut link["requested_by"]);
    }
    Ok(Json(json!({"items": items, "links": links})))
}

#[derive(Deserialize)]
pub struct Review {
    action: String,
    #[serde(default)]
    note: String,
}
#[derive(Deserialize)]
pub struct LinkReview {
    url: String,
    action: String,
    #[serde(default)]
    note: String,
}
/// POST /api/admin/shine/links: approve or reject a charity link (a reason is needed to reject).
/// Approval works at once: a stream that's live with it becomes a charity stream. Everyone using
/// the link is told. Audited.
async fn review_link(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<LinkReview>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = input.note.trim();
    if note.chars().count() > 500 {
        return Err(Fail::field("note", "Notes are up to 500 characters."));
    }
    let to = match input.action.as_str() {
        "approve" => "approved",
        "reject" if note.is_empty() => return Err(Fail::field("note", "Give a reason.")),
        "reject" => "rejected",
        _ => return Err(Fail::field("action", "Approve or reject.")),
    };
    let mut tx = app.db.begin().await?;
    let charity: Option<String> = sqlx::query_scalar("UPDATE charity_links SET status=$2,reviewed_by=$3,reviewed_at=now(),note=$4 WHERE url=$1 RETURNING charity_name")
        .bind(&input.url).bind(to).bind(&staff.id).bind(note)
        .fetch_optional(&mut *tx).await?;
    let Some(charity) = charity else {
        return Err(Fail::missing());
    };
    if to == "rejected" {
        // A rejected link stops working, including on a stream that's live with it now.
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM charity_streams cs WHERE cs.donate_url=$1 AND cs.status='none' AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.id=cs.broadcast_id AND {LIVE})")))
            .bind(&input.url).execute(&mut *tx).await?;
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        &format!("charity_link_{}", input.action),
        "charity_link",
        &input.url,
        &[],
        note,
        json!({"charity": charity}),
        false,
    )
    .await?;
    let (title, body) = if to == "approved" {
        (
            "Charity link approved",
            format!(
                "Your link for {charity} is approved: viewers see the Donate button while you're live."
            ),
        )
    } else {
        (
            "Charity link not approved",
            format!(
                "Staff didn't approve your link for {charity}. The note is in Creator Studio → Shine."
            ),
        )
    };
    sqlx::query("INSERT INTO notifications(id,user_id,kind,channel_id,event_key,payload) SELECT gen_random_uuid()::text,s.owner_id,'good_works',s.owner_id,'charity_link:'||md5($1)||':'||$2||':'||s.owner_id,jsonb_build_object('title',$3::text,'body',$4::text,'url','/studio/shine') FROM stream_settings s WHERE s.charity_url=$1 ON CONFLICT DO NOTHING")
        .bind(&input.url).bind(to).bind(title).bind(body).execute(&mut *tx).await?;
    if to == "approved" {
        sync(&mut tx, None).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"status": to})))
}
/// POST /api/admin/shine/{id}: verify or reject a submission, or revoke a badge whose proof turned
/// out false. Audited.
async fn review(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Review>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = input.note.trim();
    if note.chars().count() > 500 {
        return Err(Fail::field("note", "Notes are up to 500 characters."));
    }
    let (from, to) = match input.action.as_str() {
        "verify" => ("submitted", "verified"),
        "reject" => ("submitted", "rejected"),
        "revoke" => ("verified", "revoked"),
        _ => return Err(Fail::field("action", "Verify, reject or revoke.")),
    };
    if to != "verified" && note.is_empty() {
        return Err(Fail::field("note", "Give a reason."));
    }
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String, Option<i64>)> = sqlx::query_as("UPDATE charity_streams SET status=$3,reviewed_by=$4,reviewed_at=now(),review_note=$5 WHERE id=$1 AND status=$2 RETURNING owner_id,charity_name,raised_cents")
        .bind(&id).bind(from).bind(to).bind(&staff.id).bind(note)
        .fetch_optional(&mut *tx).await?;
    let Some((owner, charity, raised)) = row else {
        return Err(Fail::stale());
    };
    safety::audit(
        &mut tx,
        Some(&staff.id),
        &format!("good_works_{}", input.action),
        "charity_stream",
        &id,
        &[],
        note,
        json!({"owner": owner, "charity": charity, "raised_cents": raised}),
        false,
    )
    .await?;
    // The streamer hears the decision; the staff note is in Creator Studio → Shine.
    let (title, body) = match to {
        "verified" => (
            "Good Works badge added",
            format!(
                "Staff verified your charity stream for {charity}. The badge is on your channel."
            ),
        ),
        "rejected" => (
            "Good Works not verified",
            format!(
                "Staff couldn't verify your charity stream for {charity}. The note is in Creator Studio → Shine."
            ),
        ),
        _ => (
            "Good Works badge removed",
            format!(
                "Staff removed the Good Works badge for {charity}. The note is in Creator Studio → Shine."
            ),
        ),
    };
    sqlx::query("INSERT INTO notifications(id,user_id,kind,channel_id,event_key,payload) VALUES(gen_random_uuid()::text,$1,'good_works',$1,'good_works:'||$2||':'||$3,jsonb_build_object('title',$4::text,'body',$5::text,'url','/studio/shine')) ON CONFLICT DO NOTHING")
        .bind(&owner).bind(&id).bind(to).bind(title).bind(body).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"status": to})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/shine", get(mine).put(save))
        .route("/api/me/shine/{id}/submit", post(submit))
        .route("/api/admin/shine", get(queue))
        .route("/api/admin/shine/links", post(review_link))
        .route("/api/admin/shine/{id}", post(review))
}
