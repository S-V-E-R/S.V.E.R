//! Streamers on the home server (docs/DISCORD_BOT.md "Go-live alerts and streamers") against fake
//! Discord, Twitch, Kick and YouTube APIs: a S.V.E.R and a Twitch go-live each post and clear (with
//! the Now Live role, a session and stream XP), the platform throttle and non-members, Kick and
//! YouTube by polling, Twitch auto-detect and the never-again opt-out, profiles and search,
//! schedules in real time zones, the weekly posts, milestones, collab requests and LFG.
use super::*;
use axum::http::{Method, Uri};
use std::collections::HashMap;

#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    live: HashMap<&'static str, bool>,
    sent: usize,
}
type Shared = Arc<Mutex<Fake>>;

async fn fake(
    State(state): State<Shared>,
    method: Method,
    uri: Uri,
    body: String,
) -> (StatusCode, Json<Value>) {
    let path = uri.path().to_string();
    let query = uri.query().unwrap_or_default().to_string();
    let mut s = state.lock().unwrap();
    s.calls.push(format!("{method} {path}?{query} {body}"));
    let live = |s: &Fake, p: &str| s.live.get(p).copied().unwrap_or(false);
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let answer = match (method.as_str(), parts.as_slice()) {
        // Discord. 920 and 922 are in the home server; anyone else isn't.
        ("GET", ["guilds", "900", "members", "920" | "922"]) => json!({"roles": []}),
        ("GET", ["guilds", "900", "members", _]) => {
            return (StatusCode::NOT_FOUND, Json(json!({"code": 10007})));
        }
        ("GET", ["channels", "44"]) => {
            json!({"type": 15, "available_tags": [{"id": "tag-v", "name": "Valorant"}]})
        }
        ("GET", ["channels", _]) => json!({"type": 0}),
        ("POST", ["channels", _, "threads"]) => json!({"id": "th1"}),
        ("POST", ["channels", _, "messages"]) => {
            s.sent += 1;
            json!({"id": format!("m{}", s.sent)})
        }
        ("POST", ["users", "@me", "channels"]) => {
            let to: Value = serde_json::from_str(&body).unwrap();
            json!({"id": format!("dm-{}", to["recipient_id"].as_str().unwrap())})
        }
        // Twitch, and Kick's token.
        ("POST", ["oauth2", "token"]) | ("POST", ["oauth", "token"]) => {
            json!({"access_token": "app-token", "expires_in": 3600})
        }
        ("GET", ["helix", "users"]) => match query.as_str() {
            "login=streamy" => json!({"data": [{"id": "t-1", "login": "streamy"}]}),
            "login=autostream" => json!({"data": [{"id": "t-2", "login": "autostream"}]}),
            _ => json!({"data": []}),
        },
        ("GET", ["helix", "streams"]) if live(&s, "twitch") => {
            json!({"data": [{"id": "s-1", "user_id": "t-1", "title": "Speedrun night", "game_name": "Celeste",
            "thumbnail_url": "https://img.example/{width}x{height}.jpg", "viewer_count": 5}]})
        }
        ("GET", ["helix", "streams"]) => json!({"data": []}),
        ("POST", ["helix", "eventsub", "subscriptions"]) => {
            let kind: Value = serde_json::from_str(&body).unwrap();
            let id = format!("sub-{}", kind["type"].as_str().unwrap());
            return (StatusCode::ACCEPTED, Json(json!({"data": [{"id": id}]})));
        }
        // Kick's official API.
        ("GET", ["public", "v1", "channels"]) if query.contains("slug=kicker") => {
            json!({"data": [{"slug": "kicker", "stream_title": "Kick run", "category": {"name": "Chess"},
            "stream": {"is_live": live(&s, "kick"), "start_time": "2026-10-20T10:00:00Z", "viewer_count": 3, "thumbnail": ""}}]})
        }
        ("GET", ["public", "v1", "channels"]) => json!({"data": []}),
        // YouTube: the channel, its feed, then which video is live.
        ("GET", ["yt", "channels"]) if query.contains("forHandle=%40tuber") => {
            json!({"items": [{"id": "UCtube"}]})
        }
        ("GET", ["yt", "channels"]) => json!({"items": []}),
        // A closed channel's feed is gone.
        ("GET", ["feed"]) if query.contains("UCgone") => {
            return (StatusCode::NOT_FOUND, Json(json!("")));
        }
        ("GET", ["feed"]) => json!("<feed><entry><yt:videoId>v1</yt:videoId></entry></feed>"),
        ("GET", ["yt", "videos"]) => {
            json!({"items": [{"id": "v1", "snippet": {"channelId": "UCtube", "title": "Tube live",
            "liveBroadcastContent": if live(&s, "youtube") { "live" } else { "none" },
            "thumbnails": {"high": {"url": "https://img.example/v1.jpg"}}},
            "liveStreamingDetails": {"concurrentViewers": "7"}}]})
        }
        _ => json!({}),
    };
    (StatusCode::OK, Json(answer))
}

