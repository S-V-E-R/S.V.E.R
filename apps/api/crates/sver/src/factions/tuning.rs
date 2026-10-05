use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Demo values only. Production must provide its private file; never expose it in API responses.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tuning {
    pub starts_at: Option<DateTime<Utc>>,
    pub points_per_second: [i64; 2],
    pub chat_points: i64,
    pub supporter_points: i64,
    pub daily_caps: [i64; 4],
    pub chat_hourly_cap: i64,
    pub minimum_divisor: i64,
    pub flip_margin_bps: i64,
    pub neutral_minimum: i64,
    pub board_slow_seconds: i64,
}
impl Default for Tuning {
    fn default() -> Self {
        Self {
            starts_at: None,
            points_per_second: [10, 10],
            chat_points: 1,
            supporter_points: 2,
            daily_caps: [1000, 1000, 10, 10],
            chat_hourly_cap: 3,
            minimum_divisor: 2,
            flip_margin_bps: 100,
            neutral_minimum: 10,
            board_slow_seconds: 30,
        }
    }
}
impl Tuning {
    pub fn from_env(production: bool) -> Result<Self, String> {
        let value = match std::env::var("FACTIONS_TUNING_FILE") {
            Ok(path) if !path.is_empty() => serde_json::from_str::<Self>(
                &std::fs::read_to_string(path).map_err(|_| "FACTIONS_TUNING_FILE is unreadable")?,
            )
            .map_err(|_| "FACTIONS_TUNING_FILE is invalid")?,
            _ if production => return Err("FACTIONS_TUNING_FILE is required in production".into()),
            _ => Self::default(),
        };
        if value
            .points_per_second
            .iter()
            .chain(value.daily_caps.iter())
            .any(|n| !(1..=1_000_000_000).contains(n))
            || !(1..=1_000_000).contains(&value.chat_points)
            || !(1..=1_000_000).contains(&value.supporter_points)
            || !(1..=1_000_000_000).contains(&value.chat_hourly_cap)
            || !(1..=1_000_000).contains(&value.minimum_divisor)
            || !(0..=10_000).contains(&value.flip_margin_bps)
            || !(1..=1_000_000_000).contains(&value.neutral_minimum)
            || !(1..=3600).contains(&value.board_slow_seconds)
            || (production && value.starts_at.is_none())
        {
            return Err("Invalid faction tuning bounds or season start".into());
        }
        Ok(value)
    }
}
