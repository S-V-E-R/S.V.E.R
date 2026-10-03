//! Shared text and link validation for profile content (docs/PROFILES.md, "Identity fields").
use crate::profiles::Fail;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Words rejected by the shared profile text filter, matched against a compacted form.
/// The message never names the matched word.
const SLURS: &[&str] = &[
    "nigger",
    "nigga",
    "faggot",
    "kike",
    "chink",
    "tranny",
    "spic",
    "wetback",
    "retard",
    "coon",
    "gook",
    "heilhitler",
    "1488",
];

fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2300..=0x23FF | 0x2B00..=0x2BFF | 0x2190..=0x21FF
        | 0x3030 | 0x303D | 0x3297 | 0x3299 | 0x00A9 | 0x00AE | 0x203C | 0x2049 | 0x2122 | 0x2139
        | 0x24C2 | 0x25AA..=0x25FE | 0x2934 | 0x2935 | 0xE0020..=0xE007F)
}
fn is_emoji_component(c: char) -> bool {
    is_emoji(c)
        || matches!(c as u32, 0xFE0F | 0xFE0E | 0x20E3 | 0x1F3FB..=0x1F3FF)
        || c.is_ascii_digit()
        || c == '#'
        || c == '*'
}

/// NFC-normalizes and trims, then rejects control, bidi-override and zero-width characters.
/// ZWJ is allowed only between emoji. `allow_newlines` keeps `\n` (after normalizing `\r\n`).
pub fn clean(value: &str, field: &'static str, allow_newlines: bool) -> Result<String, Fail> {
    let normalized: String = value.replace("\r\n", "\n").nfc().collect();
    let text = normalized.trim().to_string();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let code = c as u32;
        let bad = (c.is_control() && !(allow_newlines && c == '\n'))
            || matches!(code, 0x200B | 0x200C | 0x200E | 0x200F | 0x2028 | 0x2029 | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xFEFF | 0x061C | 0x180E);
        if bad {
            return Err(Fail::field(
                field,
                "Remove hidden or control characters and try again.",
            ));
        }
        if code == 0x200D {
            let before = i.checked_sub(1).map(|j| chars[j]);
            let after = chars.get(i + 1).copied();
            if !(before.is_some_and(is_emoji_component) && after.is_some_and(is_emoji)) {
                return Err(Fail::field(
                    field,
                    "Remove hidden or control characters and try again.",
                ));
            }
        }
    }
    Ok(text)
}

/// Lowercase letters/digits only with common digit substitutions, for filter matching.
pub fn compact(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            '@' => 'a',
            _ => c,
        })
        .collect()
}

/// The shared filter: the existing abuse list plus the slur list. Never echoes the word.
pub fn filter(value: &str, field: &'static str) -> Result<(), Fail> {
    let plain: String = value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect();
    let compacted = compact(value);
    if SLURS
        .iter()
        .chain(crate::reserved::ABUSE.iter())
        .any(|word| compacted.contains(word) || plain.contains(word))
    {
        return Err(Fail::field(field, "Remove the blocked word and try again."));
    }
    Ok(())
}

pub fn count(value: &str) -> usize {
    value.chars().count()
}
fn line_breaks(value: &str) -> usize {
    value.matches('\n').count()
}

/// General plain text: cleaned, length-bounded, filtered. `min` 0 allows empty.
pub fn plain(
    value: &str,
    field: &'static str,
    min: usize,
    max: usize,
    max_breaks: usize,
    filtered: bool,
) -> Result<String, Fail> {
    let text = clean(value, field, max_breaks > 0)?;
    let n = count(&text);
    if n < min || n > max {
        return Err(Fail::field_owned(
            field,
            if min == 0 {
                format!("Use up to {max} characters.")
            } else if min == max {
                format!("Use exactly {max} characters.")
            } else {
                format!("Use {min}-{max} characters.")
            },
        ));
    }
    if line_breaks(&text) > max_breaks {
        return Err(Fail::field_owned(
            field,
            if max_breaks == 0 {
                "Use a single line.".to_string()
            } else {
                format!("Use up to {max_breaks} line breaks.")
            },
        ));
    }
    if filtered {
        filter(&text, field)?;
    }
    Ok(text)
}

/// Staff and brand terms that a display name may not impersonate.
const STAFF_TERMS: &[&str] = &[
    "admin",
    "moderator",
    "staff",
    "support",
    "sver",
    "svertv",
    "official",
];

