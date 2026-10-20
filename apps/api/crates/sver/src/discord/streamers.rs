//! Streamers on the home server (docs/DISCORD_BOT.md "Go-live alerts and streamers"): go-live posts
//! for S.V.E.R, Twitch, Kick and YouTube that turn into "was live" at the end and go away 4 hours
//! later, the Now Live role, stream sessions and stream XP, Twitch auto-detect, streamer profiles,
//! the directory and search, schedules with the Monday post, follower milestones, clip of the week
//! and the Streamer of the Week spotlight. Collab and LFG are in `collab`.
//!
//! S.V.E.R go-lives come from the platform's own go-live decision (`broadcasts.alert_state`, set by
//! `alerts::fan_out` under its 6-hour throttle). Twitch arrives by EventSub (through the webhook in
//! `linked_chat`) with a reconciliation poll; Kick (official API, `KICK_CLIENT_ID`/`_SECRET`) and
//! YouTube (`YOUTUBE_API_KEY`: the channel feed, then one cheap videos call) are polled.
use super::{
    call,
    commands::{ANSWERED, Interaction, MANAGE_GUILD, defer, embed, finish, private, respond},
    home, levels, role, send, sync,
};
use crate::{App, profiles::Res};
use chrono::{DateTime, Datelike, Duration, NaiveTime, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use reqwest::Method;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::Instant;

pub const PLATFORMS: [&str; 4] = ["sver", "twitch", "youtube", "kick"];
/// A new go-live post at most this often per account; a stream back within 10 minutes reuses its
/// post instead.
const POST_EVERY_HOURS: i64 = 6;
const RESUME_MINUTES: i64 = 10;
const MILESTONES: [i64; 10] = [10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub fn platform_name(platform: &str) -> &'static str {
    match platform {
        "sver" => "S.V.E.R",
        "twitch" => "Twitch",
        "youtube" => "YouTube",
        "kick" => "Kick",
        _ => "Any platform",
    }
}
fn platform_emoji(platform: &str) -> &'static str {
    match platform {
        "sver" => "⚔️",
        "twitch" => "💜",
        "youtube" => "▶️",
        "kick" => "💚",
        _ => "🔗",
    }
}
fn link_url(app: &App, platform: &str, handle: &str, key: Option<&str>) -> String {
    match platform {
        "sver" => format!("{}/{handle}", app.config.origin),
        "twitch" => format!("https://twitch.tv/{handle}"),
        "kick" => format!("https://kick.com/{handle}"),
        _ => match key {
            Some(video) => format!("https://youtube.com/watch?v={video}"),
            None => format!("https://youtube.com/{handle}"),
        },
    }
}
pub(super) fn answer(result: Res<Value>) -> Value {
    result.unwrap_or_else(|_| private("That couldn't be done right now. Try again in a minute."))
}
/// Answers "thinking…" first and the real reply when `work` finishes, for anything that calls
/// Discord or another platform and could miss the three-second window.
pub(super) fn later<F>(app: &App, i: &Interaction, only_you: bool, work: fn(App, Interaction) -> F)
where
    F: Future<Output = Res<Value>> + Send + 'static,
{
    let (Some(first), Some(second)) = (Interaction::parse(&i.raw), Interaction::parse(&i.raw))
    else {
        return;
    };
    let app = app.clone();
    tokio::spawn(async move {
        respond(&app, &first, defer(only_you)).await;
        let mut reply = answer(work(app.clone(), second).await)["data"].take();
        // An edit can't carry the private flag; the deferral already set it.
        if let Some(d) = reply.as_object_mut() {
            d.remove("flags");
        }
        finish(&app, &first, reply, None).await;
    });
}

// ---- Going live and ending ----

/// What a platform says about a live stream. `key` tells one stream from the next.
#[derive(Default)]
pub struct Live {
    pub key: String,
    pub title: String,
    pub game: Option<String>,
    pub thumb: Option<String>,
    pub viewers: Option<i64>,
}

fn card(
    app: &App,
    platform: &str,
    handle: &str,
    faction: Option<&str>,
    live: &Live,
    ended_minutes: Option<i64>,
) -> Value {
    let video = Some(live.key.as_str()).filter(|k| platform == "youtube" && !k.is_empty());
    let mut fields = Vec::new();
    if let Some(game) = live.game.as_deref().filter(|g| !g.is_empty()) {
        fields.push(json!({"name": "🎮 Category", "value": game, "inline": true}));
    }
    fields.push(json!({"name": format!("{} Platform", platform_emoji(platform)), "value": platform_name(platform), "inline": true}));
    let title = if live.title.is_empty() {
        format!("Live on {}", platform_name(platform))
    } else {
        live.title.chars().take(256).collect()
    };
    let mut e = match ended_minutes {
        None => {
            if let Some(v) = live.viewers {
                fields.push(json!({"name": "👁️ Viewers", "value": v.to_string(), "inline": true}));
            }
            json!({"color": levels::color(faction),
                "author": {"name": format!("{} {handle} is LIVE!", levels::emoji(faction))},
                "footer": {"text": "S.V.E.R bot • 🔴 LIVE"}})
        }
        Some(minutes) => json!({"color": 0x95_a5a6,
            "author": {"name": format!("{handle} was live on {}", platform_name(platform))},
            "footer": {"text": format!("S.V.E.R bot • ⏹️ Was live for {}", hours(minutes))}}),
    };
    e["title"] = json!(title);
    e["url"] = json!(link_url(app, platform, handle, video));
    e["fields"] = json!(fields);
    e["timestamp"] = json!(Utc::now().to_rfc3339());
    if let Some(thumb) = &live.thumb {
        e["image"] = json!({"url": thumb});
    }
    e
}
pub(super) fn hours(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}
fn message(embed: Value) -> Value {
    json!({"embeds": [embed], "allowed_mentions": {"parse": []}})
}

type Started = (
    String,
    String,
    bool,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);
