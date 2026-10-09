//! Progression, part 1 (docs/PROGRESSION.md): account XP and levels, and the Scout bonus. XP comes
//! only from verified viewers' Counted or Trusted playback, chat, and Scout bonuses, each capped per
//! UTC day. Purchases never give XP, and levels never touch MAGNet, rotation or influence.
use crate::{
    App,
    profiles::{self, Res},
};
use axum::{Json, Router, extract::State, routing::get};
use axum_extra::extract::cookie::CookieJar;
use serde_json::{Value, json};
use sqlx::PgConnection;

pub const MAX_LEVEL: i64 = 100;
/// Daily caps (Proposed): watching 600 (an hour at the base rate), chat 100, Scout 150 (3 bonuses).
const WATCH_CAP: i32 = 600;
const CHAT_CAP: i32 = 100;
const SCOUT_XP: i32 = 50;
const SCOUT_CAP: i32 = 150;

/// Total XP needed to reach level `l` (legacy's curve): floor(100 × (l−1)^1.5).
pub fn xp_for(level: i64) -> i64 {
    (100.0 * ((level - 1) as f64).powf(1.5)).floor() as i64
}
/// The level for a total, from 1 to 100.
pub fn level(xp: i64) -> i64 {
    (1..=MAX_LEVEL)
        .take_while(|l| xp_for(*l) <= xp)
        .last()
        .unwrap_or(1)
}
pub async fn total(db: &mut PgConnection, user: &str) -> Res<i64> {
    Ok(
        sqlx::query_scalar("SELECT coalesce(sum(xp),0)::bigint FROM xp_days WHERE user_id=$1")
            .bind(user)
            .fetch_one(db)
            .await?,
    )
}
pub fn summary(xp: i64) -> Value {
    let level = level(xp);
    json!({"xp": xp, "level": level, "level_xp": xp_for(level), "next_xp": (level < MAX_LEVEL).then(|| xp_for(level + 1))})
}

/// Each minute (with Engagement Valor's watch points): 10 XP a minute of Counted or Trusted
/// playback, 15 on a stream of the viewer's own faction; then Scout bonuses.
pub async fn award_watch(app: &App) -> Res<()> {
    sqlx::query("INSERT INTO xp_days(user_id,day,source,xp)
        SELECT v.id,current_date,'watch',v.xp FROM (
          SELECT u.id,max(CASE WHEN vf.faction IS NOT NULL AND vf.faction=ofm.faction THEN 15 ELSE 10 END) AS xp
          FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id JOIN users u ON l.viewer_key='u:'||u.id
            LEFT JOIN faction_members vf ON vf.user_id=u.id LEFT JOIN faction_members ofm ON ofm.user_id=b.owner_id
          WHERE l.expires_at>now() AND l.level IN ('counted','trusted') AND b.state IN ('LIVE','RECONNECTING')
            AND u.email_verified AND u.deleted_at IS NULL AND u.id<>b.owner_id
          GROUP BY u.id) v
        ON CONFLICT(user_id,day,source) DO UPDATE SET xp=least($1,xp_days.xp+EXCLUDED.xp),updated_at=now()
        WHERE xp_days.updated_at<=now()-interval '55 seconds'")
        .bind(WATCH_CAP)
        .execute(&app.db)
        .await?;
    // Scout: 10+ minutes of real playback that began in the broadcast's first 15 minutes, on a
    // channel with fewer than 10 broadcasts; once per channel per week, up to 3 a day.
    sqlx::query("WITH eligible AS (
          SELECT DISTINCT ON (u.id,b.owner_id) u.id AS user_id,b.owner_id,b.id AS broadcast_id
          FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id JOIN users u ON l.viewer_key='u:'||u.id
          WHERE l.expires_at>now() AND l.level IN ('counted','trusted') AND b.state IN ('LIVE','RECONNECTING')
            AND u.email_verified AND u.deleted_at IS NULL AND u.id<>b.owner_id
            AND l.created_at<=b.started_at+interval '15 minutes' AND l.created_at<=now()-interval '10 minutes'
            AND (SELECT count(*) FROM broadcasts p WHERE p.owner_id=b.owner_id)<10
            AND NOT EXISTS(SELECT 1 FROM scout_awards s WHERE s.user_id=u.id AND s.channel_id=b.owner_id AND s.at>now()-interval '7 days')
            AND (SELECT count(*) FROM scout_awards s WHERE s.user_id=u.id AND s.at>=current_date)<3),
        awarded AS (INSERT INTO scout_awards(user_id,channel_id,broadcast_id) SELECT user_id,owner_id,broadcast_id FROM eligible ON CONFLICT DO NOTHING RETURNING user_id)
        INSERT INTO xp_days(user_id,day,source,xp) SELECT user_id,current_date,'scout',$1*count(*)::int FROM awarded GROUP BY user_id
        ON CONFLICT(user_id,day,source) DO UPDATE SET xp=least($2,xp_days.xp+EXCLUDED.xp),updated_at=now()")
        .bind(SCOUT_XP)
        .bind(SCOUT_CAP)
        .execute(&app.db)
        .await?;
    Ok(())
}
/// 2 XP for a chat message, at most once a minute (inside the message's transaction).
pub(crate) async fn chatted(tx: &mut PgConnection, user: &str) -> Res<()> {
    sqlx::query(
        "INSERT INTO xp_days(user_id,day,source,xp) VALUES($1,current_date,'chat',2)
        ON CONFLICT(user_id,day,source) DO UPDATE SET xp=least($2,xp_days.xp+2),updated_at=now()
        WHERE xp_days.updated_at<=now()-interval '60 seconds'",
    )
    .bind(user)
    .bind(CHAT_CAP)
    .execute(tx)
    .await?;
    Ok(())
}

/// GET /api/me/progression: XP, level and the next level's threshold (the player card).
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let xp = total(&mut *app.db.acquire().await?, &user.id).await?;
    Ok(Json(summary(xp)))
}
pub fn routes() -> Router<App> {
    Router::new().route("/api/me/progression", get(mine))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels_follow_the_legacy_curve() {
        assert_eq!(
            (xp_for(1), xp_for(2), xp_for(10), xp_for(50)),
            (0, 100, 2700, 34300)
        );
        assert_eq!(
            (
                level(0),
                level(99),
                level(100),
                level(2700),
                level(10_000_000)
            ),
            (1, 1, 2, 10, 100)
        );
    }
}
