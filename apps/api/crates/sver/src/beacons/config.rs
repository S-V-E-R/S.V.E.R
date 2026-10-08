use serde::Deserialize;

/// Anti-abuse values that stay private in production; these are safe development examples.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tuning {
    /// The watermark holds each corner for a per-Beacon time between these bounds.
    pub watermark_min_ms: i64,
    pub watermark_max_ms: i64,
    pub likes_per_user_hour: i32,
    pub beats_per_viewer_minute: i32,
    pub beats_per_ip_minute: i32,
    pub taps_per_viewer_hour: i32,
    pub taps_per_ip_hour: i32,
    /// New guest playback sessions per network (guests can mint browser IDs).
    pub guest_sessions_per_network_hour: i32,
    pub mutes_per_user_hour: i32,
}
impl Default for Tuning {
    fn default() -> Self {
        Self {
            watermark_min_ms: 3000,
            watermark_max_ms: 6000,
            likes_per_user_hour: 120,
            beats_per_viewer_minute: 60,
            beats_per_ip_minute: 600,
            taps_per_viewer_hour: 60,
            taps_per_ip_hour: 600,
            guest_sessions_per_network_hour: 60,
            mutes_per_user_hour: 60,
        }
    }
}
#[derive(Clone)]
pub struct Config {
    /// Uploads stay off until known-abuse matching is live (docs/BEACONS.md); clip Beacons don't wait.
    pub uploads: bool,
    pub font_file: String,
    pub tuning: Tuning,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            uploads: false,
            font_file: "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".into(),
            tuning: Tuning::default(),
        }
    }
}
impl Config {
    pub fn from_env(production: bool) -> Result<Self, String> {
        let env = |key| std::env::var(key).unwrap_or_default();
        let uploads = match env("BEACON_UPLOADS").as_str() {
            "" | "off" => false,
            "on" if !production => true,
            "on" => {
                return Err(
                    "BEACON_UPLOADS stays off in production until known-abuse matching is integrated"
                        .into(),
                );
            }
            _ => return Err("BEACON_UPLOADS must be on or off".into()),
        };
        let mut config = Self {
            uploads,
            ..Self::default()
        };
        let font = env("BEACON_FONT_FILE");
        if !font.is_empty() {
            if !std::path::Path::new(&font).is_absolute() {
                return Err("BEACON_FONT_FILE must be an absolute path".into());
            }
            config.font_file = font;
        }
        let path = env("BEACON_TUNING_FILE");
        // Watermark timing and counter limits stay private wherever Beacons can be processed.
        if production && env("VOD_STORAGE") == "s3" && path.is_empty() {
            return Err(
                "Beacon processing requires a private BEACON_TUNING_FILE in production".into(),
            );
        }
        if !path.is_empty() {
            config.tuning = serde_json::from_slice(
                &std::fs::read(path).map_err(|_| "Cannot read BEACON_TUNING_FILE")?,
            )
            .map_err(|_| "Invalid BEACON_TUNING_FILE")?;
        }
        let t = &config.tuning;
        if t.watermark_min_ms < 1000
            || t.watermark_max_ms < t.watermark_min_ms
            || t.watermark_max_ms > 15000
            || t.likes_per_user_hour < 1
            || t.beats_per_viewer_minute < 1
            || t.beats_per_ip_minute < 1
            || t.taps_per_viewer_hour < 1
            || t.mutes_per_user_hour < 1
        {
            return Err("Invalid Beacon watermark timing or counter limits".into());
        }
        Ok(config)
    }
}
