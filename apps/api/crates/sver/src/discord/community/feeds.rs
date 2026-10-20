//! Outside feeds on the home server (docs/DISCORD_BOT.md "Community"):
//!
//! - **Free games** every 30 minutes from Epic, Steam (GamerPower giveaways and the featured free
//!   weekend) and GOG (its catalog, games discounted to free: the legacy giveaway endpoint no
//!   longer exists, so it never found anything), each posted once, the Free Games role pinged on
//!   the first post of a batch.
//! - **GitHub changelog**: published releases of S.V.E.R repositories, from a webhook signed with
//!   `GITHUB_WEBHOOK_SECRET` (the legacy bot accepted releases and never posted them).
//! - **WoW and Chromely** (`/wow`) against the Chromely API (`CHROMELY_API_URL`), and speedrun
//!   record posts from a webhook signed with `CHROMELY_WEBHOOK_SECRET`.
use super::{field, form};
use crate::discord::{
    call,
    commands::{ANSWERED, Interaction, embed, private},
    home, send,
    streamers::later,
};
use crate::{App, profiles::Res};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Method;
use serde_json::{Value, json};
use sha2::Sha256;

fn env_or(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.into())
}
fn secret(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty())
}
/// A `sha256=<hex>` HMAC of `body` under `secret`, compared in constant time.
fn signed(secret: &str, body: &[u8], header: Option<&str>) -> bool {
    let Some(expected) = header
        .and_then(|h| h.strip_prefix("sha256="))
        .and_then(crate::stripe::hex_decode)
    else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}
async fn get(app: &App, url: &str, query: &[(&str, &str)]) -> Option<Value> {
    app.http
        .get(url)
        .query(query)
        .header("User-Agent", "SVER-Discord-Bot/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()
}

// ---- Free games ----

#[derive(Debug, PartialEq)]
pub struct Game {
    pub store: &'static str,
    pub title: String,
    pub url: String,
    pub image: Option<String>,
    pub price: String,
    pub expires: Option<DateTime<Utc>>,
}
fn time(v: &Value) -> Option<DateTime<Utc>> {
    v.as_str()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.to_utc())
}

