//! Image pipeline and media storage (docs/PROFILES.md, "Avatar and banner").
//!
//! Uploads are decoded and re-encoded on the server, so no client-supplied bytes are ever served.
//! Storage is a local filesystem directory (development, or an explicit persistent `MEDIA_DIR` in
//! production until the bucket exists) or an S3-compatible bucket (the future `media.sver.tv`
//! bucket) configured from the external environment.
use crate::{
    App,
    profiles::{
        Fail, Res, avatar_json, banner_json, ensure_profile, ensure_unrestricted, media_url, rate,
        signed_in,
    },
};
use axum::{
    Json,
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use image::{DynamicImage, GenericImageView, ImageReader, imageops::FilterType};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use std::{io::Cursor, path::PathBuf};

/// Body limit for upload routes (10 MB banners plus multipart overhead; nginx allows 11 MB).
pub const UPLOAD_LIMIT: usize = 11 * 1024 * 1024;
#[path = "media_removal.rs"]
pub mod removal;
#[path = "media_stream.rs"]
pub mod streaming;

#[derive(Clone, Debug)]
pub struct S3 {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    /// Optional object-key prefix (for example `v2/`) so the bucket can be shared with other
    /// data, such as the legacy R2 files, without ever touching their keys.
    pub prefix: String,
}
#[derive(Clone, Debug)]
pub enum Storage {
    Filesystem(PathBuf),
    S3(S3),
    Disabled,
}
#[derive(Clone, Debug)]
pub struct MediaConfig {
    pub storage: Storage,
    pub public_base: String,
}
impl MediaConfig {
    pub fn from_env(production: bool, origin: &str) -> std::result::Result<Self, String> {
        let env = |key: &str| std::env::var(key).unwrap_or_default();
        let storage = match env("MEDIA_STORAGE").as_str() {
            "s3" => {
                let s3 = S3 {
                    endpoint: env("MEDIA_S3_ENDPOINT").trim_end_matches('/').to_string(),
                    bucket: env("MEDIA_S3_BUCKET"),
                    region: if env("MEDIA_S3_REGION").is_empty() {
                        "auto".into()
                    } else {
                        env("MEDIA_S3_REGION")
                    },
                    access_key: env("MEDIA_S3_ACCESS_KEY_ID"),
                    secret_key: env("MEDIA_S3_SECRET_ACCESS_KEY"),
                    prefix: env("MEDIA_S3_PREFIX"),
                };
                if !valid_prefix(&s3.prefix) {
                    return Err(
                        "MEDIA_S3_PREFIX must be a lowercase key prefix ending in '/', such as v2/"
                            .into(),
                    );
                }
                if !s3.endpoint.starts_with("https://")
                    || s3.bucket.is_empty()
                    || s3.access_key.is_empty()
                    || s3.secret_key.is_empty()
                {
                    return Err("MEDIA_STORAGE=s3 requires MEDIA_S3_ENDPOINT (https), MEDIA_S3_BUCKET and credentials".into());
                }
                Storage::S3(s3)
            }
            "filesystem" | "" if !production => {
                let dir = if env("MEDIA_DIR").is_empty() {
                    PathBuf::from(
                        std::env::var("USERPROFILE")
                            .or_else(|_| std::env::var("HOME"))
                            .unwrap_or_else(|_| ".".into()),
                    )
                    .join("SVER-dev")
                    .join("media")
                } else {
                    PathBuf::from(env("MEDIA_DIR"))
                };
                Storage::Filesystem(dir)
            }
            // Production filesystem storage (interim, before the bucket exists): an explicit,
            // absolute MEDIA_DIR on a persistent volume, served by `serve_local`.
            "filesystem" => {
                let dir = env("MEDIA_DIR");
                if dir.is_empty() || !std::path::Path::new(&dir).is_absolute() {
                    return Err(
                        "MEDIA_STORAGE=filesystem in production requires an absolute MEDIA_DIR on a persistent volume".into(),
                    );
                }
                Storage::Filesystem(PathBuf::from(dir))
            }
            // Production without a configured bucket: uploads are unavailable (503).
            "" | "disabled" => Storage::Disabled,
            _ => {
                return Err("MEDIA_STORAGE must be s3, filesystem or disabled".into());
            }
        };
        let public_base = if env("MEDIA_PUBLIC_BASE").is_empty() {
            if production && !matches!(storage, Storage::Filesystem(_)) {
                "https://media.sver.tv".to_string()
            } else {
                format!("{origin}/api/media")
            }
        } else {
            env("MEDIA_PUBLIC_BASE").trim_end_matches('/').to_string()
        };
        Ok(Self {
            storage,
            public_base,
        })
    }
}

/// An empty prefix, or a lowercase key prefix ending in '/' (for example `v2/`).
fn valid_prefix(prefix: &str) -> bool {
    prefix.is_empty() || (prefix.ends_with('/') && valid_key(prefix))
}
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() < 200
        && !key.contains("..")
        && !key.starts_with('/')
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"/._@-".contains(&b))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}
impl S3 {
    /// Path-style object URL, with the configured key prefix applied.
    pub fn object_url(&self, key: &str) -> String {
        format!("{}/{}/{}{}", self.endpoint, self.bucket, self.prefix, key)
    }
    /// AWS Signature Version 4 request for a single object (path-style URL).
    fn request(
        &self,
        http: &reqwest::Client,
        method: reqwest::Method,
        key: &str,
        body: &[u8],
    ) -> Result<reqwest::RequestBuilder, Fail> {
        self.request_query(http, method, key, body, &[])
    }
    fn request_query(
        &self,
        http: &reqwest::Client,
        method: reqwest::Method,
        key: &str,
        body: &[u8],
        query: &[(&str, String)],
    ) -> Res<reqwest::RequestBuilder> {
        let mut url = url::Url::parse(&self.object_url(key)).map_err(|_| Fail::internal())?;
        if !query.is_empty() {
            let mut query = query.to_vec();
            query.sort();
            let encoded = query
                .iter()
                .map(|(k, v)| format!("{}={}", aws_encode(k), aws_encode(v)))
                .collect::<Vec<_>>()
                .join("&");
            url.set_query(Some(&encoded));
        }
        let host = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
            None => url.host_str().unwrap_or_default().to_string(),
        };
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let payload = hex(&Sha256::digest(body));
        let canonical = format!(
            "{}\n{}\n{}\nhost:{host}\nx-amz-content-sha256:{payload}\nx-amz-date:{amz_date}\n\nhost;x-amz-content-sha256;x-amz-date\n{payload}",
            method.as_str(),
            url.path(),
            url.query().unwrap_or("")
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            hex(&Sha256::digest(canonical.as_bytes()))
        );
        let mut signing = hmac(format!("AWS4{}", self.secret_key).as_bytes(), &date);
        for part in [self.region.as_str(), "s3", "aws4_request"] {
            signing = hmac(&signing, part);
        }
        let signature = hex(&hmac(&signing, &to_sign));
        Ok(http
            .request(method, url)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload)
            .header("authorization", format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={signature}", self.access_key)))
    }
}
impl S3 {
    /// Query-signed PUT URL (SigV4, host header only, unsigned payload) so a browser can upload
    /// one object straight to private storage. The URL names one key and expires; nothing else
    /// about the bucket is exposed.
    pub fn presign_put(&self, key: &str, seconds: u32) -> Res<String> {
        let mut url = url::Url::parse(&self.object_url(key)).map_err(|_| Fail::internal())?;
        let host = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
            None => url.host_str().unwrap_or_default().to_string(),
        };
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let mut query = [
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            ("X-Amz-Credential", format!("{}/{scope}", self.access_key)),
            ("X-Amz-Date", amz_date.clone()),
            ("X-Amz-Expires", seconds.to_string()),
            ("X-Amz-SignedHeaders", "host".to_string()),
        ];
        query.sort();
        let encoded = query
            .iter()
            .map(|(k, v)| format!("{}={}", aws_encode(k), aws_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let canonical = format!(
            "PUT\n{}\n{encoded}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD",
            url.path()
        );
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            hex(&Sha256::digest(canonical.as_bytes()))
        );
        let mut signing = hmac(format!("AWS4{}", self.secret_key).as_bytes(), &date);
        for part in [self.region.as_str(), "s3", "aws4_request"] {
            signing = hmac(&signing, part);
        }
        let signature = hex(&hmac(&signing, &to_sign));
        url.set_query(Some(&format!("{encoded}&X-Amz-Signature={signature}")));
        Ok(url.to_string())
    }
}
fn aws_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
impl Storage {
    pub fn available(&self) -> bool {
        !matches!(self, Storage::Disabled)
    }
    pub async fn put(&self, http: &reqwest::Client, key: &str, bytes: Vec<u8>) -> Res<()> {
        self.put_typed(
            http,
            key,
            bytes,
            "image/webp",
            "public, max-age=31536000, immutable",
        )
        .await
    }
    /// Recordings use a separate private store and must never inherit public image caching.
    pub async fn put_typed(
        &self,
        http: &reqwest::Client,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
        cache_control: &str,
    ) -> Res<()> {
        if !valid_key(key) {
            return Err(Fail::internal());
        }
        match self {
            Storage::Filesystem(dir) => {
                let path = dir.join(key);
                tokio::fs::create_dir_all(path.parent().ok_or_else(Fail::internal)?)
                    .await
                    .map_err(|_| Fail::internal())?;
                tokio::fs::write(path, bytes)
                    .await
                    .map_err(|_| Fail::internal())
            }
            Storage::S3(s3) => {
                let ok = s3
                    .request(http, reqwest::Method::PUT, key, &bytes)?
                    .header("content-type", content_type)
                    .header("cache-control", cache_control)
                    .body(bytes)
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success());
                if ok {
                    Ok(())
                } else {
                    Err(Fail::unavailable(
                        "Image storage is unavailable. Try again.",
                    ))
                }
            }
            Storage::Disabled => Err(Fail::unavailable("Image uploads aren't available yet.")),
        }
    }
    pub async fn delete(&self, http: &reqwest::Client, key: &str) -> Res<()> {
        if !valid_key(key) {
            return Err(Fail::internal());
        }
        match self {
            Storage::Filesystem(dir) => match tokio::fs::remove_file(dir.join(key)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(Fail::internal()),
            },
            Storage::S3(s3) => {
                let status = s3
                    .request(http, reqwest::Method::DELETE, key, b"")?
                    .send()
                    .await
                    .map(|r| r.status())
                    .map_err(|_| Fail::unavailable("Image storage is unavailable."))?;
                if status.is_success() || status == StatusCode::NOT_FOUND {
                    Ok(())
                } else {
                    Err(Fail::unavailable("Image storage is unavailable."))
                }
            }
            Storage::Disabled => Ok(()),
        }
    }
    /// Stored byte length, or None when the object doesn't exist.
    pub async fn head(&self, http: &reqwest::Client, key: &str) -> Res<Option<u64>> {
        match self {
            Storage::Filesystem(dir) => match tokio::fs::metadata(dir.join(key)).await {
                Ok(metadata) => Ok(Some(metadata.len())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(Fail::unavailable("Media storage is unavailable.")),
            },
            Storage::S3(s3) => {
                let response = s3
                    .request(http, reqwest::Method::HEAD, key, b"")?
                    .send()
                    .await
                    .map_err(|_| Fail::unavailable("Image storage is unavailable."))?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                if !response.status().is_success() {
                    return Err(Fail::unavailable("Media storage is unavailable."));
                }
                response
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .map(Some)
                    .ok_or_else(|| Fail::unavailable("Invalid media storage response."))
            }
            Storage::Disabled => Ok(None),
        }
    }
    pub async fn get(&self, http: &reqwest::Client, key: &str) -> Res<Option<Vec<u8>>> {
        match self {
            Storage::Filesystem(dir) => match tokio::fs::read(dir.join(key)).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(Fail::unavailable("Image storage is unavailable.")),
            },
            Storage::S3(s3) => {
                let response = s3
                    .request(http, reqwest::Method::GET, key, b"")?
                    .send()
                    .await
                    .map_err(|_| Fail::unavailable("Image storage is unavailable."))?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                if !response.status().is_success() {
                    return Err(Fail::unavailable("Image storage is unavailable."));
                }
                Ok(Some(
                    response
                        .bytes()
                        .await
                        .map_err(|_| Fail::unavailable("Image storage is unavailable."))?
                        .to_vec(),
                ))
            }
            Storage::Disabled => Ok(None),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Avatar,
    Banner,
    SponsorLogo,
    FanArt,
    SongThumb,
    SetupPhoto,
    Emote,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Avatar => "avatar",
            Kind::Banner => "banner",
            Kind::SponsorLogo => "sponsor_logo",
            Kind::FanArt => "fan_art",
            Kind::SongThumb => "song_thumb",
            Kind::SetupPhoto => "setup_photo",
            Kind::Emote => "emote",
        }
    }
    pub fn max_bytes(self) -> usize {
        match self {
            Kind::Avatar | Kind::FanArt | Kind::SetupPhoto => 5 * 1024 * 1024,
            Kind::Banner => 10 * 1024 * 1024,
            Kind::SponsorLogo => 2 * 1024 * 1024,
            Kind::SongThumb => 5 * 1024 * 1024,
            Kind::Emote => 1024 * 1024,
        }
    }
    fn too_big(self) -> Fail {
        Fail::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            match self {
                Kind::Avatar => "Avatars can be up to 5 MB.",
                Kind::Banner => "Banners can be up to 10 MB.",
                Kind::SponsorLogo => "Sponsor logos can be up to 2 MB.",
                Kind::FanArt => "Fan art can be up to 5 MB.",
                Kind::SongThumb => "That image is too large to process.",
                Kind::SetupPhoto => "Setup photos can be up to 5 MB.",
                Kind::Emote => "Emotes can be up to 1 MB.",
            },
        )
    }
}
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
/// One encoded variant ready for storage.
pub struct Variant {
    pub key: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
pub struct Processed {
    /// The value stored on the owning row (prefix or full key, see `stored_prefix`).
    pub stored: String,
    pub variants: Vec<Variant>,
    pub fingerprints: Vec<String>,
}

pub fn sniff(bytes: &[u8]) -> Option<image::ImageFormat> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(image::ImageFormat::Jpeg)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(image::ImageFormat::Png)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(image::ImageFormat::WebP)
    } else {
        None
    }
}
fn decode(bytes: &[u8], kind: Kind) -> Res<DynamicImage> {
    let format =
        sniff(bytes).ok_or_else(|| Fail::field("file", "Use a JPG, PNG or WebP image."))?;
    if bytes.len() > kind.max_bytes() {
        return Err(kind.too_big());
    }
    if kind == Kind::Emote && format == image::ImageFormat::Jpeg {
        return Err(Fail::field("file", "Use a PNG or WebP image."));
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| Fail::field("file", "We couldn't read that image."))?;
    let side = if matches!(kind, Kind::FanArt | Kind::SetupPhoto) {
        4096
    } else {
        10_000
    };
    if width > side || height > side || (width as u64) * (height as u64) > 50_000_000 {
        return Err(Fail::field("file", "That image is too large to process."));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(side);
    limits.max_image_height = Some(side);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    // Animated WebP/APNG decode to their first frame.
    reader
        .decode()
        .map_err(|_| Fail::field("file", "We couldn't read that image."))
}
fn encode(image: &DynamicImage) -> Res<Vec<u8>> {
    let rgba = image.to_rgba8();
    let mut out = Vec::new();
    // The WebP encoder writes only image data: EXIF, XMP, ICC and GPS metadata are never copied.
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .encode(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|_| Fail::internal())?;
    Ok(out)
}
fn centered(width: u32, height: u32, ratio: f64) -> Crop {
    // ratio = width / height of the crop.
    if (width as f64) / (height as f64) > ratio {
        let w = ((height as f64) * ratio).round() as u32;
        Crop {
            x: (width - w) / 2,
            y: 0,
            width: w,
            height,
        }
    } else {
        let h = ((width as f64) / ratio).round() as u32;
        Crop {
            x: 0,
            y: (height - h) / 2,
            width,
            height: h,
        }
    }
}
fn content_hash(bytes: &[u8], kind: Kind, crop: &Crop) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.name().as_bytes());
    hasher.update(format!("{}:{}:{}:{}", crop.x, crop.y, crop.width, crop.height).as_bytes());
    hasher.update(bytes);
    hex(&hasher.finalize())[..32].to_string()
}

