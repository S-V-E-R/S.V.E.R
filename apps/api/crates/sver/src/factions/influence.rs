use super::{Tuning, engine};
use crate::{
    App,
    profiles::{self, Res},
    security,
};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgConnection;

#[derive(Clone, Copy)]
enum Source {
    Stream,
    Watch,
    Chat,
    Support,
}
impl Source {
    fn index(self) -> usize {
        match self {
            Self::Stream => 0,
            Self::Watch => 1,
            Self::Chat => 2,
            Self::Support => 3,
        }
    }
    fn name(self) -> &'static str {
        ["stream", "watch", "chat", "support"][self.index()]
    }
}

struct Award<'a> {
    user: &'a str,
    genre: &'a str,
    source: Source,
    event: &'a str,
    elapsed_ms: i64,
}
async fn award(
    db: &mut PgConnection,
    tuning: &Tuning,
    input: Award<'_>,
    at: DateTime<Utc>,
) -> Res<i64> {
    engine::lock(db).await?;
    engine::advance(db, tuning, at).await?;
    let Some(week) = engine::current(db, at).await? else {
        return Ok(0);
    };
    let Some(user) = profiles::channel_user_by_id(db, input.user).await? else {
        return Ok(0);
    };
    if !user.eligible || !user.email_verified {
        return Ok(0);
    }
    let member: Option<(String, DateTime<Utc>)> =
        sqlx::query_as("SELECT faction,joined_at FROM faction_members WHERE user_id=$1")
            .bind(input.user)
            .fetch_optional(&mut *db)
            .await?;
    let Some((faction, joined)) = member else {
        return Ok(0);
    };
    let event = security::digest(&format!("faction:{}:{}", input.source.name(), input.event));
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM faction_influence WHERE event_key=$1)",
    )
    .bind(&event)
    .fetch_one(&mut *db)
    .await?
    {
        return Ok(0);
    }
    let territory: Option<(Option<String>,bool)>=sqlx::query_as("SELECT holder,EXISTS(SELECT 1 FROM faction_targets WHERE week_id=$2 AND faction=$3 AND genre=$4) FROM faction_territories WHERE season_id=$1 AND genre=$4")
        .bind(week.season_id).bind(week.id).bind(&faction).bind(input.genre).fetch_optional(&mut *db).await?;
    let Some((holder, target)) = territory else {
        return Ok(0);
    };
    let mut points = match input.source {
        Source::Chat => tuning.chat_points,
        Source::Support => tuning.supporter_points,
        Source::Stream | Source::Watch => {
            let last: Option<DateTime<Utc>> = sqlx::query_scalar(
                "SELECT last_at FROM faction_time_credit WHERE user_id=$1 AND source=$2",
            )
            .bind(input.user)
            .bind(input.source.name())
            .fetch_optional(&mut *db)
            .await?;
            let since = last
                .unwrap_or(at - Duration::milliseconds(input.elapsed_ms.clamp(0, 15_000)))
                .max(joined)
                .max(week.starts_at);
            let elapsed = (at - since)
                .num_milliseconds()
                .max(0)
                .min(input.elapsed_ms.clamp(0, 15_000));
            sqlx::query("INSERT INTO faction_time_credit(user_id,source,last_at) VALUES($1,$2,$3) ON CONFLICT(user_id,source) DO UPDATE SET last_at=greatest(faction_time_credit.last_at,EXCLUDED.last_at)")
                .bind(input.user).bind(input.source.name()).bind(at).execute(&mut *db).await?;
            elapsed * tuning.points_per_second[input.source.index()] / 1000
        }
    };
    if matches!(input.source, Source::Stream | Source::Watch)
        && holder.as_deref().is_some_and(|h| h != faction)
    {
        points = points * 3 / 2;
    }
    if target {
        points = points * 11 / 10;
    }
    let day = at.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let hour = DateTime::from_timestamp(at.timestamp().div_euclid(3600) * 3600, 0).unwrap();
    let (daily,hourly):(i64,i64)=sqlx::query_as("SELECT coalesce(sum(points),0)::bigint,coalesce(sum(points) FILTER(WHERE happened_at >= $4),0)::bigint FROM faction_influence WHERE user_id=$1 AND source=$2 AND happened_at >= $3 AND happened_at<$3+interval '1 day'")
        .bind(input.user).bind(input.source.name()).bind(day).bind(hour).fetch_one(&mut *db).await?;
    points = points.min((tuning.daily_caps[input.source.index()] - daily).max(0));
    if matches!(input.source, Source::Chat) {
        points = points.min((tuning.chat_hourly_cap - hourly).max(0));
    }
    if points <= 0 {
        return Ok(0);
    }
    sqlx::query("INSERT INTO faction_influence(event_key,user_id,faction,week_id,genre,source,points,happened_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(event).bind(input.user).bind(faction).bind(week.id).bind(input.genre)
        .bind(input.source.name()).bind(points).bind(at).execute(db).await?;
    Ok(points)
}

