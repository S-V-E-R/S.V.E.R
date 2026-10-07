use super::{FACTIONS, Tuning};
use crate::{
    App,
    profiles::{self, Res},
};
use chrono::{DateTime, Datelike, Duration, Months, Utc};
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};

#[derive(FromRow, Clone)]
pub(super) struct Season {
    pub id: i64,
    pub number: i32,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub next_starts_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub winners: Vec<String>,
}
#[derive(FromRow, Clone)]
pub(super) struct Week {
    pub id: i64,
    pub season_id: i64,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub(crate) async fn lock(db: &mut PgConnection) -> Res<()> {
    // ponytail: one transaction lock serializes war writes; shard awards by member if measured
    // throughput requires it, retaining an exclusive checkpoint barrier for exact results.
    sqlx::query("SELECT pg_advisory_xact_lock(1398162770)")
        .execute(db)
        .await?;
    Ok(())
}
pub(super) async fn now(db: &mut PgConnection) -> Res<DateTime<Utc>> {
    Ok(sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(db)
        .await?)
}
pub(super) async fn current(db: &mut PgConnection, at: DateTime<Utc>) -> Res<Option<Week>> {
    Ok(sqlx::query_as(
        "SELECT * FROM faction_weeks WHERE starts_at<=$1 AND ends_at>$1 AND completed_at IS NULL",
    )
    .bind(at)
    .fetch_optional(db)
    .await?)
}
pub(super) async fn latest(db: &mut PgConnection) -> Res<Option<Season>> {
    Ok(
        sqlx::query_as("SELECT * FROM faction_seasons ORDER BY number DESC LIMIT 1")
            .fetch_optional(db)
            .await?,
    )
}
fn next_monday(at: DateTime<Utc>) -> DateTime<Utc> {
    let date = at.date_naive() + Duration::days(7 - i64::from(at.weekday().num_days_from_monday()));
    date.and_hms_opt(0, 0, 0).unwrap().and_utc()
}
async fn create_week(db: &mut PgConnection, season: &Season, start: DateTime<Utc>) -> Res<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO faction_weeks(season_id,starts_at,ends_at) VALUES($1,$2,$3) RETURNING id",
    )
    .bind(season.id)
    .bind(start)
    .bind(next_monday(start).min(season.ends_at))
    .fetch_one(db)
    .await?)
}
async fn create_season(db: &mut PgConnection, number: i32, start: DateTime<Utc>) -> Res<Season> {
    let end = start
        .checked_add_months(Months::new(3))
        .ok_or_else(profiles::Fail::internal)?;
    let season: Season = sqlx::query_as("INSERT INTO faction_seasons(number,starts_at,ends_at,next_starts_at) VALUES($1,$2,$3,$4) RETURNING *")
        .bind(number).bind(start).bind(end).bind(end+Duration::days(7)).fetch_one(&mut *db).await?;
    sqlx::query("INSERT INTO faction_territories(season_id,genre,holder) SELECT $1,id,home FROM faction_genres")
        .bind(season.id).execute(&mut *db).await?;
    create_week(db, &season, start).await?;
    Ok(season)
}

/// Must run under lock, in the same transaction as the caller's contribution/membership write.
/// Persisted completion and successor rows make recovery replay-safe, including long outages.
pub(super) async fn advance(db: &mut PgConnection, tuning: &Tuning, at: DateTime<Utc>) -> Res<()> {
    let Some(start) = tuning.starts_at else {
        return Ok(());
    };
    let mut season = match latest(db).await? {
        Some(s) => s,
        None => create_season(db, 1, start).await?,
    };
    loop {
        if at < season.starts_at {
            break;
        }
        let due: Option<Week> = sqlx::query_as("SELECT * FROM faction_weeks WHERE season_id=$1 AND completed_at IS NULL AND ends_at<=$2 ORDER BY ends_at LIMIT 1")
            .bind(season.id).bind(at).fetch_optional(&mut *db).await?;
        if let Some(week) = due {
            checkpoint(db, tuning, &season, &week, at).await?;
            if week.ends_at < season.ends_at {
                let next = create_week(db, &season, week.ends_at).await?;
                elect(db, week.id, next).await?;
            } else {
                finish(db, &season, at).await?;
                season.finished_at = Some(at);
            }
            continue;
        }
        if season.finished_at.is_some() && at >= season.next_starts_at {
            season = create_season(db, season.number + 1, season.next_starts_at).await?;
            continue;
        }
        break;
    }
    Ok(())
}