/// Decodes, validates, crops, resizes and re-encodes an image. CPU-bound: run on a blocking thread.
pub fn process(bytes: &[u8], kind: Kind, crop: Option<Crop>) -> Res<Processed> {
    let image = decode(bytes, kind)?;
    let fingerprints = removal::fingerprints(bytes, &image);
    let (width, height) = image.dimensions();
    let mut processed = match kind {
        Kind::Emote => {
            if width != height || width < 112 {
                return Err(Fail::field(
                    "file",
                    "Emotes must be square and at least 112 pixels.",
                ));
            }
            let hash = content_hash(
                bytes,
                kind,
                &Crop {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
            );
            let prefix = format!("emotes/{hash}");
            let mut variants = Vec::new();
            for size in crate::emotes::SIZES {
                let resized = image.resize_exact(size, size, FilterType::Lanczos3);
                variants.push(Variant {
                    key: format!("{prefix}/{size}.webp"),
                    bytes: encode(&resized)?,
                    width: size,
                    height: size,
                });
            }
            Ok(Processed {
                stored: prefix,
                variants,
                fingerprints,
            })
        }
        Kind::Avatar | Kind::Banner => {
            let (ratio, min_w, min_h) = if kind == Kind::Avatar {
                (1.0, 128, 128)
            } else {
                (3.0, 1200, 400)
            };
            if width < min_w || height < min_h {
                return Err(Fail::field("file", "That image is too small."));
            }
            let crop = match crop {
                Some(c) => {
                    if c.width == 0
                        || c.height == 0
                        || c.x.checked_add(c.width).is_none_or(|r| r > width)
                        || c.y.checked_add(c.height).is_none_or(|b| b > height)
                    {
                        return Err(Fail::field("crop", "The crop must fit inside the image."));
                    }
                    let expected = (c.height as f64) * ratio;
                    if ((c.width as f64) - expected).abs() > ratio.max(1.0) {
                        return Err(Fail::field(
                            "crop",
                            if kind == Kind::Avatar {
                                "Crop must be square."
                            } else {
                                "Crop must be 3:1."
                            },
                        ));
                    }
                    if c.width < min_w || c.height < min_h {
                        return Err(Fail::field("file", "That image is too small."));
                    }
                    c
                }
                None => centered(width, height, ratio),
            };
            let hash = content_hash(bytes, kind, &crop);
            let cropped = image.crop_imm(crop.x, crop.y, crop.width, crop.height);
            let mut variants = Vec::new();
            if kind == Kind::Avatar {
                let prefix = format!("avatars/{hash}");
                for size in crate::profiles::AVATAR_SIZES {
                    let resized = cropped.resize_exact(size, size, FilterType::Lanczos3);
                    variants.push(Variant {
                        key: format!("{prefix}/{size}.webp"),
                        bytes: encode(&resized)?,
                        width: size,
                        height: size,
                    });
                }
                Ok::<Processed, Fail>(Processed {
                    stored: prefix,
                    variants,
                    fingerprints,
                })
            } else {
                let prefix = format!("banners/{hash}");
                let mut largest = 0;
                for w in crate::profiles::BANNER_WIDTHS {
                    // Never upscale: the smallest size is always produced (the minimum crop exceeds it).
                    if w > crop.width && w != crate::profiles::BANNER_WIDTHS[0] {
                        continue;
                    }
                    let resized = cropped.resize_exact(w, w / 3, FilterType::Lanczos3);
                    variants.push(Variant {
                        key: format!("{prefix}/{w}.webp"),
                        bytes: encode(&resized)?,
                        width: w,
                        height: w / 3,
                    });
                    largest = w;
                }
                Ok(Processed {
                    stored: format!("{prefix}@{largest}"),
                    variants,
                    fingerprints,
                })
            }
        }
        Kind::SponsorLogo | Kind::SongThumb => {
            let size = if kind == Kind::SponsorLogo { 256 } else { 400 };
            if width < 16 || height < 16 {
                return Err(Fail::field("file", "That image is too small."));
            }
            let crop = Crop {
                x: 0,
                y: 0,
                width,
                height,
            };
            let hash = content_hash(bytes, kind, &crop);
            let resized = if width > size || height > size {
                image.resize(size, size, FilterType::Lanczos3)
            } else {
                image
            };
            let folder = if kind == Kind::SponsorLogo {
                "sponsors"
            } else {
                "songs"
            };
            let key = format!("{folder}/{hash}.webp");
            let (w, h) = resized.dimensions();
            Ok(Processed {
                stored: key.clone(),
                fingerprints,
                variants: vec![Variant {
                    key,
                    bytes: encode(&resized)?,
                    width: w,
                    height: h,
                }],
            })
        }
        Kind::FanArt | Kind::SetupPhoto => {
            if width < 64 || height < 64 {
                return Err(Fail::field("file", "That image is too small."));
            }
            let crop = Crop {
                x: 0,
                y: 0,
                width,
                height,
            };
            let hash = content_hash(bytes, kind, &crop);
            let folder = if kind == Kind::FanArt {
                "fanart"
            } else {
                "setup"
            };
            let prefix = format!("{folder}/{hash}");
            let mut variants = Vec::new();
            for size in [400u32, 1600] {
                let resized = if width > size || height > size {
                    image.resize(size, size, FilterType::Lanczos3)
                } else {
                    image.clone()
                };
                let (w, h) = resized.dimensions();
                variants.push(Variant {
                    key: format!("{prefix}/{size}.webp"),
                    bytes: encode(&resized)?,
                    width: w,
                    height: h,
                });
            }
            Ok(Processed {
                stored: prefix,
                variants,
                fingerprints,
            })
        }
    }?;
    for variant in &processed.variants {
        let image = image::load_from_memory_with_format(&variant.bytes, image::ImageFormat::WebP)
            .map_err(|_| Fail::internal())?;
        processed
            .fingerprints
            .extend(removal::fingerprints(&variant.bytes, &image));
    }
    processed.fingerprints.sort();
    processed.fingerprints.dedup();
    Ok(processed)
}
/// Import-only fallback for a legacy banner whose centered 3:1 crop is below the 1200x400
/// minimum: the same centered crop is upscaled (Lanczos3) to exactly 1200x400 and returned as
/// PNG for the normal pipeline. Crops needing more than a 2x upscale are refused. User uploads
/// never use this; they still get "That image is too small."
pub fn upscale_banner_to_minimum(bytes: &[u8]) -> Res<Vec<u8>> {
    let image = decode(bytes, Kind::Banner)?;
    let (width, height) = image.dimensions();
    let crop = centered(width, height, 3.0);
    if crop.width * 2 < 1200 || crop.height * 2 < 400 {
        return Err(Fail::field("file", "That image is too small."));
    }
    let upscaled = image
        .crop_imm(crop.x, crop.y, crop.width, crop.height)
        .resize_exact(1200, 400, FilterType::Lanczos3);
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(upscaled.to_rgba8())
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|_| Fail::internal())?;
    Ok(out.into_inner())
}
pub async fn process_async(bytes: Vec<u8>, kind: Kind, crop: Option<Crop>) -> Res<Processed> {
    tokio::task::spawn_blocking(move || process(&bytes, kind, crop))
        .await
        .map_err(|_| Fail::internal())?
}

