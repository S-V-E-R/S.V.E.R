use super::*;

/// Called by MAGNet in its feature transaction. Room signals choose the moment;
/// this module only saves a suggestion, which always needs human approval.
pub async fn spotlight(db: &mut PgConnection, owner: &str, feature: &str) -> Res<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(80742)")
        .execute(&mut *db)
        .await?;
    let source:Option<Video>=sqlx::query_as("SELECT * FROM videos WHERE owner_id=$1 AND kind='VOD' AND status='RECORDING' AND ended_at IS NULL FOR UPDATE").bind(owner).fetch_optional(&mut *db).await?;
    let Some(source) = source else { return Ok(()) };
    chapter(db, owner, "MAGNET", Some(feature), "MAGNet spotlight").await?;
    let settings = settings(db, owner).await?;
    if settings.copyright_restricted
        || settings.clip_permission == "OFF"
        || held(db, &source.id).await?
    {
        return Ok(());
    }
    let length = match source.genre.as_deref() {
        Some(
            "fps_battle_royale" | "fighting" | "sports_racing" | "speedrunning" | "rts_moba"
            | "strategy_4x" | "card_board" | "puzzle_simulation" | "mmos_rpgs" | "coop_party"
            | "cozy_sandbox",
        ) => 35000,
        Some("crafting_making" | "art" | "music") => 60000,
        _ => 45000,
    };
    let segments:Vec<(String,i64,i64)>=sqlx::query_as("SELECT s.id,s.start_ms,s.duration_ms FROM video_segments s JOIN video_objects o ON o.key=s.object_key WHERE s.video_id=$1 AND o.ready AND s.start_ms>=greatest(0,$2-$3) ORDER BY s.start_ms").bind(&source.id).bind(source.duration_ms).bind(length).fetch_all(&mut *db).await?;
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        return Ok(());
    };
    let start = first.1;
    let end = last.1 + last.2;
    let duration = end - start;
    if !(5000..=60000).contains(&duration) || segments.windows(2).any(|w| w[0].1 + w[0].2 != w[1].1)
    {
        return Ok(());
    }
    let id = profiles::new_id();
    let added=sqlx::query("INSERT INTO videos(id,owner_id,clipper_id,broadcast_id,parent_id,kind,status,approval,visibility,mature,title,category_id,category,genre,faction,started_at,ended_at,duration_ms,source_start_ms,source_end_ms,request_key) VALUES($1,$2,$2,$3,$4,'CLIP','PROCESSING','PENDING',$5,$6,'MAGNet spotlight',$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) ON CONFLICT(clipper_id,request_key) DO NOTHING")
        .bind(&id).bind(owner).bind(&source.broadcast_id).bind(&source.id).bind(&source.visibility).bind(source.mature).bind(&source.category_id).bind(&source.category).bind(&source.genre).bind(&source.faction).bind(source.started_at+chrono::Duration::milliseconds(start)).bind(source.started_at+chrono::Duration::milliseconds(end)).bind(duration).bind(start).bind(end).bind(format!("magnet:{feature}")).execute(&mut *db).await?.rows_affected();
    if added == 0 {
        return Ok(());
    }
    for (position, (segment, _, _)) in segments.iter().enumerate() {
        sqlx::query(
            "INSERT INTO video_copy_sources(video_id,position,segment_id) VALUES($1,$2,$3)",
        )
        .bind(&id)
        .bind(position as i32)
        .bind(segment)
        .execute(&mut *db)
        .await?;
    }
    copy_replay(db, &source.id, &id, owner, start, end, true).await?;
    enqueue(
        db,
        &id,
        "ASSEMBLE",
        Some(&format!("{id}:assemble")),
        json!({}),
    )
    .await
}
