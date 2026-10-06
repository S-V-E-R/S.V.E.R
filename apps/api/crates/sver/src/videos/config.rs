use crate::media::{S3, Storage};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tuning {
    pub clips_per_viewer_hour: i32,
    pub clips_per_channel_hour: i32,
    pub queued_segment_bytes: i64,
    pub clip_watch_seconds: f64,
    pub beats_per_viewer_minute: i32,
    pub beats_per_ip_minute: i32,
}
impl Default for Tuning {
    fn default() -> Self {
        // Safe development examples; production tuning stays in a private runtime file.
        Self {
            clips_per_viewer_hour: 10,
            clips_per_channel_hour: 60,
            queued_segment_bytes: 268_435_456,
            clip_watch_seconds: 3.0,
            beats_per_viewer_minute: 60,
            beats_per_ip_minute: 600,
        }
    }
}
#[derive(Clone)]
pub struct Config {
    pub storage: Storage,
    pub segment_base: String,
    pub tuning: Tuning,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            storage: Storage::Disabled,
            segment_base: String::new(),
            tuning: Tuning::default(),
        }
    }
}
impl Config {
    pub fn from_env(production: bool) -> Result<Self, String> {
        let env = |key| std::env::var(key).unwrap_or_default();
        let storage = match env("VOD_STORAGE").as_str() {
            "" | "disabled" => Storage::Disabled,
            "filesystem" if !production => {
                let path = PathBuf::from(env("VOD_DIR"));
                if !path.is_absolute() {
                    return Err("VOD_DIR must be an absolute private directory".into());
                }
                Storage::Filesystem(path)
            }
            "s3" => {
                let endpoint = env("VOD_S3_ENDPOINT").trim_end_matches('/').to_string();
                let url = url::Url::parse(&endpoint).map_err(|_| "Invalid VOD_S3_ENDPOINT")?;
                let s3 = S3 {
                    endpoint,
                    bucket: env("VOD_S3_BUCKET"),
                    region: env("VOD_S3_REGION"),
                    access_key: env("VOD_S3_ACCESS_KEY_ID"),
                    secret_key: env("VOD_S3_SECRET_ACCESS_KEY"),
                    prefix: "recordings/".into(),
                };
                if url.scheme() != "https"
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || s3.bucket.is_empty()
                    || s3.region.is_empty()
                    || s3.access_key.is_empty()
                    || s3.secret_key.is_empty()
                {
                    return Err("VOD_STORAGE=s3 requires a private bucket, HTTPS endpoint, region and credentials".into());
                }
                if production
                    && s3.bucket == env("MEDIA_S3_BUCKET")
                    && s3.endpoint == env("MEDIA_S3_ENDPOINT").trim_end_matches('/')
                {
                    return Err("Recordings must use a separate private bucket, not the public profile bucket".into());
                }
                Storage::S3(s3)
            }
            _ => {
                return Err(
                    "VOD_STORAGE must be disabled or s3 (filesystem is development-only)".into(),
                );
            }
        };
        let segment_base = env("VOD_SEGMENT_BASE").trim_end_matches('/').to_string();
        if storage.available() {
            let url = url::Url::parse(&segment_base).map_err(|_| "VOD_SEGMENT_BASE is required")?;
            if url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
                || !(url.scheme() == "https"
                    || (url.scheme() == "http"
                        && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))))
            {
                return Err(
                    "VOD_SEGMENT_BASE must be a trusted HTTPS or loopback HTTP media origin".into(),
                );
            }
        }
        let path = env("VOD_TUNING_FILE");
        if production && storage.available() && path.is_empty() {
            return Err(
                "Recording storage requires a private VOD_TUNING_FILE in production".into(),
            );
        }
        let tuning: Tuning = if path.is_empty() {
            Tuning::default()
        } else {
            serde_json::from_slice(&std::fs::read(path).map_err(|_| "Cannot read VOD_TUNING_FILE")?)
                .map_err(|_| "Invalid VOD_TUNING_FILE")?
        };
        if tuning.clips_per_viewer_hour < 1
            || tuning.clips_per_channel_hour < 1
            || tuning.queued_segment_bytes < 16_777_216
            || !tuning.clip_watch_seconds.is_finite()
            || !(0.1..=60.0).contains(&tuning.clip_watch_seconds)
            || tuning.beats_per_viewer_minute < 1
            || tuning.beats_per_ip_minute < 1
        {
            return Err("Invalid recording queue or clipping limits".into());
        }
        Ok(Self {
            storage,
            segment_base,
            tuning,
        })
    }
}
