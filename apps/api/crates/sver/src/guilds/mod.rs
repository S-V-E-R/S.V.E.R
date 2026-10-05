//! Cross-faction teams. Membership changes serialize across guilds to enforce the three-team cap.
use crate::{
    App, media,
    profiles::{self, Fail, Res},
    reserved, safety, streams, text,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};

mod read;
mod review;
pub use read::{badge_sql, followed_members, hydrate_badges, id_by_slug, notify_allowed};
pub use review::{owner, referenced, remove, remove_media, restore, snapshot, target};

#[derive(FromRow)]
struct Guild {
    id: String,
    slug: String,
    name: String,
    tag: String,
    tagline: String,
    about: String,
    leader_id: Option<String>,
    status: String,
    recruiting: bool,
    verified: bool,
    avatar_key: Option<String>,
    banner_key: Option<String>,
    emblem_visible: bool,
    revision: i64,
}
async fn lock(db: &mut PgConnection) -> Res<()> {
    // ponytail: rare team mutations share one lock; partition if measured contention warrants it.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('guild-membership',0))")
        .execute(db)
        .await?;
    Ok(())
}
async fn load(db: &mut PgConnection, slug: &str) -> Res<Guild> {
    sqlx::query_as("SELECT * FROM guilds WHERE lower(slug)=lower($1) AND status<>'REMOVED'")
        .bind(slug)
        .fetch_optional(db)
        .await?
        .ok_or_else(Fail::missing)
}
async fn role(db: &mut PgConnection, g: &Guild, user: &str) -> Res<Option<&'static str>> {
    let officer: Option<bool> =
        sqlx::query_scalar("SELECT officer FROM guild_members WHERE guild_id=$1 AND user_id=$2")
            .bind(&g.id)
            .bind(user)
            .fetch_optional(db)
            .await?;
    Ok(officer.map(|o| {
        if g.leader_id.as_deref() == Some(user) {
            "leader"
        } else if o {
            "officer"
        } else {
            "member"
        }
    }))
}
async fn manager(db: &mut PgConnection, g: &Guild, user: &str, leader: bool) -> Res<()> {
    let r = role(db, g, user).await?;
    if g.status != "ACTIVE"
        || !matches!(r, Some("leader" | "officer"))
        || (leader && r != Some("leader"))
    {
        return Err(Fail::denied("You don't manage this guild."));
    }
    eligible(db, user).await
}
async fn eligible(db: &mut PgConnection, user: &str) -> Res<()> {
    if profiles::channel_user_by_id(db, user)
        .await?
        .is_none_or(|u| !u.eligible || !u.email_verified)
    {
        return Err(Fail::denied(
            "A verified account in good standing is required.",
        ));
    }
    Ok(())
}
async fn streamer(db: &mut PgConnection, user: &str) -> Res<()> {
    eligible(db, user).await?;
    if !streams::has_streamed(db, user).await? {
        return Err(Fail::denied(
            "Stream on S.V.E.R at least once before joining a guild.",
        ));
    }
    Ok(())
}
async fn capacity(db: &mut PgConnection, user: &str) -> Res<()> {
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM guild_members m JOIN guilds g ON g.id=m.guild_id WHERE m.user_id=$1 AND g.status='ACTIVE'")
        .bind(user).fetch_one(db).await?;
    if count >= 3 {
        return Err(Fail::conflict(
            "You can belong to three guilds. Leave one before joining another.",
        ));
    }
    Ok(())
}
async fn event(
    db: &mut PgConnection,
    guild: &str,
    actor: Option<&str>,
    subject: Option<&str>,
    action: &str,
) -> Res<()> {
    sqlx::query(
        "INSERT INTO guild_events(guild_id,actor_id,subject_id,action) VALUES($1,$2,$3,$4)",
    )
    .bind(guild)
    .bind(actor)
    .bind(subject)
    .bind(action)
    .execute(db)
    .await?;
    Ok(())
}
async fn admission(db: &mut PgConnection, g: &Guild, user: &str, recruiting: bool) -> Res<()> {
    if g.status != "ACTIVE" || (recruiting && !g.recruiting) {
        return Err(Fail::conflict("This guild isn't recruiting."));
    }
    streamer(db, user).await?;
    capacity(db, user).await?;
    if role(db, g, user).await?.is_some() {
        return Err(Fail::conflict("You're already a member."));
    }
    let blocked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM guild_blocks WHERE guild_id=$1 AND user_id=$2)",
    )
    .bind(&g.id)
    .bind(user)
    .fetch_one(&mut *db)
    .await?;
    if blocked
        || if let Some(leader) = &g.leader_id {
            profiles::blocked_between(db, leader, user).await?
        } else {
            true
        }
    {
        return Err(Fail::denied("You can't apply to this guild."));
    }
    Ok(())
}
#[derive(Deserialize)]
struct Branding {
    name: String,
    slug: String,
    tag: String,
    #[serde(default)]
    tagline: String,
    #[serde(default)]
    about: String,
    #[serde(default = "yes")]
    recruiting: bool,
    revision: Option<i64>,
}
fn yes() -> bool {
    true
}
fn validate(b: &mut Branding) -> Res<()> {
    b.name = text::plain(&b.name, "name", 3, 40, 0, true)?;
    b.slug = b.slug.trim().to_ascii_lowercase();
    b.tag = b.tag.trim().to_ascii_uppercase();
    if !reserved::is_username_shaped(&b.slug) || reserved::is_reserved(&b.slug) {
        return Err(Fail::field(
            "slug",
            "Choose an available URL name: 3–25 letters, numbers or underscores.",
        ));
    }
    if !(2..=5).contains(&b.tag.len())
        || !b.tag.bytes().all(|c| c.is_ascii_alphanumeric())
        || reserved::is_reserved(&b.tag)
    {
        return Err(Fail::field(
            "tag",
            "Choose a tag of 2–5 letters or numbers.",
        ));
    }
    let compact: String = b.name.chars().filter(|c| c.is_alphanumeric()).collect();
    if reserved::is_reserved(&compact) {
        return Err(Fail::field(
            "name",
            "Choose a name that doesn't impersonate the site or its factions.",
        ));
    }
    text::filter(&b.tag, "tag")?;
    text::filter(&b.slug, "slug")?;
    b.tagline = text::plain(&b.tagline, "tagline", 0, 120, 0, true)?;
    b.about = text::plain(&b.about, "about", 0, 3000, 40, true)?;
    Ok(())
}
async fn unique(db: &mut PgConnection, b: &Branding, id: Option<&str>) -> Res<()> {
    let taken:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guilds WHERE ($4::text IS NULL OR id<>$4) AND (lower(name)=lower($1) OR lower(slug)=lower($2) OR lower(tag)=lower($3)))")
        .bind(&b.name).bind(&b.slug).bind(&b.tag).bind(id).fetch_one(db).await?;
    if taken {
        return Err(Fail::conflict(
            "That guild name, URL name or tag is already in use.",
        ));
    }
    Ok(())
}
async fn create(
    State(app): State<App>,
    jar: CookieJar,
    Json(mut b): Json<Branding>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    validate(&mut b)?;
    profiles::rate(&app, format!("guild-create:{}", user.id), 5, 86400).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    if !streams::eligible(&mut tx, &user).await? {
        return Err(Fail::denied(
            "Creating a guild requires verified email and authenticator 2FA.",
        ));
    }
    streamer(&mut tx, &user.id).await?;
    capacity(&mut tx, &user.id).await?;
    unique(&mut tx, &b, None).await?;
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM guilds WHERE leader_id=$1 AND status='ACTIVE')",
    )
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    if owned {
        return Err(Fail::conflict("You can lead one guild."));
    }
    let id = profiles::new_id();
    sqlx::query("INSERT INTO guilds(id,slug,name,tag,tagline,about,leader_id,recruiting) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&id).bind(&b.slug).bind(&b.name).bind(&b.tag).bind(&b.tagline).bind(&b.about).bind(&user.id).bind(b.recruiting).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO guild_members(guild_id,user_id) VALUES($1,$2)")
        .bind(&id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    event(&mut tx, &id, Some(&user.id), Some(&user.id), "created").await?;
    tx.commit().await?;
    Ok(Json(json!({"slug":b.slug,"id":id})))
}
async fn update(
    State(app): State<App>,
    jar: CookieJar,
    Path(slug): Path<String>,
    Json(mut b): Json<Branding>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    validate(&mut b)?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let g = load(&mut tx, &slug).await?;
    manager(&mut tx, &g, &user.id, true).await?;
    if b.revision != Some(g.revision) {
        return Err(Fail::stale());
    }
    // The public address is stable; staff alone can rename a URL to resolve impersonation.
    if b.slug != g.slug {
        return Err(Fail::bad(
            "The guild URL stays the same. Contact staff if it needs changing.",
        ));
    }
    unique(&mut tx, &b, Some(&g.id)).await?;
    sqlx::query("UPDATE guilds SET name=$2,tag=$3,tagline=$4,about=$5,recruiting=$6,revision=revision+1,verified=CASE WHEN name=$2 AND tag=$3 THEN verified ELSE false END WHERE id=$1")
        .bind(&g.id).bind(&b.name).bind(&b.tag).bind(&b.tagline).bind(&b.about).bind(b.recruiting).execute(&mut *tx).await?;
    event(&mut tx, &g.id, Some(&user.id), None, "branding_updated").await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
struct Action {
    action: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    enabled: bool,
}
async fn act(
    State(app): State<App>,
    jar: CookieJar,
    Path(slug): Path<String>,
    Json(input): Json<Action>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::rate(&app, format!("guild-action:{}", user.id), 60, 60).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let g = load(&mut tx, &slug).await?;
    let me = role(&mut tx, &g, &user.id).await?;
    match input.action.as_str() {
        "apply" => {
            admission(&mut tx, &g, &user.id, true).await?;
            let message = text::plain(&input.message, "message", 1, 500, 8, true)?;
            let existing:Option<(String,bool)>=sqlx::query_as("SELECT status,coalesce(declined_at>now()-interval '7 days',false) FROM guild_applications WHERE guild_id=$1 AND user_id=$2")
                .bind(&g.id).bind(&user.id).fetch_optional(&mut *tx).await?;
            if existing.as_ref().is_some_and(|(s, c)| s == "OPEN" || *c) {
                return Err(Fail::conflict(
                    "You already applied, or must wait seven days after a decline.",
                ));
            }
            let id = profiles::new_id();
            sqlx::query("INSERT INTO guild_applications(id,guild_id,user_id,message) VALUES($1,$2,$3,$4) ON CONFLICT(guild_id,user_id) DO UPDATE SET id=$1,message=$4,status='OPEN',note='',created_at=now(),decided_at=NULL")
                .bind(&id).bind(&g.id).bind(&user.id).bind(message).execute(&mut *tx).await?;
            let recipients: Vec<String> = sqlx::query_scalar(
                "SELECT user_id FROM guild_members WHERE guild_id=$1 AND (officer OR user_id=$2)",
            )
            .bind(&g.id)
            .bind(&g.leader_id)
            .fetch_all(&mut *tx)
            .await?;
            for recipient in recipients {
                notify(
                    &mut tx,
                    &g,
                    &recipient,
                    &user.id,
                    "guild_application",
                    &id,
                    "New guild application",
                )
                .await?;
            }
        }
        "withdraw" => {
            sqlx::query("UPDATE guild_applications SET status='WITHDRAWN',decided_at=now() WHERE guild_id=$1 AND user_id=$2 AND status='OPEN'").bind(&g.id).bind(&user.id).execute(&mut *tx).await?;
        }
        "follow" | "unfollow" | "mute" | "unmute" => {
            eligible(&mut tx, &user.id).await?;
            match input.action.as_str() {
                "follow" => {
                    if g.status != "ACTIVE" {
                        return Err(Fail::conflict("This guild is archived."));
                    }
                    sqlx::query("INSERT INTO guild_follows(guild_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(&g.id).bind(&user.id).execute(&mut *tx).await?;
                }
                "unfollow" => {
                    sqlx::query("DELETE FROM guild_follows WHERE guild_id=$1 AND user_id=$2")
                        .bind(&g.id)
                        .bind(&user.id)
                        .execute(&mut *tx)
                        .await?;
                }
                "mute" => {
                    sqlx::query("INSERT INTO guild_mutes(guild_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(&g.id).bind(&user.id).execute(&mut *tx).await?;
                }
                _ => {
                    sqlx::query("DELETE FROM guild_mutes WHERE guild_id=$1 AND user_id=$2")
                        .bind(&g.id)
                        .bind(&user.id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
        }
        "leave" => {
            if me.is_none() {
                return Err(Fail::missing());
            }
            depart(&mut tx, &g, &user.id, Some(&user.id)).await?;
        }
        "verification" => {
            manager(&mut tx, &g, &user.id, true).await?;
            let evidence = text::plain(&input.message, "message", 1, 2000, 15, true)?;
            sqlx::query("INSERT INTO guild_verification(guild_id,evidence) VALUES($1,$2) ON CONFLICT(guild_id) DO UPDATE SET evidence=$2,status='OPEN',note='',created_at=now()")
                .bind(&g.id).bind(evidence).execute(&mut *tx).await?;
            event(
                &mut tx,
                &g.id,
                Some(&user.id),
                None,
                "verification_requested",
            )
            .await?;
        }
        "clear_avatar" | "clear_banner" => {
            manager(&mut tx, &g, &user.id, true).await?;
            if input.action == "clear_avatar" {
                sqlx::query("UPDATE guilds SET avatar_key=NULL,revision=revision+1 WHERE id=$1")
                    .bind(&g.id)
                    .execute(&mut *tx)
                    .await?;
                media::queue_delete(&mut tx, g.avatar_key.as_deref(), None).await?;
            } else {
                sqlx::query("UPDATE guilds SET banner_key=NULL,revision=revision+1 WHERE id=$1")
                    .bind(&g.id)
                    .execute(&mut *tx)
                    .await?;
                media::queue_delete(&mut tx, g.banner_key.as_deref(), None).await?;
            }
        }
        "accept" | "decline" | "invite" | "remove" | "officer" | "transfer" | "block"
        | "unblock" | "title" => {
            manager(
                &mut tx,
                &g,
                &user.id,
                matches!(input.action.as_str(), "officer" | "transfer"),
            )
            .await?;
            let target =
                profiles::account_by_name(&mut tx, input.username.trim().trim_start_matches('@'))
                    .await?
                    .ok_or_else(Fail::channel_missing)?;
            if target.id == user.id {
                return Err(Fail::bad("Use Leave to leave your guild."));
            }
            let target_role = role(&mut tx, &g, &target.id).await?;
            if me == Some("officer") && matches!(target_role, Some("leader" | "officer")) {
                return Err(Fail::denied("Only the leader manages officers."));
            }
            match input.action.as_str() {
                "invite" => {
                    admission(&mut tx, &g, &target.id, true).await?;
                    if profiles::blocked_between(&mut tx, &user.id, &target.id).await? {
                        return Err(Fail::denied("You can't invite this account."));
                    }
                    let fresh=sqlx::query("INSERT INTO guild_invitations(guild_id,user_id,invited_by) VALUES($1,$2,$3) ON CONFLICT(guild_id,user_id) DO UPDATE SET invited_by=$3,created_at=now() WHERE guild_invitations.created_at<now()-interval '7 days'")
                        .bind(&g.id).bind(&target.id).bind(&user.id).execute(&mut *tx).await?.rows_affected();
                    if fresh == 0 {
                        return Err(Fail::conflict("An invitation was already sent this week."));
                    }
                    notify(
                        &mut tx,
                        &g,
                        &target.id,
                        &user.id,
                        "guild_invite",
                        &profiles::new_id(),
                        "Invitation to apply",
                    )
                    .await?;
                }
                "accept" | "decline" => {
                    let application:Option<String>=sqlx::query_scalar("SELECT id FROM guild_applications WHERE guild_id=$1 AND user_id=$2 AND status='OPEN'").bind(&g.id).bind(&target.id).fetch_optional(&mut *tx).await?;
                    let id = application
                        .ok_or_else(|| Fail::conflict("No open application for this streamer."))?;
                    let note = text::plain(&input.note, "note", 0, 500, 8, true)?;
                    if input.action == "accept" {
                        admission(&mut tx, &g, &target.id, false).await?;
                        sqlx::query("INSERT INTO guild_members(guild_id,user_id) VALUES($1,$2)")
                            .bind(&g.id)
                            .bind(&target.id)
                            .execute(&mut *tx)
                            .await?;
                        event(&mut tx, &g.id, Some(&user.id), Some(&target.id), "joined").await?;
                    }
                    let status = if input.action == "accept" {
                        "ACCEPTED"
                    } else {
                        "DECLINED"
                    };
                    sqlx::query("UPDATE guild_applications SET status=$2,note=$3,decided_at=now(),declined_at=CASE WHEN $2='DECLINED' THEN now() ELSE declined_at END WHERE id=$1").bind(&id).bind(status).bind(note).execute(&mut *tx).await?;
                    sqlx::query("DELETE FROM guild_invitations WHERE guild_id=$1 AND user_id=$2")
                        .bind(&g.id)
                        .bind(&target.id)
                        .execute(&mut *tx)
                        .await?;
                    notify(
                        &mut tx,
                        &g,
                        &target.id,
                        &user.id,
                        "guild_decision",
                        &id,
                        if input.action == "accept" {
                            "Guild application accepted"
                        } else {
                            "Guild application declined"
                        },
                    )
                    .await?;
                }
                "remove" | "block" => {
                    if target_role == Some("leader") {
                        return Err(Fail::denied(
                            "The leader must transfer leadership or leave.",
                        ));
                    }
                    if target_role.is_some() {
                        depart(&mut tx, &g, &target.id, Some(&user.id)).await?;
                    }
                    if input.action == "block" {
                        sqlx::query("INSERT INTO guild_blocks(guild_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(&g.id).bind(&target.id).execute(&mut *tx).await?;
                        sqlx::query("UPDATE guild_applications SET status='DECLINED',decided_at=now(),declined_at=now() WHERE guild_id=$1 AND user_id=$2 AND status='OPEN'").bind(&g.id).bind(&target.id).execute(&mut *tx).await?;
                        sqlx::query(
                            "DELETE FROM guild_invitations WHERE guild_id=$1 AND user_id=$2",
                        )
                        .bind(&g.id)
                        .bind(&target.id)
                        .execute(&mut *tx)
                        .await?;
                    }
                }
                "unblock" => {
                    sqlx::query("DELETE FROM guild_blocks WHERE guild_id=$1 AND user_id=$2")
                        .bind(&g.id)
                        .bind(&target.id)
                        .execute(&mut *tx)
                        .await?;
                }
                "officer" | "title" => {
                    if target_role.is_none() {
                        return Err(Fail::bad("Choose a guild member."));
                    }
                    if input.action == "officer" {
                        sqlx::query(
                            "UPDATE guild_members SET officer=$3 WHERE guild_id=$1 AND user_id=$2",
                        )
                        .bind(&g.id)
                        .bind(&target.id)
                        .bind(input.enabled)
                        .execute(&mut *tx)
                        .await?;
                    } else {
                        let title = text::plain(&input.message, "message", 0, 40, 0, true)?;
                        sqlx::query(
                            "UPDATE guild_members SET title=$3 WHERE guild_id=$1 AND user_id=$2",
                        )
                        .bind(&g.id)
                        .bind(&target.id)
                        .bind(title)
                        .execute(&mut *tx)
                        .await?;
                    }
                }
                "transfer" => {
                    eligible(&mut tx, &target.id).await?;
                    if target_role.is_none() {
                        return Err(Fail::bad("Choose a guild member."));
                    }
                    let occupied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guilds WHERE leader_id=$1 AND status='ACTIVE')").bind(&target.id).fetch_one(&mut *tx).await?;
                    if occupied {
                        return Err(Fail::conflict("That member already leads a guild."));
                    }
                    sqlx::query(
                        "UPDATE guild_members SET officer=true WHERE guild_id=$1 AND user_id=$2",
                    )
                    .bind(&g.id)
                    .bind(&user.id)
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("UPDATE guilds SET leader_id=$2,revision=revision+1 WHERE id=$1")
                        .bind(&g.id)
                        .bind(&target.id)
                        .execute(&mut *tx)
                        .await?;
                }
                _ => unreachable!(),
            }
            event(
                &mut tx,
                &g.id,
                Some(&user.id),
                Some(&target.id),
                &input.action,
            )
            .await?;
        }
        _ => return Err(Fail::bad("Choose a guild action.")),
    }
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
async fn notify(
    db: &mut PgConnection,
    g: &Guild,
    recipient: &str,
    actor: &str,
    kind: &str,
    key: &str,
    title: &str,
) -> Res<()> {
    if notify_allowed(db, &g.id, recipient).await? {
        crate::alerts::community(
            db,
            recipient,
            actor,
            kind,
            key,
            Some(&g.id),
            &json!({"title":title,"body":g.name,"url":format!("/g/{}",g.slug)}),
        )
        .await?;
    }
    Ok(())
}
async fn depart(db: &mut PgConnection, g: &Guild, user: &str, actor: Option<&str>) -> Res<()> {
    sqlx::query("DELETE FROM guild_members WHERE guild_id=$1 AND user_id=$2")
        .bind(&g.id)
        .bind(user)
        .execute(&mut *db)
        .await?;
    event(db, &g.id, actor, Some(user), "left").await?;
    if g.leader_id.as_deref() == Some(user) {
        succession(db, g).await?;
    }
    Ok(())
}
async fn succession(db: &mut PgConnection, g: &Guild) -> Res<()> {
    let candidates:Vec<String>=sqlx::query_scalar("SELECT m.user_id FROM guild_members m WHERE m.guild_id=$1 AND m.officer AND m.user_id IS DISTINCT FROM $2 AND NOT EXISTS(SELECT 1 FROM guilds other WHERE other.leader_id=m.user_id AND other.status='ACTIVE' AND other.id<>$1) ORDER BY m.joined_at,m.user_id")
        .bind(&g.id).bind(&g.leader_id).fetch_all(&mut *db).await?;
    let mut next = None;
    for id in candidates {
        if profiles::channel_user_by_id(db, &id)
            .await?
            .is_some_and(|u| u.eligible && u.email_verified)
        {
            next = Some(id);
            break;
        }
    }
    sqlx::query("UPDATE guilds SET leader_id=$2,status=CASE WHEN $2::text IS NULL THEN 'ARCHIVED' ELSE 'ACTIVE' END,revision=revision+1 WHERE id=$1")
        .bind(&g.id).bind(&next).execute(&mut *db).await?;
    event(
        db,
        &g.id,
        None,
        next.as_deref(),
        if next.is_some() {
            "leadership_passed"
        } else {
            "archived"
        },
    )
    .await
}
pub async fn tick(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let rows: Vec<Guild> = sqlx::query_as("SELECT * FROM guilds WHERE status='ACTIVE'")
        .fetch_all(&mut *tx)
        .await?;
    for g in rows {
        let absent = match &g.leader_id {
            None => true,
            Some(id) => {
                profiles::channel_user_by_id(&mut tx, id)
                    .await?
                    .is_none_or(|u| u.deleted_at.is_some())
                    || crate::bans::active(&mut tx, id).await?
            }
        };
        if absent {
            if let Some(id) = &g.leader_id {
                depart(&mut tx, &g, id, None).await?;
            } else {
                succession(&mut tx, &g).await?;
            }
        }
    }
    sqlx::query("DELETE FROM guild_invitations WHERE created_at<now()-interval '30 days'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
#[derive(Deserialize)]
struct Badge {
    guild_id: Option<String>,
}
async fn select_badge(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Badge>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    eligible(&mut tx, &user.id).await?;
    if let Some(id) = input.guild_id {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guild_members m JOIN guilds g ON g.id=m.guild_id WHERE m.guild_id=$1 AND m.user_id=$2 AND g.status='ACTIVE')").bind(&id).bind(&user.id).fetch_one(&mut *tx).await?;
        if !exists {
            return Err(Fail::denied("Choose a guild you belong to."));
        }
        sqlx::query("INSERT INTO guild_badges(user_id,guild_id) VALUES($1,$2) ON CONFLICT(user_id) DO UPDATE SET guild_id=$2").bind(&user.id).bind(id).execute(&mut *tx).await?;
    } else {
        sqlx::query("DELETE FROM guild_badges WHERE user_id=$1")
            .bind(&user.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
async fn upload(
    State(app): State<App>,
    jar: CookieJar,
    Path((slug, kind)): Path<(String, String)>,
    multipart: Multipart,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    {
        let mut db = app.db.acquire().await?;
        let g = load(&mut db, &slug).await?;
        manager(&mut db, &g, &user.id, true).await?;
    }
    let kind = match kind.as_str() {
        "avatar" => media::Kind::Emote,
        "banner" => media::Kind::Banner,
        _ => return Err(Fail::missing()),
    };
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Image uploads aren't available yet."));
    }
    profiles::rate(&app, format!("image-upload:{}", user.id), 20, 3600).await?;
    let (bytes, crop, _) = media::read_upload(multipart, kind).await?;
    let processed = media::process_async(bytes, kind, crop).await?;
    media::store(&app, &processed).await?;
    let mut tx = app.db.begin().await?;
    media::record(&mut tx, &user.id, kind, &processed).await?;
    lock(&mut tx).await?;
    let g = load(&mut tx, &slug).await?;
    manager(&mut tx, &g, &user.id, true).await?;
    if kind == media::Kind::Emote {
        sqlx::query("UPDATE guilds SET avatar_key=$2,emblem_visible=true,emblem_reviewed_at=NULL,revision=revision+1 WHERE id=$1").bind(&g.id).bind(&processed.stored).execute(&mut *tx).await?;
        media::queue_delete(&mut tx, g.avatar_key.as_deref(), Some(&processed.stored)).await?;
    } else {
        sqlx::query("UPDATE guilds SET banner_key=$2,revision=revision+1 WHERE id=$1")
            .bind(&g.id)
            .bind(&processed.stored)
            .execute(&mut *tx)
            .await?;
        media::queue_delete(&mut tx, g.banner_key.as_deref(), Some(&processed.stored)).await?;
    }
    event(&mut tx, &g.id, Some(&user.id), None, "image_uploaded").await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/guilds", get(read::list).post(create))
        .route("/api/guilds/{slug}", get(read::page).patch(update))
        .route("/api/guilds/{slug}/actions", post(act))
        .route(
            "/api/guilds/{slug}/media/{kind}",
            post(upload).layer(DefaultBodyLimit::max(10 * 1024 * 1024 + 16384)),
        )
        .route("/api/me/guilds", get(read::mine))
        .route("/api/me/guilds/badge", put(select_badge))
        .route("/api/admin/guilds", get(review::queue))
        .route("/api/admin/guilds/{id}", post(review::action))
}
