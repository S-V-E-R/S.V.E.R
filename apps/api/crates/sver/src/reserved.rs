//! The single reserved-username list (docs/PROFILES.md, "Reserved names").
//!
//! Channel pages live at root `/{username}`, so every top-level web route, public file and
//! redirect source that could be a valid username must be listed here. `tests/reserved_routes.rs`
//! fails when one is missing. Matching is case-insensitive against the name and its
//! leet-compacted form; substring blocks and the abuse list apply to the compacted form.

/// Exact reserved names, lowercase ASCII. Entries shorter than three characters or starting with
/// an underscore cannot be claimed under the current format rules but are kept so the list
/// matches the specification and any future format change stays covered.
pub const RESERVED: &[&str] = &[
    // Current web and API routes.
    "login",
    "signup",
    "forgot",
    "reset",
    "verify",
    "mfa",
    "account",
    "welcome",
    "api",
    "_next",
    // Planned module routes.
    "settings",
    "studio",
    "admin",
    "following",
    "followers",
    "browse",
    "search",
    "category",
    "categories",
    "genre",
    "genres",
    "watch",
    "live",
    "stream",
    "streams",
    "factions",
    "faction",
    "territory",
    "territories",
    "magnet",
    "beacons",
    "beacon",
    "clips",
    "clip",
    "videos",
    "video",
    "vods",
    "vod",
    "notifications",
    "messages",
    "inbox",
    "logout",
    "help",
    "support",
    "terms",
    "privacy",
    "guidelines",
    "dmca",
    "about",
    "roadmap",
    "contact",
    "credits",
    "status",
    "report",
    "reports",
    "s",
    "u",
    "user",
    "users",
    "channel",
    "channels",
    "embed",
    "popout",
    "chat",
    "overlay",
    "auth",
    "oauth",
    "go",
    "static",
    "assets",
    "media",
    "cdn",
    "img",
    "images",
    "uploads",
    "public",
    "robots",
    "sitemap",
    "favicon",
    "manifest",
    "www",
    "mail",
    "email",
    "home",
    "discover",
    "dashboard",
    // Staff and brand names.
    "administrator",
    "moderator",
    "mod",
    "staff",
    "sver",
    "svertv",
    "official",
    "security",
    "system",
    "root",
    "null",
    "undefined",
    // Faction names.
    "aetheron",
    "myria",
    "glint",
    // Legacy top-level routes and public directories that can be valid usernames.
    "accessibility",
    "activity",
    "advertise",
    "appeals",
    "blog",
    "cart",
    "challenges",
    "checkout",
    "costream",
    "creator",
    "crowdsync",
    "dev",
    "dock",
    "drops",
    "emotes",
    "feed",
    "founder",
    "founders",
    "fund",
    "gifting",
    "goals",
    "guilds",
    "g",
    "squads",
    "hype",
    "icons",
    "leaderboards",
    "marketplace",
    "markets",
    "offline",
    "onboarding",
    "orders",
    "predictions",
    "presents",
    "remote",
    "shop",
    "squad",
    "store",
    "storefronts",
    "surge",
    "team",
    "transparency",
    "trust",
    "unsubscribe",
    "vanity",
    "wallet",
];

/// Blocked anywhere inside the compacted name (staff impersonation).
const BLOCKED_SUBSTRINGS: &[&str] = &["admin", "moderator", "support", "sverstaff", "sverofficial"];

/// Maintained abuse list, matched anywhere inside the compacted name.
pub const ABUSE: &[&str] = &[
    "nigger",
    "nigga",
    "faggot",
    "kike",
    "chink",
    "tranny",
    "1488",
    "heilhitler",
];

/// Lowercases, removes underscores and maps common digit substitutions to letters.
pub fn compact(name: &str) -> String {
    name.to_ascii_lowercase()
        .chars()
        .filter(|&c| c != '_')
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            _ => c,
        })
        .collect()
}

/// True for 3-25 ASCII letters, digits or underscores: the shape of a claimable username.
pub fn is_username_shaped(value: &str) -> bool {
    (3..=25).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// True when the exact name is on the reserved list, ignoring case (no compaction).
pub fn is_listed(value: &str) -> bool {
    let name = value.to_ascii_lowercase();
    RESERVED.contains(&name.as_str())
}

/// True when a username is reserved or not permitted.
pub fn is_reserved(username: &str) -> bool {
    let compact = compact(username);
    is_listed(username)
        || RESERVED.contains(&compact.as_str())
        || BLOCKED_SUBSTRINGS.iter().any(|word| compact.contains(word))
        || ABUSE
            .iter()
            .any(|word| compact.contains(word) || username.to_ascii_lowercase().contains(word))
}