/// An account went live. Each account goes live once per stream: a second notice (EventSub and the
/// poll both seeing it) finds it already live and does nothing. `may_post` is false when the
/// platform's own throttle held the alert back.
pub async fn online(app: &App, platform: &str, id: &str, live: Live, may_post: bool) -> Res<()> {
    let Some(guild) = home::guild(app).await else {
        return Ok(());
    };
    let started: Vec<Started> = sqlx::query_as("WITH old AS (SELECT discord_user,message_id,post_channel,offline_at,posted_at FROM discord_stream_accounts WHERE platform=$1 AND platform_id=$2 AND live_since IS NULL FOR UPDATE)
        UPDATE discord_stream_accounts a SET live_since=now(),live_key=$3,title=$4,game=$5,offline_at=NULL FROM old WHERE a.discord_user=old.discord_user AND a.platform=$1
        RETURNING a.discord_user,a.handle,a.alerts,old.message_id,old.post_channel,old.offline_at,old.posted_at")
        .bind(platform).bind(id).bind(&live.key).bind(&live.title).bind(&live.game)
        .fetch_all(&app.db).await?;
    if started.is_empty() {
        return Ok(());
    }
    let settings = home::settings(app).await;
    let to = home::id(&settings, "go_live").filter(|_| home::on(&settings, "alerts_on"));
    for (user, handle, alerts, old, old_channel, offline_at, posted_at) in started {
        if let Some(r) = home::id(&settings, "now_live") {
            role(app, &guild, &user, &r, true).await;
        }
        sqlx::query("INSERT INTO discord_streamer_profiles(discord_user,last_live_at) VALUES($1,now()) ON CONFLICT(discord_user) DO UPDATE SET last_live_at=now()")
            .bind(&user).execute(&app.db).await?;
        let (Some(to), true) = (&to, alerts) else {
            continue;
        };
        let faction = levels::faction(app, &user).await?;
        let body = message(card(
            app,
            platform,
            &handle,
            faction.as_deref(),
            &live,
            None,
        ));
        let resumed =
            offline_at.is_some_and(|t| t > Utc::now() - Duration::minutes(RESUME_MINUTES));
        let recent = posted_at.is_some_and(|t| t > Utc::now() - Duration::hours(POST_EVERY_HOURS));
        let fresh = may_post && !recent;
        if let (Some(m), Some(c)) = (&old, &old_channel) {
            // Back within 10 minutes, or no new post allowed yet (the platform's throttle or the
            // 6-hour rule): the same post turns live again, an edit that pings nobody.
            if (resumed || !fresh)
                && matches!(call(app, Method::PATCH, &format!("/channels/{c}/messages/{m}"), Some(body.clone())).await, Some((s, _)) if s < 300)
            {
                continue;
            }
            // Otherwise the older post is replaced.
            call(
                app,
                Method::DELETE,
                &format!("/channels/{c}/messages/{m}"),
                None,
            )
            .await;
            sqlx::query("UPDATE discord_stream_accounts SET message_id=NULL,post_channel=NULL WHERE discord_user=$1 AND platform=$2")
                .bind(&user).bind(platform).execute(&app.db).await?;
        }
        if !fresh {
            continue;
        }
        if let Some(id) = send(app, to, body).await {
            sqlx::query("UPDATE discord_stream_accounts SET message_id=$3,post_channel=$4,posted_at=now() WHERE discord_user=$1 AND platform=$2")
                .bind(&user).bind(platform).bind(id).bind(to).execute(&app.db).await?;
        }
    }
    Ok(())
}

type Ended = (
    String,
    String,
    DateTime<Utc>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);
/// An account's stream ended: a session and its stream XP, the post turns into "was live", and the
/// Now Live role goes once nothing else of theirs is live.
pub async fn offline(app: &App, platform: &str, id: &str) -> Res<()> {
    let ended: Vec<Ended> = sqlx::query_as("WITH old AS (SELECT discord_user,live_since,live_key FROM discord_stream_accounts WHERE platform=$1 AND platform_id=$2 AND live_since IS NOT NULL FOR UPDATE)
        UPDATE discord_stream_accounts a SET live_since=NULL,live_key=NULL,offline_at=now() FROM old WHERE a.discord_user=old.discord_user AND a.platform=$1
        RETURNING a.discord_user,a.handle,old.live_since,old.live_key,a.title,a.game,a.message_id,a.post_channel")
        .bind(platform).bind(id).fetch_all(&app.db).await?;
    let guild = home::guild(app).await;
    let settings = home::settings(app).await;
    for (user, handle, since, key, title, game, message_id, channel) in ended {
        let minutes = (Utc::now() - since).num_minutes().max(0);
        if minutes > 0 {
            sqlx::query("INSERT INTO discord_stream_sessions(discord_user,platform,handle,started_at,ended_at,minutes) VALUES($1,$2,$3,$4,now(),$5)")
                .bind(&user).bind(platform).bind(&handle).bind(since).bind(minutes as i32).execute(&app.db).await?;
            levels::stream_ended(app, &user, minutes).await?;
        }
        if let (Some(m), Some(c)) = (message_id, channel) {
            let live = Live {
                key: key.unwrap_or_default(),
                title: title.unwrap_or_default(),
                game,
                ..Live::default()
            };
            let faction = levels::faction(app, &user).await?;
            let body = message(card(
                app,
                platform,
                &handle,
                faction.as_deref(),
                &live,
                Some(minutes),
            ));
            call(
                app,
                Method::PATCH,
                &format!("/channels/{c}/messages/{m}"),
                Some(body),
            )
            .await;
        }
        let still: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_stream_accounts WHERE discord_user=$1 AND live_since IS NOT NULL)")
            .bind(&user).fetch_one(&app.db).await?;
        if let (false, Some(g), Some(r)) = (still, &guild, home::id(&settings, "now_live")) {
            role(app, g, &user, &r, false).await;
        }
    }
    Ok(())
}

/// Brings one platform account in line with what its platform says now.
async fn reconcile(
    app: &App,
    platform: &str,
    id: &str,
    now: Option<Live>,
    may_post: bool,
) -> Res<()> {
    let current: Option<Option<String>> = sqlx::query_scalar("SELECT live_key FROM discord_stream_accounts WHERE platform=$1 AND platform_id=$2 AND live_since IS NOT NULL LIMIT 1")
        .bind(platform).bind(id).fetch_optional(&app.db).await?;
    match (current, now) {
        (None, Some(live)) => online(app, platform, id, live, may_post).await,
        (Some(_), None) => offline(app, platform, id).await,
        // A new stream without an end seen in between.
        (Some(key), Some(live)) if key.as_deref() != Some(live.key.as_str()) => {
            offline(app, platform, id).await?;
            online(app, platform, id, live, may_post).await
        }
        _ => Ok(()),
    }
}

/// Each minute: "was live" posts older than 4 hours are removed.
async fn tidy(app: &App) -> Res<()> {
    let old: Vec<(String, String, String, String)> = sqlx::query_as("SELECT discord_user,platform,post_channel,message_id FROM discord_stream_accounts WHERE message_id IS NOT NULL AND post_channel IS NOT NULL AND live_since IS NULL AND offline_at<now()-interval '4 hours' LIMIT 20")
        .fetch_all(&app.db).await?;
    for (user, platform, channel, m) in old {
        call(
            app,
            Method::DELETE,
            &format!("/channels/{channel}/messages/{m}"),
            None,
        )
        .await;
        sqlx::query("UPDATE discord_stream_accounts SET message_id=NULL,post_channel=NULL WHERE discord_user=$1 AND platform=$2 AND message_id=$3")
            .bind(&user).bind(&platform).bind(&m).execute(&app.db).await?;
    }
    Ok(())
}

// ---- S.V.E.R ----

type SverLive = (String, String, String, String, bool, String, Option<String>);
/// Each minute: linked S.V.E.R accounts that are live, as the platform decided (`alert_state`).
async fn sver(app: &App) -> Res<()> {
    let Some(guild) = home::guild(app).await else {
        return Ok(());
    };
    let live: Vec<SverLive> = sqlx::query_as("SELECT i.subject,u.id,u.username,b.id,b.alert_state='sent',coalesce(st.title,''),c.name
        FROM broadcasts b JOIN channel_users u ON u.id=b.owner_id AND u.eligible AND u.deleted_at IS NULL
        JOIN identities i ON i.user_id=u.id AND i.provider='discord'
        LEFT JOIN stream_settings st ON st.owner_id=u.id LEFT JOIN stream_categories c ON c.id=st.category_id
        WHERE b.state IN ('LIVE','RECONNECTING') AND b.alert_state IN ('sent','throttled')
            AND NOT EXISTS(SELECT 1 FROM discord_autolink_optouts o WHERE o.discord_user=i.subject AND o.platform='sver')")
        .fetch_all(&app.db).await?;
    for (discord, user, username, broadcast, sent, title, game) in live {
        let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_stream_accounts WHERE discord_user=$1 AND platform='sver')")
            .bind(&discord).fetch_one(&app.db).await?;
        if !known {
            // Only members of the home server get an account (and a post) here; a non-member is
            // asked about once per broadcast.
            // ponytail: in memory and never trimmed; a set of broadcast ids, tiny next to the rest.
            static OUTSIDE: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);
            let seen = |add: bool| {
                let mut set = OUTSIDE.lock().unwrap_or_else(|e| e.into_inner());
                let set = set.get_or_insert_with(Default::default);
                if add {
                    !set.insert(broadcast.clone())
                } else {
                    set.contains(&broadcast)
                }
            };
            if seen(false) {
                continue;
            }
            let member = call(
                app,
                Method::GET,
                &format!("/guilds/{guild}/members/{discord}"),
                None,
            )
            .await;
            match member {
                Some((200, _)) => {}
                Some((404, _)) => {
                    seen(true);
                    continue;
                }
                _ => continue,
            }
        }
        sqlx::query("INSERT INTO discord_stream_accounts(discord_user,platform,handle,platform_id) VALUES($1,'sver',$2,$3) ON CONFLICT(discord_user,platform) DO UPDATE SET handle=$2,platform_id=$3")
            .bind(&discord).bind(&username).bind(&user).execute(&app.db).await?;
        let live = Live {
            key: broadcast,
            title,
            game,
            ..Live::default()
        };
        reconcile(app, "sver", &user, Some(live), sent).await?;
    }
    let ended: Vec<String> = sqlx::query_scalar("SELECT DISTINCT platform_id FROM discord_stream_accounts a WHERE platform='sver' AND live_since IS NOT NULL AND platform_id IS NOT NULL
        AND NOT EXISTS(SELECT 1 FROM broadcasts b WHERE b.id=a.live_key AND b.state IN ('LIVE','RECONNECTING'))")
        .fetch_all(&app.db).await?;
    for id in ended {
        offline(app, "sver", &id).await?;
    }
    Ok(())
}

// ---- Twitch ----

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
fn twitch_live(s: &Value) -> Live {
    Live {
        key: s["id"].as_str().unwrap_or_default().into(),
        title: s["title"].as_str().unwrap_or_default().into(),
        game: s["game_name"]
            .as_str()
            .filter(|g| !g.is_empty())
            .map(String::from),
        thumb: s["thumbnail_url"]
            .as_str()
            .map(|t| t.replace("{width}", "440").replace("{height}", "248")),
        viewers: s["viewer_count"].as_i64(),
    }
}
/// A Twitch account's current login, by its id.
async fn twitch_login(app: &App, id: &str) -> Option<String> {
    let (200, body) =
        crate::linked_chat::helix(app, Method::GET, &format!("/users?id={}", encode(id)), None)
            .await?
    else {
        return None;
    };
    body["data"][0]["login"].as_str().map(String::from)
}

/// A go-live EventSub notification or revocation, from `linked_chat`'s webhook.
pub async fn twitch_event(app: &App, kind: &str, event: &Value) -> Res<()> {
    let sub = &event["subscription"];
    if kind == "revocation" {
        // Subscribed again on the next pass, unless the account is gone (the poll still covers it).
        sqlx::query("UPDATE discord_stream_accounts SET eventsub=array_remove(eventsub,$1) WHERE platform='twitch' AND $1=ANY(eventsub)")
            .bind(sub["id"].as_str().unwrap_or_default()).execute(&app.db).await?;
        return Ok(());
    }
    if kind != "notification" {
        return Ok(());
    }
    let id = event["event"]["broadcaster_user_id"]
        .as_str()
        .unwrap_or_default();
    match sub["type"].as_str() {
        Some("stream.online") => {
            let details = crate::linked_chat::helix(
                app,
                Method::GET,
                &format!("/streams?user_id={}", encode(id)),
                None,
            )
            .await;
            let live = match details {
                Some((200, b)) if b["data"][0].is_object() => twitch_live(&b["data"][0]),
                // Twitch can lag a few seconds behind its own event.
                _ => Live {
                    key: event["event"]["id"].as_str().unwrap_or_default().into(),
                    ..Live::default()
                },
            };
            reconcile(app, "twitch", id, Some(live), true).await
        }
        Some("stream.offline") => offline(app, "twitch", id).await,
        _ => Ok(()),
    }
}

/// Each minute: subscribe go-live events for a few new Twitch accounts.
async fn twitch_subscribe(app: &App) -> Res<()> {
    let due: Vec<String> = sqlx::query_scalar("SELECT DISTINCT platform_id FROM discord_stream_accounts a WHERE platform='twitch' AND platform_id IS NOT NULL AND cardinality(eventsub)=0
        AND NOT EXISTS(SELECT 1 FROM discord_stream_accounts b WHERE b.platform='twitch' AND b.platform_id=a.platform_id AND cardinality(b.eventsub)>0) LIMIT 5")
        .fetch_all(&app.db).await?;
    for id in due {
        let mut ids = Vec::new();
        for kind in ["stream.online", "stream.offline"] {
            let body = json!({"type": kind, "version": "1", "condition": {"broadcaster_user_id": id},
                "transport": {"method": "webhook", "callback": crate::linked_chat::callback(app), "secret": crate::linked_chat::webhook_secret(app)}});
            match crate::linked_chat::helix(
                app,
                Method::POST,
                "/eventsub/subscriptions",
                Some(body),
            )
            .await
            {
                Some((202, b)) => ids.extend(b["data"][0]["id"].as_str().map(String::from)),
                // Already there (after a restart): find ours, so unlinking can remove it.
                Some((409, _)) => {
                    let path = format!("/eventsub/subscriptions?user_id={}", encode(&id));
                    if let Some((200, b)) =
                        crate::linked_chat::helix(app, Method::GET, &path, None).await
                    {
                        let ours = crate::linked_chat::callback(app);
                        ids.extend(
                            b["data"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter(|s| {
                                    s["type"] == kind && s["transport"]["callback"] == ours.as_str()
                                })
                                .filter_map(|s| s["id"].as_str().map(String::from)),
                        );
                    }
                }
                // Refused for good (an account Twitch no longer has): the poll still covers it.
                Some((400 | 403 | 404, _)) => ids.push("none".into()),
                _ => return Ok(()),
            }
        }
        sqlx::query(
            "UPDATE discord_stream_accounts SET eventsub=$2 WHERE platform='twitch' AND platform_id=$1",
        )
        .bind(&id)
        .bind(&ids)
        .execute(&app.db)
        .await?;
    }
    Ok(())
}
async fn twitch_unsubscribe(app: &App, ids: &[String]) {
    for id in ids.iter().filter(|i| *i != "none") {
        let path = format!("/eventsub/subscriptions?id={}", encode(id));
        crate::linked_chat::helix(app, Method::DELETE, &path, None).await;
    }
}

/// Every 3 minutes: catches go-lives and ends EventSub missed, 100 accounts a request.
pub async fn twitch_poll(app: &App) -> Res<()> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT DISTINCT platform_id FROM discord_stream_accounts WHERE platform='twitch' AND platform_id IS NOT NULL ORDER BY 1")
        .fetch_all(&app.db).await?;
    for chunk in ids.chunks(100) {
        let query: Vec<String> = chunk
            .iter()
            .map(|i| format!("user_id={}", encode(i)))
            .collect();
        let path = format!("/streams?first=100&{}", query.join("&"));
        let Some((200, body)) = crate::linked_chat::helix(app, Method::GET, &path, None).await
        else {
            return Ok(());
        };
        let mut live: HashMap<String, Live> = body["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| Some((s["user_id"].as_str()?.to_string(), twitch_live(s))))
            .collect();
        for id in chunk {
            reconcile(app, "twitch", id, live.remove(id), true).await?;
        }
    }
    Ok(())
}

// ---- Kick ----

fn kick_keys() -> Option<(String, String)> {
    let id = std::env::var("KICK_CLIENT_ID")
        .ok()
        .filter(|v| !v.is_empty())?;
    let secret = std::env::var("KICK_CLIENT_SECRET")
        .ok()
        .filter(|v| !v.is_empty())?;
    Some((id, secret))
}
fn env_or(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.into())
}
type Token = Option<(String, Instant)>;
static KICK_TOKEN: Mutex<Token> = Mutex::new(None);
fn kick_cached() -> std::sync::MutexGuard<'static, Token> {
    KICK_TOKEN.lock().unwrap_or_else(|e| e.into_inner())
}
async fn kick_token(app: &App) -> Option<String> {
    if let Some((token, until)) = kick_cached().clone()
        && until > Instant::now()
    {
        return Some(token);
    }
    let (id, secret) = kick_keys()?;
    let body: Value = app
        .http
        .post(format!(
            "{}/oauth/token",
            env_or("KICK_ID_BASE", "https://id.kick.com")
        ))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", id.as_str()),
            ("client_secret", secret.as_str()),
        ])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let token = body["access_token"].as_str()?.to_string();
    let life = body["expires_in"]
        .as_u64()
        .unwrap_or(3600)
        .saturating_sub(300);
    *kick_cached() = Some((
        token.clone(),
        Instant::now() + std::time::Duration::from_secs(life),
    ));
    Some(token)
}
/// Kick channels by slug, from the official public API (up to 50 a request).
async fn kick_channels(app: &App, slugs: &[String]) -> Option<Vec<Value>> {
    let token = kick_token(app).await?;
    let query: Vec<(&str, &str)> = slugs.iter().map(|s| ("slug", s.as_str())).collect();
    let response = app
        .http
        .get(format!(
            "{}/public/v1/channels",
            env_or("KICK_API_BASE", "https://api.kick.com")
        ))
        .query(&query)
        .bearer_auth(token)
        .send()
        .await
        .ok()?;
    if response.status().as_u16() == 401 {
        *kick_cached() = None;
        return None;
    }
    let body: Value = response.error_for_status().ok()?.json().await.ok()?;
    body["data"].as_array().cloned()
}
fn kick_live(c: &Value) -> Option<Live> {
    let s = &c["stream"];
    (s["is_live"] == true).then(|| Live {
        key: s["start_time"].as_str().unwrap_or("live").into(),
        title: c["stream_title"].as_str().unwrap_or_default().into(),
        game: c["category"]["name"].as_str().map(String::from),
        thumb: s["thumbnail"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(String::from),
        viewers: s["viewer_count"].as_i64(),
    })
}
/// Every minute: Kick accounts' live state.
pub async fn kick_poll(app: &App) -> Res<()> {
    if kick_keys().is_none() {
        return Ok(());
    }
    let slugs: Vec<String> = sqlx::query_scalar("SELECT DISTINCT platform_id FROM discord_stream_accounts WHERE platform='kick' AND platform_id IS NOT NULL ORDER BY 1")
        .fetch_all(&app.db).await?;
    for chunk in slugs.chunks(50) {
        let Some(channels) = kick_channels(app, chunk).await else {
            return Ok(());
        };
        for slug in chunk {
            let live = channels
                .iter()
                .find(|c| c["slug"].as_str() == Some(slug.as_str()))
                .and_then(kick_live);
            reconcile(app, "kick", slug, live, true).await?;
        }
    }
    Ok(())
}

// ---- YouTube ----

fn youtube_key() -> Option<String> {
    std::env::var("YOUTUBE_API_KEY")
        .ok()
        .filter(|v| !v.is_empty())
}
fn youtube_api() -> String {
    env_or("YOUTUBE_API_BASE", "https://www.googleapis.com/youtube/v3")
}
/// A YouTube channel by @handle or channel id.
async fn youtube_channel(app: &App, name: &str) -> Option<String> {
    let key = youtube_key()?;
    let handle = format!("@{}", name.trim_start_matches('@'));
    for (param, value) in [("forHandle", handle.as_str()), ("id", name)] {
        let body: Value = app
            .http
            .get(format!("{}/channels", youtube_api()))
            .query(&[("part", "id"), (param, value), ("key", key.as_str())])
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        if let Some(id) = body["items"][0]["id"].as_str() {
            return Some(id.into());
        }
    }
    None
}
/// The newest video ids in a channel's public feed (no quota).
async fn youtube_recent(app: &App, channel: &str) -> Option<Vec<String>> {
    let feed = app
        .http
        .get(env_or(
            "YOUTUBE_FEED_BASE",
            "https://www.youtube.com/feeds/videos.xml",
        ))
        .query(&[("channel_id", channel)])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    Some(
        feed.split("<yt:videoId>")
            .skip(1)
            .filter_map(|s| s.split('<').next())
            .take(5)
            .map(String::from)
            .collect(),
    )
}
/// Every 3 minutes: each channel's newest videos, then one videos call (1 quota unit per 50 videos)
/// for which of them is live. The legacy bot's search call cost 100 units a channel.
pub async fn youtube_poll(app: &App) -> Res<()> {
    let Some(key) = youtube_key() else {
        return Ok(());
    };
    // Accounts the legacy bot linked without an API key get their channel id first.
    let unresolved: Vec<(String, String)> = sqlx::query_as("SELECT discord_user,handle FROM discord_stream_accounts WHERE platform='youtube' AND platform_id IS NULL LIMIT 5")
        .fetch_all(&app.db).await?;
    for (user, handle) in unresolved {
        if let Some(id) = youtube_channel(app, bare(&handle)).await {
            sqlx::query("UPDATE discord_stream_accounts SET platform_id=$3 WHERE discord_user=$1 AND platform='youtube' AND handle=$2")
                .bind(&user).bind(&handle).bind(id).execute(&app.db).await?;
        }
    }
    let channels: Vec<String> = sqlx::query_scalar("SELECT DISTINCT platform_id FROM discord_stream_accounts WHERE platform='youtube' AND platform_id IS NOT NULL ORDER BY 1")
        .fetch_all(&app.db).await?;
    for chunk in channels.chunks(10) {
        let mut videos = Vec::new();
        let mut read = Vec::new();
        for c in chunk {
            // A feed that can't be read (a closed channel, a hiccup) leaves that channel as it was
            // this round; the others are still checked.
            if let Some(recent) = youtube_recent(app, c).await {
                videos.extend(recent);
                read.push(c);
            }
        }
        let mut live: HashMap<String, Live> = HashMap::new();
        if !videos.is_empty() {
            let response = app
                .http
                .get(format!("{}/videos", youtube_api()))
                .query(&[
                    ("part", "snippet,liveStreamingDetails"),
                    ("id", videos.join(",").as_str()),
                    ("key", key.as_str()),
                ])
                .send()
                .await
                .ok()
                .and_then(|r| r.error_for_status().ok());
            let Some(response) = response else {
                return Ok(());
            };
            let body: Value = response.json().await.unwrap_or(Value::Null);
            for v in body["items"].as_array().into_iter().flatten() {
                let s = &v["snippet"];
                let Some(channel) = s["channelId"]
                    .as_str()
                    .filter(|_| s["liveBroadcastContent"] == "live")
                else {
                    continue;
                };
                live.insert(
                    channel.into(),
                    Live {
                        key: v["id"].as_str().unwrap_or_default().into(),
                        title: s["title"].as_str().unwrap_or_default().into(),
                        game: None,
                        thumb: s["thumbnails"]["high"]["url"].as_str().map(String::from),
                        viewers: v["liveStreamingDetails"]["concurrentViewers"]
                            .as_str()
                            .and_then(|n| n.parse().ok()),
                    },
                );
            }
        }
        for c in read {
            reconcile(app, "youtube", c, live.remove(c), true).await?;
        }
    }
    Ok(())
}

// ---- Linking ----

/// A name as typed or pasted (a profile link works too).
fn bare(name: &str) -> &str {
    let name = name.trim().trim_end_matches('/');
    name.rsplit('/').next().unwrap_or(name).trim()
}
/// The member's own Twitch account, as S.V.E.R confirmed it: their Discord is linked to a S.V.E.R
/// account that signed in with Twitch or linked Twitch for chat. (login, Twitch id).
///
/// Alerts need that proof (Joe, October 20, 2026), so nobody can link a channel that isn't theirs.
/// S.V.E.R can't confirm YouTube channels yet, so those can't be linked until it can; accounts the
/// legacy bot linked are kept.
async fn proven_twitch(app: &App, discord: &str) -> Res<Option<(String, String)>> {
    let found: Option<(Option<String>, String)> = sqlx::query_as("SELECT coalesce(t.handle,l.handle),coalesce(t.subject,l.subject)
        FROM identities d JOIN channel_users u ON u.id=d.user_id AND u.eligible AND u.deleted_at IS NULL
        LEFT JOIN identities t ON t.user_id=u.id AND t.provider='twitch'
        LEFT JOIN linked_chat_accounts l ON l.owner_id=u.id AND l.platform='twitch'
        WHERE d.provider='discord' AND d.subject=$1 AND coalesce(t.subject,l.subject) IS NOT NULL LIMIT 1")
        .bind(discord).fetch_optional(&app.db).await?;
    let Some((handle, id)) = found else {
        return Ok(None);
    };
    let login = match handle.filter(|h| !h.is_empty()) {
        Some(h) => h,
        None => twitch_login(app, &id).await.unwrap_or_else(|| id.clone()),
    };
    Ok(Some((login, id)))
}
/// The member's own Kick channel: their Discord is linked to a S.V.E.R account that signed in with
/// Kick. (slug, slug), or why it can't be linked right now.
async fn proven_kick(app: &App, discord: &str) -> Res<Result<Option<(String, String)>, String>> {
    let kick_id: Option<String> = sqlx::query_scalar(
        "SELECT k.subject FROM identities d
        JOIN channel_users u ON u.id=d.user_id AND u.eligible AND u.deleted_at IS NULL
        JOIN identities k ON k.user_id=u.id AND k.provider='kick'
        WHERE d.provider='discord' AND d.subject=$1",
    )
    .bind(discord)
    .fetch_optional(&app.db)
    .await?;
    let Some(kick_id) = kick_id else {
        return Ok(Ok(None));
    };
    if kick_keys().is_none() {
        return Ok(Err("Kick alerts aren't available yet.".into()));
    }
    let Some(token) = kick_token(app).await else {
        return Ok(Err("Kick didn't answer. Try again in a minute.".into()));
    };
    let body: Option<Value> = async {
        app.http
            .get(format!(
                "{}/public/v1/channels",
                env_or("KICK_API_BASE", "https://api.kick.com")
            ))
            .query(&[("broadcaster_user_id", kick_id.as_str())])
            .bearer_auth(token)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()
    }
    .await;
    let Some(body) = body else {
        return Ok(Err("Kick didn't answer. Try again in a minute.".into()));
    };
    Ok(Ok(body["data"]
        .as_array()
        .and_then(|c| {
            c.iter().find(|c| {
                let id = &c["broadcaster_user_id"];
                id.as_u64().map(|n| n.to_string()).as_deref() == Some(kick_id.as_str())
                    || id.as_str() == Some(kick_id.as_str())
            })
        })
        .and_then(|c| c["slug"].as_str())
        .map(|slug| (slug.to_owned(), slug.to_owned()))))
}
async fn save(app: &App, user: &str, platform: &str, handle: &str, id: &str) -> Res<()> {
    let old: Option<(Option<String>, Vec<String>)> = sqlx::query_as(
        "SELECT platform_id,eventsub FROM discord_stream_accounts WHERE discord_user=$1 AND platform=$2",
    )
    .bind(user)
    .bind(platform)
    .fetch_optional(&app.db)
    .await?;
    sqlx::query("INSERT INTO discord_stream_accounts(discord_user,platform,handle,platform_id) VALUES($1,$2,$3,$4)
        ON CONFLICT(discord_user,platform) DO UPDATE SET handle=$3,platform_id=$4,
            eventsub=CASE WHEN discord_stream_accounts.platform_id=$4 THEN discord_stream_accounts.eventsub ELSE '{}' END,
            live_since=CASE WHEN discord_stream_accounts.platform_id=$4 THEN discord_stream_accounts.live_since END")
        .bind(user).bind(platform).bind(handle).bind(id).execute(&app.db).await?;
    sqlx::query(
        "INSERT INTO discord_streamer_profiles(discord_user) VALUES($1) ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .execute(&app.db)
    .await?;
    if let Some((Some(was), subs)) = old
        && was != id
    {
        drop_subscriptions(app, platform, &was, &subs).await?;
    }
    Ok(())
}
/// Removes Twitch go-live subscriptions nobody's account still needs.
async fn drop_subscriptions(app: &App, platform: &str, id: &str, subs: &[String]) -> Res<()> {
    let shared: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM discord_stream_accounts WHERE platform=$1 AND platform_id=$2)",
    )
    .bind(platform)
    .bind(id)
    .fetch_one(&app.db)
    .await?;
    if platform == "twitch" && !shared {
        twitch_unsubscribe(app, subs).await;
    }
    Ok(())
}

async fn link_command(app: App, i: Interaction) -> Res<Value> {
    let platform = i
        .option("platform")
        .and_then(Value::as_str)
        .unwrap_or_default();
    link(&app, &i.user, platform).await
}
async fn link(app: &App, user: &str, platform: &str) -> Res<Value> {
    if platform == "sver" {
        return sver_link(app, user).await;
    }
    if platform != "twitch" && platform != "kick" {
        return Ok(private(format!(
            "{name} channels can't be linked yet. Go-live posts need proof the channel is yours, and S.V.E.R can't confirm {name} accounts yet.",
            name = platform_name(platform)
        )));
    }
    let name = platform_name(platform);
    if sync::linked(app, user).await?.is_none() {
        return Ok(private(format!(
            "To prove the channel is yours, link your Discord to S.V.E.R first: sign in with Discord, or connect it under sign-in methods at {}/account. Then connect {name} there too, and run `/alerts link` again.",
            app.config.origin
        )));
    }
    let proven = if platform == "kick" {
        match proven_kick(app, user).await? {
            Ok(found) => found,
            Err(why) => return Ok(private(why)),
        }
    } else {
        proven_twitch(app, user).await?
    };
    let Some((handle, id)) = proven else {
        return Ok(private(format!(
            "Connect {name} under sign-in methods at {}/account to prove the channel is yours, then run `/alerts link` again.",
            app.config.origin
        )));
    };
    // The proven owner takes the channel from anyone who linked it without proof.
    let others: Vec<(String, Vec<String>)> = sqlx::query_as("DELETE FROM discord_stream_accounts WHERE platform=$1 AND platform_id=$2 AND discord_user<>$3 RETURNING discord_user,eventsub")
        .bind(platform).bind(&id).bind(user).fetch_all(&app.db).await?;
    save(app, user, platform, &handle, &id).await?;
    if let Some((_, subs)) = others.into_iter().find(|o| !o.1.is_empty()) {
        // Ours now carries the subscriptions, so the next pass doesn't make them twice.
        sqlx::query("UPDATE discord_stream_accounts SET eventsub=$3 WHERE discord_user=$1 AND platform=$2 AND cardinality(eventsub)=0")
            .bind(user).bind(platform).bind(&subs).execute(&app.db).await?;
    }
    Ok(private(format!(
        "{} **{handle}** is linked. When you go live on {}, a post goes up in the go-live channel. `/alerts off` stops the posts.",
        platform_emoji(platform),
        platform_name(platform)
    )))
}
/// The S.V.E.R account comes from the Discord link itself (`identities`).
async fn sver_link(app: &App, user: &str) -> Res<Value> {
    let Some(account) = sync::linked(app, user).await? else {
        return Ok(private(format!(
            "Your S.V.E.R go-lives post here once your accounts are linked: sign in to S.V.E.R with Discord, or link Discord on your account page: {}/account",
            app.config.origin
        )));
    };
    sqlx::query("DELETE FROM discord_autolink_optouts WHERE discord_user=$1 AND platform='sver'")
        .bind(user)
        .execute(&app.db)
        .await?;
    save(app, user, "sver", &account.username, &account.id).await?;
    Ok(private(format!(
        "⚔️ Your S.V.E.R channel **{}** is linked: your S.V.E.R go-lives post in the go-live channel.",
        account.username
    )))
}

type Gone = (
    String,
    Option<String>,
    Vec<String>,
    Option<String>,
    Option<String>,
);
async fn unlink(app: &App, user: &str, platform: &str) -> Res<Value> {
    let gone: Option<Gone> = sqlx::query_as("DELETE FROM discord_stream_accounts WHERE discord_user=$1 AND platform=$2 RETURNING handle,platform_id,eventsub,message_id,post_channel")
        .bind(user).bind(platform).fetch_optional(&app.db).await?;
    let Some((handle, id, subs, m, c)) = gone else {
        return Ok(private(format!(
            "You have no {} account linked.",
            platform_name(platform)
        )));
    };
    // Never linked again automatically.
    sqlx::query("INSERT INTO discord_autolink_optouts(discord_user,platform) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(user).bind(platform).execute(&app.db).await?;
    if let (Some(m), Some(c)) = (m, c) {
        call(
            app,
            Method::DELETE,
            &format!("/channels/{c}/messages/{m}"),
            None,
        )
        .await;
    }
    if let Some(id) = id {
        drop_subscriptions(app, platform, &id, &subs).await?;
    }
    Ok(private(format!(
        "{} **{handle}** is unlinked.",
        platform_name(platform)
    )))
}

async fn alerts(app: &App, i: &Interaction) -> Res<Value> {
    let sub = i.sub().unwrap_or_default();
    if sub == "unlink" {
        let platform = i
            .option("platform")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return unlink(app, &i.user, platform).await;
    }
    if matches!(sub, "on" | "off") {
        sqlx::query("UPDATE discord_stream_accounts SET alerts=$2 WHERE discord_user=$1")
            .bind(&i.user)
            .bind(sub == "on")
            .execute(&app.db)
            .await?;
    }
    let rows: Vec<(String, String, bool, bool)> = sqlx::query_as("SELECT platform,handle,alerts,live_since IS NOT NULL FROM discord_stream_accounts WHERE discord_user=$1 ORDER BY platform")
        .bind(&i.user).fetch_all(&app.db).await?;
    if rows.is_empty() {
        return Ok(private(
            "You have no streaming accounts linked. `/alerts link` adds one.",
        ));
    }
    let lines: Vec<String> = rows
        .iter()
        .map(|(p, h, on, live)| {
            format!(
                "{} **{}**: {h}{} · posts {}",
                platform_emoji(p),
                platform_name(p),
                if *live { " 🔴 LIVE" } else { "" },
                if *on { "on" } else { "off" }
            )
        })
        .collect();
    let title = match sub {
        "on" => "Go-live posts are on",
        "off" => "Go-live posts are off",
        _ => "Your streaming accounts",
    };
    Ok(embed(
        json!({"title": title, "description": lines.join("\n"), "color": 0x8b_9bb4}),
        true,
    ))
}

// ---- Auto-detect ----

/// PRESENCE_UPDATE on the home server: a member streaming on Twitch with no Twitch account linked
/// (and who never unlinked one) is linked and told how to undo it. At most once an hour each.
pub async fn presence(app: &App, d: &Value) -> Res<()> {
    static SEEN: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let (Some(user), Some(guild)) = (d["user"]["id"].as_str(), d["guild_id"].as_str()) else {
        return Ok(());
    };
    let Some(url) = d["activities"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["type"] == 1)
        .and_then(|a| a["url"].as_str())
        .filter(|u| u.contains("twitch.tv/"))
    else {
        return Ok(());
    };
    if Some(guild) != home::guild(app).await.as_deref() {
        return Ok(());
    }
    {
        let hour = std::time::Duration::from_secs(3600);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        let seen = seen.get_or_insert_with(HashMap::new);
        if seen.get(user).is_some_and(|t| t.elapsed() < hour) {
            return Ok(());
        }
        seen.retain(|_, t| t.elapsed() < hour);
        seen.insert(user.into(), Instant::now());
    }
    let skip: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_stream_accounts WHERE discord_user=$1 AND platform='twitch') OR EXISTS(SELECT 1 FROM discord_autolink_optouts WHERE discord_user=$1 AND platform='twitch')")
        .bind(user).fetch_one(&app.db).await?;
    let login = url
        .split("twitch.tv/")
        .nth(1)
        .and_then(|s| s.split(['/', '?']).next())
        .unwrap_or_default();
    if skip || login.is_empty() {
        return Ok(());
    }
    // Only the member's own, proven channel: a status pointing at someone else's links nothing.
    let Some((handle, id)) = proven_twitch(app, user)
        .await?
        .filter(|(h, _)| h.eq_ignore_ascii_case(login))
    else {
        return Ok(());
    };
    save(app, user, "twitch", &handle, &id).await?;
    // Linked once: unlinking later is final.
    sqlx::query("INSERT INTO discord_autolink_optouts(discord_user,platform) VALUES($1,'twitch') ON CONFLICT DO NOTHING")
        .bind(user).execute(&app.db).await?;
    super::dm(app, user, json!({"content": format!(
        "🎉 You're streaming on Twitch as **{handle}**, so your go-lives now post in the S.V.E.R server's go-live channel. `/alerts off` turns the posts off; `/alerts unlink` removes the link."
    ), "allowed_mentions": {"parse": []}})).await;
    Ok(())
}

// ---- The streamer panel ----

async fn panel(app: App, i: Interaction) -> Res<Value> {
    let settings = home::settings(&app).await;
    let Some(to) = i
        .option("channel")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| home::id(&settings, "go_live"))
        .or(i.channel.clone())
    else {
        return Ok(private("Pick a channel."));
    };
    let button = |p: &str, style: u8| json!({"type": 2, "style": style, "label": platform_name(p), "emoji": {"name": platform_emoji(p)}, "custom_id": format!("str:link:{p}")});
    let body = json!({"embeds": [{"title": "🎮 Link your streaming account", "color": 0x9b_59b6,
        "description": format!("Get a post in the go-live channel whenever you go live. Link your Discord to S.V.E.R first ({}/account, sign-in methods); for Twitch or Kick, connect it there too, so we know the channel is yours. Then pick your platform below.", app.config.origin),
        "fields": [
            {"name": "🔔 Go-live posts", "value": "Your stream is announced automatically.", "inline": true},
            {"name": "⚡ Auto-detect", "value": "Streaming on your own Twitch channel? It's picked up from your Discord status.", "inline": true}]}],
        "components": [{"type": 1, "components": [button("sver", 2), button("twitch", 1), button("kick", 3)]}],
        "allowed_mentions": {"parse": []}});
    let Some(m) = send(&app, &to, body).await else {
        return Ok(private(
            "The panel couldn't be posted there. Check the bot can send messages in that channel.",
        ));
    };
    call(&app, Method::PUT, &format!("/channels/{to}/pins/{m}"), None).await;
    Ok(private(format!(
        "Streamer panel posted and pinned in <#{to}>."
    )))
}

async fn button_link(app: App, i: Interaction) -> Res<Value> {
    let platform = i.custom_id().rsplit(':').next().unwrap_or_default();
    link(&app, &i.user, platform).await
}
/// Buttons and forms whose id starts with `str:`.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let id = i.custom_id();
    let platform = id.rsplit(':').next().unwrap_or_default();
    if !PLATFORMS.contains(&platform) {
        return None;
    }
    // The S.V.E.R button explains the automatic link (the legacy one opened the wrong form); the
    // others link the proven account at once (older panels' YouTube and Kick buttons say why not).
    if id.starts_with("str:link:") {
        later(app, i, true, button_link);
        return Some(ANSWERED);
    }
    None
}