pub(super) async fn standings(
    db: &mut PgConnection,
    tuning: &Tuning,
    week: &Week,
    at: DateTime<Utc>,
) -> Res<Value> {
    let active: Vec<(String,i64)> = sqlx::query_as("SELECT faction,count(DISTINCT user_id) FROM faction_influence WHERE happened_at >= $1-interval '14 days' AND happened_at<$1 GROUP BY faction")
        .bind(at).fetch_all(&mut *db).await?;
    let totals: Vec<(String,String,i64)> = sqlx::query_as("SELECT genre,faction,sum(points)::bigint FROM faction_influence WHERE week_id=$1 GROUP BY genre,faction")
        .bind(week.id).fetch_all(&mut *db).await?;
    type GenreRow = (
        String,
        String,
        Option<String>,
        Option<String>,
        i32,
        Vec<String>,
        Option<i32>,
        Option<i32>,
        bool,
    );
    let genres: Vec<GenreRow> = sqlx::query_as("SELECT g.id,g.name,g.home,t.holder,g.position,g.neighbors,g.map_q,g.map_r,g.capital FROM faction_genres g JOIN faction_territories t ON t.genre=g.id AND t.season_id=$1 ORDER BY g.position,g.id")
        .bind(week.season_id).fetch_all(&mut *db).await?;
    let members: Vec<Value> = FACTIONS.iter().map(|f| json!({"faction":f,"active_members":active.iter().find(|(a,_)| a==f).map_or(0,|(_,n)|*n)})).collect();
    let rows: Vec<Value> = genres.into_iter().map(|(id,name,home,holder,position,neighbors,map_q,map_r,capital)| {
        let scores: Vec<Value> = FACTIONS.iter().map(|f| {
            let raw=totals.iter().find(|(g,a,_)| g==&id && a==f).map_or(0,|(_,_,n)| *n);
            let divisor=active.iter().find(|(a,_)| a==f).map_or(0,|(_,n)|*n).max(tuning.minimum_divisor);
            json!({"faction":f,"influence":raw,"score":raw as f64/divisor as f64})
        }).collect();
        json!({"id":id,"name":name,"home":home,"holder":holder,"position":position,"neighbors":neighbors,"map":map_q.zip(map_r).map(|(q,r)| json!({"q":q,"r":r})),"capital":capital,"scores":scores})
    }).collect();
    Ok(json!({"genres":rows,"factions":members}))
}