/// Uploads every variant before the database change commits. On failure, removes what it wrote.
pub async fn store(app: &App, processed: &Processed) -> Res<()> {
    let mut tx = app.db.begin().await?;
    removal::upload_allowed(&mut tx, processed).await?;
    let mut written = Vec::new();
    for variant in &processed.variants {
        if let Err(e) = app
            .config
            .media
            .storage
            .put(&app.http, &variant.key, variant.bytes.clone())
            .await
        {
            for key in written {
                let _ = app.config.media.storage.delete(&app.http, key).await;
            }
            return Err(e);
        }
        written.push(&variant.key);
        sqlx::query("INSERT INTO media_objects(key,kind,bytes,width,height,delete_after,fingerprinted) VALUES($1,'pending',$2,$3,$4,now()+interval '1 hour',true) ON CONFLICT DO NOTHING")
            .bind(&variant.key).bind(variant.bytes.len() as i64).bind(variant.width as i32).bind(variant.height as i32).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
/// Records stored variants (clearing any pending deletion of identical content).
pub async fn record(
    db: &mut PgConnection,
    owner: &str,
    kind: Kind,
    processed: &Processed,
) -> Res<()> {
    removal::upload_allowed(db, processed).await?;
    for v in &processed.variants {
        sqlx::query("INSERT INTO media_objects(key,owner_id,kind,bytes,width,height,fingerprinted) VALUES($1,$2,$3,$4,$5,$6,true) ON CONFLICT (key) DO UPDATE SET delete_after=NULL,owner_id=EXCLUDED.owner_id,kind=EXCLUDED.kind,fingerprinted=true")
            .bind(&v.key)
            .bind(owner)
            .bind(kind.name())
            .bind(v.bytes.len() as i64)
            .bind(v.width as i32)
            .bind(v.height as i32)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}
/// The object-key prefix shared by every variant of a stored value.
pub fn stored_prefix(stored: &str) -> &str {
    stored.split_once('@').map(|(p, _)| p).unwrap_or(stored)
}
/// Queues a replaced or removed image for deletion after the surrounding transaction commits.
pub async fn queue_delete(
    db: &mut PgConnection,
    stored: Option<&str>,
    keep: Option<&str>,
) -> Res<()> {
    let Some(stored) = stored else { return Ok(()) };
    if keep.is_some_and(|k| stored_prefix(k) == stored_prefix(stored)) {
        return Ok(());
    }
    sqlx::query("UPDATE media_objects SET delete_after=now() WHERE key=$1 OR key LIKE $1 || '/%'")
        .bind(stored_prefix(stored))
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Reads one multipart upload: the `file` field (bounded) and an optional JSON `crop` field.
pub async fn read_upload(
    mut multipart: Multipart,
    kind: Kind,
) -> Res<(Vec<u8>, Option<Crop>, serde_json::Map<String, Value>)> {
    let mut file = None;
    let mut crop = None;
    let mut fields = serde_json::Map::new();
    // A body over the route limit surfaces as a multipart error carrying 413.
    let unreadable = |e: axum::extract::multipart::MultipartError| {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            kind.too_big()
        } else {
            Fail::bad("The upload could not be read.")
        }
    };
    while let Some(mut field) = multipart.next_field().await.map_err(unreadable)? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            let mut bytes = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(|_| kind.too_big())? {
                bytes.extend_from_slice(&chunk);
                if bytes.len() > kind.max_bytes() {
                    return Err(kind.too_big());
                }
            }
            file = Some(bytes);
        } else {
            let value = field.text().await.map_err(unreadable)?;
            if value.len() > 4096 {
                return Err(Fail::bad("The upload could not be read."));
            }
            if name == "crop" {
                if !value.trim().is_empty() {
                    crop =
                        Some(serde_json::from_str(&value).map_err(|_| {
                            Fail::field("crop", "The crop must fit inside the image.")
                        })?);
                }
            } else {
                fields.insert(name, Value::String(value));
            }
        }
    }
    let file = file.ok_or_else(|| Fail::field("file", "Choose an image to upload."))?;
    Ok((file, crop, fields))
}

async fn upload(app: App, jar: CookieJar, multipart: Multipart, kind: Kind) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    {
        let mut db = app.db.acquire().await?;
        ensure_unrestricted(&mut db, &user.id).await?;
    }
    if !app.config.media.storage.available() {
        return Err(Fail::unavailable("Image uploads aren't available yet."));
    }
    let (bytes, crop, _) = read_upload(multipart, kind).await?;
    rate(&app, format!("image-upload:{}", user.id), 20, 3600).await?;
    let processed = process_async(bytes, kind, crop).await?;
    store(&app, &processed).await?;
    let column = if kind == Kind::Avatar {
        "avatar_key"
    } else {
        "banner_key"
    };
    let result = async {
        let mut tx = app.db.begin().await?;
        ensure_unrestricted(&mut tx, &user.id).await?;
        ensure_profile(&mut tx, &user.id).await?;
        // column is selected from the two literal media columns above; values are bound.
        let old: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT {column} FROM profiles WHERE user_id=$1 FOR UPDATE"
        )))
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
        record(&mut tx, &user.id, kind, &processed).await?;
        // column is selected from the two literal media columns above; values are bound.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE profiles SET {column}=$2,updated_at=now() WHERE user_id=$1"
        )))
        .bind(&user.id)
        .bind(&processed.stored)
        .execute(&mut *tx)
        .await?;
        queue_delete(&mut tx, old.as_deref(), Some(&processed.stored)).await?;
        tx.commit().await?;
        Ok::<_, Fail>(())
    }
    .await;
    if let Err(e) = result {
        // Nothing references the new objects; remove them unless identical content was already stored.
        let mut db = app.db.acquire().await?;
        let _ = sqlx::query("UPDATE media_objects SET delete_after=now() WHERE key LIKE $1 || '/%' AND NOT EXISTS(SELECT 1 FROM profiles WHERE avatar_key=$2 OR banner_key=$2)")
            .bind(stored_prefix(&processed.stored))
            .bind(&processed.stored)
            .execute(&mut *db)
            .await;
        return Err(e);
    }
    Ok(Json(json!({
        "saved": true,
        "avatar": if kind == Kind::Avatar { avatar_json(&app, Some(&processed.stored)) } else { Value::Null },
        "banner": if kind == Kind::Banner { banner_json(&app, Some(&processed.stored)) } else { Value::Null },
    })))
}
async fn remove(app: App, jar: CookieJar, kind: Kind) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let column = if kind == Kind::Avatar {
        "avatar_key"
    } else {
        "banner_key"
    };
    let mut tx = app.db.begin().await?;
    ensure_unrestricted(&mut tx, &user.id).await?;
    ensure_profile(&mut tx, &user.id).await?;
    // column is selected from the two literal media columns above; values are bound.
    let old: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {column} FROM profiles WHERE user_id=$1 FOR UPDATE"
    )))
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    // column is selected from the two literal media columns above; values are bound.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE profiles SET {column}=NULL,updated_at=now() WHERE user_id=$1"
    )))
    .bind(&user.id)
    .execute(&mut *tx)
    .await?;
    queue_delete(&mut tx, old.as_deref(), None).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}