// ---- Profiles, directory and search ----

#[derive(sqlx::FromRow)]
struct Profile {
    bio: Option<String>,
    tags: Vec<String>,
    games: Vec<String>,
    collab: bool,
    collab_platforms: Vec<String>,
    collab_content: Vec<String>,
    collab_size: i16,
    collab_notes: String,
    last_live_at: Option<DateTime<Utc>>,
}
async fn profile(app: &App, user: &str) -> Res<Option<Profile>> {
    Ok(sqlx::query_as("SELECT bio,tags,games,collab,collab_platforms,collab_content,collab_size,collab_notes,last_live_at FROM discord_streamer_profiles WHERE discord_user=$1")
        .bind(user).fetch_optional(&app.db).await?)
}
pub(super) fn faction_line(faction: &str) -> String {
    format!(
        "{} {}",
        levels::emoji(Some(faction)),
        super::factions::title(faction)
    )
}

async fn view(app: &App, i: &Interaction) -> Res<Value> {
    let user = i.user_or_me("user");
    let (name, avatar) = i.profile(&user);
    let Some(p) = profile(app, &user).await? else {
        return Ok(private(format!(
            "{name} has no streamer profile yet. `/alerts link` starts one."
        )));
    };
    let accounts: Vec<(String, String, bool)> = sqlx::query_as("SELECT platform,handle,live_since IS NOT NULL FROM discord_stream_accounts WHERE discord_user=$1 ORDER BY platform")
        .bind(&user).fetch_all(&app.db).await?;
    let mut fields = Vec::new();
    if !accounts.is_empty() {
        let lines: Vec<String> = accounts
            .iter()
            .map(|(pl, h, live)| {
                format!(
                    "{} [{h}]({}){}",
                    platform_emoji(pl),
                    link_url(app, pl, h, None),
                    if *live { " 🔴 LIVE" } else { "" }
                )
            })
            .collect();
        fields.push(json!({"name": "Platforms", "value": lines.join("\n")}));
    }
    if let Some(a) = sync::linked(app, &user).await? {
        fields.push(json!({"name": "S.V.E.R", "value": format!("[{}]({}/{})", a.username, app.config.origin, a.username), "inline": true}));
    }
    if !p.games.is_empty() {
        let games: Vec<String> = p.games.iter().map(|g| format!("🎮 {g}")).collect();
        fields.push(json!({"name": "Games", "value": games.join("\n"), "inline": true}));
    }
    if !p.tags.is_empty() {
        let tags: Vec<String> = p.tags.iter().map(|t| format!("#{t}")).collect();
        fields.push(json!({"name": "Tags", "value": tags.join(" "), "inline": true}));
    }
    let faction = levels::faction(app, &user).await?;
    if let Some(f) = &faction {
        fields.push(json!({"name": "Faction", "value": faction_line(f), "inline": true}));
    }
    if p.collab {
        let mut text = "✅ Open for collabs".to_string();
        if !p.collab_platforms.is_empty() {
            let names: Vec<&str> = p
                .collab_platforms
                .iter()
                .map(|x| platform_name(x))
                .collect();
            text.push_str(&format!("\n**Platforms:** {}", names.join(", ")));
        }
        if !p.collab_content.is_empty() {
            text.push_str(&format!("\n**Content:** {}", p.collab_content.join(", ")));
        }
        text.push_str(&format!("\n**Group size:** up to {}", p.collab_size));
        if !p.collab_notes.is_empty() {
            text.push_str(&format!("\n*{}*", p.collab_notes));
        }
        fields.push(json!({"name": "Collab", "value": text}));
    }
    let schedule = schedule_lines(app, &user).await?;
    if !schedule.is_empty() {
        fields.push(json!({"name": "Schedule", "value": schedule.join("\n")}));
    }
    if let Some(t) = p.last_live_at {
        fields.push(json!({"name": "Last live", "value": format!("<t:{}:R>", t.timestamp()), "inline": true}));
    }
    let mut e = json!({"title": format!("{name}'s streamer profile"), "color": levels::color(faction.as_deref()), "fields": fields});
    if let Some(bio) = p.bio {
        e["description"] = json!(bio);
    }
    if let Some(a) = avatar {
        e["thumbnail"] =
            json!({"url": format!("https://cdn.discordapp.com/avatars/{user}/{a}.png")});
    }
    Ok(embed(e, false))
}

