use super::{Tuning, engine};
use crate::{
    App,
    profiles::{self, Res},
    security,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::PgConnection;

#[derive(Clone, Copy)]
enum Source {
    Stream,
    Watch,
    Chat,
    Support,
    Raid,
    Discord,
}
impl Source {
    fn index(self) -> usize {
        match self {
            Self::Stream => 0,
            Self::Watch => 1,
            Self::Chat => 2,
            Self::Support => 3,
            Self::Raid => 4,
            Self::Discord => 5,
        }
    }
    fn name(self) -> &'static str {
        ["stream", "watch", "chat", "support", "raid", "discord"][self.index()]
    }
    fn daily_cap(self, tuning: &Tuning) -> i64 {
        match self {
            Self::Raid => tuning.raid_daily_cap,
            Self::Discord => tuning.discord_daily_cap,
            _ => tuning.daily_caps[self.index()],
        }
    }
}

struct Award<'a> {
    user: &'a str,
    genre: &'a str,
    source: Source,
    event: &'a str,
    elapsed_ms: i64,
    /// A viewer who arrived by a cross-faction raid (Raid Night counts their watch time double).
    raided: bool,
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
        Source::Raid => tuning.raid_points,
        Source::Discord => tuning.discord_points,
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
    // Faction events double influence before the caps, so the caps still bind.
    points *= super::skirmish::multiplier(db, at, input.genre, input.raided).await?;
    let day = at.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let hour = DateTime::from_timestamp(at.timestamp().div_euclid(3600) * 3600, 0).unwrap();
    let (daily,hourly):(i64,i64)=sqlx::query_as("SELECT coalesce(sum(points),0)::bigint,coalesce(sum(points) FILTER(WHERE happened_at >= $4),0)::bigint FROM faction_influence WHERE user_id=$1 AND source=$2 AND happened_at >= $3 AND happened_at<$3+interval '1 day'")
        .bind(input.user).bind(input.source.name()).bind(day).bind(hour).fetch_one(&mut *db).await?;
    points = points.min((input.source.daily_cap(tuning) - daily).max(0));
    if matches!(input.source, Source::Chat) {
        points = points.min((tuning.chat_hourly_cap - hourly).max(0));
    }
    // A capped raid or chat is still recorded (0 points), so retrying the same event on another
    // day can't earn. Time-based sources have no event to replay and aren't recorded.
    let points = points.max(0);
    if points == 0 && !matches!(input.source, Source::Raid | Source::Chat | Source::Discord) {
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
    let Some((owner, genre)) = crate::streams::influence_context(db, broadcast).await? else {
        return Ok(0);
    };
    // Watching your own stream never counts, whatever playback says.
    if owner == user || !crate::integrity::trusted(db, broadcast, Some(user)).await? {
        return Ok(0);
    }
    let event = format!("{user}:{broadcast}:{}", at.timestamp_micros());
    let raided: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playback_leases l JOIN raids r ON r.id=l.raid_id JOIN faction_members a ON a.user_id=r.raider_id JOIN faction_members b ON b.user_id=r.target_id WHERE l.broadcast_id=$1 AND l.viewer_key='u:'||$2 AND a.faction<>b.faction)")
        .bind(broadcast).bind(user).fetch_one(&mut *db).await?;
    award(
        db,
        &app.config.factions,
        Award {
            user,
            genre: &genre,
            source: Source::Watch,
            event: &event,
            elapsed_ms,
            raided,
        },
        at,
    )
    .await
}
/// A raid whose arrivals were counted, into a live faction member's channel (ally or enemy): the
/// raiding streamer earns influence in the target's genre. At least one viewer must have actually
/// arrived and the target must have Trusted playback, so an empty or bot raid earns nothing. A
/// cross-faction raid counts as raided, so a Raid Night doubles it (events still never stack).
pub(crate) async fn raid(app: &App, raid: &str) -> Res<i64> {
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String, bool)> = sqlx::query_as("SELECT r.raider_id,r.target_broadcast_id,m.faction IS DISTINCT FROM (SELECT faction FROM faction_members WHERE user_id=r.raider_id) FROM raids r JOIN faction_members m ON m.user_id=r.target_id WHERE r.id=$1 AND r.status='moved' AND r.arrivals>0")
        .bind(raid).fetch_optional(&mut *tx).await?;
    let Some((raider, target, cross)) = row else {
        return Ok(0);
    };
    let Some((_, genre)) = crate::streams::influence_context(&mut tx, &target).await? else {
        return Ok(0);
    };
    if !crate::integrity::trusted(&mut tx, &target, None).await? {
        return Ok(0);
    }
    let at: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let points = award(
        &mut tx,
        &app.config.factions,
        Award {
            user: &raider,
            genre: &genre,
            source: Source::Raid,
            event: raid,
            elapsed_ms: 0,
            raided: cross,
        },
        at,
    )
    .await?;
    tx.commit().await?;
    Ok(points)
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
            raided: false,
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
    // Chatting in your own channel never counts.
    if owner == user || !crate::integrity::trusted(db, &broadcast, Some(user)).await? {
        return Ok(0);
    }
    let at = engine::now(db).await?;
    // The message's id, scoped to its sender.
    let event = format!("{user}:{message}");
    award(
        db,
        &app.config.factions,
        Award {
            user,
            genre: &genre,
            source: Source::Chat,
            event: &event,
            elapsed_ms: 0,
            raided: false,
        },
        at,
    )
    .await
}
/// Chat or a voice minute on the S.V.E.R Discord server, in a channel staff mapped to `genre`, by
/// the linked account `user` (docs/DISCORD_BOT.md "Influence"). `event` is the message id, or the
/// member and minute for voice. The war's usual rules apply, with the Discord daily cap.
pub(crate) async fn discord(
    app: &App,
    db: &mut PgConnection,
    user: &str,
    genre: &str,
    event: &str,
) -> Res<i64> {
    let at = engine::now(db).await?;
    award(
        db,
        &app.config.factions,
        Award {
            user,
            genre,
            source: Source::Discord,
            event,
            elapsed_ms: 0,
            raided: false,
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
            raided: false,
        },
        at,
    )
    .await
}

