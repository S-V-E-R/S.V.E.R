//! Achievements and cosmetics (docs/ACHIEVEMENTS.md): milestones earned from records S.V.E.R
//! already keeps (checked every 10 minutes, never from a client report), and the cosmetics levels
//! unlock: a chat name style, a chosen badge frame and a showcase of up to 3 achievements.
use crate::{
    App,
    profiles::{self, Fail, Res},
    progression,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::HashMap;

/// One achievement: its key, name, what earns it, its category and band, and its rule: a query
/// of (user ID, progress) pairs, earned once progress reaches `need`. Streamer bands follow the
/// creator tiers (Scout to Pathfinder); viewer bands follow account level (1, 25, 50, 100).
pub struct Achievement {
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub band: u8,
    sql: &'static str,
    need: i64,
}
impl Achievement {
    pub fn streamer(&self) -> bool {
        CATEGORIES
            .iter()
            .any(|(k, _, streamer)| *k == self.category && *streamer)
    }
}
/// Key, name, whether it's a streamer category.
pub const CATEGORIES: [(&str, &str, bool); 10] = [
    ("community", "Community", false),
    ("scouting", "Scouting", false),
    ("orders", "Orders and loyalty", false),
    ("levels", "Levels", false),
    ("support", "Support given", false),
    ("streaming", "Streaming", true),
    ("audience", "Audience", true),
    ("raids", "Raids", true),
    ("subscribers", "Subscribers", true),
    ("dedication", "Tiers and dedication", true),
];

// Viewer rules.
const FOLLOWS: &str = "SELECT follower_id,count(*) FROM follows GROUP BY 1";
const CHATTED: &str =
    "SELECT user_id,count(*) FROM engagement WHERE last_chat_at IS NOT NULL GROUP BY 1";
const WATCHED: &str = "SELECT user_id,(sum(watch_minutes)/60)::bigint FROM engagement GROUP BY 1";
const LIGHTS: &str = "SELECT from_id,count(*) FROM lights GROUP BY 1";
const SCOUTED: &str = "SELECT user_id,count(*) FROM scout_awards GROUP BY 1";
const SCOUT_CHANNELS: &str =
    "SELECT user_id,count(DISTINCT channel_id) FROM scout_awards GROUP BY 1";
const SCOUT_DAYS: &str = "SELECT user_id,count(DISTINCT at::date) FROM scout_awards GROUP BY 1";
const ORDERS: &str =
    "SELECT user_id,count(*) FROM daily_orders WHERE completed_at IS NOT NULL GROUP BY 1";
const WEEK_ORDERS: &str = "SELECT user_id,max(orders)::bigint FROM order_milestones GROUP BY 1";
const LOYALTY: &str = "SELECT user_id,max(earned)::bigint FROM engagement GROUP BY 1";
const XP: &str = "SELECT user_id,sum(xp)::bigint FROM xp_days GROUP BY 1";
const TRIBUTES: &str = "SELECT detail->>'from',count(*) FROM ledger_transactions WHERE kind='tribute' AND detail->>'from' IN (SELECT id FROM users) GROUP BY 1";
const SUB_MONTHS: &str = "SELECT user_id,max(months)::bigint FROM channel_subs GROUP BY 1";
const SUBSCRIBED: &str = "SELECT user_id,count(*) FROM channel_subs GROUP BY 1";
const WALLS: &str = "SELECT author_id,count(*) FROM wall_posts WHERE status='APPROVED' AND deleted_at IS NULL AND author_id<>wall_owner_id GROUP BY 1";
const COUNCIL: &str = "SELECT user_id,count(*) FROM war_council GROUP BY 1";
// Streamer rules.
const LIVE: &str =
    "SELECT owner_id,count(*) FROM broadcasts WHERE confirmed_live_at IS NOT NULL GROUP BY 1";
const HOURS: &str = "SELECT owner_id,floor(sum(extract(epoch FROM coalesce(ended_at,now())-started_at))/3600)::bigint FROM broadcasts WHERE confirmed_live_at IS NOT NULL GROUP BY 1";
const DAYS: &str = "SELECT owner_id,count(DISTINCT (started_at AT TIME ZONE 'America/New_York')::date) FROM broadcasts WHERE confirmed_live_at IS NOT NULL GROUP BY 1";
/// Verified followers (docs/PROFILES.md), the count creator tiers use.
const FOLLOWERS: &str =
    "SELECT following_id,count(*) FROM follows WHERE counted_at IS NOT NULL GROUP BY 1";
const UNIQUE: &str = "SELECT owner_id,count(*) FROM creator_viewers GROUP BY 1";
const PEAK: &str = "SELECT b.owner_id,max(s.trusted)::bigint FROM integrity_snapshots s JOIN broadcasts b ON b.id=s.broadcast_id GROUP BY 1";
const RAIDS_LED: &str = "SELECT raider_id,count(*) FROM raids WHERE status='moved' GROUP BY 1";
const RAIDS_IN: &str = "SELECT target_id,count(*) FROM raids WHERE status='moved' GROUP BY 1";
const ARRIVALS: &str =
    "SELECT target_id,coalesce(sum(arrivals),0)::bigint FROM raids WHERE status='moved' GROUP BY 1";
const SUBS_EVER: &str = "SELECT channel_id,count(*) FROM channel_subs GROUP BY 1";
const SUBS_NOW: &str =
    "SELECT channel_id,count(*) FROM channel_subs WHERE paid_through>now() GROUP BY 1";
const SUB_MONTHS_IN: &str = "SELECT channel_id,sum(months)::bigint FROM channel_subs GROUP BY 1";
const TIER: &str = "SELECT user_id,tier::bigint FROM creator_tiers";
const WEEKS: &str = "SELECT owner_id,count(DISTINCT date_trunc('week',started_at AT TIME ZONE 'America/New_York')) FROM broadcasts WHERE confirmed_live_at IS NOT NULL GROUP BY 1";
const MONTHS: &str = "SELECT owner_id,count(DISTINCT date_trunc('month',started_at AT TIME ZONE 'America/New_York')) FROM broadcasts WHERE confirmed_live_at IS NOT NULL GROUP BY 1";

/// Every achievement (docs/ACHIEVEMENTS.md): at least 3 per category per band. Numbers are
/// Proposed. Keys are permanent: earned rows keep them.
pub fn catalog() -> Vec<Achievement> {
    let level = progression::xp_for;
    let loyalty = |r: usize| progression::LOYALTY[r].1;
    let week = |m: usize| i64::from(progression::MILESTONES[m].0);
    #[rustfmt::skip]
    let list: [(&str, &str, &str, &str, u8, &str, i64); 120] = [
        // Community
        ("first_follow", "First Follow", "Followed a channel", "community", 0, FOLLOWS, 1),
        ("first_words", "First Words", "Chatted in a channel", "community", 0, CHATTED, 1),
        ("first_hour", "Settling In", "Watched 1 hour live", "community", 0, WATCHED, 1),
        ("follows_25", "Wide Net", "Followed 25 channels", "community", 1, FOLLOWS, 25),
        ("voice", "Voice of the Crowd", "Chatted in 10 different channels", "community", 1, CHATTED, 10),
        ("watched_25", "Regular Seat", "Watched 25 hours live", "community", 1, WATCHED, 25),
        ("chatted_50", "Known Voice", "Chatted in 50 different channels", "community", 2, CHATTED, 50),
        ("watched_100", "Hundred-Hour Witness", "Watched 100 hours live", "community", 2, WATCHED, 100),
        ("lights_50", "Lamplighter", "Gave 50 Lights", "community", 2, LIGHTS, 50),
        ("chatted_150", "Voice of the Realm", "Chatted in 150 different channels", "community", 3, CHATTED, 150),
        ("watched_500", "Ever-Watchful", "Watched 500 hours live", "community", 3, WATCHED, 500),
        ("lights_250", "Beacon Keeper", "Gave 250 Lights", "community", 3, LIGHTS, 250),
        // Scouting
        ("scout", "Scout", "Scouted a new stream", "scouting", 0, SCOUTED, 1),
        ("scout_days_3", "Wanderer", "Scouted on 3 different days", "scouting", 0, SCOUT_DAYS, 3),
        ("scouted_5", "Trail Finder", "Scouted 5 new streams", "scouting", 0, SCOUTED, 5),
        ("pathfinder", "Pathfinder", "Scouted 10 new streams", "scouting", 1, SCOUTED, 10),
        ("scout_channels_10", "Many Roads", "Scouted 10 different channels", "scouting", 1, SCOUT_CHANNELS, 10),
        ("scout_days_10", "Restless", "Scouted on 10 different days", "scouting", 1, SCOUT_DAYS, 10),
        ("scouted_50", "Far Seer", "Scouted 50 new streams", "scouting", 2, SCOUTED, 50),
        ("scout_channels_25", "Cartographer", "Scouted 25 different channels", "scouting", 2, SCOUT_CHANNELS, 25),
        ("scout_days_30", "Long Patrol", "Scouted on 30 different days", "scouting", 2, SCOUT_DAYS, 30),
        ("scouted_200", "Herald of the New", "Scouted 200 new streams", "scouting", 3, SCOUTED, 200),
        ("scout_channels_100", "Mapmaker", "Scouted 100 different channels", "scouting", 3, SCOUT_CHANNELS, 100),
        ("scout_days_100", "Eternal Scout", "Scouted on 100 different days", "scouting", 3, SCOUT_DAYS, 100),
        // Orders and loyalty
        ("first_order", "Under Orders", "Finished a daily order", "orders", 0, ORDERS, 1),
        ("week_orders_7", "Steady Hand", "7 daily orders in one week", "orders", 0, WEEK_ORDERS, week(0)),
        ("regular", "Regular", "Regular loyalty rank in a channel", "orders", 0, LOYALTY, loyalty(1)),
        ("orders_50", "Dutiful", "Finished 50 daily orders", "orders", 1, ORDERS, 50),
        ("order_keeper", "Order Keeper", "Every daily order in a week (21)", "orders", 1, WEEK_ORDERS, week(2)),
        ("devoted", "Devoted", "Devoted loyalty rank in a channel", "orders", 1, LOYALTY, loyalty(2)),
        ("orders_250", "Sworn", "Finished 250 daily orders", "orders", 2, ORDERS, 250),
        ("veteran", "Veteran", "Veteran loyalty rank in a channel", "orders", 2, LOYALTY, loyalty(3)),
        ("orders_500", "Unbroken Oath", "Finished 500 daily orders", "orders", 2, ORDERS, 500),
        ("orders_1000", "Thousand Orders", "Finished 1,000 daily orders", "orders", 3, ORDERS, 1000),
        ("legend", "Legend", "Legend loyalty rank in a channel", "orders", 3, LOYALTY, loyalty(4)),
        ("orders_2000", "Order Eternal", "Finished 2,000 daily orders", "orders", 3, ORDERS, 2000),
        // Levels
        ("level_5", "Level 5", "Reached level 5", "levels", 0, XP, level(5)),
        ("level_10", "Level 10", "Reached level 10", "levels", 0, XP, level(10)),
        ("level_20", "Level 20", "Reached level 20", "levels", 0, XP, level(20)),
        ("level_25", "Level 25", "Reached level 25", "levels", 1, XP, level(25)),
        ("level_30", "Level 30", "Reached level 30", "levels", 1, XP, level(30)),
        ("level_40", "Level 40", "Reached level 40", "levels", 1, XP, level(40)),
        ("level_50", "Level 50", "Reached level 50", "levels", 2, XP, level(50)),
        ("level_65", "Level 65", "Reached level 65", "levels", 2, XP, level(65)),
        ("level_80", "Level 80", "Reached level 80", "levels", 2, XP, level(80)),
        ("level_100", "Level 100", "Reached level 100", "levels", 3, XP, level(100)),
        ("xp_150k", "Paragon", "Earned 150,000 XP", "levels", 3, XP, 150_000),
        ("xp_250k", "Ascendant", "Earned 250,000 XP", "levels", 3, XP, 250_000),
        // Support given
        ("first_tribute", "Patron", "Paid a first tribute", "support", 0, TRIBUTES, 1),
        ("first_sub", "Backer", "Subscribed to a channel", "support", 0, SUBSCRIBED, 1),
        ("signatory", "Signatory", "Signed someone's Wall", "support", 0, WALLS, 1),
        ("tributes_10", "Generous", "Paid 10 tributes", "support", 1, TRIBUTES, 10),
        ("faithful", "Faithful", "Subscribed to a channel for 6 months", "support", 1, SUB_MONTHS, 6),
        ("council_of_war", "Council of War", "Filled all 8 War Council seats", "support", 1, COUNCIL, 8),
        ("tributes_50", "Benefactor", "Paid 50 tributes", "support", 2, TRIBUTES, 50),
        ("sub_months_12", "Year of Faith", "Subscribed to a channel for 12 months", "support", 2, SUB_MONTHS, 12),
        ("subscribed_5", "Pillar", "Subscribed to 5 channels", "support", 2, SUBSCRIBED, 5),
        ("tributes_200", "Treasure of the Crowd", "Paid 200 tributes", "support", 3, TRIBUTES, 200),
        ("sub_months_24", "Oathbound", "Subscribed to a channel for 24 months", "support", 3, SUB_MONTHS, 24),
        ("walls_25", "Chronicler", "Signed 25 Walls", "support", 3, WALLS, 25),
        // Streaming
        ("first_light", "First Light", "Went live for the first time", "streaming", 0, LIVE, 1),
        ("streams_5", "Kindling", "Went live 5 times", "streaming", 0, LIVE, 5),
        ("hours_10", "Ten Hours Lit", "Streamed 10 hours", "streaming", 0, HOURS, 10),
        ("steady_flame", "Steady Flame", "Streamed on 10 different days", "streaming", 1, DAYS, 10),
        ("streams_25", "Hearth", "Went live 25 times", "streaming", 1, LIVE, 25),
        ("hours_50", "Fifty Hours Lit", "Streamed 50 hours", "streaming", 1, HOURS, 50),
        ("days_40", "Keeper of the Flame", "Streamed on 40 different days", "streaming", 2, DAYS, 40),
        ("streams_100", "Bonfire", "Went live 100 times", "streaming", 2, LIVE, 100),
        ("hours_100", "Hundred Hours Lit", "Streamed 100 hours", "streaming", 2, HOURS, 100),
        ("days_120", "Undying Flame", "Streamed on 120 different days", "streaming", 3, DAYS, 120),
        ("streams_365", "A Year of Fire", "Went live 365 times", "streaming", 3, LIVE, 365),
        ("hours_500", "Five Hundred Hours Lit", "Streamed 500 hours", "streaming", 3, HOURS, 500),
        // Audience
        ("followers_10", "First Ten", "Reached 10 verified followers", "audience", 0, FOLLOWERS, 10),
        ("unique_25", "Faces in the Crowd", "25 unique Trusted viewers", "audience", 0, UNIQUE, 25),
        ("peak_5", "Full Table", "5 Trusted viewers at once", "audience", 0, PEAK, 5),
        ("followers_100", "Gathering", "Reached 100 verified followers", "audience", 1, FOLLOWERS, 100),
        ("audience_100", "A Hundred Eyes", "100 unique Trusted viewers", "audience", 1, UNIQUE, 100),
        ("peak_20", "Packed Hall", "20 Trusted viewers at once", "audience", 1, PEAK, 20),
        ("followers_1000", "Host of a Thousand", "Reached 1,000 verified followers", "audience", 2, FOLLOWERS, 1000),
        ("unique_1000", "A Thousand Eyes", "1,000 unique Trusted viewers", "audience", 2, UNIQUE, 1000),
        ("peak_60", "Great Hall", "60 Trusted viewers at once", "audience", 2, PEAK, 60),
        ("followers_2500", "Banner Lord", "Reached 2,500 verified followers", "audience", 3, FOLLOWERS, 2500),
        ("unique_10000", "Seen by the Realm", "10,000 unique Trusted viewers", "audience", 3, UNIQUE, 10_000),
        ("peak_200", "Arena", "200 Trusted viewers at once", "audience", 3, PEAK, 200),
        // Raids
        ("raider", "Raider", "Led a raid", "raids", 0, RAIDS_LED, 1),
        ("raided", "Welcomed", "A raid arrived in your channel", "raids", 0, RAIDS_IN, 1),
        ("arrivals_10", "Open Gates", "10 viewers arrived by raids", "raids", 0, ARRIVALS, 10),
        ("raids_led_10", "Warband", "Led 10 raids", "raids", 1, RAIDS_LED, 10),
        ("raids_in_10", "Crossroads", "10 raids arrived in your channel", "raids", 1, RAIDS_IN, 10),
        ("arrivals_100", "Hundred Arrivals", "100 viewers arrived by raids", "raids", 1, ARRIVALS, 100),
        ("raids_led_50", "Warlord", "Led 50 raids", "raids", 2, RAIDS_LED, 50),
        ("raids_in_50", "Meeting Ground", "50 raids arrived in your channel", "raids", 2, RAIDS_IN, 50),
        ("arrivals_500", "Muster", "500 viewers arrived by raids", "raids", 2, ARRIVALS, 500),
        ("raids_led_150", "Raid Marshal", "Led 150 raids", "raids", 3, RAIDS_LED, 150),
        ("raids_in_150", "Capital of Raids", "150 raids arrived in your channel", "raids", 3, RAIDS_IN, 150),
        ("arrivals_2500", "Great Muster", "2,500 viewers arrived by raids", "raids", 3, ARRIVALS, 2500),
        // Subscribers
        ("supported", "Supported", "Got a first subscriber", "subscribers", 0, SUBS_EVER, 1),
        ("subs_now_3", "Small Circle", "3 active subscribers at once", "subscribers", 0, SUBS_NOW, 3),
        ("sub_months_in_10", "Ten Months Backed", "10 subscriber months in total", "subscribers", 0, SUB_MONTHS_IN, 10),
        ("subs_ever_25", "Growing Circle", "25 subscribers over time", "subscribers", 1, SUBS_EVER, 25),
        ("subs_10", "Ten Strong", "10 active subscribers at once", "subscribers", 1, SUBS_NOW, 10),
        ("sub_months_in_100", "Hundred Months Backed", "100 subscriber months in total", "subscribers", 1, SUB_MONTHS_IN, 100),
        ("subs_ever_100", "Household", "100 subscribers over time", "subscribers", 2, SUBS_EVER, 100),
        ("subs_now_40", "Forty Strong", "40 active subscribers at once", "subscribers", 2, SUBS_NOW, 40),
        ("sub_months_in_500", "Five Hundred Months Backed", "500 subscriber months in total", "subscribers", 2, SUB_MONTHS_IN, 500),
        ("subs_ever_500", "Great House", "500 subscribers over time", "subscribers", 3, SUBS_EVER, 500),
        ("subs_now_100", "Hundred Strong", "100 active subscribers at once", "subscribers", 3, SUBS_NOW, 100),
        ("sub_months_in_2500", "Dynasty", "2,500 subscriber months in total", "subscribers", 3, SUB_MONTHS_IN, 2500),
        // Tiers and dedication
        ("weeks_2", "Second Week", "Streamed in 2 different weeks", "dedication", 0, WEEKS, 2),
        ("weeks_4", "Month of Weeks", "Streamed in 4 different weeks", "dedication", 0, WEEKS, 4),
        ("months_2", "Returning Flame", "Streamed in 2 different months", "dedication", 0, MONTHS, 2),
        ("tier_1", "Trailblazer Tier", "Reached the Trailblazer creator tier", "dedication", 1, TIER, 1),
        ("weeks_12", "Season of Streams", "Streamed in 12 different weeks", "dedication", 1, WEEKS, 12),
        ("months_3", "Three Moons", "Streamed in 3 different months", "dedication", 1, MONTHS, 3),
        ("tier_2", "Pioneer Tier", "Reached the Pioneer creator tier", "dedication", 2, TIER, 2),
        ("weeks_26", "Half a Year", "Streamed in 26 different weeks", "dedication", 2, WEEKS, 26),
        ("months_6", "Six Moons", "Streamed in 6 different months", "dedication", 2, MONTHS, 6),
        ("tier_3", "Pathfinder Tier", "Reached the Pathfinder creator tier", "dedication", 3, TIER, 3),
        ("weeks_52", "Year of Streams", "Streamed in 52 different weeks", "dedication", 3, WEEKS, 52),
        ("months_12", "Twelve Moons", "Streamed in 12 different months", "dedication", 3, MONTHS, 12),
    ];
    list.into_iter()
        .map(
            |(key, name, description, category, band, sql, need)| Achievement {
                key,
                name,
                description,
                category,
                band,
                sql,
                need,
            },
        )
        .collect()
}
/// Chat name styles and the level each unlocks at (always in the faction's colors).
pub const STYLES: [(&str, &str, i64); 5] = [
    ("plain", "Plain", 1),
    ("bold", "Bold", 10),
    ("smallcaps", "Small caps", 20),
    ("accent", "Accent underline", 30),
    ("outlined", "Outlined", 50),
];

/// Every 10 minutes: grants every achievement whose rule now holds, skipping people who already
/// have it, with an unlock notice. Achievements sharing a query (levels, hours, followers, tiers)
/// are granted from one pass over it. A brand-new achievement's first grant is a quiet backfill.
/// Earned ones are kept.
pub async fn award(app: &App) -> Res<()> {
    let catalog = catalog();
    let mut queries: Vec<&str> = catalog.iter().map(|a| a.sql).collect();
    queries.sort_unstable();
    queries.dedup();
    for sql in queries {
        let group: Vec<&Achievement> = catalog.iter().filter(|a| a.sql == sql).collect();
        let keys: Vec<&str> = group.iter().map(|a| a.key).collect();
        let needs: Vec<i64> = group.iter().map(|a| a.need).collect();
        let mut tx = app.db.begin().await?;
        // ponytail: "nobody holds it yet" marks a backfill, so the very first earner of a new
        // achievement gets no notice; record backfills explicitly if that ever matters.
        let backfill: Vec<String> = sqlx::query_scalar("SELECT k FROM unnest($1::text[]) k WHERE NOT EXISTS(SELECT 1 FROM user_achievements WHERE key=k)")
            .bind(&keys).fetch_all(&mut *tx).await?;
        let earned: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO user_achievements(user_id,key) SELECT s.u,k.key FROM ({sql}) s(u,v) JOIN unnest($1::text[],$2::bigint[]) k(key,need) ON s.v>=k.need
            WHERE s.u IS NOT NULL AND s.v>=$3 AND NOT EXISTS(SELECT 1 FROM user_achievements a WHERE a.user_id=s.u AND a.key=k.key) ON CONFLICT DO NOTHING RETURNING user_id,key"
        )))
        .bind(&keys)
        .bind(&needs)
        .bind(needs.iter().min().copied().unwrap_or(0))
        .fetch_all(&mut *tx)
        .await?;
        let notices: Vec<(String, &Achievement)> = earned
            .into_iter()
            .filter(|(_, k)| !backfill.contains(k))
            .filter_map(|(u, k)| group.iter().find(|a| a.key == k).map(|a| (u, *a)))
            .collect();
        if !notices.is_empty() {
            let users: Vec<&str> = notices.iter().map(|n| n.0.as_str()).collect();
            let (keys, (names, descriptions)): (Vec<&str>, (Vec<&str>, Vec<&str>)) = notices
                .iter()
                .map(|n| (n.1.key, (n.1.name, n.1.description)))
                .unzip();
            sqlx::query("INSERT INTO notifications(id,user_id,kind,channel_id,event_key,payload) SELECT gen_random_uuid()::text,c.id,'achievement',c.id,'achievement:'||n.key,jsonb_build_object('title','Achievement unlocked: '||n.name,'body',n.description,'url','/'||c.username)
                FROM unnest($1::text[],$2::text[],$3::text[],$4::text[]) n(u,key,name,description) JOIN channel_users c ON c.id=n.u ON CONFLICT DO NOTHING")
                .bind(&users).bind(&keys).bind(&names).bind(&descriptions)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

/// Each achievement's progress for one user, in catalog order: one round trip, one lookup per
/// distinct rule.
async fn progress(db: &mut PgConnection, user: &str) -> Res<Vec<i64>> {
    let catalog = catalog();
    let mut queries: Vec<&str> = catalog.iter().map(|a| a.sql).collect();
    queries.sort_unstable();
    queries.dedup();
    let parts: Vec<String> = queries
        .iter()
        .enumerate()
        .map(|(i, sql)| {
            format!(
                "SELECT {i} AS n,coalesce((SELECT max(v) FROM ({sql}) s(u,v) WHERE u=$1),0)::bigint"
            )
        })
        .collect();
    let rows: Vec<(i32, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(parts.join(" UNION ALL ")))
        .bind(user)
        .fetch_all(db)
        .await?;
    let value = |sql: &str| {
        let i = queries.iter().position(|q| *q == sql).unwrap_or_default() as i32;
        rows.iter().find(|r| r.0 == i).map_or(0, |r| r.1)
    };
    Ok(catalog.iter().map(|a| value(a.sql)).collect())
}
/// Share of accounts (not deleted or internal) holding each achievement, in percent.
async fn rarity(db: &mut PgConnection) -> Res<HashMap<String, f64>> {
    let accounts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channel_users WHERE NOT internal AND deleted_at IS NULL",
    )
    .fetch_one(&mut *db)
    .await?;
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT key,count(*) FROM user_achievements GROUP BY key")
            .fetch_all(db)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(k, n)| {
            (
                k,
                (n as f64 * 1000.0 / accounts.max(1) as f64).round() / 10.0,
            )
        })
        .collect())
}
/// Every achievement for its owner: earned or not, with progress toward it and its rarity.
pub async fn all(db: &mut PgConnection, user: &str) -> Res<Vec<Value>> {
    let have: HashMap<String, DateTime<Utc>> =
        sqlx::query_as("SELECT key,earned_at FROM user_achievements WHERE user_id=$1")
            .bind(user)
            .fetch_all(&mut *db)
            .await?
            .into_iter()
            .collect();
    let progress = progress(db, user).await?;
    let rarity = rarity(db).await?;
    Ok(catalog()
        .iter()
        .zip(progress)
        .map(|(a, v)| {
            json!({
                "key": a.key, "name": a.name, "description": a.description, "category": a.category,
                "band": a.band, "streamer": a.streamer(),
                "earned_at": have.get(a.key), "progress": v.min(a.need), "need": a.need,
                "rarity": rarity.get(a.key).copied().unwrap_or(0.0),
            })
        })
        .collect())
}