/// Epic's free-games promotions running now at 100% off.
pub fn epic(body: &Value) -> Vec<Game> {
    let now = Utc::now();
    body["data"]["Catalog"]["searchStore"]["elements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let promo = &g["promotions"]["promotionalOffers"][0]["promotionalOffers"][0];
            let (start, end) = (time(&promo["startDate"])?, time(&promo["endDate"]));
            if start > now
                || end.is_some_and(|e| e < now)
                || promo["discountSetting"]["discountPercentage"] != 0
            {
                return None;
            }
            let slug = g["catalogNs"]["mappings"][0]["pageSlug"]
                .as_str()
                .or(g["productSlug"].as_str())
                .or(g["urlSlug"].as_str());
            let images = g["keyImages"].as_array().cloned().unwrap_or_default();
            let image = ["OfferImageWide", "Thumbnail"]
                .iter()
                .find_map(|k| images.iter().find(|i| i["type"] == *k))
                .or(images.first())
                .and_then(|i| i["url"].as_str())
                .map(String::from);
            Some(Game {
                store: "epic",
                title: g["title"].as_str()?.into(),
                url: slug.map_or("https://store.epicgames.com".into(), |s| {
                    format!("https://store.epicgames.com/en-US/p/{s}")
                }),
                image,
                price: g["price"]["totalPrice"]["fmtPrice"]["originalPrice"]
                    .as_str()
                    .unwrap_or("Free")
                    .into(),
                expires: end,
            })
        })
        .collect()
}
/// GamerPower's active Steam giveaways (keys to keep).
pub fn gamerpower(body: &Value) -> Vec<Game> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter(|g| g["status"] == "Active")
        .filter_map(|g| {
            let expires = g["end_date"]
                .as_str()
                .and_then(|t| chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").ok())
                .map(|t| t.and_utc());
            Some(Game {
                store: "steam",
                title: g["title"].as_str()?.into(),
                url: g["open_giveaway_url"]
                    .as_str()
                    .or(g["gamerpower_url"].as_str())?
                    .into(),
                image: g["image"]
                    .as_str()
                    .or(g["thumbnail"].as_str())
                    .map(String::from),
                price: g["worth"]
                    .as_str()
                    .filter(|w| *w != "N/A")
                    .unwrap_or("Free")
                    .into(),
                expires,
            })
        })
        .collect()
}
/// GOG's catalog: games and packs discounted to free right now.
pub fn gog(body: &Value) -> Vec<Game> {
    body["products"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["price"]["finalMoney"]["amount"].as_str() == Some("0.00"))
        .filter_map(|p| {
            Some(Game {
                store: "gog",
                title: p["title"].as_str()?.into(),
                url: p["storeLink"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| {
                        format!(
                            "https://www.gog.com/en/game/{}",
                            p["slug"].as_str().unwrap_or_default()
                        )
                    }),
                image: p["coverHorizontal"].as_str().map(String::from),
                price: p["price"]["base"].as_str().unwrap_or("Free").into(),
                expires: None,
            })
        })
        .collect()
}
/// Steam's featured free weekend, when the store spotlights one (Steam lists no others).
async fn free_weekends(app: &App) -> Vec<Game> {
    let base = env_or("STEAM_STORE_BASE", "https://store.steampowered.com");
    let Some(featured) = get(
        app,
        &format!("{base}/api/featuredcategories"),
        &[("cc", "us"), ("l", "english")],
    )
    .await
    else {
        return vec![];
    };
    let mut ids: Vec<String> = featured
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .flat_map(|c| c["items"].as_array().cloned().unwrap_or_default())
        .filter(|i| {
            i["name"]
                .as_str()
                .is_some_and(|n| n.to_lowercase().contains("free weekend"))
        })
        .filter_map(|i| {
            let url = i["url"].as_str()?;
            let id = url.split("/app/").nth(1)?.split(['/', '?']).next()?;
            (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| id.to_string())
        })
        .collect();
    ids.dedup();
    let mut games = Vec::new();
    for id in ids.into_iter().take(5) {
        let Some(details) = get(
            app,
            &format!("{base}/api/appdetails"),
            &[("appids", id.as_str()), ("cc", "us"), ("l", "english")],
        )
        .await
        else {
            continue;
        };
        let d = &details[id.as_str()]["data"];
        let Some(title) = d["name"].as_str() else {
            continue;
        };
        games.push(Game {
            store: "steam",
            title: format!("{title} (free weekend)"),
            url: format!("https://store.steampowered.com/app/{id}"),
            image: d["header_image"].as_str().map(String::from),
            price: d["price_overview"]["initial_formatted"]
                .as_str()
                .filter(|p| !p.is_empty())
                .or(d["price_overview"]["final_formatted"].as_str())
                .unwrap_or("Free")
                .into(),
            expires: None,
        });
    }
    games
}
/// Every free game the stores list now.
async fn find_games(app: &App) -> Vec<Game> {
    let mut all = Vec::new();
    if let Some(b) = get(
        app,
        &env_or(
            "EPIC_FREE_GAMES",
            "https://store-site-backend-static-ipv4.ak.epicgames.com/freeGamesPromotions",
        ),
        &[
            ("locale", "en-US"),
            ("country", "US"),
            ("allowCountries", "US"),
        ],
    )
    .await
    {
        all.extend(epic(&b));
    }
    if let Some(b) = get(
        app,
        &env_or("GAMERPOWER_API", "https://www.gamerpower.com/api/giveaways"),
        &[("platform", "steam"), ("type", "game")],
    )
    .await
    {
        all.extend(gamerpower(&b));
    }
    all.extend(free_weekends(app).await);
    if let Some(b) = get(
        app,
        &env_or("GOG_CATALOG", "https://catalog.gog.com/v1/catalog"),
        &[
            ("limit", "48"),
            ("price", "between:0,0"),
            ("discounted", "eq:true"),
            ("productType", "in:game,pack"),
            ("countryCode", "US"),
            ("locale", "en-US"),
            ("currencyCode", "USD"),
        ],
    )
    .await
    {
        all.extend(gog(&b));
    }
    all
}
fn store_name(store: &str) -> &'static str {
    match store {
        "epic" => "Epic Games Store",
        "steam" => "Steam",
        _ => "GOG",
    }
}
/// Every 30 minutes: free games not posted before, each claimed first so it's posted once.
pub async fn free_games(app: &App) -> Res<()> {
    let settings = home::settings(app).await;
    let Some(channel) =
        home::id(&settings, "free_games").filter(|_| home::on(&settings, "free_games_on"))
    else {
        return Ok(());
    };
    let ping = home::id(&settings, "free_games_role");
    let mut first = true;
    for g in find_games(app).await {
        if g.expires.is_some_and(|t| t <= Utc::now()) {
            continue;
        }
        let new = sqlx::query("INSERT INTO discord_free_games(store,title,url,expires_at) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(g.store).bind(&g.title).bind(&g.url).bind(g.expires).execute(&app.db).await?.rows_affected() == 1;
        if !new {
            continue;
        }
        let ends = g.expires.map_or("Limited time".into(), |t| {
            format!("<t:{}:f>", t.timestamp())
        });
        let mut e = json!({"author": {"name": format!("🎮 Free on {}", store_name(g.store))}, "title": g.title, "url": g.url,
            "color": match g.store { "epic" => 0x00_78f2, "steam" => 0x1b_2838, _ => 0x86_328a },
            "fields": [{"name": "💰 Price", "value": format!("~~{}~~ → **FREE**", g.price), "inline": true},
                {"name": "⏰ Ends", "value": ends, "inline": true},
                {"name": "🏪 Store", "value": store_name(g.store), "inline": true}],
            "footer": {"text": "S.V.E.R bot • Free games"}});
        if let Some(i) = &g.image {
            e["image"] = json!({"url": i});
        }
        let mut body = json!({"embeds": [e], "allowed_mentions": {"parse": []}});
        if let (true, Some(r)) = (first, &ping) {
            body["content"] = json!(format!("<@&{r}> 🎉 Free game alert!"));
            body["allowed_mentions"] = json!({"roles": [r]});
        }
        match send(app, &channel, body).await {
            Some(m) => {
                first = false;
                sqlx::query(
                    "UPDATE discord_free_games SET message_id=$3 WHERE store=$1 AND title=$2",
                )
                .bind(g.store)
                .bind(&g.title)
                .bind(m)
                .execute(&app.db)
                .await?;
            }
            // Not posted: the next run tries again.
            None => {
                sqlx::query("DELETE FROM discord_free_games WHERE store=$1 AND title=$2")
                    .bind(g.store)
                    .bind(&g.title)
                    .execute(&app.db)
                    .await?;
            }
        }
    }
    Ok(())
}

// ---- GitHub changelog ----

/// Release notes without their Markdown images (they don't show in an embed), at most 2,000
/// characters.
fn notes(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        rest = match rest[start..].find(')') {
            Some(end) => &rest[start + end + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    out.trim().chars().take(2000).collect()
}
/// POST /api/integrations/github: GitHub's webhook. Only a body signed with the shared secret is
/// read; a published release of a S.V.E.R repository (or of one staff listed) is posted.
pub async fn github(State(app): State<App>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(key) = secret("GITHUB_WEBHOOK_SECRET") else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let header = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    if !signed(&key, &body, header("x-hub-signature-256")) {
        return StatusCode::UNAUTHORIZED;
    }
    if header("x-github-event") != Some("release") {
        return StatusCode::NO_CONTENT;
    }
    let Ok(event) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    if event["action"] != "published" {
        return StatusCode::NO_CONTENT;
    }
    let settings = home::settings(&app).await;
    let repo = event["repository"]["full_name"]
        .as_str()
        .unwrap_or_default();
    let lower = repo.to_lowercase();
    let listed = home::words(&settings, "github_repos");
    let ours = if listed.is_empty() {
        lower.starts_with("s-v-e-r/")
    } else {
        listed.contains(&lower)
    };
    let Some(channel) = home::id(&settings, "changelog").filter(|_| ours) else {
        return StatusCode::NO_CONTENT;
    };
    let r = &event["release"];
    let tag = r["tag_name"].as_str().unwrap_or("release");
    let name = r["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(tag);
    let text = notes(r["body"].as_str().unwrap_or_default());
    let color = if lower.contains("bot") || lower.contains("discord") {
        0x72_89da
    } else if lower.contains("chromely") {
        0x00_70dd
    } else {
        0xe7_4c3c
    };
    let post = json!({"embeds": [{"author": {"name": repo}, "title": format!("📦 {tag} — {name}"), "url": r["html_url"],
        "description": if text.is_empty() { "No notes." } else { text.as_str() }, "color": color,
        "timestamp": r["published_at"], "footer": {"text": "S.V.E.R bot • GitHub release"}}], "allowed_mentions": {"parse": []}});
    match send(&app, &channel, post).await {
        Some(_) => StatusCode::NO_CONTENT,
        // GitHub retries a failed delivery.
        None => StatusCode::BAD_GATEWAY,
    }
}

// ---- WoW and Chromely ----

fn chromely() -> Option<String> {
    secret("CHROMELY_API_URL").map(|u| u.trim_end_matches('/').to_string())
}
/// A run time in seconds as 1:02:05.0 or 4:05.5.
fn seconds(v: &Value) -> String {
    let Some(s) = v
        .as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    else {
        return "—".into();
    };
    let (h, m, rest) = ((s / 3600.0) as u64, ((s % 3600.0) / 60.0) as u64, s % 60.0);
    if h > 0 {
        format!("{h}:{m:02}:{rest:04.1}")
    } else {
        format!("{m}:{rest:04.1}")
    }
}
fn class_color(class: &str) -> u32 {
    match class.to_lowercase().as_str() {
        "warrior" => 0xc7_9c6e,
        "paladin" => 0xf5_8cba,
        "hunter" => 0xab_d473,
        "rogue" => 0xff_f569,
        "priest" => 0xff_ffff,
        "death knight" => 0xc4_1f3b,
        "shaman" => 0x00_70dd,
        "mage" => 0x69_ccf0,
        "warlock" => 0x94_82c9,
        "monk" => 0x00_ff96,
        "druid" => 0xff_7d0a,
        "demon hunter" => 0xa3_30c9,
        "evoker" => 0x33_937f,
        _ => 0x58_65f2,
    }
}
/// The first of `keys` that's a non-empty string (Chromely names some fields two ways).
fn s<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| v[*k].as_str())
        .filter(|s| !s.is_empty())
}
fn time_of(v: &Value) -> &Value {
    if v["time"].is_null() {
        &v["bestTime"]
    } else {
        &v["time"]
    }
}
fn character_card(c: &Value) -> Value {
    let class = s(c, &["class", "className"]).unwrap_or_default();
    let mut fields = Vec::new();
    if let (Some(race), false) = (s(c, &["race"]), class.is_empty()) {
        fields.push(json!({"name": "🏰 Race and class", "value": format!("{race} {class}"), "inline": true}));
    }
    if let Some(l) = c["level"].as_i64() {
        fields.push(json!({"name": "📊 Level", "value": l.to_string(), "inline": true}));
    }
    if let Some(n) = c["totalRuns"].as_i64() {
        fields.push(json!({"name": "🏆 Runs", "value": n.to_string(), "inline": true}));
    }
    if !c["bestTime"].is_null() {
        fields.push(
            json!({"name": "⚡ Best time", "value": seconds(&c["bestTime"]), "inline": true}),
        );
    }
    if !c["averageTime"].is_null() {
        fields.push(
            json!({"name": "📈 Average", "value": seconds(&c["averageTime"]), "inline": true}),
        );
    }
    let mut e = json!({"title": format!("{} — {}", s(c, &["name", "characterName"]).unwrap_or("Character"), s(c, &["realm"]).unwrap_or("")),
        "color": class_color(class), "fields": fields, "footer": {"text": "S.V.E.R bot • Chromely"}});
    if let Some(a) = s(c, &["avatarUrl", "avatar"]) {
        e["thumbnail"] = json!({"url": a});
    }
    e
}
fn leaderboard_card(entries: &[Value], milestone: Option<i64>) -> Value {
    let lines: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(n, e)| {
            let medal = match n {
                0 => "🥇".to_string(),
                1 => "🥈".into(),
                2 => "🥉".into(),
                _ => format!("`#{}`", n + 1),
            };
            let realm = s(e, &["realm"])
                .map(|r| format!(" ({r})"))
                .unwrap_or_default();
            let class = s(e, &["class", "className"])
                .map(|c| format!(" — {c}"))
                .unwrap_or_default();
            format!(
                "{medal} ⚡ {}  {}{realm}{class}",
                seconds(time_of(e)),
                s(e, &["name", "characterName"]).unwrap_or("Unknown")
            )
        })
        .collect();
    let title = match milestone {
        Some(m) => format!("🏆 WoW speedrun leaderboard — level {m}"),
        None => "🏆 WoW speedrun leaderboard".into(),
    };
    let description = if lines.is_empty() {
        "*No entries yet.*".to_string()
    } else {
        lines.join("\n")
    };
    json!({"title": title, "description": description, "color": 0xff_d700, "footer": {"text": "S.V.E.R bot • Chromely"}})
}
async fn leaderboard(app: &App, api: &str, milestone: Option<i64>) -> Vec<Value> {
    let m = milestone.map(|m| m.to_string());
    let mut query = vec![("limit", "10")];
    if let Some(m) = &m {
        query.push(("milestone", m.as_str()));
    }
    let body = get(app, &format!("{api}/api/bot/leaderboard"), &query)
        .await
        .unwrap_or(Value::Null);
    body.as_array()
        .or(body["entries"].as_array())
        .or(body["leaderboard"].as_array())
        .cloned()
        .unwrap_or_default()
}
fn encode(v: &str) -> String {
    url::form_urlencoded::byte_serialize(v.as_bytes()).collect()
}

/// A run from a Chromely export, in its JSON or text form, as Chromely's run submission takes it;
/// or why it can't be submitted.
pub fn parse_export(text: &str, now: i64) -> Result<Value, String> {
    if text.contains("CHROMELY SPEEDRUN EXPORT") {
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let get = |key: &str| {
            lines.iter().find_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.trim().eq_ignore_ascii_case(key))
                    .map(|(_, v)| v.trim().to_string())
            })
        };
        let (Some(character), Some(realm)) = (get("Character"), get("Realm")) else {
            return Err("The export has no character or realm. Copy the whole export.".into());
        };
        let parts: Vec<i64> = get("Total Time")
            .unwrap_or_default()
            .split(':')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        let total = match parts.as_slice() {
            [h, m, s] => h * 3600 + m * 60 + s,
            [m, s] => m * 60 + s,
            _ => return Err("The run time should look like HH:MM:SS.".into()),
        };
        if total < 30 {
            return Err("A run takes at least 30 seconds.".into());
        }
        let deaths = lines
            .iter()
            .skip_while(|l| **l != "DEATHS")
            .skip(1)
            .take_while(|l| !l.starts_with("==="))
            .filter(|l| {
                l.split('.')
                    .next()
                    .is_some_and(|n| n.parse::<u32>().is_ok())
            })
            .count();
        let raw = get("Class").unwrap_or_else(|| "Unknown".into());
        let mut class = raw.to_lowercase();
        if let Some(first) = class.get(..1) {
            class = format!("{}{}", first.to_uppercase(), &class[1..]);
        }
        let or = |k: &str, d: &str| get(k).unwrap_or_else(|| d.into());
        return Ok(
            json!({"character": character, "realm": realm, "region": "US",
            "guid": format!("manual-{}-{}", character.to_lowercase(), realm.to_lowercase()),
            "race": or("Race", "Unknown"), "class": class, "faction": or("Faction", "Unknown"), "category": or("Category", "SSF"),
            "hardcore": get("Hardcore").is_some_and(|h| h.eq_ignore_ascii_case("yes")),
            "final_level": get("Level").and_then(|l| l.parse::<i64>().ok()).unwrap_or(60),
            "start_time": now - total, "end_time": now, "splits": {}, "deaths": [], "death_count": deaths,
            "tracking_percentage": 100, "verification_hash": get("Hash"), "addon_version": "discord-submit"}),
        );
    }
    let v: Value = serde_json::from_str(text).map_err(|_| {
        "That isn't a Chromely export. Paste the whole export (`/cl export` in game).".to_string()
    })?;
    let (c, r) = (&v["character"], &v["run"]);
    let (Some(character), Some(realm)) = (c["name"].as_str(), c["realm"].as_str()) else {
        return Err("The export has no character or realm.".into());
    };
    let total = r["total_time"]
        .as_f64()
        .filter(|t| *t > 0.0)
        .ok_or("The export has no run time.")?;
    let tracking = r["tracking_pct"]
        .as_f64()
        .ok_or("The export has no tracking percentage.")?;
    if tracking < 80.0 {
        return Err(format!(
            "Run refused: the addon tracked only {tracking:.1}% of it (80% is needed)."
        ));
    }
    if total < 30.0 {
        return Err("A run takes at least 30 seconds.".into());
    }
    let guid = c["guid"].as_str().map_or_else(
        || {
            format!(
                "manual-{}-{}",
                character.to_lowercase(),
                realm.to_lowercase()
            )
        },
        String::from,
    );
    Ok(
        json!({"character": character, "realm": realm, "region": "US", "guid": guid,
        "race": c["race"].as_str().unwrap_or("Unknown"), "class": c["class"].as_str().unwrap_or("Unknown"),
        "faction": c["faction"].as_str().unwrap_or("Unknown"), "category": r["category"].as_str().unwrap_or("SSF"),
        "hardcore": r["hardcore"].as_bool().unwrap_or(false), "final_level": r["final_level"].as_i64().unwrap_or(60),
        "start_time": r["started_at"].as_i64().unwrap_or(now - total as i64), "end_time": r["ended_at"].as_i64().unwrap_or(now),
        "splits": {}, "deaths": [], "death_count": v["deaths"]["count"].as_i64().unwrap_or(0),
        "tracking_percentage": tracking, "verification_hash": v["verification"]["hash"], "addon_version": "discord-submit"}),
    )
}

