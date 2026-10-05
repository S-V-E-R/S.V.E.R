use super::*;
use axum::extract::Query;
use chrono::{DateTime, Utc};

async fn card(app: &App, db: &mut PgConnection, g: &Guild) -> Res<Value> {
    let avatar = if g.emblem_visible {
        g.avatar_key.as_deref()
    } else {
        None
    };
    let image = match avatar {
        Some(k) if !media::removal::held(db, k).await? => {
            json!(profiles::media_url(app, &format!("{k}/112.webp")))
        }
        _ => Value::Null,
    };
    let banner = match g.banner_key.as_deref() {
        Some(k) if !media::removal::held(db, k).await? => profiles::banner_json(app, Some(k)),
        _ => Value::Null,
    };
    Ok(
        json!({"id":g.id,"slug":g.slug,"name":g.name,"tag":g.tag,"tagline":g.tagline,"about":g.about,"status":g.status,"recruiting":g.recruiting&&g.status=="ACTIVE","verified":g.verified,"avatar":image,"banner":banner,"revision":g.revision}),
    )
}
#[derive(Deserialize)]
pub(super) struct ListQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: i64,
}
pub(super) async fn list(State(app): State<App>, Query(q): Query<ListQuery>) -> Res<Json<Value>> {
    if q.q.len() > 100 || !(0..=10000).contains(&q.offset) {
        return Err(Fail::bad("Invalid guild search."));
    }
    let mut db = app.db.acquire().await?;
    let rows:Vec<Guild>=sqlx::query_as("SELECT * FROM guilds WHERE status='ACTIVE' AND (strpos(lower(name),lower($1))>0 OR strpos(lower(tag),lower($1))>0) ORDER BY lower(name),id LIMIT 31 OFFSET $2")
        .bind(q.q.trim()).bind(q.offset).fetch_all(&mut *db).await?;
    let more = rows.len() > 30;
    let mut items = Vec::new();
    for g in rows.into_iter().take(30) {
        items.push(card(&app, &mut db, &g).await?);
    }
    Ok(Json(
        json!({"items":items,"next_offset":more.then_some(q.offset+30)}),
    ))
}
pub(super) async fn page(
    State(app): State<App>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> Res<Json<Value>> {
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let g = load(&mut db, &slug).await?;
    // An orphaned archive has no account left for content review; retain it only for staff.
    if g.leader_id.is_none() && super::owner(&mut db, &g.id).await?.is_none() {
        return Err(Fail::missing());
    }
    let me = match &viewer {
        Some(u) => role(&mut db, &g, &u.id).await?,
        None => None,
    };
    let manage = matches!(me, Some("leader" | "officer"))
        && g.status == "ACTIVE"
        && match &viewer {
            Some(u) => profiles::channel_user_by_id(&mut db, &u.id)
                .await?
                .is_some_and(|p| p.eligible),
            None => false,
        };
    let rows:Vec<(String,bool,String,DateTime<Utc>)>=sqlx::query_as("SELECT user_id,officer,title,joined_at FROM guild_members WHERE guild_id=$1 ORDER BY joined_at,user_id")
        .bind(&g.id).fetch_all(&mut *db).await?;
    let users = profiles::public_channels(
        &mut db,
        &rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
    )
    .await?;
    let mut members = Vec::new();
    let mut schedule = Vec::new();
    for (id, officer, title, at) in rows {
        let user = match users.iter().find(|u| u.id == id) {
            Some(u) => Some(u.clone()),
            None if manage => profiles::channel_user_by_id(&mut db, &id).await?,
            None => None,
        };
        let Some(user) = user else {
            continue;
        };
        if let Some(v) = &viewer
            && !manage
            && profiles::blocked_between(&mut db, &v.id, &id).await?
        {
            continue;
        }
        let live = user.eligible && crate::playback::is_live(&mut db, &id).await?;
        let chip = profiles::chip(&app, &user);
        members.push(json!({"user":chip,"live":live,"role":if g.leader_id.as_deref()==Some(&id){"leader"}else if officer{"officer"}else{"member"},"title":title,"joined_at":at}));
        if !user.eligible {
            continue;
        }
        // ponytail: schedule expansion is per member; batch it when large guilds cause measured latency.
        let upcoming = crate::studio::next_occurrences(&mut db, &id, 20).await?;
        for occurrence in upcoming["items"].as_array().into_iter().flatten() {
            schedule.push(json!({"user":chip,"occurrence":occurrence}));
        }
    }
    // Existing schedule expansion owns DST. Team lists never sort by audience size.
    members.sort_by_key(|m| !m["live"].as_bool().unwrap_or(false));
    schedule.sort_by(|a, b| {
        a["occurrence"]["start_at"]
            .as_str()
            .cmp(&b["occurrence"]["start_at"].as_str())
    });
    schedule.truncate(100);
    let (follow, muted, application, badge, invite) = if let Some(u) = &viewer {
        let follow: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM guild_follows WHERE guild_id=$1 AND user_id=$2)",
        )
        .bind(&g.id)
        .bind(&u.id)
        .fetch_one(&mut *db)
        .await?;
        let muted = !notify_allowed(&mut db, &g.id, &u.id).await?;
        let application:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('status',status,'message',message,'note',note,'created_at',created_at,'reapply_at',declined_at+interval '7 days') FROM guild_applications WHERE guild_id=$1 AND user_id=$2").bind(&g.id).bind(&u.id).fetch_optional(&mut *db).await?;
        let badge: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM guild_badges WHERE guild_id=$1 AND user_id=$2)",
        )
        .bind(&g.id)
        .bind(&u.id)
        .fetch_one(&mut *db)
        .await?;
        let invite:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guild_invitations WHERE guild_id=$1 AND user_id=$2 AND created_at>now()-interval '30 days')").bind(&g.id).bind(&u.id).fetch_one(&mut *db).await?;
        (follow, muted, application, badge, invite)
    } else {
        (false, false, None, false, false)
    };
    let management = if manage {
        let applications:Vec<Value>=sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT jsonb_build_object('id',a.id,'user',{},'message',a.message,'created_at',a.created_at) FROM guild_applications a JOIN channel_users c ON c.id=a.user_id WHERE a.guild_id=$1 AND a.status='OPEN' ORDER BY a.created_at,a.id",profiles::chip_sql("c"))))
            .bind(&g.id).fetch_all(&mut *db).await?;
        let events:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',e.id,'action',e.action,'actor',(SELECT username FROM channel_users WHERE id=e.actor_id AND deleted_at IS NULL),'subject',(SELECT username FROM channel_users WHERE id=e.subject_id AND deleted_at IS NULL),'created_at',e.created_at) FROM guild_events e WHERE e.guild_id=$1 ORDER BY e.id DESC LIMIT 100").bind(&g.id).fetch_all(&mut *db).await?;
        let blocks:Vec<String>=sqlx::query_scalar("SELECT c.username FROM guild_blocks b JOIN channel_users c ON c.id=b.user_id WHERE b.guild_id=$1 AND c.deleted_at IS NULL ORDER BY lower(c.username)").bind(&g.id).fetch_all(&mut *db).await?;
        let verification:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('status',status,'note',note) FROM guild_verification WHERE guild_id=$1").bind(&g.id).fetch_optional(&mut *db).await?;
        let mut value = json!({"applications":applications,"events":events,"blocks":blocks,"verification":verification});
        profiles::hydrate(&app, &mut value);
        value
    } else {
        Value::Null
    };
    Ok(Json(
        json!({"guild":card(&app,&mut db,&g).await?,"members":members,"schedule":schedule,"viewer":{"signed_in":viewer.is_some(),"role":me,"can_manage":manage,"following":follow,"muted":muted,"application":application,"badge":badge,"invited":invite},"management":management}),
    ))
}
pub(super) async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let rows:Vec<Guild>=sqlx::query_as("SELECT g.* FROM guilds g WHERE g.status<>'REMOVED' AND (EXISTS(SELECT 1 FROM guild_members m WHERE m.guild_id=g.id AND m.user_id=$1) OR EXISTS(SELECT 1 FROM guild_invitations i WHERE i.guild_id=g.id AND i.user_id=$1) OR EXISTS(SELECT 1 FROM guild_applications a WHERE a.guild_id=g.id AND a.user_id=$1)) ORDER BY lower(g.name)")
        .bind(&user.id).fetch_all(&mut *db).await?;
    let mut items = Vec::new();
    for g in rows {
        let mut c = card(&app, &mut db, &g).await?;
        c["role"] = json!(role(&mut db, &g, &user.id).await?);
        items.push(c);
    }
    let can_create = streams::eligible(&mut db, &user).await?
        && streams::has_streamed(&mut db, &user.id).await?
        && !items
            .iter()
            .any(|g| g["role"] == "leader" && g["status"] == "ACTIVE")
        && items
            .iter()
            .filter(|g| !g["role"].is_null() && g["status"] == "ACTIVE")
            .count()
            < 3;
    let badge: Option<String> =
        sqlx::query_scalar("SELECT guild_id FROM guild_badges WHERE user_id=$1")
            .bind(&user.id)
            .fetch_optional(&mut *db)
            .await?;
    Ok(Json(
        json!({"items":items,"can_create":can_create,"badge":badge}),
    ))
}
pub async fn followed_members(
    db: &mut PgConnection,
    user: &str,
) -> Res<Vec<(String, DateTime<Utc>, Value)>> {
    Ok(sqlx::query_as("SELECT m.user_id,max(f.created_at),jsonb_agg(jsonb_build_object('slug',g.slug,'name',g.name)) FROM guild_follows f JOIN guilds g ON g.id=f.guild_id AND g.status='ACTIVE' JOIN guild_members m ON m.guild_id=g.id WHERE f.user_id=$1 AND m.user_id<>$1 GROUP BY m.user_id")
        .bind(user).fetch_all(db).await?)
}
pub async fn notify_allowed(db: &mut PgConnection, guild: &str, user: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guilds WHERE id=$1 AND status='ACTIVE') AND NOT EXISTS(SELECT 1 FROM guild_mutes WHERE guild_id=$1 AND user_id=$2)")
        .bind(guild).bind(user).fetch_one(db).await?)
}
pub async fn id_by_slug(db: &mut PgConnection, slug: &str) -> Res<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM guilds WHERE lower(slug)=lower($1)")
            .bind(slug)
            .fetch_optional(db)
            .await?,
    )
}
pub fn badge_sql(user: &'static str) -> String {
    format!(
        "(SELECT jsonb_build_object('slug',g.slug,'name',g.name,'tag',g.tag,'guild_image',CASE WHEN g.emblem_visible THEN g.avatar_key END) FROM guild_badges b JOIN guilds g ON g.id=b.guild_id AND g.status='ACTIVE' WHERE b.user_id={user})"
    )
}
pub async fn hydrate_badges(app: &App, items: &mut [Value]) -> Res<()> {
    let roots: Vec<String> = items
        .iter()
        .filter_map(|m| {
            m["author"]["guild"]["guild_image"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    let held = media::removal::held_roots(&mut *app.db.acquire().await?, &roots).await?;
    for item in items {
        if let Some(g) = item
            .get_mut("author")
            .and_then(|a| a.get_mut("guild"))
            .and_then(Value::as_object_mut)
        {
            let key = g.remove("guild_image");
            let image = key
                .as_ref()
                .and_then(Value::as_str)
                .filter(|k| !held.contains(*k))
                .map(|k| profiles::media_url(app, &format!("{k}/28.webp")));
            g.insert("image".into(), json!(image));
        }
    }
    Ok(())
}