pub async fn upload_avatar(
    State(app): State<App>,
    jar: CookieJar,
    multipart: Multipart,
) -> Res<Json<Value>> {
    upload(app, jar, multipart, Kind::Avatar).await
}
pub async fn upload_banner(
    State(app): State<App>,
    jar: CookieJar,
    multipart: Multipart,
) -> Res<Json<Value>> {
    upload(app, jar, multipart, Kind::Banner).await
}
pub async fn remove_avatar(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    remove(app, jar, Kind::Avatar).await
}
pub async fn remove_banner(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    remove(app, jar, Kind::Banner).await
}

/// GET /api/media/{key}: serves the filesystem store (development, and production while the
/// interim filesystem adapter is configured). Keys are content-hashed, so responses are immutable.
pub async fn serve_local(State(app): State<App>, Path(key): Path<String>) -> Response {
    if !valid_key(&key) || !key.ends_with(".webp") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let held = match app.db.acquire().await {
        Ok(mut db) => removal::held(&mut db, &key).await.unwrap_or(true),
        Err(_) => true,
    };
    if held {
        return StatusCode::NOT_FOUND.into_response();
    }
    let dir = match &app.config.media.storage {
        Storage::Filesystem(dir) => dir,
        // After moving to a bucket, URLs issued under the interim filesystem store keep working:
        // keys are unchanged, so they redirect permanently to the public bucket URL.
        Storage::S3(_) => {
            return (
                StatusCode::PERMANENT_REDIRECT,
                [(header::LOCATION, media_url(&app, &key))],
            )
                .into_response();
        }
        Storage::Disabled => return StatusCode::NOT_FOUND.into_response(),
    };
    match tokio::fs::read(dir.join(&key)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "image/webp"),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            Bytes::from(bytes),
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Deletes queued objects from storage, except media retained by open report snapshots or
/// strikes. Runs from the maintenance job.
pub async fn cleanup(app: &App) -> Res<usize> {
    let keys: Vec<String> = sqlx::query_scalar("SELECT m.key FROM media_objects m WHERE m.delete_after<=now() AND NOT EXISTS(SELECT 1 FROM media_removal_holds h WHERE m.key=h.root OR m.key LIKE h.root||'/%') AND NOT EXISTS(SELECT 1 FROM reports r WHERE r.status='OPEN' AND position(split_part(split_part(m.key,'/',1)||'/'||split_part(m.key,'/',2),'@',1) IN r.snapshot::text)>0) AND NOT EXISTS(SELECT 1 FROM strikes s WHERE position(split_part(split_part(m.key,'/',1)||'/'||split_part(m.key,'/',2),'@',1) IN s.content_snapshot::text)>0) ORDER BY m.delete_after LIMIT 100")
        .fetch_all(&app.db)
        .await?;
    let mut deleted = 0;
    for key in keys {
        let mut tx = app.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(1414087745)")
            .execute(&mut *tx)
            .await?;
        if removal::held(&mut tx, &key).await? {
            continue;
        }
        if (key.starts_with("emotes/") && crate::emotes::referenced(&mut tx, &key).await?)
            || crate::guilds::referenced(&mut tx, &key).await?
        {
            continue;
        }
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM media_objects WHERE key=$1 AND delete_after<=now())",
        )
        .bind(&key)
        .fetch_one(&mut *tx)
        .await?;
        if !pending {
            continue;
        }
        if app
            .config
            .media
            .storage
            .delete(&app.http, &key)
            .await
            .is_ok()
        {
            sqlx::query("DELETE FROM media_objects WHERE key=$1 AND delete_after<=now()")
                .bind(&key)
                .execute(&mut *tx)
                .await?;
            deleted += 1;
        }
        tx.commit().await?;
    }
    Ok(deleted)
}