/// Called only after integrity has recorded verified, visible, advancing Trusted playback.
/// The caller's transaction also owns the lease, so an award cannot outlive a failed heartbeat.
pub(crate) async fn watch(
    app: &App,
    db: &mut PgConnection,
    broadcast: &str,
    user: &str,
    elapsed_ms: i64,
    at: DateTime<Utc>,
) -> Res<i64> {
    let Some((_, genre)) = crate::streams::influence_context(db, broadcast).await? else {
        return Ok(0);
    };
    if !crate::integrity::trusted(db, broadcast, Some(user)).await? {
        return Ok(0);
    }
    let event = format!("{user}:{broadcast}:{}", at.timestamp_micros());
    award(
        db,
        &app.config.factions,
        Award {
            user,
            genre: &genre,
            source: Source::Watch,
            event: &event,
            elapsed_ms,
        },
        at,
    )
    .await
}
/// SRS polling supplies actual advancing media time, never a browser claim.
pub(crate) async fn stream(
    app: &App,
    db: &mut PgConnection,
    broadcast: &str,
    elapsed_ms: i64,
    at: DateTime<Utc>,
) -> Res<i64> {
    let Some((owner, genre)) = crate::streams::influence_context(db, broadcast).await? else {
        return Ok(0);
    };
    if !crate::integrity::trusted(db, broadcast, None).await? {
        return Ok(0);
    }
    let event = format!("{broadcast}:{}", at.timestamp_micros());
    award(
        db,
        &app.config.factions,
        Award {
            user: &owner,
            genre: &genre,
            source: Source::Stream,
            event: &event,
            elapsed_ms,
        },
        at,
    )
    .await
}
pub(crate) async fn chat(
    app: &App,
    db: &mut PgConnection,
    owner: &str,
    user: &str,
    message: &str,
) -> Res<i64> {
    let Some(broadcast) = crate::streams::live_broadcast(db, owner).await? else {
        return Ok(0);
    };
    let Some((_, genre)) = crate::streams::influence_context(db, &broadcast).await? else {
        return Ok(0);
    };
    if !crate::integrity::trusted(db, &broadcast, Some(user)).await? {
        return Ok(0);
    }
    let at = engine::now(db).await?;
    award(
        db,
        &app.config.factions,
        Award {
            user,
            genre: &genre,
            source: Source::Chat,
            event: message,
            elapsed_ms: 0,
        },
        at,
    )
    .await
}
/// Module 6 calls this inside its settled support transaction. There is intentionally no HTTP
/// award endpoint. Distinct supporters, not transaction amounts, contribute to the creator's side.
pub async fn support(
    app: &App,
    db: &mut PgConnection,
    broadcast: &str,
    supporter: &str,
    settled_event: &str,
) -> Res<i64> {
    engine::lock(db).await?;
    let at = engine::now(db).await?;
    let event = security::digest(&format!("support:{settled_event}"));
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM faction_support_events WHERE event_key=$1)",
    )
    .bind(&event)
    .fetch_one(&mut *db)
    .await?
    {
        return Ok(0);
    }
    let Some((owner, genre)) = crate::streams::influence_context(db, broadcast).await? else {
        // Remember even an ineligible settled event; replay cannot move it to a later season.
        sqlx::query("INSERT INTO faction_support_events(event_key,pair_key) VALUES($1,$1)")
            .bind(&event)
            .execute(db)
            .await?;
        return Ok(0);
    };
    let pair = security::digest(&format!("{owner}:{supporter}:{}", at.date_naive()));
    let counted: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_support_events WHERE pair_key=$1)")
            .bind(&pair)
            .fetch_one(&mut *db)
            .await?;
    sqlx::query("INSERT INTO faction_support_events(event_key,pair_key) VALUES($1,$2)")
        .bind(&event)
        .bind(pair)
        .execute(&mut *db)
        .await?;
    if counted {
        return Ok(0);
    }
    if owner == supporter || !crate::integrity::trusted(db, broadcast, Some(supporter)).await? {
        return Ok(0);
    }
    if !profiles::channel_user_by_id(db, supporter)
        .await?
        .is_some_and(|u| u.eligible && u.email_verified)
    {
        return Ok(0);
    }
    award(
        db,
        &app.config.factions,
        Award {
            user: &owner,
            genre: &genre,
            source: Source::Support,
            event: &event,
            elapsed_ms: 0,
        },
        at,
    )
    .await
}
