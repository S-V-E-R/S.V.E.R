//! Community features on the home server (docs/DISCORD_BOT.md "Community") against fake Discord,
//! store and Chromely APIs: the welcome and DM onboarding (and its fallback, and a held member
//! welcomed once approved), one goodbye, birthdays in the member's own zone, anniversaries,
//! seasonal events announced once, spotlights, events with RSVPs, reminders, ends and repeats,
//! polls that close, giveaways drawn on time, reminders, mentors, the role panel, threads and the
//! forum setup, temp voice rooms, custom commands, embeds, free games, the signed GitHub changelog
//! and WoW/Chromely.
use super::*;
use axum::http::{HeaderMap, Method, Uri};
use chrono::{Datelike, Duration, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    sent: usize,
}
type Shared = Arc<Mutex<Fake>>;

async fn fake(
    State(state): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, Json<Value>) {
    let path = uri.path().to_string();
    let query = uri.query().unwrap_or_default().to_string();
    let mut s = state.lock().unwrap();
    s.calls.push(format!("{method} {path}?{query} {body}"));
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    // Joined two years and a few days ago: no anniversary today.
    let two_years = (Utc::now() - Duration::days(733)).to_rfc3339();
    let today_two_years = format!(
        "{}-{:02}-{:02}T12:00:00+00:00",
        Utc::now().year() - 2,
        Utc::now().month(),
        Utc::now().day()
    );
    let answer = match (method.as_str(), parts.as_slice()) {
        // Discord. Member 951 keeps DMs closed.
        ("POST", ["users", "@me", "channels"]) => {
            let to: Value = serde_json::from_str(&body).unwrap();
            if to["recipient_id"] == "951" {
                return (StatusCode::FORBIDDEN, Json(json!({"code": 50007})));
            }
            json!({"id": format!("dm-{}", to["recipient_id"].as_str().unwrap())})
        }
        ("POST", ["channels", _, "messages"]) => {
            s.sent += 1;
            json!({"id": format!("m{}", s.sent)})
        }
        ("POST", ["channels", _, "threads"]) => {
            s.sent += 1;
            json!({"id": format!("th{}", s.sent)})
        }
        ("GET", ["channels", "163" | "164"]) => {
            json!({"type": 15, "available_tags": [{"id": "tag-wow", "name": "WoW"}]})
        }
        ("GET", ["channels", "166"]) => json!({"type": 2, "parent_id": "cat-1"}),
        ("GET", ["channels", "170", "messages", m]) => {
            json!({"id": m, "author": {"id": "500"}, "embeds": [{"title": "Old", "description": "Old text"}]})
        }
        ("GET", ["channels", _]) => json!({"type": 0}),
        // A forum's tags come back with ids.
        ("PATCH", ["channels", _]) => {
            let mut b: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            if let Some(tags) = b["available_tags"].as_array_mut() {
                for (n, t) in tags.iter_mut().enumerate() {
                    t["id"] = json!(format!("tag-{n}"));
                }
            }
            b
        }
        ("GET", ["guilds", "900", "members"]) => json!([
            {"user": {"id": "953", "username": "veteran"}, "roles": [], "joined_at": today_two_years},
            {"user": {"id": "954", "username": "other"}, "roles": [], "joined_at": two_years},
            {"user": {"id": "999", "bot": true}, "roles": [], "joined_at": today_two_years}]),
        ("GET", ["guilds", "900", "members", user]) => {
            json!({"user": {"id": user, "username": format!("u{user}")}, "roles": [], "joined_at": two_years})
        }
        ("POST", ["guilds", "900", "channels"]) => json!({"id": "room-1"}),
        // Role 55 holds moderator permissions (the held-account buttons check them).
        ("GET", ["guilds", "900"]) => json!({"owner_id": "44", "roles": [
            {"id": "900", "position": 0, "permissions": "0"},
            {"id": "55", "position": 6, "permissions": "1099511635990"}]}),
        // Stores.
        ("GET", ["epic"]) => json!({"data": {"Catalog": {"searchStore": {"elements": [
            {"title": "Epic Freebie", "productSlug": "epic-freebie", "keyImages": [], "price": {"totalPrice": {"fmtPrice": {"originalPrice": "$24.99"}}},
             "promotions": {"promotionalOffers": [{"promotionalOffers": [{"startDate": two_years, "endDate": (Utc::now() + Duration::days(3)).to_rfc3339(), "discountSetting": {"discountPercentage": 0}}]}]}}]}}}}),
        ("GET", ["gamerpower"]) => {
            json!([{"title": "Steam Key", "status": "Active", "open_giveaway_url": "https://gp.example/open/1", "worth": "$9.99", "end_date": "N/A"}])
        }
        ("GET", ["steam", "api", "featuredcategories"]) => {
            json!({"specials": {"items": [{"name": "Free Weekend", "url": "https://store.steampowered.com/app/123/Weekend/"}]}})
        }
        ("GET", ["steam", "api", "appdetails"]) => {
            json!({"123": {"success": true, "data": {"name": "Weekend Game", "price_overview": {"initial_formatted": "$29.99"}}}})
        }
        ("GET", ["gog"]) => {
            json!({"products": [{"title": "GOG Gift", "storeLink": "https://www.gog.com/en/game/gog_gift", "price": {"base": "$5.99", "finalMoney": {"amount": "0.00"}}}]})
        }
        // Chromely.
        ("GET", ["chromely", "api", "bot", "character", "Joe-Dream"]) => {
            json!({"name": "Joe", "realm": "Dream", "class": "Rogue", "race": "Human", "level": 60})
        }
        ("GET", ["chromely", "api", "bot", "character", _]) => {
            return (StatusCode::NOT_FOUND, Json(json!({})));
        }
        // The rival string needs the bot's secret.
        ("GET", ["chromely", "api", "bot", "rival-string", "Joe"])
            if headers
                .get("x-bot-secret")
                .is_some_and(|v| v == "test-chromely-secret") =>
        {
            json!({"importString": "CR1|Joe|Rogue|Alliance|3600|0"})
        }
        ("GET", ["chromely", "api", "bot", "rival-string", _]) => {
            return (StatusCode::NOT_FOUND, Json(json!({})));
        }
        ("POST", ["chromely", "api", "chromely", "runs"]) => {
            json!({"success": true, "run_id": 5, "message": "Run accepted at rank 3"})
        }
        ("GET", ["chromely", "api", "bot", "leaderboard"]) => {
            json!([{"name": "Joe", "realm": "Dream", "time": 3725}])
        }
        _ => json!({}),
    };
    (StatusCode::OK, Json(answer))
}