pub async fn exercise(e: &Env) {
    use sver::discord::{commands::interaction, gateway::dispatch, streamers};
    let state: Shared = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new().fallback(fake).with_state(state.clone());
    tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let base = format!("http://{address}");
    // SAFETY: only the Discord tests read these variables, one at a time.
    unsafe {
        for var in [
            "DISCORD_API",
            "TWITCH_API_BASE",
            "TWITCH_ID_BASE",
            "KICK_API_BASE",
            "KICK_ID_BASE",
        ] {
            std::env::set_var(var, &base);
        }
        std::env::set_var("YOUTUBE_API_BASE", format!("{base}/yt"));
        std::env::set_var("YOUTUBE_FEED_BASE", format!("{base}/feed"));
    }
    let taken = || std::mem::take(&mut state.lock().unwrap().calls);
    let set_live = |p: &'static str, on: bool| {
        state.lock().unwrap().live.insert(p, on);
    };
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
    let count = async |sql: &'static str| -> i64 {
        sqlx::query_scalar(sql).fetch_one(&e.app.db).await.unwrap()
    };
    let minute = async || streamers::minute(&e.app).await.unwrap();
    // A command, button or form and its answer: the reply, or the edit after "thinking…".
    let ask = async |id: &str, user: &str, staff: bool, kind: u64, data: Value| -> String {
        interaction(&e.app, json!({"id": id, "token": id, "type": kind, "guild_id": "900", "channel_id": "10",
            "member": {"user": {"id": user, "username": format!("u{user}")}, "permissions": if staff { "32" } else { "0" }, "roles": []}, "data": data}))
            .await;
        let edit = format!("PATCH /webhooks/app1/{id}/messages/@original");
        for _ in 0..200 {
            let found = {
                let log = &state.lock().unwrap().calls;
                let reply = log
                    .iter()
                    .find(|l| l.starts_with(&format!("POST /interactions/{id}/")))
                    .cloned();
                match reply {
                    Some(r) if r.contains(r#""type":5"#) => {
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
    let link = |platform: &str| sub("alerts", "link", json!([opt("platform", json!(platform))]));

    e.sql(r#"UPDATE discord_home SET settings=settings||'{"go_live": "40", "now_live": "41", "announcements": "42", "clips": "43", "lfg": "44", "stream_promo": "45", "zone": "America/New_York"}'"#).await;
    e.sql("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES('ds-streamer','ds@example.test','dsstreamer',true,'1990-01-01'),('ds-outsider','dso@example.test','dsoutsider',true,'1990-01-01')").await;
    e.sql("INSERT INTO identities(provider,subject,user_id) VALUES('discord','920','ds-streamer'),('discord','921','ds-outsider')").await;
    taken();

    // S.V.E.R: the platform's go-live posts once for a member of the home server, with the Now
    // Live role; a linked account that isn't in the server gets nothing.
    let broadcast = |id: &str, owner: &str, alert: &str| {
        sqlx::AssertSqlSafe(format!(
            "INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES('{id}','{owner}','{id}',1,'LIVE','s','v','c',now(),now(),now(),'{alert}')"
        ))
    };
    e.sql(broadcast("ds-b1", "ds-streamer", "sent")).await;
    e.sql(broadcast("ds-b2", "ds-outsider", "sent")).await;
    minute().await;
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/40/messages",
            "dsstreamer is LIVE!",
            "S.V.E.R",
        ],
    );
    find(&c, &["PUT /guilds/900/members/920/roles/41"]);
    none(&c, "members/921/roles");
    assert_eq!(
        count("SELECT count(*) FROM discord_stream_accounts WHERE discord_user='921'").await,
        0
    );
    minute().await;
    none(&taken(), "POST /channels/40/messages");
    // The end: "was live" with its length, a session and stream XP, and the role goes.
    e.sql("UPDATE discord_stream_accounts SET live_since=now()-interval '61 minutes' WHERE discord_user='920'").await;
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id IN ('ds-b1','ds-b2')")
        .await;
    minute().await;
    let c = taken();
    find(
        &c,
        &[
            "PATCH /channels/40/messages/m1",
            "dsstreamer was live on S.V.E.R",
            "Was live for 1h 1m",
        ],
    );
    find(&c, &["DELETE /guilds/900/members/920/roles/41"]);
    assert_eq!(
        count("SELECT minutes::int8 FROM discord_stream_sessions WHERE discord_user='920'").await,
        61
    );
    assert!(
        count("SELECT stream_minutes::int8 FROM discord_xp WHERE discord_user='920'").await >= 61
    );
    // A stream the platform throttled is tracked (the role) but not posted again.
    e.sql(broadcast("ds-b3", "ds-streamer", "throttled")).await;
    minute().await;
    let c = taken();
    find(&c, &["PUT /guilds/900/members/920/roles/41"]);
    none(&c, "POST /channels/40/messages");
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='ds-b3'")
        .await;
    minute().await;
    taken();
    // Past the 10 minutes but inside the 6 hours: the "was live" post turns live again; it isn't
    // deleted with nothing in its place.
    e.sql("UPDATE discord_stream_accounts SET offline_at=now()-interval '30 minutes' WHERE discord_user='920'").await;
    e.sql(broadcast("ds-b4", "ds-streamer", "sent")).await;
    minute().await;
    let c = taken();
    find(
        &c,
        &["PATCH /channels/40/messages/m1", "dsstreamer is LIVE!"],
    );
    none(&c, "DELETE /channels/40/messages/m1");
    none(&c, "POST /channels/40/messages");
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now() WHERE id='ds-b4'")
        .await;
    minute().await;
    taken();
    // "Was live" posts go 4 hours after the end.
    e.sql("UPDATE discord_stream_accounts SET offline_at=now()-interval '5 hours' WHERE discord_user='920'").await;
    minute().await;
    find(&taken(), &["DELETE /channels/40/messages/m1"]);
    assert_eq!(
        count("SELECT count(*) FROM discord_stream_accounts WHERE message_id IS NOT NULL").await,
        0
    );

    // Twitch: only the member's own channel, proven through S.V.E.R (their Discord linked to an
    // account that signed in with that Twitch); then subscribed to its go-live events, posted on
    // EventSub (once, though the poll sees it too), and cleared by the poll.
    let outside = ask("st0", "923", false, 2, link("twitch")).await;
    assert!(
        outside.contains("link your Discord to S.V.E.R first"),
        "{outside}"
    );
    let unproven = ask("st1", "920", false, 2, link("twitch")).await;
    assert!(
        unproven.contains("Connect Twitch under sign-in methods"),
        "{unproven}"
    );
    // Someone linked it without proof before (the legacy bot): its proven owner takes it.
    e.sql("INSERT INTO discord_stream_accounts(discord_user,platform,handle,platform_id) VALUES('926','twitch','streamy','t-1')").await;
    e.sql("INSERT INTO identities(provider,subject,user_id,handle) VALUES('twitch','t-1','ds-streamer','streamy')").await;
    let linked = ask("st2", "920", false, 2, link("twitch")).await;
    assert!(
        linked.contains("streamy") && linked.contains("is linked"),
        "{linked}"
    );
    assert_eq!(count("SELECT count(*) FROM discord_stream_accounts WHERE platform_id='t-1' AND discord_user<>'920'").await, 0);
    minute().await;
    let c = taken();
    find(
        &c,
        &[
            "POST /helix/eventsub/subscriptions",
            "stream.online",
            r#""broadcaster_user_id":"t-1""#,
        ],
    );
    find(
        &c,
        &["POST /helix/eventsub/subscriptions", "stream.offline"],
    );
    assert_eq!(count("SELECT cardinality(eventsub)::int8 FROM discord_stream_accounts WHERE platform='twitch'").await, 2);
    set_live("twitch", true);
    let notice = json!({"subscription": {"id": "sub-stream.online", "type": "stream.online"},
        "event": {"id": "s-1", "broadcaster_user_id": "t-1", "broadcaster_user_login": "streamy"}});
    for _ in 0..2 {
        streamers::twitch_event(&e.app, "notification", &notice)
            .await
            .unwrap();
    }
    streamers::twitch_poll(&e.app).await.unwrap();
    find(
        &taken(),
        &[
            "POST /channels/40/messages",
            "streamy is LIVE!",
            "Speedrun night",
            "Celeste",
            "440x248",
        ],
    );
    set_live("twitch", false);
    streamers::twitch_poll(&e.app).await.unwrap();
    find(
        &taken(),
        &["PATCH /channels/40/messages/", "streamy was live on Twitch"],
    );
    // A revoked subscription is made again on the next pass.
    let revoked = json!({"subscription": {"id": "sub-stream.online", "type": "stream.online"}});
    streamers::twitch_event(&e.app, "revocation", &revoked)
        .await
        .unwrap();
    assert_eq!(count("SELECT cardinality(eventsub)::int8 FROM discord_stream_accounts WHERE platform='twitch'").await, 1);

    // Kick and YouTube can't be linked: S.V.E.R can't confirm those channels yet. Accounts the
    // legacy bot linked are still polled once the keys are set.
    let kick = ask("st3", "920", false, 2, link("kick")).await;
    assert!(kick.contains("can't be linked yet"), "{kick}");
    // SAFETY: as above.
    unsafe {
        std::env::set_var("KICK_CLIENT_ID", "test-kick");
        std::env::set_var("KICK_CLIENT_SECRET", "test-only-secret");
        std::env::set_var("YOUTUBE_API_KEY", "test-only-key");
    }
    e.sql("INSERT INTO discord_stream_accounts(discord_user,platform,handle,platform_id) VALUES('920','kick','kicker','kicker'),('920','youtube','@tuber','UCtube')").await;
    taken();
    // A closed channel, polled before UCtube, doesn't stop the others being checked.
    e.sql("INSERT INTO discord_stream_accounts(discord_user,platform,handle,platform_id) VALUES('925','youtube','gone','UCgone')").await;
    set_live("kick", true);
    set_live("youtube", true);
    streamers::kick_poll(&e.app).await.unwrap();
    streamers::youtube_poll(&e.app).await.unwrap();
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/40/messages",
            "kicker is LIVE!",
            "Kick run",
            "https://kick.com/kicker",
        ],
    );
    find(
        &c,
        &[
            "POST /channels/40/messages",
            "Tube live",
            "https://youtube.com/watch?v=v1",
        ],
    );
    // Live in two places at once, the role stays until the last one ends.
    set_live("kick", false);
    streamers::kick_poll(&e.app).await.unwrap();
    let c = taken();
    find(&c, &["kicker was live on Kick"]);
    none(&c, "DELETE /guilds/900/members/920/roles/41");
    // A YouTube account imported without its channel id is resolved by the poll.
    e.sql("INSERT INTO discord_stream_accounts(discord_user,platform,handle) VALUES('924','youtube','tuber')").await;
    set_live("youtube", false);
    streamers::youtube_poll(&e.app).await.unwrap();
    find(&taken(), &["DELETE /guilds/900/members/920/roles/41"]);
    assert_eq!(count("SELECT count(*) FROM discord_stream_accounts WHERE discord_user='924' AND platform_id='UCtube'").await, 1);
    // `/alerts off` keeps the tracking (role, sessions) and stops the posts.
    let off = ask("st6", "920", false, 2, sub("alerts", "off", json!([]))).await;
    assert!(
        off.contains("Go-live posts are off") && off.contains("kicker"),
        "{off}"
    );
    set_live("kick", true);
    e.sql("UPDATE discord_stream_accounts SET posted_at=NULL")
        .await;
    streamers::kick_poll(&e.app).await.unwrap();
    let c = taken();
    find(&c, &["PUT /guilds/900/members/920/roles/41"]);
    none(&c, "POST /channels/40/messages");
    set_live("kick", false);
    streamers::kick_poll(&e.app).await.unwrap();
    ask("st7", "920", false, 2, sub("alerts", "on", json!([]))).await;
    taken();

    // Auto-detect: streaming on your own, proven Twitch channel links it once and says how to undo
    // it; unlinking is final. A status pointing at a channel that isn't proven yours links nothing.
    e.sql("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES('ds-auto','dsa@example.test','dsauto',true,'1990-01-01')").await;
    e.sql("INSERT INTO identities(provider,subject,user_id,handle) VALUES('discord','922','ds-auto',NULL),('twitch','t-2','ds-auto','autostream')").await;
    let pretender = json!({"guild_id": "900", "user": {"id": "927"},
        "activities": [{"type": 1, "url": "https://www.twitch.tv/autostream"}]});
    dispatch(&e.app, "PRESENCE_UPDATE", pretender).await;
    assert_eq!(
        count("SELECT count(*) FROM discord_stream_accounts WHERE discord_user='927'").await,
        0
    );
    let streaming = json!({"guild_id": "900", "user": {"id": "922"},
        "activities": [{"type": 1, "url": "https://www.twitch.tv/autostream"}]});
    dispatch(&e.app, "PRESENCE_UPDATE", streaming.clone()).await;
    find(&taken(), &["POST /channels/dm-922/messages", "autostream"]);
    assert_eq!(count("SELECT count(*) FROM discord_stream_accounts WHERE discord_user='922' AND platform_id='t-2'").await, 1);
    let unlink = sub(
        "alerts",
        "unlink",
        json!([opt("platform", json!("twitch"))]),
    );
    let gone = ask("st8", "922", false, 2, unlink).await;
    assert!(
        gone.contains("autostream") && gone.contains("unlinked"),
        "{gone}"
    );
    assert_eq!(count("SELECT count(*) FROM discord_autolink_optouts WHERE discord_user='922' AND platform='twitch'").await, 1);
    dispatch(&e.app, "PRESENCE_UPDATE", streaming).await;
    assert_eq!(
        count("SELECT count(*) FROM discord_stream_accounts WHERE discord_user='922'").await,
        0
    );

    // Profiles: edit, view and search; schedules in the member's own zone.
    for (field, value) in [
        ("bio", "Speedruns and chill"),
        ("tags", "#Speedrun, chill"),
        ("games", "Celeste, Hollow Knight"),
        ("collab", "yes;twitch,kick;speedruns;3;evenings"),
    ] {
        let edit = sub(
            "streamer",
            "edit",
            json!([opt("field", json!(field)), opt("value", json!(value))]),
        );
        let r = ask(&format!("pe-{field}"), "920", false, 2, edit).await;
        assert!(!r.contains("couldn't"), "{r}");
    }
    let edit = sub(
        "streamer",
        "edit",
        json!([opt("field", json!("bio")), opt("value", json!("hi"))]),
    );
    let r = ask("pe-x", "923", false, 2, edit).await;
    assert!(r.contains("Link a streaming account first"), "{r}");
    let monday_8pm = sub(
        "schedule",
        "set",
        json!([
            opt("day", json!(1)),
            opt("time", json!("8pm")),
            opt("game", json!("Celeste"))
        ]),
    );
    let set = ask("sc1", "920", false, 2, monday_8pm).await;
    assert!(
        set.contains("Monday") && set.contains("20:00 America/New_York"),
        "{set}"
    );
    let mars = sub(
        "schedule",
        "set",
        json!([
            opt("day", json!(2)),
            opt("time", json!("8pm")),
            opt("zone", json!("Mars/Base"))
        ]),
    );
    let bad = ask("sc2", "920", false, 2, mars).await;
    assert!(bad.contains("time zone isn't known"), "{bad}");
    let view = ask("pv1", "921", false, 2, json!({"name": "streamer", "options": [{"type": 1, "name": "view", "options": [opt("user", json!("920"))]}],
        "resolved": {"users": {"920": {"id": "920", "username": "streamy", "global_name": "Streamy"}}}})).await;
    for needle in [
        "Streamy's streamer profile",
        "Speedruns and chill",
        "#speedrun",
        "Hollow Knight",
        "Open for collabs",
        "Twitch, Kick",
        "**Monday** <t:",
        "https://twitch.tv/streamy",
        "/dsstreamer",
    ] {
        assert!(view.contains(needle), "{needle} in {view}");
    }
    let search = sub(
        "streamer",
        "search",
        json!([
            opt("game", json!("hollow")),
            opt("platform", json!("kick")),
            opt("collab", json!(true))
        ]),
    );
    let found = ask("ps1", "921", false, 2, search).await;
    assert!(found.contains("<@920>"), "{found}");
    let live_only = sub("streamer", "search", json!([opt("live", json!(true))]));
    let empty = ask("ps2", "921", false, 2, live_only).await;
    assert!(empty.contains("No streamers match"), "{empty}");

    // Weekly posts, each once in its week: the schedule (Monday 08:00 UTC), the Streamer of the
    // Week (Monday 10:00, by minutes streamed) and the clip of the week (Sunday 18:00).
    e.sql("INSERT INTO videos(id,owner_id,kind,status,visibility,title,started_at,views) VALUES('ds-clip','ds-streamer','CLIP','READY','PUBLIC','Big jump',now(),40)").await;
    let monday = "2026-10-19T10:30:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let sunday = "2026-10-25T18:05:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    for _ in 0..2 {
        streamers::weekly_posts(&e.app, monday).await.unwrap();
        streamers::weekly_posts(&e.app, sunday).await.unwrap();
    }
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/45/messages",
            "This week's stream schedule",
            "<@920>",
            "Celeste",
        ],
    );
    find(
        &c,
        &[
            "POST /channels/40/messages",
            "Streamer of the Week",
            "<@920>",
        ],
    );
    find(&c, &["PUT /channels/40/messages/", "reactions"]);
    find(
        &c,
        &[
            "POST /channels/43/messages",
            "Clip of the Week",
            "Big jump",
            "/clips/ds-clip",
        ],
    );
    none(&c, "Views");

    // Follower milestones: every threshold reached is recorded, the highest posted, once. The first
    // look only records where the account already is.
    streamers::milestones(&e.app).await.unwrap();
    none(&taken(), "just hit");
    for n in 0..12 {
        let id = format!("ds-fan{n}");
        sqlx::query("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES($1,$1||'@example.test',$1,true,'1990-01-01')")
            .bind(&id).execute(&e.app.db).await.unwrap();
        sqlx::query("INSERT INTO follows(follower_id,following_id) VALUES($1,'ds-streamer')")
            .bind(&id)
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    streamers::milestones(&e.app).await.unwrap();
    streamers::milestones(&e.app).await.unwrap();
    find(
        &taken(),
        &[
            "POST /channels/40/messages",
            "<@920> just hit **10 followers**",
        ],
    );

    // Collab: a form, one open request each, interest reaches the poster, closing.
    let post = sub("collab", "post", json!([opt("platform", json!("twitch"))]));
    let form = ask("co1", "920", false, 2, post).await;
    assert!(
        form.contains(r#""type":9"#) && form.contains("collab:modal:twitch"),
        "{form}"
    );
    let fields = |d: &str| {
        json!({"custom_id": "collab:modal:twitch", "components": [
        {"type": 1, "components": [{"type": 4, "custom_id": "game", "value": "Celeste"}]},
        {"type": 1, "components": [{"type": 4, "custom_id": "schedule", "value": ""}]},
        {"type": 1, "components": [{"type": 4, "custom_id": "description", "value": d}]}]})
    };
    let posted = ask("co2", "920", false, 5, fields("Race me")).await;
    assert!(
        posted.contains("Collab request #")
            && posted.contains("Race me")
            && posted.contains("collab:respond:"),
        "{posted}"
    );
    let again = ask("co3", "920", false, 5, fields("Again")).await;
    assert!(again.contains("already have an open"), "{again}");
    let id: i64 = sqlx::query_scalar("SELECT id FROM discord_collabs WHERE discord_user='920'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    let respond = json!({"custom_id": format!("collab:respond:{id}"), "component_type": 2});
    let own = ask("co4", "920", false, 3, respond.clone()).await;
    assert!(own.contains("your own request"), "{own}");
    taken();
    let yes = ask("co5", "921", false, 3, respond.clone()).await;
    assert!(yes.contains("<@920> has been told"), "{yes}");
    find(
        &taken(),
        &[
            "POST /channels/dm-920/messages",
            "interested in your collab request",
        ],
    );
    let twice = ask("co6", "921", false, 3, respond).await;
    assert!(twice.contains("already said"), "{twice}");
    assert_eq!(count("SELECT count(*) FROM discord_achievements WHERE discord_user='920' AND key='collab_posted'").await, 1);
    let browse = sub(
        "collab",
        "browse",
        json!([opt("platform", json!("twitch"))]),
    );
    let browse = ask("co7", "921", false, 2, browse).await;
    assert!(
        browse.contains("Race me") && browse.contains("Page 1/1"),
        "{browse}"
    );
    let close = json!({"custom_id": format!("collab:close:{id}"), "component_type": 2});
    let close = ask("co8", "920", false, 3, close).await;
    assert!(
        close.contains(r#""type":7"#) && close.contains("closed"),
        "{close}"
    );
    let history = ask("co9", "921", false, 2, sub("collab", "history", json!([]))).await;
    assert!(
        history.contains(&format!("#{id}")) && history.contains("cancelled"),
        "{history}"
    );

    // LFG: a post in the LFG forum tagged with its game, and a join button.
    let lfg = json!({"name": "lfg", "options": [opt("game", json!("valorant")), opt("players", json!(3)), opt("rank", json!("Gold"))]});
    let lfg = ask("lf1", "920", false, 2, lfg).await;
    assert!(lfg.contains("<#th1>"), "{lfg}");
    let c = taken();
    find(&c, &["POST /channels/44/threads", "need 3 (Gold)", "tag-v"]);
    find(&c, &["POST /channels/th1/messages", "lfg:join:th1"]);
    let join = json!({"custom_id": "lfg:join:th1", "component_type": 2});
    let joined = ask("lf2", "921", false, 3, join).await;
    assert!(joined.contains("You're in"), "{joined}");
    find(&taken(), &["PUT /channels/th1/thread-members/921"]);

    // The streamer panel: staff post and pin it; its buttons link the proven account at once (an
    // older panel's Kick button says why not), and S.V.E.R's explains the automatic link.
    let panel = ask("pa1", "920", false, 2, json!({"name": "streamerpanel"})).await;
    assert!(panel.contains("Only staff"), "{panel}");
    let panel = ask("pa2", "920", true, 2, json!({"name": "streamerpanel"})).await;
    assert!(panel.contains("posted and pinned in <#40>"), "{panel}");
    let c = taken();
    find(
        &c,
        &[
            "POST /channels/40/messages",
            "str:link:twitch",
            "str:link:sver",
        ],
    );
    find(&c, &["PUT /channels/40/pins/"]);
    none(&c, "str:link:kick");
    let open = json!({"custom_id": "str:link:kick", "component_type": 2});
    let open = ask("pa3", "921", false, 3, open).await;
    assert!(open.contains("can't be linked yet"), "{open}");
    let sver_button = json!({"custom_id": "str:link:sver", "component_type": 2});
    let sver_button = ask("pa4", "921", false, 3, sver_button).await;
    assert!(
        sver_button.contains("dsoutsider") && sver_button.contains("is linked"),
        "{sver_button}"
    );

    // SAFETY: as above.
    unsafe {
        for var in ["KICK_CLIENT_ID", "KICK_CLIENT_SECRET", "YOUTUBE_API_KEY"] {
            std::env::remove_var(var);
        }
    }
}