/// Integer cross multiplication avoids floating-point territory flips at the margin.
fn beats(a: (i64, i64), b: (i64, i64), margin: i64) -> bool {
    i128::from(a.0) * i128::from(b.1) * 10_000
        > i128::from(b.0) * i128::from(a.1) * i128::from(10_000 + margin)
}
async fn checkpoint(
    db: &mut PgConnection,
    tuning: &Tuning,
    season: &Season,
    week: &Week,
    at: DateTime<Utc>,
) -> Res<()> {
    let active: Vec<(String,i64)> = sqlx::query_as("SELECT faction,count(DISTINCT user_id) FROM faction_influence WHERE happened_at >= $1-interval '14 days' AND happened_at<$1 GROUP BY faction")
        .bind(week.ends_at).fetch_all(&mut *db).await?;
    let totals: Vec<(String,String,i64)> = sqlx::query_as("SELECT genre,faction,sum(points)::bigint FROM faction_influence WHERE week_id=$1 GROUP BY genre,faction")
        .bind(week.id).fetch_all(&mut *db).await?;
    let territories: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT genre,holder FROM faction_territories WHERE season_id=$1 ORDER BY genre",
    )
    .bind(season.id)
    .fetch_all(&mut *db)
    .await?;
    let mut result = standings(db, tuning, week, week.ends_at).await?;
    for (genre, holder) in territories {
        let scores: Vec<(&str, (i64, i64))> = FACTIONS
            .iter()
            .map(|f| {
                (
                    *f,
                    (
                        totals
                            .iter()
                            .find(|(g, a, _)| g == &genre && a == f)
                            .map_or(0, |(_, _, n)| *n),
                        active
                            .iter()
                            .find(|(a, _)| a == f)
                            .map_or(0, |(_, n)| *n)
                            .max(tuning.minimum_divisor),
                    ),
                )
            })
            .collect();
        let winner = scores
            .iter()
            .find(|(f, score)| {
                if holder.as_deref() == Some(*f) || score.0 == 0 {
                    return false;
                }
                if holder.is_none() && score.0 < tuning.neutral_minimum {
                    return false;
                }
                scores.iter().all(|(other, other_score)| {
                    other == f
                        || beats(
                            *score,
                            *other_score,
                            if holder.is_none() || holder.as_deref() == Some(*other) {
                                tuning.flip_margin_bps
                            } else {
                                0
                            },
                        )
                })
            })
            .map(|(f, _)| *f)
            .or(holder.as_deref());
        sqlx::query("UPDATE faction_territories SET holder=$3 WHERE season_id=$1 AND genre=$2")
            .bind(season.id)
            .bind(&genre)
            .bind(winner)
            .execute(&mut *db)
            .await?;
        for item in result["genres"].as_array_mut().unwrap() {
            if item["id"] == genre {
                item["previous_holder"] = json!(holder);
                item["holder"] = json!(winner);
            }
        }
    }
    sqlx::query("UPDATE faction_weeks SET completed_at=$2,result=$3,last_error=NULL,attempts=attempts+1 WHERE id=$1 AND completed_at IS NULL")
        .bind(week.id).bind(at).bind(result).execute(db).await?;
    Ok(())
}
async fn elect(db: &mut PgConnection, previous: i64, next: i64) -> Res<()> {
    // Votes are private; no voter identities leave this module. A switch invalidates a stale vote.
    sqlx::query("INSERT INTO faction_targets(week_id,faction,genre) SELECT $2,faction,genre FROM (SELECT v.faction,v.genre,row_number() OVER(PARTITION BY v.faction ORDER BY count(*) DESC,v.genre) AS rank FROM faction_votes v JOIN faction_members m ON m.user_id=v.user_id AND m.faction=v.faction WHERE v.week_id=$1 GROUP BY v.faction,v.genre) t WHERE rank=1")
        .bind(previous).bind(next).execute(&mut *db).await?;
    let rows: Vec<(String,String,i64)> = sqlx::query_as("SELECT v.faction,v.candidate_id,count(*) FROM faction_moderator_votes v JOIN faction_members voter ON voter.user_id=v.user_id AND voter.faction=v.faction JOIN faction_members c ON c.user_id=v.candidate_id AND c.faction=v.faction AND c.moderator_candidate WHERE v.week_id=$1 GROUP BY v.faction,v.candidate_id HAVING count(*)>=3 ORDER BY count(*) DESC,v.candidate_id")
        .bind(previous).fetch_all(&mut *db).await?;
    let mut elected = [0; 3];
    for (faction, candidate, _) in rows {
        let index = FACTIONS.iter().position(|f| *f == faction).unwrap();
        if elected[index] >= 3 {
            continue;
        }
        if !profiles::channel_user_by_id(db, &candidate)
            .await?
            .is_some_and(|u| u.eligible && u.email_verified)
        {
            continue;
        }
        sqlx::query("INSERT INTO faction_moderators(week_id,user_id,faction) VALUES($1,$2,$3)")
            .bind(next)
            .bind(candidate)
            .bind(faction)
            .execute(&mut *db)
            .await?;
        elected[index] += 1;
    }
    Ok(())
}
async fn finish(db: &mut PgConnection, season: &Season, at: DateTime<Utc>) -> Res<()> {
    let counts: Vec<(String,i64)> = sqlx::query_as("SELECT holder,count(*) FROM faction_territories WHERE season_id=$1 AND holder IS NOT NULL GROUP BY holder")
        .bind(season.id).fetch_all(&mut *db).await?;
    let history: Vec<(String,i64)> = sqlx::query_as("SELECT g->>'holder',count(*) FROM faction_weeks w CROSS JOIN LATERAL jsonb_array_elements(w.result->'genres') g WHERE w.season_id=$1 AND g->>'holder' IS NOT NULL GROUP BY 1")
        .bind(season.id).fetch_all(&mut *db).await?;
    let score = |f: &str| {
        (
            counts.iter().find(|(a, _)| a == f).map_or(0, |(_, n)| *n),
            history.iter().find(|(a, _)| a == f).map_or(0, |(_, n)| *n),
        )
    };
    let best = FACTIONS.iter().map(|f| score(f)).max().unwrap();
    // If both specified tie-breaks are equal, award joint winners; never choose by faction ID.
    let winners: Vec<String> = FACTIONS
        .iter()
        .filter(|f| score(f) == best)
        .map(|f| f.to_string())
        .collect();
    sqlx::query(
        "UPDATE faction_seasons SET finished_at=$2,winners=$3 WHERE id=$1 AND finished_at IS NULL",
    )
    .bind(season.id)
    .bind(at)
    .bind(&winners)
    .execute(&mut *db)
    .await?;
    sqlx::query("INSERT INTO faction_rewards(season_id,user_id,faction,awarded_at) SELECT $1,user_id,faction,$2 FROM faction_members WHERE faction=ANY($3) ON CONFLICT DO NOTHING")
        .bind(season.id).bind(at).bind(winners).execute(db).await?;
    Ok(())
}

pub async fn tick(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    lock(&mut tx).await?;
    let at = now(&mut tx).await?;
    let outcome = advance(&mut tx, &app.config.factions, at).await;
    match outcome {
        Ok(()) => {
            tx.commit().await?;
            Ok(())
        }
        Err(error) => {
            tx.rollback().await?;
            sqlx::query("UPDATE faction_weeks SET attempts=attempts+1,last_error='Checkpoint failed; retry is safe.' WHERE completed_at IS NULL AND ends_at<=$1")
                .bind(at).execute(&app.db).await?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_edges() {
        assert!(!beats((105, 25), (100, 25), 500));
        assert!(beats((106, 25), (100, 25), 500));
        assert!(!beats((1000, 100), (250, 25), 500));
        let monday = "2026-10-05T00:00:00Z".parse().unwrap();
        assert_eq!(
            next_monday(monday),
            "2026-10-12T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
    }
}