#[cfg(test)]
mod storage_tests {
    use super::*;

    #[test]
    fn emotes_are_static_sized_webp_with_transparency() {
        let animated = process(
            include_bytes!("../tests/fixtures/emote-animated.png"),
            Kind::Emote,
            None,
        )
        .unwrap();
        for v in animated.variants {
            let decoded = image::load_from_memory(&v.bytes).unwrap().to_rgba8();
            assert_eq!(
                decoded.get_pixel(14, 14).0,
                [200, 30, 30, 255],
                "Animation uses its first frame"
            );
        }
        let source = include_bytes!("../tests/fixtures/avatar-indexed-trns.png");
        let processed = process(source, Kind::Emote, None).unwrap();
        assert_eq!(
            processed
                .variants
                .iter()
                .map(|v| v.width)
                .collect::<Vec<_>>(),
            vec![28, 56, 112]
        );
        for v in &processed.variants {
            assert_eq!(alpha_at(&v.bytes, 0, 0), 0);
            assert_eq!(v.width, v.height);
            assert!(!v.bytes.windows(4).any(|w| w == b"ANIM" || w == b"EXIF"));
        }
        assert!(process(&processed.variants[2].bytes, Kind::Emote, None).is_ok());
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(112, 112)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        assert!(process(&jpeg.into_inner(), Kind::Emote, None).is_err());
    }