fn hmac(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256={hex}")
}
/// A webhook posted to the API as GitHub or Chromely would: no Origin, its own headers.
async fn hook(e: &Env, path: &str, headers: &[(&str, String)], body: &str) -> StatusCode {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
    for (k, v) in headers {
        request = request.header(*k, v);
    }
    sver::router(e.app.clone())
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
        .status()
}

pub async fn exercise(e: &Env) {
    use sver::discord::{
        commands::interaction,
        community::{events, feeds, rituals, tools},
        gateway::dispatch,
    };
    let state: Shared = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new().fallback(fake).with_state(state.clone());
    tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let base = format!("http://{address}");
    // SAFETY: only the Discord tests read these variables, one at a time.
    unsafe {
        std::env::set_var("DISCORD_API", &base);
        std::env::set_var("EPIC_FREE_GAMES", format!("{base}/epic"));
        std::env::set_var("GAMERPOWER_API", format!("{base}/gamerpower"));
        std::env::set_var("STEAM_STORE_BASE", format!("{base}/steam"));
        std::env::set_var("GOG_CATALOG", format!("{base}/gog"));
        std::env::set_var("CHROMELY_API_URL", format!("{base}/chromely"));
        std::env::set_var("GITHUB_WEBHOOK_SECRET", "test-github-secret");
        std::env::set_var("CHROMELY_WEBHOOK_SECRET", "test-chromely-secret");
    }
    let taken = || std::mem::take(&mut state.lock().unwrap().calls);
    let find = |calls: &[String], needles: &[&str]| {
        let hits = calls
            .iter()
            .filter(|c| needles.iter().all(|n| c.contains(n)))
            .count();
        assert_eq!(hits, 1, "{needles:?} in {calls:#?}");
    };
    let none = |calls: &[String], needle: &str| {
        assert!(
            !calls.iter().any(|c| c.contains(needle)),
            "{needle} in {calls:#?}"
        );
    };
    let count = async |sql: &str| -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
            .fetch_one(&e.app.db)
            .await
            .unwrap()
    };
    // A command, button or form (in the server, or in a DM when `guild` is false) and its answer:
    // the reply, or the edit after "thinking…".
    let ask =
        async |id: &str, user: &str, perms: &str, kind: u64, data: Value, guild: bool| -> String {
            let mut raw =
                json!({"id": id, "token": id, "type": kind, "channel_id": "10", "data": data});
            let who = json!({"id": user, "username": format!("u{user}")});
            if guild {
                raw["guild_id"] = json!("900");
                raw["member"] = json!({"user": who, "permissions": perms, "roles": []});
            } else {
                raw["channel_id"] = json!(format!("dm-{user}"));
                raw["user"] = who;
            }
            interaction(&e.app, raw).await;
            let edit = format!("PATCH /webhooks/app1/{id}/messages/@original");
            for _ in 0..200 {
                let found = {
                    let log = &state.lock().unwrap().calls;
                    let reply = log
                        .iter()
                        .find(|l| l.starts_with(&format!("POST /interactions/{id}/")))
                        .cloned();
                    match reply {
                        // "Thinking…" (5) or a deferred update (6): the answer is the edit after it.
                        Some(r) if r.contains(r#""type":5}"#) || r.contains(r#"{"type":6}"#) => {
                            log.iter().find(|l| l.starts_with(&edit)).cloned()
                        }
                        other => other,
                    }
                };
                if let Some(answer) = found {
                    return answer;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            panic!("no answer to {id}");
        };
    let sub = |name: &str, sub: &str, options: Value| json!({"name": name, "options": [{"type": 1, "name": sub, "options": options}]});
    let opt = |name: &str, value: Value| json!({"name": name, "value": value});
    let button = |id: &str| json!({"custom_id": id, "component_type": 2});
    let submit = |id: &str, fields: &[(&str, &str)]| {
        let rows: Vec<Value> = fields
            .iter()
            .map(|(k, v)| json!({"type": 1, "components": [{"type": 4, "custom_id": k, "value": v}]}))
            .collect();
        json!({"custom_id": id, "components": rows})
    };
    const STAFF: &str = "32";
    const MEMBER: &str = "0";
    // Timeouts, kick, ban, manage messages and channels: a moderator, for the held-account buttons.
    const MODERATOR: &str = "1099511635990";

    e.sql(r#"UPDATE discord_home SET settings=(settings-'raid_joins'-'raid_seconds')||'{"welcome": "150", "goodbye": "151", "community": "252", "pending": "253",
        "role_twitch": "254", "birthdays": "155", "birthday_role": "256", "events": "157", "giveaways": "158", "mod_log": "159", "free_games": "160",
        "free_games_role": "261", "changelog": "162", "game_chat": "163", "wow_leaderboard": "164", "stream_help": "165", "temp_room": "166",
        "mentor_role": "267", "announcements": "142", "new_account_days": 7, "zone": "America/New_York"}'"#).await;
    e.sql("UPDATE discord_lockdowns SET unlocked_at=now() WHERE unlocked_at IS NULL")
        .await;
    e.sql("DELETE FROM discord_joins").await;
    taken();

    // ---- Welcome and onboarding ----
    dispatch(
        &e.app,
        "GUILD_MEMBER_ADD",
        json!({"guild_id": "900", "user": {"id": "950", "username": "newbie", "avatar": "a1"}}),
    )
    .await;
    let c = taken();
    find(&c, &["PUT /guilds/900/members/950/roles/253"]);
    find(
        &c,
        &[
            "POST /channels/150/messages",
            "Welcome to S.V.E.R!",
            "<@950>",
        ],
    );
    find(&c, &["POST /channels/dm-950/messages", "onb:rules"]);
    // Each step moves the same DM along, once.
    let step = ask("ob1", "950", MEMBER, 3, button("onb:rules"), false).await;
    assert!(step.contains("Choose your faction"), "{step}");
    let again = ask("ob2", "950", MEMBER, 3, button("onb:rules"), false).await;
    assert!(again.contains("already done"), "{again}");
    let step = ask("ob3", "950", MEMBER, 3, button("onb:faction:glint"), false).await;
    assert!(step.contains("Where do you stream"), "{step}");
    assert_eq!(
        count("SELECT count(*) FROM discord_members WHERE discord_user='950' AND faction='glint'")
            .await,
        1
    );
    let picked = json!({"custom_id": "onb:platforms", "component_type": 3, "values": ["twitch"]});
    let step = ask("ob4", "950", MEMBER, 3, picked, false).await;
    // The S.V.E.R step is real now: how to link, with the link.
    assert!(
        step.contains("Your S.V.E.R account") && step.contains("/account"),
        "{step}"
    );
    find(&taken(), &["PUT /guilds/900/members/950/roles/254"]);
    let step = ask("ob5", "950", MEMBER, 3, button("onb:link"), false).await;
    assert!(step.contains("bday:open:onb"), "{step}");
    let opened = ask("ob6", "950", MEMBER, 3, button("bday:open:onb"), false).await;
    assert!(
        opened.contains(r#""type":9"#) && opened.contains("bday:save:onb"),
        "{opened}"
    );
    let done = ask(
        "ob7",
        "950",
        MEMBER,
        5,
        submit(
            "bday:save:onb",
            &[("month", "7"), ("day", "4"), ("zone", "Europe/London")],
        ),
        false,
    )
    .await;
    assert!(
        done.contains("July 4") && done.contains("all set"),
        "{done}"
    );
    let c = taken();
    find(&c, &["PUT /guilds/900/members/950/roles/252"]);
    find(&c, &["DELETE /guilds/900/members/950/roles/253"]);
    find(&c, &["POST /channels/150/messages", "has joined Glint"]);
    assert_eq!(count("SELECT count(*) FROM discord_members WHERE discord_user='950' AND onboarded_at IS NOT NULL AND onboarding IS NULL").await, 1);
    assert_eq!(count("SELECT count(*) FROM discord_birthdays WHERE discord_user='950' AND zone='Europe/London'").await, 1);
    // Back after leaving: Community Member again, no second onboarding.
    dispatch(
        &e.app,
        "GUILD_MEMBER_ADD",
        json!({"guild_id": "900", "user": {"id": "950", "username": "newbie"}}),
    )
    .await;
    let c = taken();
    find(&c, &["PUT /guilds/900/members/950/roles/252"]);
    none(&c, "POST /channels/dm-950/messages");
    // DMs closed: a note in the welcome channel instead.
    dispatch(
        &e.app,
        "GUILD_MEMBER_ADD",
        json!({"guild_id": "900", "user": {"id": "951", "username": "closed"}}),
    )
    .await;
    find(
        &taken(),
        &[
            "POST /channels/150/messages",
            "<@951>",
            "couldn't send you a DM",
        ],
    );
    // A brand-new account is held, and welcomed once a moderator approves it.
    let fresh = {
        let ms = Utc::now().timestamp_millis() as u64 - 1_420_070_400_000;
        ((ms << 22) | 7).to_string()
    };
    dispatch(
        &e.app,
        "GUILD_MEMBER_ADD",
        json!({"guild_id": "900", "user": {"id": fresh, "username": "fresh"}}),
    )
    .await;
    // Moderation tells them they're held; onboarding waits.
    let c = taken();
    none(&c, "onb:rules");
    none(&c, "POST /channels/150/messages");
    let raw = json!({"id": "ap1", "token": "ap1", "type": 3, "guild_id": "900", "channel_id": "159",
        "member": {"user": {"id": "42"}, "permissions": MODERATOR, "roles": ["55"]}, "data": button(&format!("mod:new:approve:{fresh}"))});
    interaction(&e.app, raw).await;
    let c = taken();
    find(
        &c,
        &[&format!("POST /channels/dm-{fresh}/messages"), "onb:rules"],
    );
    find(
        &c,
        &[
            "POST /channels/150/messages",
            &format!("<@{fresh}>"),
            "Welcome to S.V.E.R!",
        ],
    );
    // ---- Goodbye: one post ----
    dispatch(
        &e.app,
        "GUILD_MEMBER_REMOVE",
        json!({"guild_id": "900", "user": {"id": "950", "username": "newbie"}}),
    )
    .await;
    let c = taken();
    find(&c, &["has left the server"]);
    find(&c, &["POST /channels/151/messages", "newbie", "Glint"]);

    // ---- Birthdays in the member's own zone ----
    let bad = ask(
        "bd1",
        "952",
        MEMBER,
        2,
        sub(
            "birthday",
            "set",
            json!([opt("month", json!(2)), opt("day", json!(30))]),
        ),
        true,
    )
    .await;
    assert!(bad.contains("February has 29 days"), "{bad}");
    // Kiritimati is UTC+14: its date often isn't UTC's.
    let kiritimati: chrono_tz::Tz = "Pacific/Kiritimati".parse().unwrap();
    let local = Utc::now().with_timezone(&kiritimati).date_naive();
    let birthday = |year: bool| {
        let mut options = vec![
            opt("month", json!(local.month())),
            opt("day", json!(local.day())),
            opt("zone", json!("Pacific/Kiritimati")),
        ];
        if year {
            options.push(opt("year", json!(2000)));
        }
        sub("birthday", "set", json!(options))
    };
    let set = ask("bd2", "952", MEMBER, 2, birthday(true), true).await;
    assert!(set.contains("Pacific/Kiritimati"), "{set}");
    // The day's anniversaries run first here (the minute runs them too).
    rituals::anniversaries(&e.app, Utc::now()).await.unwrap();
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/155/messages",
            "<@953>",
            "2nd server anniversary",
            "XP bonus",
        ],
    );
    none(&c, "<@954>");
    none(&c, "<@999>");
    // 100 XP a year, through the multipliers running now.
    assert!(count("SELECT total FROM discord_xp WHERE discord_user='953'").await >= 200);
    rituals::anniversaries(&e.app, Utc::now()).await.unwrap();
    none(&taken(), "anniversary");
    rituals::minute(&e.app).await.unwrap();
    let c = taken();
    find(&c, &["PUT /guilds/900/members/952/roles/256"]);
    find(
        &c,
        &[
            "POST /channels/155/messages",
            "Happy birthday <@952>",
            &format!("turning **{}**", local.year() - 2000),
        ],
    );
    assert_eq!(count("SELECT count(*) FROM discord_multipliers WHERE kind='user' AND target='952' AND factor=2 AND ends_at>now()").await, 1);
    // Once a year, however often the minute runs or the date is set again.
    rituals::minute(&e.app).await.unwrap();
    ask("bd3", "952", MEMBER, 2, birthday(false), true).await;
    rituals::minute(&e.app).await.unwrap();
    none(&taken(), "Happy birthday");
    // The role comes off when their day is over.
    e.sql("UPDATE discord_birthdays SET role_until=now()-interval '1 minute' WHERE discord_user='952'").await;
    rituals::minute(&e.app).await.unwrap();
    find(&taken(), &["DELETE /guilds/900/members/952/roles/256"]);
    let upcoming = ask(
        "bd4",
        "952",
        MEMBER,
        2,
        sub("birthday", "upcoming", json!([])),
        true,
    )
    .await;
    assert!(upcoming.contains("<@952>"), "{upcoming}");

    // ---- Seasonal events: announced once at the start and once at the end ----
    let made = ask(
        "se1",
        "940",
        STAFF,
        5,
        submit(
            "ritual:season",
            &[
                ("name", "Winter Fest"),
                ("dates", "2030-12-01 to 2030-12-31"),
                ("factor", "2"),
            ],
        ),
        true,
    )
    .await;
    assert!(
        made.contains("Winter Fest") && made.contains("runs from"),
        "{made}"
    );
    let rituals_sub = |group: &str, name: &str, options: Value| json!({"name": "rituals", "options": [{"type": 2, "name": group, "options": [{"type": 1, "name": name, "options": options}]}]});
    let refused = ask(
        "se2",
        "941",
        MEMBER,
        2,
        rituals_sub("event", "create", json!([])),
        true,
    )
    .await;
    assert!(refused.contains("Only staff"), "{refused}");
    e.sql("UPDATE discord_seasonal_events SET starts_at=now()-interval '1 minute',ends_at=now()+interval '1 hour' WHERE name='Winter Fest'").await;
    rituals::minute(&e.app).await.unwrap();
    rituals::minute(&e.app).await.unwrap();
    find(
        &taken(),
        &[
            "POST /channels/142/messages",
            "Winter Fest has begun",
            "2× XP",
        ],
    );
    assert_eq!(count("SELECT count(*) FROM discord_multipliers WHERE kind='event' AND name='Winter Fest (seasonal)'").await, 1);
    e.sql("UPDATE discord_seasonal_events SET ends_at=now() WHERE name='Winter Fest'")
        .await;
    rituals::minute(&e.app).await.unwrap();
    rituals::minute(&e.app).await.unwrap();
    find(
        &taken(),
        &["POST /channels/142/messages", "Winter Fest has ended"],
    );

    // ---- Spotlights and nominations ----
    let nominate = |user: &str, reason: &str| {
        rituals_sub(
            "spotlight",
            "nominate",
            json!([opt("user", json!(user)), opt("reason", json!(reason))]),
        )
    };
    let hero = ask(
        "sp1",
        "941",
        MEMBER,
        2,
        nominate("953", "Always helps new streamers"),
        true,
    )
    .await;
    assert!(
        hero.contains("Community Hero") && hero.contains("Always helps"),
        "{hero}"
    );
    find(
        &taken(),
        &["POST /channels/40/messages", "<@953>", "community hero"],
    );
    assert_eq!(count("SELECT count(*) FROM discord_achievements WHERE discord_user='941' AND key='hero_nominated'").await, 1);
    let twice = ask("sp2", "941", MEMBER, 2, nominate("952", "Also great"), true).await;
    assert!(twice.contains("in the last day"), "{twice}");
    let me = ask("sp3", "942", MEMBER, 2, nominate("942", "Me"), true).await;
    assert!(me.contains("someone else"), "{me}");

    // ---- Events and RSVPs ----
    let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
    let day = (Utc::now() + Duration::days(2))
        .with_timezone(&ny)
        .date_naive();
    let created = ask(
        "ev1",
        "960",
        MEMBER,
        2,
        sub(
            "event",
            "create",
            json!([
                opt("title", json!("Game night")),
                opt("date", json!(day.to_string())),
                opt("time", json!("8:00 PM")),
                opt("duration", json!("2h")),
                opt("repeat", json!("weekly")),
                opt("type", json!("game_night"))
            ]),
        ),
        true,
    )
    .await;
    assert!(created.contains("is up in <#157>"), "{created}");
    find(
        &taken(),
        &["POST /channels/157/messages", "Game night", "event:rsvp:"],
    );
    let event: i64 = count("SELECT id FROM discord_events WHERE title='Game night'").await;
    // Members have up to 3 upcoming events at once (a repeating series counts once).
    for n in 2..=4 {
        let more = ask(
            &format!("ev{n}"),
            "960",
            MEMBER,
            2,
            sub(
                "event",
                "create",
                json!([
                    opt("title", json!(format!("Extra {n}"))),
                    opt("date", json!(day.to_string())),
                    opt("time", json!("9:00 PM"))
                ]),
            ),
            true,
        )
        .await;
        assert_eq!(more.contains("upcoming events already"), n == 4, "{more}");
    }
    taken();
    e.sql("DELETE FROM discord_events WHERE title LIKE 'Extra %'")
        .await;
    // The host going to their own event earns nothing.
    e.sql("INSERT INTO discord_members(discord_user,faction) VALUES('960','glint') ON CONFLICT(discord_user) DO UPDATE SET faction='glint'").await;
    let own = ask(
        "rs-host",
        "960",
        MEMBER,
        3,
        button(&format!("event:rsvp:{event}:going")),
        true,
    )
    .await;
    assert!(own.contains(r#""type":7"#), "{own}");
    assert_eq!(
        count("SELECT count(*) FROM discord_points WHERE discord_user='960' AND kind='event'")
            .await,
        0
    );
    e.sql("INSERT INTO discord_members(discord_user,faction) VALUES('961','myria') ON CONFLICT(discord_user) DO UPDATE SET faction='myria'").await;
    for (n, status) in ["going", "maybe", "going", "going"].iter().enumerate() {
        let r = ask(
            &format!("rs{n}"),
            "961",
            MEMBER,
            3,
            button(&format!("event:rsvp:{event}:{status}")),
            true,
        )
        .await;
        assert!(r.contains(r#""type":7"#) && r.contains("going"), "{r}");
    }
    // Going earns the event's points and quest progress once, however often it's pressed.
    assert_eq!(
        count("SELECT count(*) FROM discord_points WHERE discord_user='961' AND kind='event'")
            .await,
        1
    );
    let starts_in = |minutes: i64| {
        sqlx::AssertSqlSafe(format!(
            "UPDATE discord_events SET starts_at=now()+interval '{minutes} minutes',ends_at=now()+interval '{} minutes' WHERE id={event}",
            minutes + 120
        ))
    };
    e.sql(starts_in(30)).await;
    events::minute(&e.app).await.unwrap();
    events::minute(&e.app).await.unwrap();
    find(
        &taken(),
        &["POST /channels/157/messages", "starts in 1 hour", "<@961>"],
    );
    e.sql(starts_in(10)).await;
    events::minute(&e.app).await.unwrap();
    find(
        &taken(),
        &["POST /channels/157/messages", "starts in 15 minutes"],
    );
    e.sql(starts_in(-1)).await;
    events::minute(&e.app).await.unwrap();
    find(
        &taken(),
        &["PATCH /channels/157/messages/", "HAPPENING NOW"],
    );
    // Over: closed, and the weekly one comes round again.
    e.sql(sqlx::AssertSqlSafe(format!("UPDATE discord_events SET starts_at=now()-interval '3 hours',ends_at=now()-interval '1 hour' WHERE id={event}"))).await;
    events::minute(&e.app).await.unwrap();
    let c = taken();
    find(&c, &["PATCH /channels/157/messages/", "DONE"]);
    find(
        &c,
        &["POST /channels/157/messages", "Game night", "Repeats"],
    );
    assert_eq!(count(&format!("SELECT count(*) FROM discord_events WHERE parent_id={event} AND status='scheduled' AND starts_at>now()")).await, 1);
    // An event with no end closes after 24 hours.
    e.sql("INSERT INTO discord_events(title,zone,starts_at,status,channel_id,created_by) VALUES('Open end','UTC',now()-interval '25 hours','active','157','960')").await;
    events::minute(&e.app).await.unwrap();
    assert_eq!(
        count("SELECT count(*) FROM discord_events WHERE title='Open end' AND status='completed'")
            .await,
        1
    );
    taken();

    // ---- Polls close at their time ----
    let poll = ask(
        "po1",
        "960",
        MEMBER,
        2,
        json!({"name": "poll", "options": [opt("question", json!("Best snack?")),
        opt("option1", json!("Chips")), opt("option2", json!("Fruit")), opt("duration", json!("1h"))]}),
        true,
    )
    .await;
    assert!(poll.contains("Poll #"), "{poll}");
    find(
        &taken(),
        &["POST /channels/10/messages", "Best snack?", "poll:vote:"],
    );
    let p: i64 = count("SELECT id FROM discord_polls WHERE question='Best snack?'").await;
    let vote = |n: usize| button(&format!("poll:vote:{p}:{n}"));
    ask("pv1", "961", MEMBER, 3, vote(1), true).await;
    ask("pv2", "962", MEMBER, 3, vote(0), true).await;
    let changed = ask("pv3", "962", MEMBER, 3, vote(1), true).await;
    assert!(changed.contains("Total votes: 2"), "{changed}");
    e.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE discord_polls SET ends_at=now()-interval '1 second' WHERE id={p}"
    )))
    .await;
    events::due(&e.app).await.unwrap();
    events::due(&e.app).await.unwrap();
    let c = taken();
    find(&c, &["PATCH /channels/10/messages/", "**Closed**"]);
    find(
        &c,
        &[
            "POST /channels/10/messages",
            "is closed",
            "**Fruit** wins with 2 votes",
        ],
    );
    let late = ask("pv4", "963", MEMBER, 3, vote(0), true).await;
    assert!(late.contains("closed"), "{late}");

    // ---- Giveaways: reviewed, entered by button or 🎉, drawn on time ----
    let submitted = ask(
        "gv1",
        "964",
        MEMBER,
        2,
        sub(
            "giveaway",
            "submit",
            json!([
                opt("prize", json!("A game key")),
                opt("duration", json!("1h"))
            ]),
        ),
        true,
    )
    .await;
    assert!(submitted.contains("with staff for review"), "{submitted}");
    find(
        &taken(),
        &[
            "POST /channels/159/messages",
            "Giveaway for review",
            "give:approve:",
        ],
    );
    let g: i64 = count("SELECT id FROM discord_giveaways WHERE prize='A game key'").await;
    let nope = ask(
        "gv2",
        "961",
        MEMBER,
        3,
        button(&format!("give:approve:{g}")),
        true,
    )
    .await;
    assert!(nope.contains("Only staff"), "{nope}");
    let ok = ask(
        "gv3",
        "940",
        STAFF,
        3,
        button(&format!("give:approve:{g}")),
        true,
    )
    .await;
    assert!(ok.contains("Approved by <@940>"), "{ok}");
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/158/messages",
            "GIVEAWAY: A game key",
            "give:enter:",
        ],
    );
    find(
        &c,
        &["PUT /channels/158/messages/", "/reactions/%F0%9F%8E%89/@me"],
    );
    find(&c, &["POST /channels/dm-964/messages", "approved"]);
    let enter = button(&format!("give:enter:{g}"));
    let entered = ask("ge1", "961", MEMBER, 3, enter.clone(), true).await;
    assert!(entered.contains("You're in"), "{entered}");
    let host = ask("ge2", "964", MEMBER, 3, enter.clone(), true).await;
    assert!(host.contains("your own giveaway"), "{host}");
    let message: String =
        sqlx::query_scalar("SELECT message_id FROM discord_giveaways WHERE id=$1")
            .bind(g)
            .fetch_one(&e.app.db)
            .await
            .unwrap();
    // 🎉 on the post enters too, and earns no reaction XP.
    dispatch(&e.app, "MESSAGE_REACTION_ADD", json!({"guild_id": "900", "channel_id": "158", "message_id": message, "user_id": "965",
        "message_author_id": "500", "emoji": {"name": "🎉"}, "member": {"user": {"id": "965"}, "roles": []}})).await;
    assert_eq!(
        count(&format!(
            "SELECT count(*) FROM discord_giveaway_entries WHERE giveaway_id={g}"
        ))
        .await,
        2
    );
    assert_eq!(
        count("SELECT count(*) FROM discord_xp WHERE discord_user='965' AND reactions>0").await,
        0
    );
    e.sql(sqlx::AssertSqlSafe(format!(
        "UPDATE discord_giveaways SET ends_at=now()-interval '1 second' WHERE id={g}"
    )))
    .await;
    taken();
    events::due(&e.app).await.unwrap();
    events::due(&e.app).await.unwrap();
    find(
        &taken(),
        &[
            "POST /channels/158/messages",
            "Congratulations",
            "A game key",
        ],
    );
    let winners = async || -> Vec<String> {
        sqlx::query_scalar("SELECT unnest(winner_ids) FROM discord_giveaways WHERE id=$1")
            .bind(g)
            .fetch_all(&e.app.db)
            .await
            .unwrap()
    };
    let first = winners().await;
    assert!(
        first.len() == 1 && ["961", "965"].contains(&first[0].as_str()),
        "{first:?}"
    );
    let reroll = ask(
        "gv4",
        "940",
        STAFF,
        2,
        sub("giveaway", "reroll", json!([opt("id", json!(g))])),
        true,
    )
    .await;
    assert!(reroll.contains("rerolled"), "{reroll}");
    let all = winners().await;
    assert!(all.len() == 2 && all[0] != all[1], "{all:?}");
    let closed = ask("ge3", "966", MEMBER, 3, enter, true).await;
    assert!(closed.contains("closed"), "{closed}");

    // ---- Reminders fire ----
    let set = ask(
        "rm1",
        "966",
        MEMBER,
        2,
        sub(
            "remind",
            "set",
            json!([opt("time", json!("30m")), opt("message", json!("Stretch"))]),
        ),
        true,
    )
    .await;
    assert!(set.contains("Reminder #"), "{set}");
    e.sql(
        "UPDATE discord_reminders SET remind_at=now()-interval '1 second' WHERE message='Stretch'",
    )
    .await;
    taken();
    events::due(&e.app).await.unwrap();
    events::due(&e.app).await.unwrap();
    find(
        &taken(),
        &[
            "POST /channels/10/messages",
            "<@966>, reminder: **Stretch**",
        ],
    );

    // ---- Mentors ----
    let signed_up = ask(
        "mt1",
        "967",
        MEMBER,
        2,
        sub(
            "mentor",
            "signup",
            json!([
                opt("platforms", json!("Twitch, S.V.E.R")),
                opt("specialties", json!("OBS, audio"))
            ]),
        ),
        true,
    )
    .await;
    assert!(signed_up.contains("mentor now"), "{signed_up}");
    let c = taken();
    find(&c, &["PUT /guilds/900/members/967/roles/267"]);
    find(&c, &["POST /channels/165/messages", "New mentor available"]);
    ask(
        "mt2",
        "968",
        MEMBER,
        2,
        sub(
            "mentor",
            "request",
            json!([opt("help_with", json!("obs scenes"))]),
        ),
        true,
    )
    .await;
    find(
        &taken(),
        &[
            "POST /channels/165/messages",
            "Mentor match",
            "mentor:accept:",
        ],
    );
    let m: i64 = count("SELECT id FROM discord_mentorships WHERE mentee='968'").await;
    let accept = button(&format!("mentor:accept:{m}"));
    let other = ask("mt3", "969", MEMBER, 3, accept.clone(), true).await;
    assert!(other.contains("Only the suggested mentor"), "{other}");
    let accepted = ask("mt4", "967", MEMBER, 3, accept, true).await;
    assert!(accepted.contains("is mentoring <@968>"), "{accepted}");
    let c = taken();
    find(&c, &["POST /channels/165/threads", r#""type":12"#]);
    find(&c, &["/thread-members/968"]);
    let complete = ask(
        "mt5",
        "967",
        MEMBER,
        2,
        sub("mentor", "complete", json!([opt("mentee", json!("968"))])),
        true,
    )
    .await;
    assert!(complete.contains("complete"), "{complete}");

    // ---- The role-select panel ----
    let posted = ask(
        "rp1",
        "940",
        STAFF,
        2,
        json!({"name": "rolepanel", "options": [opt("channel", json!("171"))]}),
        true,
    )
    .await;
    assert!(posted.contains("posted in <#171>"), "{posted}");
    find(
        &taken(),
        &[
            "POST /channels/171/messages",
            "panel:faction:myria",
            "str:link:twitch",
            "bday:open:panel",
            "panel:freegames",
        ],
    );
    ask("rp2", "970", MEMBER, 3, button("panel:freegames"), true).await;
    find(&taken(), &["PUT /guilds/900/members/970/roles/261"]);
    let joined = ask(
        "rp3",
        "970",
        MEMBER,
        3,
        button("panel:faction:aetheron"),
        true,
    )
    .await;
    assert!(joined.contains("Aetheron"), "{joined}");

    // ---- Threads and the forum setup ----
    let thread = ask(
        "th1",
        "970",
        MEMBER,
        2,
        sub(
            "thread",
            "create",
            json!([
                opt("name", json!("Elden Ring")),
                opt("channel", json!("163"))
            ]),
        ),
        true,
    )
    .await;
    assert!(thread.contains("Your thread is up"), "{thread}");
    find(
        &taken(),
        &[
            "POST /channels/163/threads",
            "Elden Ring",
            "Started by <@970>",
        ],
    );
    let outside = ask(
        "th2",
        "970",
        MEMBER,
        2,
        sub(
            "thread",
            "create",
            json!([opt("name", json!("x")), opt("channel", json!("10"))]),
        ),
        true,
    )
    .await;
    assert!(outside.contains("Threads can be made in"), "{outside}");
    let staff = super::bans::staff(e, "dcc-staff", "DccStaff").await;
    let forums = async |token: Option<&str>| {
        super::chat::call(e, "POST", "/api/admin/discord/forums", token, Value::Null).await
    };
    let (status, report) = forums(Some(&staff)).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let made = |report: &Value, key: &str| {
        report["forums"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == key)
            .map(|f| (f["made"].as_i64().unwrap(), f["skipped"].as_i64().unwrap()))
    };
    assert_eq!(made(&report, "game_chat"), Some((9, 0)), "{report}");
    assert_eq!(made(&report, "stream_help"), Some((6, 0)), "{report}");
    let c = taken();
    find(
        &c,
        &[
            "PATCH /channels/163?",
            "Start a new post for any game",
            "Battle Royale",
        ],
    );
    find(
        &c,
        &["POST /channels/163/threads", r#""name":"WoW""#, "tag-"],
    );
    // A text channel gets threads with the starter text.
    find(&c, &["POST /channels/165/threads", "OBS setup guide"]);
    let (_, again) = forums(Some(&staff)).await;
    assert_eq!(made(&again, "game_chat"), Some((0, 9)), "{again}");
    let (status, _) = forums(None).await;
    assert_ne!(status, StatusCode::OK);
    taken();

    // ---- Temp voice rooms, deleted once empty ----
    let voice = |channel: Value| {
        json!({"guild_id": "900", "user_id": "975", "channel_id": channel,
        "member": {"user": {"id": "975", "username": "roomer"}, "roles": []}})
    };
    dispatch(&e.app, "VOICE_STATE_UPDATE", voice(json!("166"))).await;
    let c = taken();
    find(&c, &["POST /guilds/900/channels", "roomer's Room", "cat-1"]);
    find(&c, &["PATCH /guilds/900/members/975", "room-1"]);
    dispatch(&e.app, "VOICE_STATE_UPDATE", voice(json!("room-1"))).await;
    dispatch(&e.app, "VOICE_STATE_UPDATE", voice(Value::Null)).await;
    // A new room waits for its owner a moment; then the empty one goes.
    none(&taken(), "DELETE /channels/room-1");
    e.sql("UPDATE discord_temp_rooms SET created_at=now()-interval '1 minute'")
        .await;
    tools::minute(&e.app).await.unwrap();
    find(&taken(), &["DELETE /channels/room-1"]);
    assert_eq!(count("SELECT count(*) FROM discord_temp_rooms").await, 0);

    // ---- Custom commands and embeds ----
    let add = sub(
        "command",
        "add",
        json!([
            opt("name", json!("Rules")),
            opt("response", json!("Be kind"))
        ]),
    );
    let refused = ask("cc1", "970", MEMBER, 2, add.clone(), true).await;
    assert!(refused.contains("Only staff"), "{refused}");
    ask("cc2", "940", STAFF, 2, add, true).await;
    let used = ask(
        "cc3",
        "970",
        MEMBER,
        2,
        json!({"name": "c", "options": [opt("name", json!("rules"))]}),
        true,
    )
    .await;
    assert!(
        used.contains("Be kind") && used.contains(r#""type":4}"#) && !used.contains("\"flags\":64"),
        "{used}"
    );
    assert_eq!(
        count("SELECT uses::int8 FROM discord_custom_commands WHERE name='rules'").await,
        1
    );
    let form = ask(
        "em1",
        "940",
        STAFF,
        2,
        sub("embed", "create", json!([opt("channel", json!("170"))])),
        true,
    )
    .await;
    assert!(form.contains("embed:create:170"), "{form}");
    ask(
        "em2",
        "940",
        STAFF,
        5,
        submit(
            "embed:create:170",
            &[("description", "Hello world"), ("color", "#FF0000")],
        ),
        true,
    )
    .await;
    find(
        &taken(),
        &["POST /channels/170/messages", "Hello world", "16711680"],
    );
    let bad = ask(
        "em3",
        "940",
        STAFF,
        5,
        submit(
            "embed:create:170",
            &[("description", "x"), ("color", "red")],
        ),
        true,
    )
    .await;
    assert!(bad.contains("A colour looks like"), "{bad}");
    let edit = ask(
        "em4",
        "940",
        STAFF,
        2,
        sub(
            "embed",
            "edit",
            json!([opt("message", json!("m77")), opt("channel", json!("170"))]),
        ),
        true,
    )
    .await;
    assert!(
        edit.contains("embed:edit:170:m77") && edit.contains("Old text"),
        "{edit}"
    );

    // ---- Free games: each once, the role pinged on the batch's first post ----
    taken();
    feeds::free_games(&e.app).await.unwrap();
    let c = taken();
    let posts: Vec<&String> = c
        .iter()
        .filter(|l| l.starts_with("POST /channels/160/messages"))
        .collect();
    assert_eq!(posts.len(), 4, "{c:#?}");
    assert!(posts[0].contains("<@&261>"), "{}", posts[0]);
    assert_eq!(posts.iter().filter(|p| p.contains("<@&261>")).count(), 1);
    for title in [
        "Epic Freebie",
        "Steam Key",
        "Weekend Game (free weekend)",
        "GOG Gift",
    ] {
        find(&c, &["POST /channels/160/messages", title]);
    }
    feeds::free_games(&e.app).await.unwrap();
    none(&taken(), "POST /channels/160/messages");

    // ---- The GitHub changelog: signed releases of S.V.E.R repositories ----
    let release = |repo: &str| {
        json!({"action": "published", "repository": {"full_name": repo},
        "release": {"tag_name": "v1.2.0", "name": "Community", "body": "Welcome flow ![shot](https://img.example/x.png) and more",
            "html_url": "https://github.example/releases/v1.2.0"}})
        .to_string()
    };
    let body = release("S-V-E-R/sver");
    let headers = |sig: String| {
        vec![
            ("x-github-event", "release".to_string()),
            ("x-hub-signature-256", sig),
        ]
    };
    let github = "/api/integrations/github";
    assert_eq!(
        hook(e, github, &headers("sha256=00".into()), &body).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        hook(e, github, &[("x-github-event", "release".into())], &body).await,
        StatusCode::UNAUTHORIZED
    );
    none(&taken(), "POST /channels/162/messages");
    let good = headers(hmac("test-github-secret", body.as_bytes()));
    assert_eq!(hook(e, github, &good, &body).await, StatusCode::NO_CONTENT);
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/162/messages",
            "v1.2.0 — Community",
            "Welcome flow  and more",
        ],
    );
    none(&c, "img.example");
    let elsewhere = release("someone/else");
    let good = headers(hmac("test-github-secret", elsewhere.as_bytes()));
    assert_eq!(
        hook(e, github, &good, &elsewhere).await,
        StatusCode::NO_CONTENT
    );
    none(&taken(), "POST /channels/162/messages");

    // ---- WoW and Chromely ----
    let link = |character: &str| {
        sub(
            "wow",
            "link",
            json!([
                opt("character", json!(character)),
                opt("realm", json!("Dream"))
            ]),
        )
    };
    let linked = ask("ww1", "976", MEMBER, 2, link("Joe"), true).await;
    assert!(
        linked.contains("is linked") && linked.contains("Rogue"),
        "{linked}"
    );
    let missing = ask("ww2", "976", MEMBER, 2, link("Nobody"), true).await;
    assert!(missing.contains("isn't on Chromely"), "{missing}");
    let board = ask(
        "ww3",
        "976",
        MEMBER,
        2,
        sub("wow", "leaderboard", json!([])),
        true,
    )
    .await;
    assert!(
        board.contains("1:02:05.0") && board.contains("Joe"),
        "{board}"
    );
    // The rival string goes by DM; an unknown character is said so.
    let rival = |c: &str| sub("wow", "rival", json!([opt("character", json!(c))]));
    let sent = ask("ww4", "976", MEMBER, 2, rival("Joe"), true).await;
    assert!(sent.contains("in your DMs"), "{sent}");
    find(
        &taken(),
        &[
            "POST /channels/dm-976/messages",
            "CR1|Joe|Rogue",
            "/cl import",
        ],
    );
    let unknown = ask("ww5", "976", MEMBER, 2, rival("Nobody"), true).await;
    assert!(unknown.contains("wasn't found"), "{unknown}");
    // A submitted run goes to Chromely and is recorded with its answer.
    let export = "=== CHROMELY SPEEDRUN EXPORT ===
Character: Joe
Realm: Dream
Class: ROGUE
Total Time: 1:00:00";
    let run = ask(
        "ww6",
        "976",
        MEMBER,
        5,
        submit(
            "wow:submit",
            &[("export", export), ("proof", "https://img.example/run.png")],
        ),
        true,
    )
    .await;
    assert!(run.contains("Run accepted at rank 3"), "{run}");
    find(
        &taken(),
        &["POST /chromely/api/chromely/runs", r#""character":"Joe""#],
    );
    assert_eq!(count("SELECT count(*) FROM discord_wow_runs WHERE discord_user='976' AND status='accepted' AND seconds=3600 AND proof_url IS NOT NULL").await, 1);
    let record = json!({"event": "speedrun.record", "data": {"characterName": "Joe", "realm": "Dream", "time": 3600, "class": "Rogue"}}).to_string();
    let chromely = "/api/integrations/chromely";
    let signed = vec![(
        "x-chromely-signature",
        hmac("test-chromely-secret", record.as_bytes()),
    )];
    assert_eq!(
        hook(
            e,
            chromely,
            &[("x-chromely-signature", "sha256=00".to_string())],
            &record
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    taken();
    assert_eq!(
        hook(e, chromely, &signed, &record).await,
        StatusCode::NO_CONTENT
    );
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/163/threads",
            "WoW Speedruns",
            "NEW SPEEDRUN RECORD",
        ],
    );
    find(
        &c,
        &[
            "POST /channels/164/threads",
            "WoW speedrun leaderboard",
            "Joe",
        ],
    );
    // The next record goes in the same thread, and the leaderboard post is edited.
    assert_eq!(
        hook(e, chromely, &signed, &record).await,
        StatusCode::NO_CONTENT
    );
    let c = taken();
    none(&c, "POST /channels/163/threads");
    find(&c, &["POST /channels/th", "NEW SPEEDRUN RECORD"]);
    find(
        &c,
        &[
            "PATCH /channels/th",
            "/messages/th",
            "WoW speedrun leaderboard",
        ],
    );

    // SAFETY: as above.
    unsafe {
        for var in [
            "GITHUB_WEBHOOK_SECRET",
            "CHROMELY_WEBHOOK_SECRET",
            "CHROMELY_API_URL",
        ] {
            std::env::remove_var(var);
        }
    }
}