/// Public, account-scoped answer for "does this live activity qualify?" No points, remaining
/// allowance, or integrity signals. Reads only: it does not open a spotlight or run a checkpoint.
pub(crate) async fn activity(
    app: &App,
    viewer: Option<&crate::auth::User>,
    owner: &str,
    broadcast: &str,
) -> Res<Value> {
    let mut db = app.db.acquire().await?;
    let at = engine::now(&mut db).await?;
    let open = engine::current(&mut db, at).await?.is_some();
    let season = engine::latest(&mut db).await?;
    let season = if open {
        "open"
    } else if season
        .as_ref()
        .is_some_and(|s| s.finished_at.is_some() && at < s.next_starts_at)
    {
        "between"
    } else if season
        .as_ref()
        .is_some_and(|s| s.finished_at.is_none() && at < s.ends_at)
    {
        "due"
    } else {
        "closed"
    };
    let account = match viewer {
        Some(v) => profiles::channel_user_by_id(&mut db, &v.id).await?,
        None => None,
    };
    let faction = match viewer {
        Some(v) => super::membership(&mut db, &v.id).await?,
        None => None,
    };
    let media = crate::streams::influence_context(&mut db, broadcast).await?;
    let genre = media.as_ref().map(|(_, g)| g.clone());
    let playback_counts = match viewer {
        Some(v) if v.id == owner => crate::integrity::trusted(&mut db, broadcast, None).await?,
        Some(v) => crate::integrity::trusted(&mut db, broadcast, Some(&v.id)).await?,
        None => false,
    };
    let enemy = match (&faction, &genre) {
        (Some(f), Some(g)) => sqlx::query_scalar("SELECT holder FROM faction_territories t JOIN faction_seasons s ON s.id=t.season_id WHERE t.genre=$1 AND s.finished_at IS NULL AND s.starts_at<=$2 AND s.ends_at>$2")
            .bind(g).bind(at).fetch_optional(&mut *db).await?.flatten().is_some_and(|h: String| h != *f),
        _ => false,
    };
    let council = match (&faction, &genre) {
        (Some(f), Some(g)) => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_targets t JOIN faction_weeks w ON w.id=t.week_id WHERE w.completed_at IS NULL AND w.starts_at<=$3 AND w.ends_at>$3 AND t.faction=$1 AND t.genre=$2)")
            .bind(f).bind(g).bind(at).fetch_one(&mut *db).await?,
        _ => false,
    };
    let event = match &genre {
        Some(g) => {
            let skirmish: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_skirmishes WHERE cancelled_at IS NULL AND kind='skirmish' AND starts_at<=$1 AND starts_at+interval '1 hour'>$1)")
                .bind(at).fetch_one(&mut *db).await?;
            let hill: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_seasons WHERE finished_at IS NULL AND starts_at<=$2 AND ends_at>$2 AND hill=$1)")
                .bind(g).bind(at).fetch_one(&mut *db).await?;
            let spot: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_spotlights WHERE day=($2 AT TIME ZONE 'UTC')::date AND genre=$1)")
                .bind(g).bind(at).fetch_one(&mut *db).await?;
            skirmish || hill || spot
        }
        None => false,
    };
    Ok(explain(ActivityFacts {
        signed_in: viewer.is_some(),
        eligible: account.as_ref().is_some_and(|a| a.eligible),
        verified: account.as_ref().is_some_and(|a| a.email_verified),
        member: faction.is_some(),
        season,
        owner: viewer.is_some_and(|v| v.id == owner),
        media_ready: media.is_some(),
        playback_counts,
        enemy_turf: enemy,
        council_target: council,
        event,
    }))
}