/// Display name: 1-32 characters, collapsed whitespace, letters/digits in any script plus a
/// small punctuation set, no staff impersonation unless it equals the owner's own username.
pub fn display_name(value: &str, own_username: &str) -> Result<String, Fail> {
    let cleaned = clean(value, "display_name", false)?;
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = count(&collapsed);
    if !(1..=32).contains(&n) {
        return Err(Fail::field("display_name", "Use 1-32 characters."));
    }
    if !collapsed
        .chars()
        .all(|c| c.is_alphanumeric() || c == ' ' || ".-_'!?&()".contains(c) || c.is_mark())
    {
        return Err(Fail::field(
            "display_name",
            "Use letters, numbers, spaces and . _ - ' ! ? & ( ) only.",
        ));
    }
    let compacted = compact(&collapsed);
    if !compacted.eq_ignore_ascii_case(&own_username.to_ascii_lowercase().replace('_', ""))
        && !collapsed.eq_ignore_ascii_case(own_username)
        && (STAFF_TERMS.contains(&compacted.as_str())
            || ["admin", "moderator", "support", "sverstaff", "sverofficial"]
                .iter()
                .any(|w| compacted.contains(w)))
    {
        return Err(Fail::field(
            "display_name",
            "That display name isn't allowed.",
        ));
    }
    filter(&collapsed, "display_name")?;
    Ok(collapsed)
}
trait Mark {
    fn is_mark(&self) -> bool;
}
impl Mark for char {
    /// Combining marks (needed for scripts such as Devanagari after NFC).
    fn is_mark(&self) -> bool {
        matches!(*self as u32, 0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A | 0x064B..=0x065F | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
    }
}

/// Mood: exactly one emoji grapheme, or empty.
pub fn mood(value: &str) -> Result<String, Fail> {
    let text = clean(value, "mood_emoji", false)?;
    if text.is_empty() {
        return Ok(text);
    }
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    if graphemes.len() != 1
        || !graphemes[0].chars().any(is_emoji)
        || graphemes[0]
            .chars()
            .any(|c| c.is_alphanumeric() && !is_emoji_component(c))
    {
        return Err(Fail::field("mood_emoji", "Choose a single emoji."));
    }
    Ok(text)
}
pub fn valid_mood(value: &str) -> bool {
    mood(value).is_ok_and(|m| m == value)
}

/// Social link platforms and their allowed hosts.
pub const PLATFORMS: &[(&str, &[&str])] = &[
    ("twitch", &["twitch.tv"]),
    ("youtube", &["youtube.com", "youtu.be"]),
    ("kick", &["kick.com"]),
    ("tiktok", &["tiktok.com"]),
    ("instagram", &["instagram.com"]),
    ("x", &["x.com", "twitter.com"]),
    ("bluesky", &["bsky.app"]),
    ("discord", &["discord.gg", "discord.com"]),
    ("facebook", &["facebook.com"]),
    ("patreon", &["patreon.com"]),
    ("kofi", &["ko-fi.com"]),
    ("fourthwall", &["4thwall.com"]),
    ("website", &[]),
];

fn host_matches(host: &str, allowed: &str) -> bool {
    host == allowed || host.ends_with(&format!(".{allowed}"))
}

/// Validates an https URL usable as a Website link (and the base rules for every link).
pub fn website_url(value: &str, field: &'static str) -> Result<String, Fail> {
    let value = value.trim();
    let invalid = || Fail::field(field, "Use a full https:// link.");
    if value.is_empty()
        || value.len() > 2048
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(invalid());
    }
    let url = url::Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    let host = match url.host() {
        Some(url::Host::Domain(host)) => host.to_ascii_lowercase(),
        _ => {
            return Err(Fail::field(
                field,
                "Use a link to a website, not an IP address.",
            ));
        }
    };
    if host == "localhost"
        || host.ends_with(".localhost")
        || !host.contains('.')
        || host_matches(&host, "sver.tv")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return Err(Fail::field(field, "That link isn't allowed."));
    }
    Ok(url.to_string())
}

/// Validates a social link for a platform, returning the normalized URL.
pub fn social_link(platform: &str, value: &str) -> Result<String, Fail> {
    let (_, hosts) = PLATFORMS
        .iter()
        .find(|(name, _)| *name == platform)
        .ok_or_else(|| Fail::field("platform", "Choose a supported platform."))?;
    let url = website_url(value, "url")?;
    if hosts.is_empty() {
        return Ok(url);
    }
    let parsed =
        url::Url::parse(&url).map_err(|_| Fail::field("url", "Use a full https:// link."))?;
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let ok = match platform {
        "discord" => {
            host_matches(&host, "discord.gg")
                || (host_matches(&host, "discord.com") && parsed.path().starts_with("/invite/"))
        }
        "fourthwall" => host.ends_with(".4thwall.com"),
        _ => hosts.iter().any(|allowed| host_matches(&host, allowed)),
    };
    if !ok {
        return Err(Fail::field("url", "That link doesn't match the platform."));
    }
    Ok(url)
}

/// Markdown-subset bodies (About and Panel blocks): length and character rules only; the web
/// renders the subset without HTML.
pub fn markdown(value: &str, field: &'static str, max: usize) -> Result<String, Fail> {
    plain(value, field, 1, max, 200, true)
}

pub fn contains_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("http://") || lower.contains("https://") || lower.contains("www.") || {
        // Bare domains such as example.com/path.
        lower.split_whitespace().any(|word| {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric());
            word.split_once('.').is_some_and(|(a, b)| {
                !a.is_empty()
                    && b.len() >= 2
                    && b.chars()
                        .take_while(|c| *c != '/')
                        .all(|c| c.is_ascii_alphabetic() || c == '.')
                    && [
                        "com", "net", "org", "gg", "tv", "io", "co", "ly", "me", "xyz", "app",
                        "dev",
                    ]
                    .iter()
                    .any(|tld| b.starts_with(tld))
            })
        })
    }
}