fn name(key: &str) -> Option<(&'static str, &'static str)> {
    catalog()
        .into_iter()
        .find(|a| a.key == key)
        .map(|a| (a.name, a.description))
}
async fn earned(db: &mut PgConnection, user: &str) -> Res<Vec<Value>> {
    let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT key,earned_at FROM user_achievements WHERE user_id=$1 ORDER BY earned_at",
    )
    .bind(user)
    .fetch_all(db)
    .await?;
    let catalog = catalog();
    Ok(rows
        .into_iter()
        .filter_map(|(key, at)| {
            catalog.iter().find(|a| a.key == key).map(|a| {
                json!({"key": key, "name": a.name, "description": a.description, "category": a.category,
                    "band": a.band, "streamer": a.streamer(), "earned_at": at})
            })
        })
        .collect())
}
/// The badge frame to draw: the chosen one if still unlocked, else the newest unlocked.
pub async fn frame(db: &mut PgConnection, user: &str, level: i64) -> Res<i64> {
    let chosen: Option<i16> =
        sqlx::query_scalar("SELECT frame FROM user_cosmetics WHERE user_id=$1")
            .bind(user)
            .fetch_optional(db)
            .await?
            .flatten();
    let newest = progression::frame(level);
    Ok(chosen.map_or(newest, |f| i64::from(f).min(newest)))
}
/// Up to 3 showcased achievements, for the profile and user card, with when each was earned.
pub async fn showcase(db: &mut PgConnection, user: &str) -> Res<Vec<Value>> {
    let rows: Vec<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT s.key,a.earned_at FROM user_cosmetics c CROSS JOIN LATERAL unnest(c.showcase) WITH ORDINALITY s(key,n) LEFT JOIN user_achievements a ON a.user_id=c.user_id AND a.key=s.key WHERE c.user_id=$1 ORDER BY s.n",
    )
    .bind(user)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(k, at)| {
            name(&k).map(|(n, d)| json!({"key": k, "name": n, "description": d, "earned_at": at}))
        })
        .collect())
}

