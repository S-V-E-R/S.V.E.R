use super::*;

pub async fn owner(db: &mut PgConnection, id: &str) -> Res<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT coalesce(leader_id,(SELECT actor_id FROM guild_events WHERE guild_id=$1 AND action='created' AND actor_id IS NOT NULL ORDER BY id LIMIT 1)) FROM guilds WHERE id=$1")
            .bind(id)
            .fetch_optional(db)
            .await?.flatten(),
    )
}
pub async fn snapshot(db: &mut PgConnection, id: &str) -> Res<Value> {
    let mut value:Value=sqlx::query_scalar("SELECT jsonb_build_object('name',name,'tag',tag,'slug',slug,'tagline',tagline,'about',about,'image',CASE WHEN emblem_visible AND avatar_key IS NOT NULL THEN avatar_key||'/112.webp' END,'banner',banner_key,'status',status) FROM guilds WHERE id=$1")
        .bind(id).fetch_optional(&mut *db).await?.unwrap_or(Value::Null);
    for field in ["image", "banner"] {
        if let Some(key) = value[field].as_str()
            && media::removal::held(db, key).await?
        {
            value[field] = Value::Null;
        }
    }
    Ok(value)
}
pub async fn target(
    db: &mut PgConnection,
    id: &str,
    reporter: &str,
    emblem: bool,
) -> Res<Option<(String, String, Value)>> {
    let row:Option<bool>=sqlx::query_scalar("SELECT emblem_visible AND avatar_key IS NOT NULL FROM guilds WHERE id=$1 AND status<>'REMOVED'").bind(id).fetch_optional(&mut *db).await?;
    let Some(visible) = row else {
        return Ok(None);
    };
    let Some(leader) = owner(db, id).await? else {
        return Ok(None);
    };
    if emblem && !visible {
        return Ok(None);
    }
    let Some(user) = profiles::channel_user_by_id(db, &leader).await? else {
        return Ok(None);
    };
    if profiles::blocked_between(db, reporter, &leader).await? {
        return Ok(None);
    }
    let value = snapshot(db, id).await?;
    if emblem && value["image"].is_null() {
        return Ok(None);
    }
    Ok(Some((leader, user.username, value)))
}
pub async fn remove(db: &mut PgConnection, id: &str, emblem: bool) -> Res<Value> {
    lock(db).await?;
    let previous: String = if emblem {
        let visible: bool = sqlx::query_scalar("SELECT emblem_visible FROM guilds WHERE id=$1")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(Fail::missing)?;
        sqlx::query("UPDATE guilds SET emblem_visible=false,emblem_reviewed_at=now(),revision=revision+1 WHERE id=$1").bind(id).execute(&mut *db).await?;
        visible.to_string()
    } else {
        let status: String = sqlx::query_scalar("SELECT status FROM guilds WHERE id=$1")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(Fail::missing)?;
        sqlx::query(
            "UPDATE guilds SET status='REMOVED',verified=false,revision=revision+1 WHERE id=$1",
        )
        .bind(id)
        .execute(&mut *db)
        .await?;
        status
    };
    event(
        db,
        id,
        None,
        None,
        if emblem {
            "emblem_removed"
        } else {
            "disbanded"
        },
    )
    .await?;
    Ok(json!({"type":if emblem{"guild_emblem"}else{"guild"},"id":id,"previous":previous}))
}
pub async fn restore(db: &mut PgConnection, id: &str, previous: &str, emblem: bool) -> Res<()> {
    lock(db).await?;
    if emblem {
        sqlx::query("UPDATE guilds SET emblem_visible=$2,revision=revision+1 WHERE id=$1")
            .bind(id)
            .bind(previous == "true")
            .execute(&mut *db)
            .await?;
    } else if ["ACTIVE", "ARCHIVED"].contains(&previous) {
        let conflict:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guild_members m WHERE m.guild_id=$1 AND (SELECT count(*) FROM guild_members other JOIN guilds g ON g.id=other.guild_id AND g.status='ACTIVE' WHERE other.user_id=m.user_id AND other.guild_id<>$1)>=3) OR EXISTS(SELECT 1 FROM guilds g JOIN guilds other ON other.leader_id=g.leader_id AND other.id<>g.id AND other.status='ACTIVE' WHERE g.id=$1)")
            .bind(id).fetch_one(&mut *db).await?;
        if previous == "ACTIVE" && conflict {
            return Err(Fail::conflict(
                "Resolve conflicting guild memberships or leadership before restoring this guild.",
            ));
        }
        sqlx::query(
            "UPDATE guilds SET status=$2,revision=revision+1 WHERE id=$1 AND status='REMOVED'",
        )
        .bind(id)
        .bind(previous)
        .execute(&mut *db)
        .await?;
    }
    event(db, id, None, None, "restored").await
}
pub async fn referenced(db: &mut PgConnection, key: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guilds WHERE avatar_key=$1 OR banner_key=$2 OR split_part(banner_key,'@',1)=$1)")
        .bind(media::removal::root_of_key(key)).bind(key).fetch_one(db).await?)
}
pub async fn remove_media(db: &mut PgConnection, root: &str) -> Res<Vec<String>> {
    let owners = sqlx::query_scalar("SELECT DISTINCT leader_id FROM guilds WHERE leader_id IS NOT NULL AND (avatar_key=$1 OR split_part(banner_key,'@',1)=$1)")
        .bind(root).fetch_all(&mut *db).await?;
    sqlx::query("UPDATE guilds SET avatar_key=CASE WHEN avatar_key=$1 THEN NULL ELSE avatar_key END,banner_key=CASE WHEN split_part(banner_key,'@',1)=$1 THEN NULL ELSE banner_key END,revision=revision+1 WHERE avatar_key=$1 OR split_part(banner_key,'@',1)=$1")
        .bind(root).execute(db).await?;
    Ok(owners)
}
#[derive(Deserialize)]
pub(super) struct Query {
    #[serde(default)]
    q: String,
}
pub(super) async fn queue(
    State(app): State<App>,
    jar: CookieJar,
    axum::extract::Query(q): axum::extract::Query<Query>,
) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    if q.q.len() > 100 {
        return Err(Fail::bad("Search is too long."));
    }
    let mut db = app.db.acquire().await?;
    let rows:Vec<(String,Value)>=sqlx::query_as("SELECT g.id,jsonb_build_object('id',g.id,'name',g.name,'slug',g.slug,'tag',g.tag,'verified',g.verified,'status',g.status,'emblem_pending',g.avatar_key IS NOT NULL AND g.emblem_visible AND g.emblem_reviewed_at IS NULL,'verification_status',v.status,'evidence',v.evidence,'note',v.note) FROM guilds g LEFT JOIN guild_verification v ON v.guild_id=g.id WHERE ($1='' AND (v.status='OPEN' OR (g.avatar_key IS NOT NULL AND g.emblem_visible AND g.emblem_reviewed_at IS NULL))) OR ($1<>'' AND strpos(lower(g.name||' '||g.slug||' '||g.tag),lower($1))>0) ORDER BY g.created_at LIMIT 100")
        .bind(q.q.trim()).fetch_all(&mut *db).await?;
    let mut items = Vec::new();
    for (id, mut row) in rows {
        row["content"] = snapshot(&mut db, &id).await?;
        if let Some(k) = row["content"]["image"].as_str() {
            row["content"]["image"] = json!(profiles::media_url(&app, k));
        }
        if let Some(k) = row["content"]["banner"].as_str() {
            row["content"]["banner"] = profiles::banner_json(&app, Some(k));
        }
        items.push(row);
    }
    Ok(Json(json!({"items":items})))
}
#[derive(Deserialize)]
pub(super) struct StaffAction {
    action: String,
    note: String,
    branding: Option<Branding>,
}
pub(super) async fn action(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(mut input): Json<StaffAction>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let note = safety::note(Some(&input.note), "note", true)?;
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM guilds WHERE id=$1)")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(Fail::missing());
    }
    match input.action.as_str() {
        "verify" | "decline_verification" => {
            let changed=sqlx::query("UPDATE guild_verification SET status=$2,note=$3 WHERE guild_id=$1 AND status='OPEN'").bind(&id).bind(if input.action=="verify"{"APPROVED"}else{"DECLINED"}).bind(&note).execute(&mut *tx).await?.rows_affected();
            if changed == 0 {
                return Err(Fail::conflict("There is no pending verification request."));
            }
            sqlx::query(
                "UPDATE guilds SET verified=$2,revision=revision+1 WHERE id=$1 AND status='ACTIVE'",
            )
            .bind(&id)
            .bind(input.action == "verify")
            .execute(&mut *tx)
            .await?;
        }
        "unverify" => {
            sqlx::query("UPDATE guilds SET verified=false,revision=revision+1 WHERE id=$1")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
        "approve_emblem" => {
            sqlx::query(
                "UPDATE guilds SET emblem_reviewed_at=now() WHERE id=$1 AND emblem_visible",
            )
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        }
        "remove_emblem" => {
            remove(&mut tx, &id, true).await?;
        }
        "disband" => {
            remove(&mut tx, &id, false).await?;
        }
        "reset" => {
            let (avatar, banner): (Option<String>, Option<String>) =
                sqlx::query_as("SELECT avatar_key,banner_key FROM guilds WHERE id=$1")
                    .bind(&id)
                    .fetch_one(&mut *tx)
                    .await?;
            sqlx::query("UPDATE guilds SET tagline='',about='',avatar_key=NULL,banner_key=NULL,verified=false,revision=revision+1 WHERE id=$1").bind(&id).execute(&mut *tx).await?;
            media::queue_delete(&mut tx, avatar.as_deref(), None).await?;
            media::queue_delete(&mut tx, banner.as_deref(), None).await?;
        }
        "rename" => {
            let b = input
                .branding
                .as_mut()
                .ok_or_else(|| Fail::bad("Supply the corrected guild identity."))?;
            validate(b)?;
            unique(&mut tx, b, Some(&id)).await?;
            sqlx::query("UPDATE guilds SET name=$2,slug=$3,tag=$4,verified=false,revision=revision+1 WHERE id=$1").bind(&id).bind(&b.name).bind(&b.slug).bind(&b.tag).execute(&mut *tx).await?;
        }
        _ => return Err(Fail::bad("Choose a staff action.")),
    }
    safety::audit(
        &mut tx,
        Some(&staff.id),
        &input.action,
        "guild",
        &id,
        &[],
        &note,
        json!({}),
        false,
    )
    .await?;
    event(&mut tx, &id, Some(&staff.id), None, &input.action).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