fn items(value: &str, tags: bool) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| {
            if tags {
                v.trim_start_matches('#').to_lowercase()
            } else {
                v.to_string()
            }
        })
        .collect()
}
async fn edit(app: &App, i: &Interaction) -> Res<Value> {
    let has: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM discord_streamer_profiles WHERE discord_user=$1)",
    )
    .bind(&i.user)
    .fetch_one(&app.db)
    .await?;
    if !has {
        return Ok(private(
            "Link a streaming account first with `/alerts link`.",
        ));
    }
    let field = i
        .option("field")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let value = i
        .option("value")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let reply = match field {
        "bio" if value.chars().count() > 500 => {
            return Ok(private("A bio can be up to 500 characters."));
        }
        "bio" => {
            sqlx::query("UPDATE discord_streamer_profiles SET bio=nullif($2,''),updated_at=now() WHERE discord_user=$1")
                .bind(&i.user).bind(value).execute(&app.db).await?;
            "Your bio is updated.".to_string()
        }
        "tags" => {
            let tags = items(value, true);
            if tags.len() > 10 || tags.iter().any(|t| t.chars().count() > 32) {
                return Ok(private(
                    "Up to 10 tags of up to 32 characters, separated by commas.",
                ));
            }
            sqlx::query("UPDATE discord_streamer_profiles SET tags=$2,updated_at=now() WHERE discord_user=$1")
                .bind(&i.user).bind(&tags).execute(&app.db).await?;
            let shown: Vec<String> = tags.iter().map(|t| format!("#{t}")).collect();
            format!("Your tags: {}", shown.join(" "))
        }
        "games" => {
            let games = items(value, false);
            if games.len() > 20 || games.iter().any(|g| g.chars().count() > 64) {
                return Ok(private(
                    "Up to 20 games of up to 64 characters, separated by commas.",
                ));
            }
            sqlx::query("UPDATE discord_streamer_profiles SET games=$2,updated_at=now() WHERE discord_user=$1")
                .bind(&i.user).bind(&games).execute(&app.db).await?;
            format!("Your games: {}", games.join(", "))
        }
        "collab" => {
            // "yes;twitch,youtube;fps,variety;4;notes"
            let parts: Vec<&str> = value.split(';').collect();
            let on = parts.first().is_some_and(|p| {
                matches!(
                    p.trim().to_lowercase().as_str(),
                    "yes" | "y" | "on" | "true"
                )
            });
            let platforms: Vec<String> = parts
                .get(1)
                .map(|p| items(p, true))
                .unwrap_or_default()
                .into_iter()
                .filter(|p| PLATFORMS.contains(&p.as_str()))
                .collect();
            let content: Vec<String> = parts
                .get(2)
                .map(|p| items(p, false))
                .unwrap_or_default()
                .into_iter()
                .take(10)
                .map(|c| c.chars().take(32).collect())
                .collect();
            let size = parts
                .get(3)
                .and_then(|s| s.trim().parse::<i16>().ok())
                .unwrap_or(4)
                .clamp(2, 20);
            let notes: String = parts
                .get(4)
                .map(|s| s.trim())
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect();
            sqlx::query("UPDATE discord_streamer_profiles SET collab=$2,collab_platforms=$3,collab_content=$4,collab_size=$5,collab_notes=$6,updated_at=now() WHERE discord_user=$1")
                .bind(&i.user).bind(on).bind(&platforms).bind(&content).bind(size).bind(&notes).execute(&app.db).await?;
            if on {
                "You're listed as open for collabs.".into()
            } else {
                "You're no longer listed as open for collabs.".into()
            }
        }
        // One schedule, not the legacy bot's two.
        "schedule" => {
            return Ok(private(
                "Set your schedule day by day with `/schedule set`.",
            ));
        }
        _ => return Ok(private("Pick bio, tags, games or collab.")),
    };
    Ok(private(reply))
}