/// GET /api/users/{username}/achievements: earned achievements with their rarity and the
/// showcase; the owner also gets the locked ones with progress (progress can reveal private
/// records, such as subscriptions, so visitors never see it).
async fn public(
    State(app): State<App>,
    jar: CookieJar,
    Path(username): Path<String>,
) -> Res<Json<Value>> {
    let viewer = profiles::viewer(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let id: String = sqlx::query_scalar("SELECT id FROM channel_users WHERE lower(username)=lower($1) AND eligible AND deleted_at IS NULL")
        .bind(&username).fetch_optional(&mut *db).await?.ok_or_else(Fail::missing)?;
    let (earned, locked) = if viewer.is_some_and(|v| v.id == id) {
        let (earned, locked): (Vec<Value>, Vec<Value>) = all(&mut db, &id)
            .await?
            .into_iter()
            .partition(|a| !a["earned_at"].is_null());
        (earned, Some(locked))
    } else {
        let rarity = rarity(&mut db).await?;
        let mut earned = earned(&mut db, &id).await?;
        for a in &mut earned {
            a["rarity"] = json!(
                rarity
                    .get(a["key"].as_str().unwrap_or_default())
                    .copied()
                    .unwrap_or(0.0)
            );
        }
        (earned, None)
    };
    Ok(Json(json!({
        "achievements": earned, "locked": locked, "showcase": showcase(&mut db, &id).await?,
        "total": catalog().len(),
    })))
}
/// GET /api/me/cosmetics: what's unlocked and chosen.
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let level = progression::level(progression::total(&mut db, &me.id).await?);
    let (style, chosen_frame, show): (Option<String>, Option<i16>, Option<Vec<String>>) =
        sqlx::query_as("SELECT name_style,frame,showcase FROM user_cosmetics WHERE user_id=$1")
            .bind(&me.id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or((None, None, None));
    let styles: Vec<Value> = STYLES
        .iter()
        .map(|(k, n, l)| json!({"key": k, "name": n, "level": l, "unlocked": level >= *l}))
        .collect();
    Ok(Json(json!({
        "level": level, "name_style": style.unwrap_or_else(|| "plain".into()), "frame": chosen_frame,
        "max_frame": progression::frame(level), "showcase": show.unwrap_or_default(),
        "styles": styles, "earned": earned(&mut db, &me.id).await?,
    })))
}
#[derive(Deserialize)]
pub struct Choice {
    name_style: String,
    frame: Option<i64>,
    #[serde(default)]
    showcase: Vec<String>,
}
/// PUT /api/me/cosmetics: only what the account has unlocked or earned.
async fn choose(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Choice>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    {
        let mut db = app.db.acquire().await?;
        let level = progression::level(progression::total(&mut db, &me.id).await?);
        let style = STYLES
            .iter()
            .find(|(k, _, _)| *k == input.name_style)
            .ok_or_else(|| Fail::field("name_style", "Choose a name style."))?;
        if level < style.2 {
            return Err(Fail::field(
                "name_style",
                "That style unlocks at a higher level.",
            ));
        }
        if input
            .frame
            .is_some_and(|f| !(0..=progression::frame(level)).contains(&f))
        {
            return Err(Fail::field(
                "frame",
                "That frame unlocks at a higher level.",
            ));
        }
        let show = input.showcase.clone();
        let unique: std::collections::HashSet<&String> = show.iter().collect();
        if show.len() > 3 || unique.len() != show.len() {
            return Err(Fail::field(
                "showcase",
                "Showcase up to 3 different achievements.",
            ));
        }
        let have: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM user_achievements WHERE user_id=$1 AND key=ANY($2)",
        )
        .bind(&me.id)
        .bind(&show)
        .fetch_one(&mut *db)
        .await?;
        if have != show.len() as i64 {
            return Err(Fail::field(
                "showcase",
                "Showcase only achievements you've earned.",
            ));
        }
        sqlx::query("INSERT INTO user_cosmetics(user_id,name_style,frame,showcase) VALUES($1,$2,$3,$4) ON CONFLICT(user_id) DO UPDATE SET name_style=EXCLUDED.name_style,frame=EXCLUDED.frame,showcase=EXCLUDED.showcase")
            .bind(&me.id).bind(&input.name_style).bind(input.frame.map(|f| f as i16)).bind(&show).execute(&mut *db).await?;
    }
    mine(State(app), jar).await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/users/{username}/achievements", get(public))
        .route("/api/me/cosmetics", get(mine).put(choose))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_per_category_per_band_with_unique_keys() {
        let all = catalog();
        let keys: std::collections::HashSet<&str> = all.iter().map(|a| a.key).collect();
        assert_eq!(keys.len(), all.len(), "keys are unique");
        for (category, _, _) in CATEGORIES {
            for band in 0..4 {
                let n = all
                    .iter()
                    .filter(|a| a.category == category && a.band == band)
                    .count();
                assert!(n >= 3, "{category} band {band} has {n}");
            }
        }
        // Keys earned before the catalog grew stay valid.
        for key in [
            "first_follow",
            "voice",
            "scout",
            "pathfinder",
            "order_keeper",
            "legend",
            "level_10",
            "level_25",
            "level_50",
            "level_100",
            "first_light",
            "hours_10",
            "hours_100",
            "followers_100",
            "followers_1000",
            "raider",
            "supported",
            "first_tribute",
            "faithful",
            "signatory",
            "council_of_war",
        ] {
            assert!(keys.contains(key), "{key}");
        }
    }
}