pub struct ActivityFacts<'a> {
    pub signed_in: bool,
    pub eligible: bool,
    pub verified: bool,
    pub member: bool,
    pub season: &'a str,
    pub owner: bool,
    pub media_ready: bool,
    pub playback_counts: bool,
    pub enemy_turf: bool,
    pub council_target: bool,
    pub event: bool,
}

const RULES: [&str; 6] = [
    "Verified members earn influence from streaming, watching, and chat while a season is running.",
    "Watching or chatting in your own channel never counts, and your own preview never supplies the viewer a stream needs.",
    "Streaming needs a confirmed live signal and at least one other viewer whose playback counts. Chat counts only while that viewer's playback counts.",
    "Watching another faction still counts for your own faction. It does not change who is featured.",
    "Daily limits apply. This page does not show points earned or how much of a limit is left.",
    "Payments do not award war influence.",
];

/// `counts` is true only when this activity can earn right now. The words never name an amount.
pub fn explain(f: ActivityFacts<'_>) -> Value {
    use serde_json::json;
    let (code, title, mut detail, counts) = if !f.signed_in {
        ("signed_out", "Sign in for this to count", "Watching works either way. War influence needs a signed-in verified member of a faction.".to_string(), false)
    } else if !f.eligible {
        ("ineligible", "This account cannot earn influence", "This account is not in good standing, so streaming, watching, and chat here do not count for the war.".to_string(), false)
    } else if !f.verified {
        ("unverified", "Confirm your email first", "War influence from streaming, watching, and chat starts after the email on this account is confirmed.".to_string(), false)
    } else if !f.member {
        ("no_faction", "Pick a side first", "This activity does not earn influence until the account joins Myria, Aetheron, or Glint. Joining is not ranked by contribution.".to_string(), false)
    } else if f.season == "between" {
        ("between", "The season is on break", "Influence is not earned during the break. The next season starts from home turf with influence at zero. Switching factions is open during the break.".to_string(), false)
    } else if f.season == "due" {
        ("checkpoint", "The checkpoint is catching up", "This week's results are not applied yet, so influence is paused. Membership is unchanged. Earning resumes when the next week is open.".to_string(), false)
    } else if f.season != "open" {
        (
            "season_off",
            "No season is running",
            "This activity does not earn influence until a season starts.".to_string(),
            false,
        )
    } else if !f.media_ready {
        ("no_signal", "No confirmed live signal", "This broadcast needs a confirmed live signal in a category before streaming, watching, or chat can earn influence. Your own preview never counts as a viewer.".to_string(), false)
    } else if f.owner && !f.playback_counts {
        ("no_audience", "Waiting for a viewer whose playback counts", "Streaming influence needs at least one other viewer whose playback counts. Your own preview never counts. Chat in your own channel does not count.".to_string(), false)
    } else if !f.owner && !f.playback_counts {
        ("waiting", "Your playback is not counting yet", "Watching and chat earn influence only while your playback counts. This page does not show points or how much of a limit is left.".to_string(), false)
    } else if f.owner {
        ("ready", "Streaming can earn influence", "A qualifying viewer is present and the live signal is confirmed, so streaming influence can count for your faction. Chat in your own channel still does not. Daily limits apply. This page does not show points earned or how much of a limit is left.".to_string(), true)
    } else {
        ("ready", "This watch can earn influence", "Your playback counts, so watching and chat can earn influence for your faction. Daily limits apply. This page does not show points earned or how much of a limit is left.".to_string(), true)
    };
    if matches!(code, "ready" | "waiting" | "no_audience") {
        if f.enemy_turf {
            detail.push_str(" This category is held by another faction, so streaming and watching time count for more.");
        }
        if f.council_target {
            detail.push_str(" Your faction chose this category as its target, so influence here counts for more.");
        }
        if f.event {
            detail.push_str(" A faction event applies here, so influence counts for more, and the usual limits still apply.");
        }
    }
    json!({"code": code, "title": title, "detail": detail, "counts": counts, "rules": RULES})
}