type Found = (
    String,
    Option<String>,
    Vec<String>,
    Vec<String>,
    bool,
    Option<String>,
    Vec<String>,
    Option<String>,
);
async fn search(app: &App, i: &Interaction) -> Res<Value> {
    let text = |k: &str| {
        i.option(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    let flag = |k: &str| i.option(k).and_then(Value::as_bool) == Some(true);
    // Every filter is optional; games and tags match by part, ignoring case. The platform filter
    // reads the linked accounts (the legacy one read profile columns nothing filled).
    let rows: Vec<Found> = sqlx::query_as("SELECT p.discord_user,p.bio,p.games,p.tags,p.collab,m.faction,
            coalesce((SELECT array_agg(a.platform ORDER BY a.platform) FROM discord_stream_accounts a WHERE a.discord_user=p.discord_user),'{}'),
            (SELECT a.platform FROM discord_stream_accounts a WHERE a.discord_user=p.discord_user AND a.live_since IS NOT NULL LIMIT 1)
        FROM discord_streamer_profiles p LEFT JOIN discord_members m ON m.discord_user=p.discord_user
        WHERE ($1::text IS NULL OR EXISTS(SELECT 1 FROM unnest(p.games) g WHERE strpos(lower(g),lower($1))>0))
          AND ($2::text IS NULL OR EXISTS(SELECT 1 FROM unnest(p.tags) t WHERE strpos(lower(t),lower($2))>0))
          AND ($3::text IS NULL OR m.faction=$3)
          AND ($4::text IS NULL OR EXISTS(SELECT 1 FROM discord_stream_accounts a WHERE a.discord_user=p.discord_user AND a.platform=$4))
          AND (NOT $5 OR p.collab)
          AND (NOT $6 OR EXISTS(SELECT 1 FROM discord_stream_accounts a WHERE a.discord_user=p.discord_user AND a.live_since IS NOT NULL))
        ORDER BY p.featured DESC,p.last_live_at DESC NULLS LAST,p.created_at DESC LIMIT 11")
        .bind(text("game")).bind(text("tag")).bind(text("faction")).bind(text("platform")).bind(flag("collab")).bind(flag("live"))
        .fetch_all(&app.db).await?;
    if rows.is_empty() {
        return Ok(private("No streamers match that."));
    }
    let more = rows.len() > 10;
    let entries: Vec<String> = rows
        .into_iter()
        .take(10)
        .map(
            |(user, bio, games, tags, collab, faction, platforms, live)| {
                let live = live
                    .map(|p| format!(" 🔴 LIVE on {}", platform_name(&p)))
                    .unwrap_or_default();
                let mut parts = vec![format!("<@{user}>{live}")];
                if let Some(b) = bio {
                    let short: String = b.chars().take(100).collect();
                    parts.push(if b.chars().count() > 100 {
                        format!("{short}…")
                    } else {
                        short
                    });
                }
                if !games.is_empty() {
                    parts.push(format!("🎮 {}", games[..games.len().min(3)].join(", ")));
                }
                if !tags.is_empty() {
                    let shown: Vec<String> = tags.iter().take(5).map(|t| format!("#{t}")).collect();
                    parts.push(shown.join(" "));
                }
                let mut badges: Vec<&str> = platforms.iter().map(|p| platform_emoji(p)).collect();
                if collab {
                    badges.push("✅ collab");
                }
                if let Some(f) = faction.as_deref() {
                    badges.push(levels::emoji(Some(f)));
                }
                parts.push(badges.join(" "));
                parts.join("\n")
            },
        )
        .collect();
    let mut e =
        json!({"title": "🔍 Streamers", "description": entries.join("\n\n"), "color": 0x58_65f2});
    if more {
        e["footer"] = json!({"text": "Showing the first 10. Add a filter to narrow it down."});
    }
    Ok(embed(e, false))
}

// ---- Schedules ----

/// A legacy abbreviation or an IANA zone.
pub(super) fn zone(name: &str) -> Option<Tz> {
    let name = match name.trim().to_uppercase().as_str() {
        "EST" | "EDT" | "ET" => "America/New_York",
        "CST" | "CDT" | "CT" => "America/Chicago",
        "MST" | "MDT" | "MT" => "America/Denver",
        "PST" | "PDT" | "PT" => "America/Los_Angeles",
        "UTC" | "GMT" => "UTC",
        _ => name.trim(),
    };
    name.parse().ok()
}
/// "20:00", "8pm", "8:30 PM" or "20".
pub(super) fn clock(text: &str) -> Option<NaiveTime> {
    let t = text.trim().to_lowercase().replace(' ', "");
    let (t, pm) = match (t.strip_suffix("pm"), t.strip_suffix("am")) {
        (Some(t), _) => (t.to_string(), Some(true)),
        (_, Some(t)) => (t.to_string(), Some(false)),
        _ => (t.clone(), None),
    };
    let (h, m) = t.split_once(':').unwrap_or((&t, "0"));
    let (mut h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    match pm {
        Some(_) if !(1..=12).contains(&h) => return None,
        Some(true) if h != 12 => h += 12,
        Some(false) if h == 12 => h = 0,
        _ => {}
    }
    NaiveTime::from_hms_opt(h, m, 0)
}
/// The next time a weekly slot comes round at or after `after`, in its own zone (daylight saving
/// included; a time the clock skips moves an hour later).
fn next(day: u32, at: NaiveTime, zone: Tz, after: DateTime<Utc>) -> DateTime<Utc> {
    let today = after.with_timezone(&zone).date_naive();
    (0..8)
        .map(|n| today + Duration::days(n))
        .filter(|d| d.weekday().num_days_from_sunday() == day)
        .filter_map(|d| {
            let local = d.and_time(at);
            local.and_local_timezone(zone).earliest().or_else(|| {
                (local + Duration::hours(1))
                    .and_local_timezone(zone)
                    .earliest()
            })
        })
        .map(|t| t.with_timezone(&Utc))
        .find(|t| *t >= after)
        .unwrap_or(after + Duration::days(7))
}
#[derive(sqlx::FromRow)]
struct Slot {
    discord_user: String,
    day: i16,
    at: NaiveTime,
    zone: String,
    game: Option<String>,
    description: Option<String>,
    platform: Option<String>,
}
fn slot_line(s: &Slot, with_user: bool) -> (DateTime<Utc>, String) {
    let when = next(
        s.day as u32,
        s.at,
        zone(&s.zone).unwrap_or(Tz::UTC),
        Utc::now(),
    );
    let mut line = format!("🔴 **{}** <t:{}:t>", DAYS[s.day as usize], when.timestamp());
    if with_user {
        line.push_str(&format!(" — <@{}>", s.discord_user));
    }
    line.push_str(&format!(" — {}", s.game.as_deref().unwrap_or("Variety")));
    if let Some(p) = &s.platform {
        line.push_str(&format!(" ({})", platform_name(p)));
    }
    if let Some(d) = &s.description {
        line.push_str(&format!("\n  {d}"));
    }
    (when, line)
}
async fn schedule_lines(app: &App, user: &str) -> Res<Vec<String>> {
    let slots: Vec<Slot> = sqlx::query_as(
        "SELECT discord_user,day,at,zone,game,description,platform FROM discord_schedules WHERE discord_user=$1 ORDER BY day",
    )
    .bind(user)
    .fetch_all(&app.db)
    .await?;
    Ok(slots.iter().map(|s| slot_line(s, false).1).collect())
}
async fn schedule(app: &App, i: &Interaction) -> Res<Value> {
    let day = i
        .option("day")
        .and_then(Value::as_i64)
        .filter(|d| (0..7).contains(d));
    let text = |k: &str, max: usize| {
        i.option(k)
            .and_then(Value::as_str)
            .map(|v| v.trim().chars().take(max).collect::<String>())
            .filter(|v| !v.is_empty())
    };
    match i.sub() {
        Some("set") => {
            let Some(day) = day else {
                return Ok(private("Pick a day."));
            };
            let Some(at) = i.option("time").and_then(Value::as_str).and_then(clock) else {
                return Ok(private("Give a time like 20:00 or 8:00 PM."));
            };
            let fallback = home::settings(app)
                .await
                .get("zone")
                .and_then(Value::as_str)
                .unwrap_or("UTC")
                .to_string();
            let zone_name = text("zone", 40).unwrap_or(fallback);
            let Some(tz) = zone(&zone_name) else {
                return Ok(private(
                    "That time zone isn't known. Use a name like America/New_York or Europe/London.",
                ));
            };
            let platform = i
                .option("platform")
                .and_then(Value::as_str)
                .filter(|p| PLATFORMS.contains(p));
            sqlx::query("INSERT INTO discord_schedules(discord_user,day,at,zone,game,description,platform) VALUES($1,$2,$3,$4,$5,$6,$7)
                ON CONFLICT(discord_user,day) DO UPDATE SET at=$3,zone=$4,game=$5,description=$6,platform=$7")
                .bind(&i.user).bind(day as i16).bind(at).bind(tz.name()).bind(text("game", 100)).bind(text("description", 200)).bind(platform)
                .execute(&app.db).await?;
            let when = next(day as u32, at, tz, Utc::now());
            Ok(private(format!(
                "Scheduled: **{}** at {} {} (<t:{}:t> your time).",
                DAYS[day as usize],
                at.format("%H:%M"),
                tz.name(),
                when.timestamp()
            )))
        }
        Some("clear") => {
            let Some(day) = day else {
                return Ok(private("Pick a day."));
            };
            let gone =
                sqlx::query("DELETE FROM discord_schedules WHERE discord_user=$1 AND day=$2")
                    .bind(&i.user)
                    .bind(day as i16)
                    .execute(&app.db)
                    .await?
                    .rows_affected();
            Ok(private(if gone > 0 {
                format!("Your **{}** slot is cleared.", DAYS[day as usize])
            } else {
                format!("You have nothing on **{}**.", DAYS[day as usize])
            }))
        }
        _ => {
            let user = i.user_or_me("user");
            let (name, _) = i.profile(&user);
            let lines = schedule_lines(app, &user).await?;
            if lines.is_empty() {
                return Ok(private(if user == i.user {
                    "You have no schedule yet. `/schedule set` adds a day.".to_string()
                } else {
                    format!("{name} has no schedule.")
                }));
            }
            Ok(embed(
                json!({"title": format!("📅 {name}'s stream schedule"), "description": lines.join("\n"), "color": 0x34_98db,
                "footer": {"text": "Times show in your own time zone"}}),
                false,
            ))
        }
    }
}

// ---- Weekly posts and milestones ----

/// True once for `job` on `day` from `hour` UTC: the run is recorded, so a restart never repeats it.
async fn weekly(app: &App, now: DateTime<Utc>, job: &str, day: Weekday, hour: u32) -> Res<bool> {
    if now.weekday() != day || now.hour() < hour {
        return Ok(false);
    }
    Ok(sqlx::query(
        "INSERT INTO discord_weekly_runs(job,period) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(job)
    .bind(now.date_naive())
    .execute(&app.db)
    .await?
    .rows_affected()
        == 1)
}
fn either(settings: &Map<String, Value>, first: &str, second: &str) -> Option<String> {
    home::id(settings, first).or_else(|| home::id(settings, second))
}

/// Monday 08:00 UTC: the week's schedule, each slot in every reader's own time zone.
async fn schedule_post(app: &App, now: DateTime<Utc>) -> Res<()> {
    let settings = home::settings(app).await;
    let Some(to) = either(&settings, "stream_promo", "go_live") else {
        return Ok(());
    };
    if !weekly(app, now, "schedule", Weekday::Mon, 8).await? {
        return Ok(());
    }
    let slots: Vec<Slot> = sqlx::query_as(
        "SELECT discord_user,day,at,zone,game,description,platform FROM discord_schedules",
    )
    .fetch_all(&app.db)
    .await?;
    let mut lines: Vec<(DateTime<Utc>, String)> =
        slots.iter().map(|s| slot_line(s, true)).collect();
    lines.sort_by_key(|l| l.0);
    let mut text = String::new();
    for (_, line) in &lines {
        if text.len() + line.len() > 3900 {
            text.push_str("\n…and more: `/schedule view` shows anyone's.");
            break;
        }
        text.push_str(line);
        text.push('\n');
    }
    if text.is_empty() {
        text = "*No streams scheduled this week.*\n\nAdd yours with `/schedule set`.".into();
    }
    send(app, &to, message(json!({"title": "📅 This week's stream schedule", "description": text, "color": 0x34_98db,
        "footer": {"text": "S.V.E.R bot • Times show in your own time zone • /schedule set"}}))).await;
    Ok(())
}

/// Monday 10:00 UTC: the Streamer of the Week, by minutes streamed in the last 7 days (time live on
/// several platforms at once counts once).
async fn spotlight(app: &App, now: DateTime<Utc>) -> Res<()> {
    if either(&home::settings(app).await, "go_live", "announcements").is_none()
        || !weekly(app, now, "spotlight", Weekday::Mon, 10).await?
    {
        return Ok(());
    }
    streamer_spotlight(app).await.map(drop)
}
/// Posts the Streamer of the Week, unless one went up in the last 6 days or nobody streamed: the
/// winner, when posted (also staff's `/rituals spotlight streamer`).
pub(super) async fn streamer_spotlight(app: &App) -> Res<Option<String>> {
    let Some(to) = either(&home::settings(app).await, "go_live", "announcements") else {
        return Ok(None);
    };
    let recent: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_spotlights WHERE kind='streamer' AND created_at>now()-interval '6 days')")
        .fetch_one(&app.db).await?;
    if recent {
        return Ok(None);
    }
    let top: Option<(String, f64)> = sqlx::query_as("SELECT discord_user,sum(extract(epoch FROM upper(r)-lower(r)))::float8/60 FROM (
            SELECT discord_user,unnest(range_agg(tstzrange(greatest(started_at,now()-interval '7 days'),least(ended_at,now())))) r FROM (
                SELECT discord_user,started_at,ended_at FROM discord_stream_sessions WHERE ended_at>now()-interval '7 days'
                UNION ALL SELECT discord_user,live_since,now() FROM discord_stream_accounts WHERE live_since IS NOT NULL) s
            WHERE least(ended_at,now())>greatest(started_at,now()-interval '7 days') GROUP BY discord_user) m
        GROUP BY discord_user ORDER BY 2 DESC LIMIT 1")
        .fetch_optional(&app.db).await?;
    let Some((user, minutes)) = top.filter(|t| t.1 >= 1.0) else {
        return Ok(None);
    };
    let faction = levels::faction(app, &user).await?;
    let reason = format!(
        "Top streamer this week with {} streamed!",
        hours(minutes.round() as i64)
    );
    let id: i64 = sqlx::query_scalar("INSERT INTO discord_spotlights(kind,discord_user,title,reason,faction,channel_id) VALUES('streamer',$1,'🎬 Streamer of the Week',$2,$3,$4) RETURNING id")
        .bind(&user).bind(&reason).bind(&faction).bind(&to).fetch_one(&app.db).await?;
    let mut fields = Vec::new();
    if let Some(f) = &faction {
        fields.push(json!({"name": "Faction", "value": faction_line(f), "inline": true}));
    }
    if let Some(p) = profile(app, &user).await?.filter(|p| !p.games.is_empty()) {
        fields.push(json!({"name": "Games", "value": p.games[..p.games.len().min(5)].join(", "), "inline": true}));
    }
    let accounts: Vec<(String, String)> = sqlx::query_as(
        "SELECT platform,handle FROM discord_stream_accounts WHERE discord_user=$1 ORDER BY platform",
    )
    .bind(&user)
    .fetch_all(&app.db)
    .await?;
    if !accounts.is_empty() {
        let links: Vec<String> = accounts
            .iter()
            .map(|(p, h)| format!("[{}]({})", platform_name(p), link_url(app, p, h, None)))
            .collect();
        fields.push(json!({"name": "Links", "value": links.join(" · "), "inline": true}));
    }
    let body = json!({"content": format!("🎬 **Streamer of the Week:** <@{user}>"), "allowed_mentions": {"parse": []},
        "embeds": [{"title": "🎬 Streamer of the Week", "description": reason, "color": levels::color(faction.as_deref()), "fields": fields, "timestamp": Utc::now().to_rfc3339()}]});
    if let Some(m) = send(app, &to, body).await {
        sqlx::query("UPDATE discord_spotlights SET message_id=$2 WHERE id=$1")
            .bind(id)
            .bind(&m)
            .execute(&app.db)
            .await?;
        let react = format!("/channels/{to}/messages/{m}/reactions/%F0%9F%8E%89/@me");
        call(app, Method::PUT, &react, None).await;
    }
    Ok(Some(user))
}

/// Sunday 18:00 UTC: a clip of the week in the clips channel (a forum gets a post of its own),
/// drawn fairly from the week's public S.V.E.R clips. Never the most watched: nothing on S.V.E.R is
/// ranked by viewers (Joe, October 20, 2026), so the post shows no view count either.
async fn clip_of_the_week(app: &App, now: DateTime<Utc>) -> Res<()> {
    let settings = home::settings(app).await;
    let Some(to) = home::id(&settings, "clips") else {
        return Ok(());
    };
    if !weekly(app, now, "clip", Weekday::Sun, 18).await? {
        return Ok(());
    }
    let clip: Option<(String, String, String, Option<String>)> = sqlx::query_as("SELECT v.id,v.title,u.display_name,v.faction FROM videos v JOIN channel_users u ON u.id=v.owner_id AND u.eligible
        WHERE v.kind='CLIP' AND v.status='READY' AND v.approval='APPROVED' AND v.visibility='PUBLIC' AND NOT v.mature AND v.created_at>now()-interval '7 days'
            AND NOT EXISTS(SELECT 1 FROM video_holds h WHERE h.video_id=v.id)
        ORDER BY random() LIMIT 1")
        .fetch_optional(&app.db).await?;
    let Some((id, title, by, faction)) = clip else {
        return Ok(());
    };
    let url = format!("{}/clips/{id}", app.config.origin);
    let badge = faction
        .as_deref()
        .map(|f| levels::emoji(Some(f)))
        .unwrap_or_default();
    let post = json!({"embeds": [{"title": "🎬 Clip of the Week", "description": title, "url": url, "color": levels::color(faction.as_deref()),
            "fields": [{"name": "By", "value": format!("{by} {badge}"), "inline": true}],
            "footer": {"text": "S.V.E.R bot • React 👍 if you enjoyed it"}}],
        "components": [{"type": 1, "components": [{"type": 2, "style": 5, "label": "▶️ Watch the clip", "url": url}]}],
        "allowed_mentions": {"parse": []}});
    let forum = matches!(call(app, Method::GET, &format!("/channels/{to}"), None).await, Some((200, c)) if c["type"] == 15);
    if forum {
        let name: String = format!("🎬 Clip of the Week — {title}")
            .chars()
            .take(100)
            .collect();
        let thread = json!({"name": name, "message": post});
        call(
            app,
            Method::POST,
            &format!("/channels/{to}/threads"),
            Some(thread),
        )
        .await;
    } else {
        send(app, &to, post).await;
    }
    Ok(())
}

/// Every 30 minutes: S.V.E.R follower milestones (10 to 10,000) of linked streamers. Every threshold
/// reached is recorded at once and only the highest is posted.
pub async fn milestones(app: &App) -> Res<()> {
    let settings = home::settings(app).await;
    let Some(to) = either(&settings, "go_live", "announcements") else {
        return Ok(());
    };
    let rows: Vec<(String, i64)> = sqlx::query_as("SELECT a.discord_user,(SELECT count(*) FROM follows f JOIN channel_users c ON c.id=f.follower_id WHERE f.following_id=a.platform_id AND NOT c.internal AND c.deleted_at IS NULL)
        FROM discord_stream_accounts a WHERE a.platform='sver' AND a.platform_id IS NOT NULL")
        .fetch_all(&app.db).await?;
    for (user, followers) in rows {
        // The first look at an account records where it already is without a post, so linking
        // a channel with 500 followers doesn't announce that it "just hit" 500.
        let first = sqlx::query("INSERT INTO discord_milestones(discord_user,kind,value) VALUES($1,'followers',0) ON CONFLICT DO NOTHING")
            .bind(&user).execute(&app.db).await?.rows_affected() == 1;
        let reached: Vec<i32> = MILESTONES
            .iter()
            .filter(|m| **m <= followers)
            .map(|m| *m as i32)
            .collect();
        let new: Vec<i32> = sqlx::query_scalar("INSERT INTO discord_milestones(discord_user,kind,value) SELECT $1,'followers',v FROM unnest($2::int[]) v ON CONFLICT DO NOTHING RETURNING value")
            .bind(&user).bind(&reached).fetch_all(&app.db).await?;
        let Some(top) = new.into_iter().max().filter(|_| !first) else {
            continue;
        };
        let faction = levels::faction(app, &user).await?;
        let line = faction
            .as_deref()
            .map(|f| format!("\n{}", faction_line(f)))
            .unwrap_or_default();
        send(app, &to, json!({"content": format!("🎉 <@{user}> just hit **{top} followers** on S.V.E.R!{line}\n\nKeep going! 💪"),
            "allowed_mentions": {"parse": []}})).await;
    }
    Ok(())
}

/// The schedule (Monday 08:00 UTC), the spotlight (Monday 10:00) and the clip of the week (Sunday
/// 18:00), each once in its week, as of `now`.
pub async fn weekly_posts(app: &App, now: DateTime<Utc>) -> Res<()> {
    schedule_post(app, now).await?;
    spotlight(app, now).await?;
    clip_of_the_week(app, now).await
}

/// Each minute (on the Gateway's ticker): S.V.E.R go-lives, Twitch subscriptions, Kick, and the
/// slower jobs on their own schedules.
pub async fn minute(app: &App) -> Res<()> {
    sver(app).await?;
    twitch_subscribe(app).await?;
    kick_poll(app).await?;
    if super::factions::due("twitch_poll", 3) {
        twitch_poll(app).await?;
    }
    if super::factions::due("youtube_poll", 3) {
        youtube_poll(app).await?;
    }
    tidy(app).await?;
    weekly_posts(app, Utc::now()).await?;
    if super::factions::due("milestones", 30) {
        milestones(app).await?;
    }
    Ok(())
}

// ---- Commands ----

pub fn definitions() -> Vec<Value> {
    let platforms: Vec<Value> = PLATFORMS
        .iter()
        .map(|p| json!({"name": platform_name(p), "value": p}))
        .collect();
    let days: Vec<Value> = [1, 2, 3, 4, 5, 6, 0]
        .iter()
        .map(|d| json!({"name": DAYS[*d], "value": d}))
        .collect();
    let factions = json!([{"name": "Myria", "value": "myria"}, {"name": "Aetheron", "value": "aetheron"}, {"name": "Glint", "value": "glint"}]);
    vec![
        json!({"name": "alerts", "description": "Go-live posts for your streams.", "options": [
            {"type": 1, "name": "link", "description": "Link your own streaming account (proven through S.V.E.R)", "options": [
                {"type": 3, "name": "platform", "description": "The platform", "required": true, "choices": [
                    {"name": "S.V.E.R", "value": "sver"}, {"name": "Twitch", "value": "twitch"}, {"name": "Kick", "value": "kick"}]}]},
            {"type": 1, "name": "unlink", "description": "Unlink a streaming account", "options": [
                {"type": 3, "name": "platform", "description": "The platform", "required": true, "choices": platforms}]},
            {"type": 1, "name": "on", "description": "Turn your go-live posts on"},
            {"type": 1, "name": "off", "description": "Turn your go-live posts off"},
            {"type": 1, "name": "list", "description": "Your linked accounts"}]}),
        json!({"name": "streamer", "description": "Streamer profiles and the directory.", "options": [
            {"type": 1, "name": "view", "description": "A streamer profile", "options": [
                {"type": 6, "name": "user", "description": "Whose (you by default)"}]},
            {"type": 1, "name": "edit", "description": "Edit your streamer profile", "options": [
                {"type": 3, "name": "field", "description": "What to change", "required": true, "choices": [
                    {"name": "Bio", "value": "bio"}, {"name": "Tags", "value": "tags"}, {"name": "Games", "value": "games"},
                    {"name": "Collab (yes;platforms;content;size;notes)", "value": "collab"}, {"name": "Schedule", "value": "schedule"}]},
                {"type": 3, "name": "value", "description": "The new value (commas between items)", "required": true, "max_length": 600}]},
            {"type": 1, "name": "search", "description": "Find streamers", "options": [
                {"type": 3, "name": "game", "description": "Plays this game", "max_length": 64},
                {"type": 3, "name": "tag", "description": "Has this tag", "max_length": 32},
                {"type": 3, "name": "faction", "description": "In this faction", "choices": factions},
                {"type": 3, "name": "platform", "description": "Streams here", "choices": platforms},
                {"type": 5, "name": "collab", "description": "Only those open for collabs"},
                {"type": 5, "name": "live", "description": "Only those live now"}]}]}),
        json!({"name": "schedule", "description": "Stream schedules.", "options": [
            {"type": 1, "name": "set", "description": "Set your stream for a day", "options": [
                {"type": 4, "name": "day", "description": "Day of the week", "required": true, "choices": days},
                {"type": 3, "name": "time", "description": "Start time, like 20:00 or 8:00 PM", "required": true, "max_length": 10},
                {"type": 3, "name": "zone", "description": "Time zone, like America/New_York (the server's by default)", "max_length": 40},
                {"type": 3, "name": "game", "description": "What you'll play", "max_length": 100},
                {"type": 3, "name": "description", "description": "A line about the stream", "max_length": 200},
                {"type": 3, "name": "platform", "description": "Where", "choices": platforms}]},
            {"type": 1, "name": "clear", "description": "Clear a day", "options": [
                {"type": 4, "name": "day", "description": "Day of the week", "required": true, "choices": days}]},
            {"type": 1, "name": "view", "description": "A stream schedule", "options": [
                {"type": 6, "name": "user", "description": "Whose (you by default)"}]}]}),
        json!({"name": "streamerpanel", "description": "Post the streamer link panel.", "default_member_permissions": MANAGE_GUILD.to_string(), "options": [
            {"type": 7, "name": "channel", "description": "Where (the go-live channel by default)", "channel_types": [0, 5]}]}),
    ]
}

/// Answers this module's commands; None when the command isn't one of them.
pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(match (i.name(), i.sub()) {
        ("alerts", Some("link")) => {
            later(app, i, true, link_command);
            ANSWERED
        }
        ("alerts", _) => answer(alerts(app, i).await),
        ("streamer", Some("edit")) => answer(edit(app, i).await),
        ("streamer", Some("search")) => answer(search(app, i).await),
        ("streamer", _) => answer(view(app, i).await),
        ("schedule", _) => answer(schedule(app, i).await),
        ("streamerpanel", _) if !i.staff() => private("Only staff can use that."),
        ("streamerpanel", _) => {
            later(app, i, true, panel);
            ANSWERED
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_times_and_zones() {
        assert_eq!(clock("20:00"), NaiveTime::from_hms_opt(20, 0, 0));
        assert_eq!(clock("8:30 PM"), NaiveTime::from_hms_opt(20, 30, 0));
        assert_eq!(clock("12am"), NaiveTime::from_hms_opt(0, 0, 0));
        assert_eq!(clock("12pm"), NaiveTime::from_hms_opt(12, 0, 0));
        assert_eq!(clock("13pm"), None);
        assert_eq!(clock("25:00"), None);
        assert_eq!(zone("EST").map(|z| z.name()), Some("America/New_York"));
        assert_eq!(
            zone("Europe/London").map(|z| z.name()),
            Some("Europe/London")
        );
        assert!(zone("Mars/Base").is_none());
    }
    #[test]
    fn next_slot_follows_daylight_saving() {
        let ny = zone("America/New_York").unwrap();
        let at = NaiveTime::from_hms_opt(20, 0, 0).unwrap();
        // Monday 20:00 in New York while on EDT is 00:00 UTC Tuesday.
        let after = "2026-10-19T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            next(1, at, ny, after).to_rfc3339(),
            "2026-10-20T00:00:00+00:00"
        );
        // After the November change it's EST: 01:00 UTC.
        let after = "2026-11-09T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            next(1, at, ny, after).to_rfc3339(),
            "2026-11-10T01:00:00+00:00"
        );
        // Already past this week's: next week's.
        let after = "2026-10-20T01:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            next(1, at, ny, after).to_rfc3339(),
            "2026-10-27T00:00:00+00:00"
        );
    }
    #[test]
    fn names_and_durations() {
        assert_eq!(bare("https://www.twitch.tv/SomeOne/"), "SomeOne");
        assert_eq!(bare(" kick_name "), "kick_name");
        assert_eq!(hours(45), "45m");
        assert_eq!(hours(120), "2h");
        assert_eq!(hours(135), "2h 15m");
    }
}