    #[test]
    fn signing_hmac_matches_rfc_4231() {
        // Published vectors cover both normal and hashed (over-block-size) keys.
        assert_eq!(
            hex(&hmac(&[0x0b; 20], "Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex(&hmac(
                &[0xaa; 131],
                "Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    fn alpha_at(webp: &[u8], x: u32, y: u32) -> u8 {
        let image = image::load_from_memory_with_format(webp, image::ImageFormat::WebP).unwrap();
        image.to_rgba8().get_pixel(x, y)[3]
    }

    /// Transparent PNGs (RGBA and indexed with tRNS) keep their transparency in every variant;
    /// opaque images stay opaque with no matte or padding.
    #[test]
    fn avatars_keep_transparency() {
        let rgba = image::RgbaImage::from_fn(300, 300, |x, y| {
            if (100..200).contains(&x) && (100..200).contains(&y) {
                image::Rgba([200, 30, 30, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba.clone())
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let webp = encode(&DynamicImage::ImageRgba8(rgba)).unwrap();
        let fixtures = [
            include_bytes!("../tests/fixtures/avatar-indexed-trns.png").to_vec(),
            include_bytes!("../tests/fixtures/avatar-rgb-trns.png").to_vec(),
            include_bytes!("../tests/fixtures/avatar-gray-alpha.png").to_vec(),
        ];
        for source in [png.into_inner(), webp].into_iter().chain(fixtures) {
            let processed = process(&source, Kind::Avatar, None).unwrap();
            assert_eq!(processed.variants.len(), 3);
            for v in &processed.variants {
                assert_eq!(
                    alpha_at(&v.bytes, 0, 0),
                    0,
                    "{} corner is transparent",
                    v.key
                );
                assert_eq!(
                    alpha_at(&v.bytes, v.width - 1, v.height - 1),
                    0,
                    "{}",
                    v.key
                );
                assert_eq!(
                    alpha_at(&v.bytes, v.width / 2, v.height / 2),
                    255,
                    "{} center is opaque",
                    v.key
                );
            }
        }
        // An opaque JPEG fills the whole square: no white matte or padding is added.
        let rgb = image::RgbImage::from_fn(300, 200, |_, _| image::Rgb([20, 40, 160]));
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(rgb)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let processed = process(&jpeg.into_inner(), Kind::Avatar, None).unwrap();
        for v in &processed.variants {
            let image = image::load_from_memory_with_format(&v.bytes, image::ImageFormat::WebP)
                .unwrap()
                .to_rgba8();
            for (x, y) in [
                (0, 0),
                (v.width - 1, 0),
                (0, v.height - 1),
                (v.width - 1, v.height - 1),
            ] {
                let p = image.get_pixel(x, y);
                assert_eq!(p[3], 255);
                assert!(
                    p[2] > 120 && p[0] < 60,
                    "{} edge keeps the image colour",
                    v.key
                );
            }
        }
    }

    #[test]
    fn prefixes_and_object_urls() {
        for ok in ["", "v2/", "media/v2/"] {
            assert!(valid_prefix(ok), "{ok}");
        }
        for bad in ["v2", "/v2/", "V2/", "../", "a//../", "v2 /"] {
            assert!(!valid_prefix(bad), "{bad}");
        }
        let mut s3 = S3 {
            endpoint: "https://account.r2.cloudflarestorage.com".into(),
            bucket: "sver".into(),
            region: "auto".into(),
            access_key: "k".into(),
            secret_key: "s".into(),
            prefix: String::new(),
        };
        assert_eq!(
            s3.object_url("avatars/abc/64.webp"),
            "https://account.r2.cloudflarestorage.com/sver/avatars/abc/64.webp"
        );
        s3.prefix = "v2/".into();
        assert_eq!(
            s3.object_url("avatars/abc/64.webp"),
            "https://account.r2.cloudflarestorage.com/sver/v2/avatars/abc/64.webp"
        );
    }
}