#[cfg(test)]
mod activity_tests {
    use super::*;

    fn facts() -> ActivityFacts<'static> {
        ActivityFacts {
            signed_in: true,
            eligible: true,
            verified: true,
            member: true,
            season: "open",
            owner: false,
            media_ready: true,
            playback_counts: true,
            enemy_turf: false,
            council_target: false,
            event: false,
        }
    }

    #[test]
    fn status_follows_the_gates_and_hides_allowances() {
        let ready = explain(facts());
        assert_eq!(ready["code"], "ready");
        assert_eq!(ready["counts"], true);
        let mut waiting = facts();
        waiting.playback_counts = false;
        let waiting = explain(waiting);
        assert_eq!(waiting["code"], "waiting");
        assert_eq!(waiting["counts"], false);
        let mut own = facts();
        own.owner = true;
        own.playback_counts = false;
        assert_eq!(explain(own)["code"], "no_audience");
        let mut guest = facts();
        guest.signed_in = false;
        assert_eq!(explain(guest)["counts"], false);
        let mut unverified = facts();
        unverified.verified = false;
        assert_eq!(explain(unverified)["code"], "unverified");
        let text = explain(facts()).to_string();
        assert!(text.contains("Payments do not award war influence."));
        assert!(!text.contains("remaining"));
        assert!(!text.to_lowercase().contains("threshold"));
        assert!(!text.to_lowercase().contains("risk"));
        let mut bonus = facts();
        bonus.enemy_turf = true;
        bonus.council_target = true;
        bonus.event = true;
        let bonus = explain(bonus)["detail"].as_str().unwrap().to_string();
        assert!(bonus.contains("another faction"));
        assert!(bonus.contains("target"));
        assert!(bonus.contains("event"));
        assert!(!bonus.chars().any(|c| c.is_ascii_digit()));
    }
}