async fn wow(app: App, i: Interaction) -> Res<Value> {
    let Some(api) = chromely() else {
        return Ok(private("WoW commands aren't available yet."));
    };
    let milestone = i.option("milestone").and_then(Value::as_i64);
    match i.sub() {
        Some("link") => {
            let name = super::text(&i, "character", 40).unwrap_or_default();
            let realm = super::text(&i, "realm", 40).unwrap_or_default();
            let path = format!(
                "{api}/api/bot/character/{}-{}",
                encode(&name),
                encode(&realm)
            );
            let Some(c) = get(&app, &path, &[]).await else {
                return Ok(private(
                    "That character isn't on Chromely. Make sure you've submitted runs with the addon.",
                ));
            };
            sqlx::query("INSERT INTO discord_wow(discord_user,character,realm) VALUES($1,$2,$3) ON CONFLICT(discord_user) DO UPDATE SET character=$2,realm=$3,linked_at=now()")
                .bind(&i.user).bind(&name).bind(&realm).execute(&app.db).await?;
            let mut reply = embed(character_card(&c), true);
            reply["data"]["content"] = json!(format!("**{name}** ({realm}) is linked."));
            Ok(reply)
        }
        Some("profile") => {
            let user = i.user_or_me("user");
            let linked: Option<(String, String)> =
                sqlx::query_as("SELECT character,realm FROM discord_wow WHERE discord_user=$1")
                    .bind(&user)
                    .fetch_optional(&app.db)
                    .await?;
            let Some((name, realm)) = linked else {
                return Ok(private(if user == i.user {
                    "You haven't linked a character. `/wow link` does it.".to_string()
                } else {
                    format!("<@{user}> hasn't linked a WoW character.")
                }));
            };
            let path = format!(
                "{api}/api/bot/character/{}-{}",
                encode(&name),
                encode(&realm)
            );
            Ok(match get(&app, &path, &[]).await {
                Some(c) => embed(character_card(&c), false),
                None => private("Chromely didn't answer. Try again in a minute."),
            })
        }
        Some("leaderboard") => Ok(embed(
            leaderboard_card(&leaderboard(&app, &api, milestone).await, milestone),
            false,
        )),
        Some("record") => {
            let m = milestone.unwrap_or(70);
            let r = get(
                &app,
                &format!("{api}/api/chromely/leaderboards/wr/{m}"),
                &[],
            )
            .await;
            let description = match &r {
                Some(r) => {
                    let mut d = vec![
                        format!(
                            "**{}**{}",
                            s(r, &["name", "characterName"]).unwrap_or("Unknown"),
                            s(r, &["realm"])
                                .map(|x| format!(" ({x})"))
                                .unwrap_or_default()
                        ),
                        format!("⚡ Time: **{}**", seconds(time_of(r))),
                    ];
                    if let Some(c) = s(r, &["class", "className"]) {
                        d.push(format!("⚔️ Class: {c}"));
                    }
                    d.join("\n")
                }
                None => "*No world record yet.*".into(),
            };
            Ok(embed(
                json!({"title": format!("🏆 World record — level {m}"), "description": description, "color": 0xff_d700}),
                false,
            ))
        }
        Some("rivals") => {
            let r = get(&app, &format!("{api}/api/chromely/rivals/status"), &[])
                .await
                .unwrap_or(Value::Null);
            let description = match (s(&r, &["status"]), r["rivals"].as_array()) {
                (Some(status), _) => status.to_string(),
                (None, Some(list)) if !list.is_empty() => list
                    .iter()
                    .map(|x| {
                        format!(
                            "**{}** — {}",
                            s(x, &["name"]).unwrap_or("Unknown"),
                            s(x, &["status"]).unwrap_or("active")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => "*No rival data yet.*".into(),
            };
            Ok(embed(
                json!({"title": "⚔️ Rivals", "description": description, "color": 0xe7_4c3c}),
                false,
            ))
        }
        _ => Ok(private("Pick a /wow command.")),
    }
}
async fn submitted(app: App, i: Interaction) -> Res<Value> {
    let Some(api) = chromely() else {
        return Ok(private("WoW commands aren't available yet."));
    };
    let run = match parse_export(
        &field(&i, "export").unwrap_or_default(),
        Utc::now().timestamp(),
    ) {
        Ok(run) => run,
        Err(why) => return Ok(private(format!("❌ {why}"))),
    };
    let answer = app
        .http
        .post(format!("{api}/api/chromely/runs"))
        .json(&run)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;
    Ok(private(match answer {
        Ok(r) if r.status().is_success() => {
            let body: Value = r.json().await.unwrap_or(Value::Null);
            format!(
                "✅ {}",
                body["message"]
                    .as_str()
                    .unwrap_or("Run submitted to the leaderboard!")
            )
        }
        Ok(r) => {
            let code = r.status().as_u16();
            let body: Value = r.json().await.unwrap_or(Value::Null);
            let why = body["details"][0]["message"]
                .as_str()
                .or(body["error"].as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("Chromely answered {code}."));
            format!("❌ Run refused: {why}")
        }
        Err(_) => "Chromely couldn't be reached. Try again later.".into(),
    }))
}

/// The bot's thread for `category` in `channel`, made on first use with `first` as its opening
/// message: its id, and whether it was just made.
async fn thread_for(
    app: &App,
    channel: &str,
    category: &str,
    name: &str,
    first: &Value,
) -> Option<(String, bool)> {
    let known: Option<String> = sqlx::query_scalar("SELECT thread_id FROM discord_threads WHERE channel_id=$1 AND category=$2 ORDER BY created_at DESC LIMIT 1")
        .bind(channel).bind(category).fetch_optional(&app.db).await.ok()?;
    if let Some(t) = known {
        // An archived thread opens again.
        call(
            app,
            Method::PATCH,
            &format!("/channels/{t}"),
            Some(json!({"archived": false})),
        )
        .await;
        return Some((t, false));
    }
    let forum = matches!(call(app, Method::GET, &format!("/channels/{channel}"), None).await, Some((200, c)) if c["type"] == 15);
    let thread = if forum {
        json!({"name": name, "message": first})
    } else {
        json!({"name": name, "type": 11, "auto_archive_duration": 10080})
    };
    let t = call(
        app,
        Method::POST,
        &format!("/channels/{channel}/threads"),
        Some(thread),
    )
    .await
    .filter(|r| r.0 < 300)
    .and_then(|r| r.1["id"].as_str().map(String::from))?;
    sqlx::query("INSERT INTO discord_threads(thread_id,channel_id,name,category) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(&t).bind(channel).bind(name).bind(category).execute(&app.db).await.ok()?;
    if !forum {
        send(app, &t, first.clone()).await;
    }
    Some((t, true))
}
/// A new speedrun record: posted in game chat's WoW Speedruns thread, and the leaderboard post in
/// the WoW leaderboard forum brought up to date.
async fn record(app: &App, data: &Value) -> Res<()> {
    let settings = home::settings(app).await;
    let mut lines = vec![
        format!(
            "**{}{}** just set a new record!",
            s(data, &["characterName", "character"]).unwrap_or("Someone"),
            s(data, &["realm"])
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        ),
        String::new(),
    ];
    if !data["time"].is_null() {
        lines.push(format!("⚡ Time: **{}**", seconds(&data["time"])));
    }
    let previous = if data["previousTime"].is_null() {
        &data["previousRecord"]
    } else {
        &data["previousTime"]
    };
    if !previous.is_null() {
        lines.push(format!("📉 Previous: {}", seconds(previous)));
    }
    if let Some(d) = s(data, &["dungeon"]) {
        lines.push(format!("🏰 Dungeon: {d}"));
    }
    if let Some(c) = s(data, &["class", "className"]) {
        lines.push(format!("⚔️ Class: {c}"));
    }
    let post = json!({"embeds": [{"title": "🏆 NEW SPEEDRUN RECORD!", "description": lines.join("\n"), "color": 0xff_d700,
        "footer": {"text": "S.V.E.R bot • Chromely"}}], "allowed_mentions": {"parse": []}});
    if let Some(chat) = home::id(&settings, "game_chat")
        && let Some((t, false)) = thread_for(app, &chat, "wow", "WoW Speedruns", &post).await
        && send(app, &t, post.clone()).await.is_none()
    {
        // The thread is gone: a fresh one next time.
        sqlx::query("DELETE FROM discord_threads WHERE thread_id=$1")
            .bind(&t)
            .execute(&app.db)
            .await?;
    }
    if let (Some(forum), Some(api)) = (home::id(&settings, "wow_leaderboard"), chromely()) {
        let board = json!({"embeds": [leaderboard_card(&leaderboard(app, &api, None).await, None)], "allowed_mentions": {"parse": []}});
        if let Some((t, false)) = thread_for(
            app,
            &forum,
            "wow_leaderboard",
            "📊 WoW speedrun leaderboard",
            &board,
        )
        .await
        {
            // A forum post's first message has the post's own id.
            let edited = matches!(call(app, Method::PATCH, &format!("/channels/{t}/messages/{t}"), Some(board)).await, Some((s, _)) if s < 300);
            if !edited {
                sqlx::query("DELETE FROM discord_threads WHERE thread_id=$1")
                    .bind(&t)
                    .execute(&app.db)
                    .await?;
            }
        }
    }
    Ok(())
}
/// POST /api/integrations/chromely: Chromely's webhook, signed with `CHROMELY_WEBHOOK_SECRET` in
/// X-Chromely-Signature.
pub async fn chromely_hook(State(app): State<App>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(key) = secret("CHROMELY_WEBHOOK_SECRET") else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let header = headers
        .get("x-chromely-signature")
        .and_then(|v| v.to_str().ok());
    if !signed(&key, &body, header) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(event) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    if event["event"] == "speedrun.record" && record(&app, &event["data"]).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::NO_CONTENT
}

// ---- Commands and forms ----

pub fn definitions() -> Vec<Value> {
    let milestone = json!({"type": 4, "name": "milestone", "description": "Level milestone (70 by default)", "min_value": 1, "max_value": 80});
    vec![
        json!({"name": "wow", "description": "WoW speedruns with Chromely.", "options": [
            {"type": 1, "name": "link", "description": "Link your WoW character", "options": [
                {"type": 3, "name": "character", "description": "Character name", "required": true, "max_length": 40},
                {"type": 3, "name": "realm", "description": "Realm", "required": true, "max_length": 40}]},
            {"type": 1, "name": "profile", "description": "A linked character", "options": [
                {"type": 6, "name": "user", "description": "Whose (you by default)"}]},
            {"type": 1, "name": "leaderboard", "description": "The speedrun leaderboard", "options": [milestone.clone()]},
            {"type": 1, "name": "record", "description": "The world record", "options": [milestone]},
            {"type": 1, "name": "rivals", "description": "Rival status"},
            {"type": 1, "name": "submit", "description": "Submit a speedrun from the Chromely export"}]}),
    ]
}

pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    if i.name() != "wow" {
        return None;
    }
    if i.sub() == Some("submit") {
        return Some(form(
            "wow:submit",
            "Submit a speedrun",
            &[
                (
                    "export",
                    "Chromely export (paste all of it)",
                    true,
                    true,
                    4000,
                    "",
                ),
                (
                    "proof",
                    "Screenshot or proof link (optional)",
                    false,
                    false,
                    500,
                    "",
                ),
            ],
        ));
    }
    later(app, i, i.sub() == Some("link"), wow);
    Some(ANSWERED)
}

/// The speedrun form.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    if i.custom_id() != "wow:submit" {
        return None;
    }
    later(app, i, true, submitted);
    Some(ANSWERED)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_the_stores_and_exports() {
        let soon = (Utc::now() + chrono::Duration::days(3)).to_rfc3339();
        let past = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        let epic_body = json!({"data": {"Catalog": {"searchStore": {"elements": [
            {"title": "Free one", "productSlug": "free-one", "keyImages": [{"type": "Thumbnail", "url": "https://img/t.png"}],
             "price": {"totalPrice": {"fmtPrice": {"originalPrice": "$19.99"}}},
             "promotions": {"promotionalOffers": [{"promotionalOffers": [{"startDate": past, "endDate": soon, "discountSetting": {"discountPercentage": 0}}]}]}},
            {"title": "Half off", "promotions": {"promotionalOffers": [{"promotionalOffers": [{"startDate": past, "endDate": soon, "discountSetting": {"discountPercentage": 50}}]}]}},
            {"title": "Coming", "promotions": {"promotionalOffers": []}}]}}}});
        let games = epic(&epic_body);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].url, "https://store.epicgames.com/en-US/p/free-one");
        assert_eq!(games[0].price, "$19.99");
        let gog_body = json!({"products": [
            {"title": "Giveaway", "storeLink": "https://www.gog.com/en/game/giveaway", "price": {"base": "$9.99", "finalMoney": {"amount": "0.00"}}},
            {"title": "Sale", "slug": "sale", "price": {"base": "$9.99", "finalMoney": {"amount": "2.99"}}}]});
        let games = gog(&gog_body);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Giveaway");
        let gp = json!([{"title": "Key", "status": "Active", "open_giveaway_url": "https://gp/open/1", "worth": "N/A", "end_date": "2026-12-01 23:59:00"},
            {"title": "Old", "status": "Expired", "open_giveaway_url": "https://gp/open/2"}]);
        let games = gamerpower(&gp);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].price, "Free");
        assert_eq!(
            games[0].expires.unwrap().to_rfc3339(),
            "2026-12-01T23:59:00+00:00"
        );

        let text = "=== CHROMELY SPEEDRUN EXPORT ===\nCharacter: Joe\nRealm: Dream\nClass: ROGUE\nTotal Time: 50:37:15\nHardcore: Yes\nDEATHS\n1. Level 12 in Elwynn\n2. Level 30 in STV\n=== END ===";
        let run = parse_export(text, 1_000_000).unwrap();
        assert_eq!(run["class"], "Rogue");
        assert_eq!(run["death_count"], 2);
        assert_eq!(run["start_time"], 1_000_000 - (50 * 3600 + 37 * 60 + 15));
        assert_eq!(run["hardcore"], true);
        let low = json!({"character": {"name": "A", "realm": "B"}, "run": {"total_time": 999.0, "tracking_pct": 50.0}});
        assert!(
            parse_export(&low.to_string(), 0)
                .unwrap_err()
                .contains("80%")
        );
        assert!(parse_export("hello", 0).is_err());

        let mut mac = Hmac::<Sha256>::new_from_slice(b"k").unwrap();
        mac.update(b"body");
        let good: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(signed("k", b"body", Some(&format!("sha256={good}"))));
        assert!(!signed("k", b"body", Some("sha256=00")));
        assert!(!signed("k", b"body", None));
        assert_eq!(seconds(&json!(65.5)), "1:05.5");
        assert_eq!(seconds(&json!(3725)), "1:02:05.0");
        assert_eq!(
            notes("Fixes ![shot](https://x/y.png) and more\n![b](c)"),
            "Fixes  and more"
        );
    }
}
