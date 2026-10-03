//! Legacy profile import (docs/PROFILES.md, "Legacy profile import"): one-time and insert-only.
//! Reads the private export and media snapshot, writes profiles, follows, War Council, links and
//! the Wall for the already-imported accounts, verifies everything inside the transaction and
//! never modifies `users`, `identities` or `legacy_account_data`. Output is aggregate only.
// Row tuples from runtime sqlx queries read more clearly inline than as aliases.
#![allow(clippy::type_complexity)]
use crate::{
    App,
    media::{self, Kind},
    profiles::new_id,
    social, studio, text,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

pub const RUN_NAME: &str = "legacy-profiles";
/// Stored in `profiles.song_notice` for the legacy Spotify record (shown to its owner only).
pub const SPOTIFY_NOTICE: &str = "spotify";

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegacyFollow {
    pub follower_id: String,
    pub following_id: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTop8 {
    pub user_id: String,
    pub target_user_id: String,
    pub position: i32,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegacyLink {
    pub user_id: String,
    pub platform: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegacyPost {
    pub id: String,
    pub author_id: String,
    pub wall_owner_user_id: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub moderation_status: String,
    pub moderated_at: Option<DateTime<Utc>>,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReply {
    pub id: String,
    pub post_id: String,
    pub author_id: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReaction {
    pub post_id: String,
    pub user_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub created_at: DateTime<Utc>,
}
/// The private export, keyed by legacy model name (see `scripts/export-legacy-profiles.sql`).
#[derive(Deserialize, Clone, Default)]
pub struct Export {
    #[serde(rename = "Follow")]
    pub follows: Vec<LegacyFollow>,
    #[serde(rename = "ProfileTop8")]
    pub war_council: Vec<LegacyTop8>,
    #[serde(rename = "SocialLink")]
    pub links: Vec<LegacyLink>,
    #[serde(rename = "WallPost")]
    pub posts: Vec<LegacyPost>,
    #[serde(rename = "WallReply")]
    pub replies: Vec<LegacyReply>,
    #[serde(rename = "WallReaction")]
    pub reactions: Vec<LegacyReaction>,
}

/// One snapshot media file, already checked against the snapshot checksum.
#[derive(Clone)]
pub struct MediaFile {
    pub profile_id: String,
    pub kind: String,
    pub url: String,
    pub bytes: Vec<u8>,
}
#[derive(Deserialize)]
struct ManifestEntry {
    profile_id: String,
    kind: String,
    url: String,
    file: String,
    sha256: String,
}
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// Loads the media manifest and files from an extracted snapshot directory, verifying each file
/// against the manifest checksum and, when present, the snapshot's `checksums.json`.
pub fn load_media(dir: &Path) -> Result<Vec<MediaFile>, String> {
    let dir = dir
        .canonicalize()
        .map_err(|_| "Media snapshot unavailable")?;
    let manifest: Vec<ManifestEntry> = serde_json::from_slice(
        &std::fs::read(dir.join("media-manifest.json")).map_err(|_| "Media manifest unreadable")?,
    )
    .map_err(|_| "Invalid media manifest")?;
    let checksums: Option<HashMap<String, String>> = std::fs::read(dir.join("checksums.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let mut files = Vec::new();
    for entry in manifest {
        let candidates = [dir.join(&entry.file), dir.join("media").join(&entry.file)];
        let path = candidates
            .iter()
            .find_map(|p| p.canonicalize().ok().filter(|p| p.is_file()))
            .ok_or("A media file listed in the manifest is missing")?;
        if !path.starts_with(&dir) {
            return Err("A media file path escapes the snapshot directory".into());
        }
        let bytes = std::fs::read(&path).map_err(|_| "Media file unreadable")?;
        let digest = sha256_hex(&bytes);
        if !digest.eq_ignore_ascii_case(&entry.sha256) {
            return Err("Media checksum mismatch; nothing was imported".into());
        }
        if let Some(sums) = &checksums {
            let relative = path
                .strip_prefix(&dir)
                .map_err(|_| "Media path error")?
                .to_string_lossy()
                .replace('\\', "/");
            if sums
                .get(&relative)
                .is_some_and(|s| !s.eq_ignore_ascii_case(&digest))
            {
                return Err("Media snapshot checksum mismatch; nothing was imported".into());
            }
        }
        if !matches!(entry.kind.as_str(), "avatar" | "banner") {
            return Err("Unexpected media kind in manifest".into());
        }
        files.push(MediaFile {
            profile_id: entry.profile_id,
            kind: entry.kind,
            url: entry.url,
            bytes,
        });
    }
    Ok(files)
}

/// Test-only fault injection used to prove that a verification mismatch rolls back.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fault {
    FollowMismatch,
}
#[derive(Clone, Default)]
pub struct Options {
    pub commit: bool,
    pub fault: Option<Fault>,
    /// Rehearsal-only preview: continue past the internal-account stop so the remaining counts
    /// can be reviewed. Live modes never set it.
    pub preview_extra_internal: bool,
    /// Operator decision (October 3, 2026): only admin, support and SVER are internal. Accounts
    /// carrying only the legacy system flag import as normal public profiles and are counted, so
    /// they can be hidden later. Allowed in every mode.
    pub named_internal_only: bool,
}
pub type Counts = BTreeMap<String, u64>;
pub struct Outcome {
    /// Aggregate counts only; safe to print.
    pub counts: Counts,
    /// Private per-row report (row identifiers and reasons). Never print it.
    pub dropped: Vec<Value>,
}
struct Tally {
    counts: Counts,
    dropped: Vec<Value>,
}
impl Tally {
    fn add(&mut self, key: &str) {
        *self.counts.entry(key.to_string()).or_default() += 1;
    }
    fn set(&mut self, key: &str, n: usize) {
        self.counts.insert(key.to_string(), n as u64);
    }
    fn drop(&mut self, category: &str, reason: &str, row: Value) {
        self.add(&format!("{category}.dropped.{reason}"));
        self.dropped
            .push(json!({"category": category, "reason": reason, "row": row}));
    }
}

fn micros(t: &DateTime<Utc>) -> i64 {
    t.timestamp_micros()
}
fn opt_micros(t: &Option<DateTime<Utc>>) -> Value {
    t.as_ref()
        .map(micros)
        .map(Value::from)
        .unwrap_or(Value::Null)
}
fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
}
fn string(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// Maps a legacy social platform name to the new list; unknown names become Website.
pub fn map_platform(legacy: &str) -> &'static str {
    let compact: String = legacy
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    match compact.as_str() {
        "twitch" => "twitch",
        "youtube" => "youtube",
        "kick" => "kick",
        "tiktok" => "tiktok",
        "instagram" => "instagram",
        "x" | "twitter" | "twitterx" => "x",
        "bluesky" | "bsky" => "bluesky",
        "discord" => "discord",
        "facebook" => "facebook",
        "patreon" => "patreon",
        "kofi" => "kofi",
        "fourthwall" | "4thwall" => "fourthwall",
        _ => "website",
    }
}
/// Applies the new link rules; `http://` is upgraded to `https://` on import only.
pub fn import_link(platform: &str, url: &str) -> Option<String> {
    let trimmed = url.trim();
    let upgraded = match trimmed.get(..7) {
        Some(p) if p.eq_ignore_ascii_case("http://") => format!("https://{}", &trimmed[7..]),
        _ => trimmed.to_string(),
    };
    text::social_link(platform, &upgraded).ok()
}
fn who_can_post(settings: &Value) -> &'static str {
    match settings.get("whoCanPost") {
        None | Some(Value::Null) => "ANYONE",
        Some(v) => match v.as_str() {
            Some("ANYONE") => "ANYONE",
            Some("FOLLOWING") => "FOLLOWING",
            Some("MUTUAL") => "MUTUAL",
            // SUBSCRIBERS waits for subscriptions; legacy treated invalid values as NONE.
            _ => "NONE",
        },
    }
}

fn fail<E>(message: &'static str) -> impl Fn(E) -> String {
    move |_| message.to_string()
}

struct Account {
    id: String,
    username: String,
    account: Value,
    profile: Option<Value>,
}
const LOCKED: &str = "profiles, social_links, follows, war_council, wall_posts, wall_replies, wall_likes, media_objects, import_runs";

async fn digest(db: &mut PgConnection) -> Result<(String, String, String), String> {
    sqlx::query_as("SELECT (SELECT md5(coalesce(string_agg(t::text, '|' ORDER BY t.id COLLATE \"C\"), '')) FROM users t), (SELECT md5(coalesce(string_agg(t::text, '|' ORDER BY t.provider COLLATE \"C\", t.subject COLLATE \"C\"), '')) FROM identities t), (SELECT md5(coalesce(string_agg(t::text, '|' ORDER BY t.user_id COLLATE \"C\"), '')) FROM legacy_account_data t)")
        .fetch_one(&mut *db)
        .await
        .map_err(|_| "Preservation digest failed".to_string())
}

/// Runs the import in one transaction. Uploaded object keys are appended to `uploaded` as they
/// are written, so the caller can remove (check, rehearsal) or queue (failed apply) them.
pub async fn run(
    app: &App,
    export: &Export,
    media_files: &[MediaFile],
    options: &Options,
    uploaded: &mut Vec<String>,
) -> Result<Outcome, String> {
    let mut tally = Tally {
        counts: Counts::new(),
        dropped: Vec::new(),
    };
    let mut tx = app
        .db
        .begin()
        .await
        .map_err(fail("Import transaction failed"))?;
    sqlx::query("SET LOCAL lock_timeout='10s'")
        .execute(&mut *tx)
        .await
        .map_err(fail("Could not set import lock timeout"))?;
    for statement in [
        "LOCK TABLE users, identities, legacy_account_data IN SHARE MODE".to_string(),
        format!("LOCK TABLE {LOCKED} IN SHARE ROW EXCLUSIVE MODE"),
    ] {
        sqlx::query(&statement)
            .execute(&mut *tx)
            .await
            .map_err(fail(
                "Could not lock import tables; retry when writes settle",
            ))?;
    }
    let already: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM import_runs WHERE name=$1)")
            .bind(RUN_NAME)
            .fetch_one(&mut *tx)
            .await
            .map_err(fail("Import history check failed"))?;
    if already {
        return Err("Legacy profiles were already imported; refusing a second run".into());
    }
    let rows: Vec<(String, String, Value, Option<Value>)> = sqlx::query_as("SELECT d.user_id,u.username,d.account,d.profile FROM legacy_account_data d JOIN users u ON u.id=d.user_id ORDER BY d.user_id COLLATE \"C\"")
        .fetch_all(&mut *tx)
        .await
        .map_err(fail("Imported-account read failed"))?;
    if rows.is_empty() {
        return Err("No imported accounts found; the account import must run first".into());
    }
    let accounts: Vec<Account> = rows
        .into_iter()
        .map(|(id, username, account, profile)| Account {
            id,
            username,
            account,
            profile: profile.filter(|p| p.is_object()),
        })
        .collect();
    let ids: Vec<String> = accounts.iter().map(|a| a.id.clone()).collect();
    let known: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let before = digest(&mut tx).await?;
    let occupied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM profiles WHERE user_id=ANY($1)) OR EXISTS(SELECT 1 FROM follows WHERE follower_id=ANY($1) OR following_id=ANY($1)) OR EXISTS(SELECT 1 FROM war_council WHERE user_id=ANY($1) OR member_id=ANY($1)) OR EXISTS(SELECT 1 FROM social_links WHERE user_id=ANY($1)) OR EXISTS(SELECT 1 FROM wall_posts WHERE wall_owner_id=ANY($1) OR author_id=ANY($1)) OR EXISTS(SELECT 1 FROM wall_replies WHERE author_id=ANY($1)) OR EXISTS(SELECT 1 FROM wall_likes WHERE user_id=ANY($1))")
        .bind(&ids)
        .fetch_one(&mut *tx)
        .await
        .map_err(fail("Existing-row check failed"))?;
    if occupied {
        return Err(
            "Profile, follow, War Council, link or Wall rows already exist for imported accounts; nothing was imported".into(),
        );
    }
    tally.set("accounts.read", accounts.len());
    tally.set(
        "accounts.without_legacy_profile",
        accounts.iter().filter(|a| a.profile.is_none()).count(),
    );

    // ------------------------------------------------------------------ Profiles
    let named = |a: &Account| {
        matches!(
            a.username.to_ascii_lowercase().as_str(),
            "admin" | "support" | "sver"
        )
    };
    let flagged_only = accounts
        .iter()
        .filter(|a| !named(a) && a.account["isSystemAccount"] == true)
        .count();
    let internal: HashSet<&str> = accounts
        .iter()
        .filter(|a| {
            named(a) || (!options.named_internal_only && a.account["isSystemAccount"] == true)
        })
        .map(|a| a.id.as_str())
        .collect();
    tally.set("accounts.internal", internal.len());
    if options.named_internal_only {
        tally.set("accounts.internal_added_by_system_flag", 0);
        tally.set("accounts.system_flag_imported_as_public", flagged_only);
    } else {
        tally.set("accounts.internal_added_by_system_flag", flagged_only);
    }
    // Decision 3: the legacy system-account flag may not add internal accounts beyond the
    // three named ones (and the set may never exceed three).
    let added = tally.counts["accounts.internal_added_by_system_flag"];
    if added > 0 || internal.len() > 3 {
        if !options.preview_extra_internal {
            return Err(format!(
                "The legacy system-account flag adds {added} internal accounts beyond admin, support and SVER ({} internal in total); the import stops for review",
                internal.len()
            ));
        }
        tally.set("accounts.internal_stop_bypassed_for_rehearsal_preview", 1);
    }
    let mut expected_profiles = Vec::new();
    let mut profile_owner: HashMap<String, String> = HashMap::new();
    let mut pins: HashMap<String, Vec<String>> = HashMap::new();
    for a in &accounts {
        let empty = json!({});
        let p = a.profile.as_ref().unwrap_or(&empty);
        if let Some(pid) = p["id"].as_str() {
            profile_owner.insert(pid.to_string(), a.id.clone());
        }
        let display = [&p["displayName"], &a.account["displayName"]]
            .iter()
            .filter_map(|v| v.as_str())
            .find(|s| !s.trim().is_empty())
            .unwrap_or(a.username.as_str())
            .to_string();
        if text::display_name(&display, &a.username).is_err() {
            tally.add("profiles.display_name_fails_new_rules");
        }
        let bio = string(&p["bio"]);
        if !bio.is_empty() && text::plain(&bio, "bio", 0, 300, 4, true).is_err() {
            tally.add("profiles.bio_fails_new_rules");
        }
        let mood_raw = string(&p["moodEmoji"]);
        let mood = if mood_raw.is_empty() || text::valid_mood(&mood_raw) {
            mood_raw
        } else {
            tally.add("profiles.mood_emptied");
            String::new()
        };
        let status_raw = string(&p["status"]);
        let status = if status_raw.is_empty() {
            status_raw
        } else {
            match text::plain(&status_raw, "status", 0, 80, 0, true) {
                Ok(v) => v,
                Err(_) => {
                    tally.add("profiles.status_emptied");
                    String::new()
                }
            }
        };
        let settings = &p["wallSettings"];
        let who = who_can_post(settings);
        let flag = |k: &str| settings.get(k).and_then(Value::as_bool).unwrap_or(false);
        let mut song: (Option<&str>, Option<String>, Option<String>) = (None, None, None);
        let mut notice: Option<&str> = None;
        let song_url = string(&p["profileSongUrl"]);
        if !song_url.trim().is_empty() {
            let host = url::Url::parse(song_url.trim())
                .ok()
                .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
                .unwrap_or_default();
            if host == "spotify.com" || host.ends_with(".spotify.com") || host == "spotify.link" {
                notice = Some(SPOTIFY_NOTICE);
                tally.add("songs.spotify_notice");
            } else {
                match studio::parse_song_url(&song_url) {
                    Ok(s) => {
                        tally.add(&format!("songs.imported.{}", s.provider));
                        // SoundCloud track IDs (and every thumbnail) come from the post-import job.
                        song = (
                            Some(s.provider),
                            Some(s.media_id.unwrap_or_default()),
                            Some(s.url),
                        );
                    }
                    Err(_) => tally.drop("songs", "unsupported_url", json!({"user_id": a.id})),
                }
            }
        }
        let title = song.0.and_then(|_| {
            p["profileSongTitle"]
                .as_str()
                .map(|s| s.trim().chars().take(100).collect::<String>())
                .filter(|s| !s.is_empty())
        });
        let artist = song.0.and_then(|_| {
            p["profileSongArtist"]
                .as_str()
                .map(|s| s.trim().chars().take(100).collect::<String>())
                .filter(|s| !s.is_empty())
        });
        let volume = p["profileSongDefaultVolume"]
            .as_i64()
            .unwrap_or(70)
            .clamp(0, 100) as i32;
        let created = parse_time(&p["createdAt"])
            .or_else(|| parse_time(&a.account["createdAt"]))
            .unwrap_or_else(Utc::now);
        let is_internal = internal.contains(a.id.as_str());
        sqlx::query("INSERT INTO profiles(user_id,display_name,bio,mood_emoji,status_text,internal,who_can_post,require_approval,hold_links,hold_new_accounts,song_provider,song_media_id,song_url,song_title,song_artist,song_volume,song_notice,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)")
            .bind(&a.id).bind(&display).bind(&bio).bind(&mood).bind(&status).bind(is_internal)
            .bind(who).bind(flag("requireApproval")).bind(flag("autoHideLinks")).bind(flag("autoHideNewAccounts"))
            .bind(song.0).bind(&song.1).bind(&song.2).bind(&title).bind(&artist).bind(volume).bind(notice).bind(created)
            .execute(&mut *tx).await.map_err(fail("Profile insert failed; import rolled back"))?;
        tally.add("profiles.imported");
        if let Some(list) = p["pinnedWallPostIds"].as_array() {
            pins.insert(
                a.id.clone(),
                list.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
            );
        }
        expected_profiles.push(json!({
            "user_id": a.id, "display_name": display, "bio": bio, "mood_emoji": mood,
            "status_text": status, "internal": is_internal, "who_can_post": who,
            "require_approval": flag("requireApproval"), "hold_links": flag("autoHideLinks"),
            "hold_new_accounts": flag("autoHideNewAccounts"), "song_provider": song.0,
            "song_media_id": song.1, "song_url": song.2, "song_title": title, "song_artist": artist,
            "song_volume": volume, "song_notice": notice, "avatar_key": Value::Null, "banner_key": Value::Null,
        }));
    }

    // ------------------------------------------------------------------ Follows
    tally.set("follows.read", export.follows.len());
    let mut follows: Vec<(String, String, i64)> = Vec::new();
    let mut seen = HashSet::new();
    let mut follows_sorted = export.follows.clone();
    follows_sorted.sort_by_key(|f| f.created_at);
    for f in &follows_sorted {
        let row = json!({"follower_id": f.follower_id, "following_id": f.following_id});
        if f.follower_id == f.following_id {
            tally.drop("follows", "self", row);
        } else if !known.contains(f.follower_id.as_str())
            || !known.contains(f.following_id.as_str())
        {
            tally.drop("follows", "unknown_user", row);
        } else if !seen.insert((f.follower_id.clone(), f.following_id.clone())) {
            tally.drop("follows", "duplicate", row);
        } else {
            sqlx::query(
                "INSERT INTO follows(follower_id,following_id,created_at) VALUES($1,$2,$3)",
            )
            .bind(&f.follower_id)
            .bind(&f.following_id)
            .bind(f.created_at)
            .execute(&mut *tx)
            .await
            .map_err(fail("Follow insert failed; import rolled back"))?;
            follows.push((
                f.follower_id.clone(),
                f.following_id.clone(),
                micros(&f.created_at),
            ));
        }
    }
    tally.set("follows.imported", follows.len());
    tally.set(
        "follows.touching_internal_accounts",
        follows
            .iter()
            .filter(|(a, b, _)| internal.contains(a.as_str()) || internal.contains(b.as_str()))
            .count(),
    );

    // ------------------------------------------------------------------ War Council
    tally.set("war_council.read", export.war_council.len());
    let mut council: BTreeMap<&str, Vec<&LegacyTop8>> = BTreeMap::new();
    for pick in &export.war_council {
        council.entry(pick.user_id.as_str()).or_default().push(pick);
    }
    let mut expected_council: Vec<(String, i32, String)> = Vec::new();
    for (owner, mut picks) in council {
        picks.sort_by_key(|p| p.position);
        let mut members = HashSet::new();
        let mut position = 0;
        for p in picks {
            let row = json!({"user_id": p.user_id, "target_user_id": p.target_user_id});
            if p.user_id == p.target_user_id {
                tally.drop("war_council", "self", row);
            } else if !known.contains(owner) || !known.contains(p.target_user_id.as_str()) {
                tally.drop("war_council", "unknown_user", row);
            } else if !members.insert(p.target_user_id.as_str()) {
                tally.drop("war_council", "duplicate", row);
            } else if position == 8 {
                tally.drop("war_council", "over_limit", row);
            } else {
                position += 1;
                expected_council.push((owner.to_string(), position, p.target_user_id.clone()));
            }
        }
    }
    for (owner, position, member) in &expected_council {
        sqlx::query("INSERT INTO war_council(user_id,position,member_id) VALUES($1,$2,$3)")
            .bind(owner)
            .bind(position)
            .bind(member)
            .execute(&mut *tx)
            .await
            .map_err(fail("War Council insert failed; import rolled back"))?;
    }
    tally.set("war_council.imported", expected_council.len());

    // ------------------------------------------------------------------ Social links
    tally.set("links.read", export.links.len());
    let mut by_user: BTreeMap<&str, Vec<&LegacyLink>> = BTreeMap::new();
    for l in &export.links {
        by_user.entry(l.user_id.as_str()).or_default().push(l);
    }
    let mut expected_links: Vec<(String, i32, String, String, i64)> = Vec::new();
    for (owner, mut list) in by_user {
        list.sort_by_key(|l| l.created_at);
        let mut kept: Vec<(&'static str, String, &LegacyLink)> = Vec::new();
        for l in list {
            let row = json!({"user_id": l.user_id, "created_at": l.created_at});
            if !known.contains(owner) {
                tally.drop("links", "unknown_user", row);
                continue;
            }
            let platform = map_platform(&l.platform);
            let named: String = l
                .platform
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect();
            if platform == "website" && !named.eq_ignore_ascii_case("website") {
                tally.add("links.unknown_platform_as_website");
            }
            let Some(url) = import_link(platform, &l.url) else {
                tally.drop("links", "invalid_url", row);
                continue;
            };
            if l.url.trim().to_ascii_lowercase().starts_with("http://") {
                tally.add("links.upgraded_to_https");
            }
            let same = kept.iter().filter(|(p, _, _)| *p == platform).count();
            if same >= if platform == "website" { 2 } else { 1 } {
                tally.drop("links", "platform_limit", row);
                continue;
            }
            if kept.len() == 5 {
                tally.drop("links", "over_limit", row);
                continue;
            }
            kept.push((platform, url, l));
        }
        for (i, (platform, url, l)) in kept.into_iter().enumerate() {
            let position = i as i32 + 1;
            sqlx::query("INSERT INTO social_links(id,user_id,position,platform,url,created_at) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(new_id()).bind(owner).bind(position).bind(platform).bind(&url).bind(l.created_at)
                .execute(&mut *tx).await.map_err(fail("Link insert failed; import rolled back"))?;
            expected_links.push((
                owner.to_string(),
                position,
                platform.to_string(),
                url,
                micros(&l.created_at),
            ));
        }
    }
    tally.set("links.imported", expected_links.len());

    // ------------------------------------------------------------------ Wall
    tally.set("wall_posts.read", export.posts.len());
    let mut posts: BTreeMap<String, &LegacyPost> = BTreeMap::new();
    for p in &export.posts {
        let row = json!({"id": p.id});
        if !known.contains(p.author_id.as_str()) || !known.contains(p.wall_owner_user_id.as_str()) {
            tally.drop("wall_posts", "unknown_user", row);
        } else if !matches!(
            p.moderation_status.as_str(),
            "APPROVED" | "PENDING" | "REJECTED"
        ) {
            tally.drop("wall_posts", "unknown_status", row);
        } else if posts.contains_key(&p.id) {
            tally.drop("wall_posts", "duplicate", row);
        } else {
            posts.insert(p.id.clone(), p);
        }
    }
    let mut pinned: HashMap<String, i32> = HashMap::new();
    for (owner, list) in &pins {
        let mut position = 0;
        let mut used = HashSet::new();
        for id in list {
            if position == 3 {
                break;
            }
            let Some(p) = posts.get(id) else { continue };
            if &p.wall_owner_user_id == owner
                && p.moderation_status == "APPROVED"
                && p.deleted_at.is_none()
                && used.insert(id.clone())
            {
                position += 1;
                pinned.insert(id.clone(), position);
            }
        }
    }
    tally.set("wall_posts.pinned", pinned.len());
    for p in posts.values() {
        sqlx::query("INSERT INTO wall_posts(id,wall_owner_id,author_id,body,status,pinned_position,created_at,deleted_at,moderated_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(&p.id).bind(&p.wall_owner_user_id).bind(&p.author_id).bind(&p.body).bind(&p.moderation_status)
            .bind(pinned.get(&p.id)).bind(p.created_at).bind(p.deleted_at).bind(p.moderated_at)
            .execute(&mut *tx).await.map_err(fail("Wall post insert failed; import rolled back"))?;
    }
    tally.set("wall_posts.imported", posts.len());
    tally.set(
        "wall_posts.imported_soft_deleted",
        posts.values().filter(|p| p.deleted_at.is_some()).count(),
    );
    for status in ["APPROVED", "PENDING", "REJECTED"] {
        tally.set(
            &format!("wall_posts.imported_{}", status.to_ascii_lowercase()),
            posts
                .values()
                .filter(|p| p.moderation_status == status)
                .count(),
        );
    }
    tally.set("wall_replies.read", export.replies.len());
    let mut replies: BTreeMap<String, &LegacyReply> = BTreeMap::new();
    for r in &export.replies {
        let row = json!({"id": r.id});
        if !posts.contains_key(&r.post_id) {
            tally.drop("wall_replies", "missing_post", row);
        } else if !known.contains(r.author_id.as_str()) {
            tally.drop("wall_replies", "unknown_user", row);
        } else if replies.contains_key(&r.id) {
            tally.drop("wall_replies", "duplicate", row);
        } else {
            replies.insert(r.id.clone(), r);
        }
    }
    for r in replies.values() {
        sqlx::query("INSERT INTO wall_replies(id,post_id,author_id,body,status,created_at,deleted_at) VALUES($1,$2,$3,$4,'APPROVED',$5,$6)")
            .bind(&r.id).bind(&r.post_id).bind(&r.author_id).bind(&r.body).bind(r.created_at).bind(r.deleted_at)
            .execute(&mut *tx).await.map_err(fail("Wall reply insert failed; import rolled back"))?;
    }
    tally.set("wall_replies.imported", replies.len());
    tally.set("wall_reactions.read", export.reactions.len());
    let mut reactions = export.reactions.clone();
    reactions.sort_by_key(|r| r.created_at);
    let mut likes: BTreeMap<(String, String), i64> = BTreeMap::new();
    let mut liked: HashSet<(String, String)> = HashSet::new();
    for r in &reactions {
        let row = json!({"post_id": r.post_id, "user_id": r.user_id});
        if !posts.contains_key(&r.post_id) {
            tally.drop("wall_reactions", "missing_post", row);
        } else if !known.contains(r.user_id.as_str()) {
            tally.drop("wall_reactions", "unknown_user", row);
        } else if !matches!(r.kind.as_str(), "LIKE" | "VALOR") {
            tally.drop("wall_reactions", "unknown_type", row);
        } else if !liked.insert((r.post_id.clone(), r.user_id.clone())) {
            tally.drop("wall_reactions", "duplicate", row);
        } else {
            if r.kind == "VALOR" {
                tally.add("wall_reactions.valor_as_like");
            }
            likes.insert(
                (r.post_id.clone(), r.user_id.clone()),
                micros(&r.created_at),
            );
            sqlx::query("INSERT INTO wall_likes(post_id,user_id,created_at) VALUES($1,$2,$3)")
                .bind(&r.post_id)
                .bind(&r.user_id)
                .bind(r.created_at)
                .execute(&mut *tx)
                .await
                .map_err(fail("Wall like insert failed; import rolled back"))?;
        }
    }
    tally.set("wall_reactions.imported_as_likes", likes.len());

    // ------------------------------------------------------------------ Counts
    let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    social::refresh_counts(&mut tx, &id_refs)
        .await
        .map_err(fail("Follow count refresh failed"))?;

    // ------------------------------------------------------------------ Media
    tally.set("media.read", media_files.len());
    let mut stored_variants: Vec<(String, u64, u32, u32)> = Vec::new();
    for file in media_files {
        let Some(owner) = profile_owner.get(&file.profile_id) else {
            tally.drop("media", "unknown_profile", json!({"kind": file.kind}));
            continue;
        };
        let profile = accounts
            .iter()
            .find(|a| &a.id == owner)
            .and_then(|a| a.profile.as_ref());
        let (kind, field, column) = if file.kind == "avatar" {
            (Kind::Avatar, "avatarUrl", "avatar_key")
        } else {
            (Kind::Banner, "bannerUrl", "banner_key")
        };
        if profile.and_then(|p| p[field].as_str()) != Some(file.url.as_str()) {
            tally.drop(
                "media",
                "not_current_image",
                json!({"user_id": owner, "kind": file.kind}),
            );
            continue;
        }
        let mut result = media::process_async(file.bytes.clone(), kind, None).await;
        if kind == Kind::Banner
            && result
                .as_ref()
                .is_err_and(|e| e.message.contains("too small"))
        {
            // Import-only fallback: upscale the centered 3:1 crop to the 1200x400 minimum.
            let bytes = file.bytes.clone();
            let upscaled =
                tokio::task::spawn_blocking(move || media::upscale_banner_to_minimum(&bytes))
                    .await
                    .map_err(fail("Banner fallback failed; import rolled back"))?;
            if let Ok(png) = upscaled {
                let fallback = media::process_async(png, kind, None).await;
                if fallback.is_ok() {
                    tally.add("media.banner_upscaled_to_minimum");
                    result = fallback;
                }
            }
        }
        let processed = match result {
            Ok(p) => p,
            Err(e) => {
                // The normal pipeline applies: a crop below the minimum size is "too small".
                let reason = if e.message.contains("too small") {
                    "too_small_after_crop"
                } else {
                    "decode_failed"
                };
                tally.drop(
                    "media",
                    reason,
                    json!({"user_id": owner, "kind": file.kind}),
                );
                continue;
            }
        };
        for v in &processed.variants {
            app.config
                .media
                .storage
                .put(&app.http, &v.key, v.bytes.clone())
                .await
                .map_err(fail("Media upload failed; import rolled back"))?;
            uploaded.push(v.key.clone());
            stored_variants.push((v.key.clone(), v.bytes.len() as u64, v.width, v.height));
        }
        media::record(&mut tx, owner, kind, &processed)
            .await
            .map_err(fail("Media record failed; import rolled back"))?;
        sqlx::query(&format!("UPDATE profiles SET {column}=$2 WHERE user_id=$1"))
            .bind(owner)
            .bind(&processed.stored)
            .execute(&mut *tx)
            .await
            .map_err(fail("Media key update failed; import rolled back"))?;
        if let Some(e) = expected_profiles
            .iter_mut()
            .find(|e| e["user_id"] == owner.as_str())
        {
            e[column] = json!(processed.stored);
        }
        tally.add(&format!("media.processed_{}", file.kind));
    }

    if options.fault == Some(Fault::FollowMismatch) {
        sqlx::query("UPDATE follows SET created_at=created_at+interval '1 second' WHERE (follower_id,following_id)=(SELECT follower_id,following_id FROM follows WHERE follower_id=ANY($1) LIMIT 1)")
            .bind(&ids)
            .execute(&mut *tx)
            .await
            .map_err(fail("Fault injection failed"))?;
    }

    // ------------------------------------------------------------------ Verification
    let mismatch = |what: &str| format!("{what} comparison failed; import rolled back");
    let stored: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(jsonb_build_object('user_id',user_id,'display_name',display_name,'bio',bio,'mood_emoji',mood_emoji,'status_text',status_text,'internal',internal,'who_can_post',who_can_post,'require_approval',require_approval,'hold_links',hold_links,'hold_new_accounts',hold_new_accounts,'song_provider',song_provider,'song_media_id',song_media_id,'song_url',song_url,'song_title',song_title,'song_artist',song_artist,'song_volume',song_volume,'song_notice',song_notice,'avatar_key',avatar_key,'banner_key',banner_key) ORDER BY user_id COLLATE \"C\"),'[]'::jsonb) FROM profiles WHERE user_id=ANY($1)")
        .bind(&ids).fetch_one(&mut *tx).await.map_err(fail("Profile read-back failed"))?;
    if stored != Value::Array(expected_profiles.clone()) {
        return Err(mismatch("Profile"));
    }
    let mut want_follows = follows.clone();
    want_follows.sort();
    let got_follows: Vec<(String, String, i64)> = sqlx::query_as("SELECT follower_id,following_id,(extract(epoch FROM created_at)*1000000)::bigint FROM follows WHERE follower_id=ANY($1) OR following_id=ANY($1) ORDER BY follower_id COLLATE \"C\",following_id COLLATE \"C\"")
        .bind(&ids).fetch_all(&mut *tx).await.map_err(fail("Follow read-back failed"))?;
    if got_follows != want_follows {
        return Err(mismatch("Follow"));
    }
    let mut want_council = expected_council.clone();
    want_council.sort();
    let got_council: Vec<(String, i32, String)> = sqlx::query_as("SELECT user_id,position,member_id FROM war_council WHERE user_id=ANY($1) OR member_id=ANY($1) ORDER BY user_id COLLATE \"C\",position")
        .bind(&ids).fetch_all(&mut *tx).await.map_err(fail("War Council read-back failed"))?;
    if got_council != want_council {
        return Err(mismatch("War Council"));
    }
    let mut want_links = expected_links.clone();
    want_links.sort();
    let got_links: Vec<(String, i32, String, String, i64)> = sqlx::query_as("SELECT user_id,position,platform,url,(extract(epoch FROM created_at)*1000000)::bigint FROM social_links WHERE user_id=ANY($1) ORDER BY user_id COLLATE \"C\",position")
        .bind(&ids).fetch_all(&mut *tx).await.map_err(fail("Link read-back failed"))?;
    if got_links != want_links {
        return Err(mismatch("Link"));
    }
    let want_posts: Vec<Value> = posts
        .values()
        .map(|p| {
            json!([
                p.id,
                p.wall_owner_user_id,
                p.author_id,
                p.body,
                p.moderation_status,
                pinned.get(&p.id),
                micros(&p.created_at),
                opt_micros(&p.deleted_at),
                opt_micros(&p.moderated_at)
            ])
        })
        .collect();
    let got_posts: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(jsonb_build_array(id,wall_owner_id,author_id,body,status,pinned_position,(extract(epoch FROM created_at)*1000000)::bigint,(extract(epoch FROM deleted_at)*1000000)::bigint,(extract(epoch FROM moderated_at)*1000000)::bigint) ORDER BY id COLLATE \"C\"),'[]'::jsonb) FROM wall_posts WHERE wall_owner_id=ANY($1) OR author_id=ANY($1)")
        .bind(&ids).fetch_one(&mut *tx).await.map_err(fail("Wall post read-back failed"))?;
    if got_posts != Value::Array(want_posts) {
        return Err(mismatch("Wall post"));
    }
    let want_replies: Vec<Value> = replies
        .values()
        .map(|r| {
            json!([
                r.id,
                r.post_id,
                r.author_id,
                r.body,
                "APPROVED",
                micros(&r.created_at),
                opt_micros(&r.deleted_at)
            ])
        })
        .collect();
    let got_replies: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(jsonb_build_array(r.id,r.post_id,r.author_id,r.body,r.status,(extract(epoch FROM r.created_at)*1000000)::bigint,(extract(epoch FROM r.deleted_at)*1000000)::bigint) ORDER BY r.id COLLATE \"C\"),'[]'::jsonb) FROM wall_replies r JOIN wall_posts p ON p.id=r.post_id WHERE p.wall_owner_id=ANY($1) OR r.author_id=ANY($1)")
        .bind(&ids).fetch_one(&mut *tx).await.map_err(fail("Wall reply read-back failed"))?;
    if got_replies != Value::Array(want_replies) {
        return Err(mismatch("Wall reply"));
    }
    let got_likes: Vec<(String, String, i64)> = sqlx::query_as("SELECT l.post_id,l.user_id,(extract(epoch FROM l.created_at)*1000000)::bigint FROM wall_likes l JOIN wall_posts p ON p.id=l.post_id WHERE p.wall_owner_id=ANY($1) OR l.user_id=ANY($1) ORDER BY l.post_id COLLATE \"C\",l.user_id COLLATE \"C\"")
        .bind(&ids).fetch_all(&mut *tx).await.map_err(fail("Wall like read-back failed"))?;
    let want_likes: Vec<(String, String, i64)> = likes
        .iter()
        .map(|((p, u), t)| (p.clone(), u.clone(), *t))
        .collect();
    if got_likes != want_likes {
        return Err(mismatch("Wall like"));
    }
    let counts_ok: bool = sqlx::query_scalar("SELECT coalesce(bool_and(follower_count=(SELECT count(*) FROM follows f WHERE f.following_id=p.user_id) AND following_count=(SELECT count(*) FROM follows f WHERE f.follower_id=p.user_id)),false) FROM profiles p WHERE user_id=ANY($1)")
        .bind(&ids).fetch_one(&mut *tx).await.map_err(fail("Count check failed"))?;
    if !counts_ok {
        return Err(mismatch("Follow count"));
    }
    for (key, bytes, width, height) in &stored_variants {
        let recorded: Option<(i64, Option<i32>, Option<i32>)> =
            sqlx::query_as("SELECT bytes,width,height FROM media_objects WHERE key=$1")
                .bind(key)
                .fetch_optional(&mut *tx)
                .await
                .map_err(fail("Media record check failed"))?;
        let object = app
            .config
            .media
            .storage
            .get(&app.http, key)
            .await
            .map_err(fail("Stored media check failed"))?;
        let dims = object
            .as_ref()
            .and_then(|b| image::load_from_memory(b).ok())
            .map(|i| (i.width(), i.height()));
        if recorded != Some((*bytes as i64, Some(*width as i32), Some(*height as i32)))
            || object.as_ref().map(|b| b.len() as u64) != Some(*bytes)
            || dims != Some((*width, *height))
        {
            return Err(mismatch("Stored media"));
        }
    }
    tally.set("media.objects_verified", stored_variants.len());
    if digest(&mut tx).await? != before {
        return Err("Users, identities or legacy account data changed; import rolled back".into());
    }
    sqlx::query("INSERT INTO import_runs(name,counts) VALUES($1,$2)")
        .bind(RUN_NAME)
        .bind(json!(tally.counts))
        .execute(&mut *tx)
        .await
        .map_err(fail("Import run record failed; import rolled back"))?;
    if options.commit {
        tx.commit().await.map_err(fail(
            "Import commit failed; inspect the target before retrying",
        ))?;
    } else {
        tx.rollback()
            .await
            .map_err(fail("Import check rollback failed"))?;
    }
    Ok(Outcome {
        counts: tally.counts,
        dropped: tally.dropped,
    })
}

/// Queues objects written by a failed or rolled-back import for deletion by the media job.
pub async fn queue_orphans(app: &App, keys: &[String]) -> Result<(), String> {
    for key in keys {
        sqlx::query("INSERT INTO media_objects(key,kind,bytes,delete_after) VALUES($1,'import_orphan',0,now()) ON CONFLICT (key) DO UPDATE SET delete_after=now()")
            .bind(key)
            .execute(&app.db)
            .await
            .map_err(|_| "Could not queue uploaded media for deletion".to_string())?;
    }
    Ok(())
}

/// Post-import song job: fetches oEmbed metadata for imported songs (marked by a NULL
/// `song_updated_at`), at most one request per second. Fills the SoundCloud track ID and a
/// re-hosted thumbnail; a failure leaves no thumbnail and is not retried.
pub async fn song_job(app: &App, limit: i64) -> Result<usize, sqlx::Error> {
    let pending: Vec<(String, String, String, String)> = sqlx::query_as("SELECT user_id,song_provider,coalesce(song_media_id,''),song_url FROM profiles WHERE song_provider IS NOT NULL AND song_updated_at IS NULL ORDER BY user_id LIMIT $1")
        .bind(limit)
        .fetch_all(&app.db)
        .await?;
    let mut done = 0;
    for (i, (user_id, provider, media_id, url)) in pending.into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let song = studio::SongRef {
            provider: if provider == "youtube" {
                "youtube"
            } else {
                "soundcloud"
            },
            media_id: (!media_id.is_empty()).then_some(media_id),
            url,
        };
        match studio::oembed(app, &song).await {
            Ok(found) => {
                let thumb =
                    studio::copy_thumbnail(app, &user_id, found.thumbnail_url.as_deref()).await;
                sqlx::query("UPDATE profiles SET song_media_id=CASE WHEN song_provider='soundcloud' THEN $2 ELSE song_media_id END,song_thumb_key=$3,song_title=coalesce(song_title,nullif($4,'')),song_artist=coalesce(song_artist,nullif($5,'')),song_updated_at=now() WHERE user_id=$1 AND song_updated_at IS NULL")
                    .bind(&user_id).bind(&found.media_id).bind(thumb).bind(&found.title).bind(&found.author)
                    .execute(&app.db).await?;
                eprintln!("profile_event=import_song outcome=ok");
            }
            Err(_) => {
                sqlx::query("UPDATE profiles SET song_updated_at=now() WHERE user_id=$1 AND song_updated_at IS NULL")
                    .bind(&user_id)
                    .execute(&app.db)
                    .await?;
                eprintln!("profile_event=import_song outcome=failed");
            }
        }
        done += 1;
    }
    Ok(done)
}
