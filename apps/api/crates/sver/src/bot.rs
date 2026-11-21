//! The channel bot, part 2 (docs/COMMUNITY.md "Chat commands and the channel bot"): event lines in
//! the chosen personality (docs/LORE.md "The faction bots"), AutoMod with its warn-and-timeout
//! ladder, giveaways, and the Nightbot/Fossabot command import (docs/DEVELOPER_PLATFORM.md §7).
use crate::{
    App, auth,
    commands::{bot, say},
    moderation::{self, Role},
    profiles::{self, Fail, Res},
};
use axum::{Json, Router, extract::State, routing::put};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Event {
    Follow,
    Sub,
    Raid,
    Timeout,
    Surge,
    Milestone,
}
/// Lines per bot (PYRE, ECHO, FAVOR, VOLK), personality (chill, battle, event) and event (follow,
/// sub, raid, timeout, Surge level, follower milestone), three of each (docs/LORE.md "The faction
/// bots"). `{user}` is the person, or a list of names for a burst of follows or subs (so those
/// lines never make a verb agree with it); `{count}` a month, raid size, Surge level or follower
/// count.
const LINES: [[[[&str; 3]; 6]; 3]; 4] = [
    [
        [
            [
                "Welcome, {user}.",
                "Glad you're here, {user}.",
                "Pull up a seat by the fire, {user}.",
            ],
            [
                "Thanks for subscribing, {user}.",
                "Thank you, {user}. Month {count}.",
                "Month {count} for {user}. Thank you.",
            ],
            [
                "{user} is raiding with {count}. Welcome, everyone.",
                "{count} friends from {user}'s stream. Make them welcome.",
                "Welcome, raiders. Thanks for bringing {count}, {user}.",
            ],
            [
                "{user}, take a short break.",
                "{user}, a short pause. See you soon.",
                "Easy, {user}. Back in a bit.",
            ],
            [
                "Surge level {count}. Keep it burning.",
                "Surge level {count}. Nicely done.",
                "Surge level {count}. The fire's warm.",
            ],
            [
                "{count} followers. Thank you, all of you.",
                "{count} followers. Grateful for every one.",
                "We just reached {count} followers. Thank you.",
            ],
        ],
        [
            [
                "Another one walks into the fire. Welcome, {user}.",
                "The flame takes in {user}. Stand with us.",
                "Ash to ember: welcome, {user}.",
            ],
            [
                "Oath sworn, {user}. The forge remembers. Month {count}.",
                "Month {count}. The forge holds your name, {user}.",
                "Steel and oath, {user}. Month {count}.",
            ],
            [
                "Warband at the gate: {user} with {count}. Stand and greet them.",
                "{user} brings {count} through the smoke. Make room.",
                "{count} blades with {user}. The fire grows.",
            ],
            [
                "Cool off, {user}. Come back sharper.",
                "Discipline, {user}. Sit this one out.",
                "The flame cools you, {user}. Return sharper.",
            ],
            [
                "Surge {count}! The fire climbs.",
                "Surge {count}. Feed the forge.",
                "Surge {count}. Discipline holds the flame.",
            ],
            [
                "{count} sworn to the flame. Onward.",
                "{count} in the flame. None turned back.",
                "{count} followers. From ashes, we rise.",
            ],
        ],
        [
            [
                "INTO THE FIRE: {user}!",
                "THE FORGE WELCOMES {user}!",
                "NEW EMBERS: {user}!",
            ],
            [
                "OATH SWORN: {user}! Month {count}! The forge roars!",
                "MONTH {count}! THE FORGE ROARS FOR {user}!",
                "STEEL AND OATH! {user}, MONTH {count}!",
            ],
            [
                "WARBAND OF {count} AT THE GATE, LED BY {user}!",
                "{user} BRINGS {count} BLADES! OPEN THE GATE!",
                "RAID! {count} STRONG WITH {user}! THE FIRE ROARS!",
            ],
            [
                "{user}, cool off. The fire waits.",
                "{user}, step back from the flame a moment.",
                "{user}, cool your steel. Back soon.",
            ],
            [
                "SURGE LEVEL {count}! THE FORGE ROARS!",
                "SURGE {count}! FEED THE FIRE!",
                "SURGE {count}! FROM ASHES, WE RISE!",
            ],
            [
                "{count} FOLLOWERS! THE FIRE SPREADS!",
                "{count} STRONG! THE FORGE BLAZES!",
                "{count} EMBERS! NONE BURN ALONE!",
            ],
        ],
    ],
    [
        [
            [
                "Welcome, {user}. Glad you found us.",
                "Hello, {user}. Make yourself at home.",
                "Welcome aboard, {user}.",
            ],
            [
                "Thanks for subscribing, {user}. Month {count}.",
                "Month {count}, {user}. Much appreciated.",
                "Thank you for the support, {user}. Month {count}.",
            ],
            [
                "{user} brought {count} friends. Welcome.",
                "Hello to {user} and {count} visitors.",
                "{count} new faces from {user}. Welcome in.",
            ],
            [
                "{user}, paused for a bit.",
                "{user}, a short timeout. See you soon.",
                "{user}, taking a short pause.",
            ],
            [
                "Surge level {count} reached.",
                "Surge level {count}. Well done, everyone.",
                "Surge level {count}, logged with thanks.",
            ],
            [
                "{count} followers. Thank you.",
                "{count} followers now. Thank you, all.",
                "Milestone reached: {count} followers.",
            ],
        ],
        [
            [
                "New signal detected: {user}. Logging it.",
                "Archived: {user}. Welcome to the pattern.",
                "Entry recorded: {user}. Knowledge grows.",
            ],
            [
                "A pattern begins: {user}, month {count}.",
                "Month {count} recorded for {user}. The pattern persists.",
                "{user}, month {count}. Filed, with thanks.",
            ],
            [
                "Arrivals inbound: {user} with {count}. Adjusting the model.",
                "{count} new signals from {user}. Recalculating.",
                "Raid detected: {user}, {count} strong. Welcome to the archive.",
            ],
            [
                "Pattern flagged, {user}. Take a pause.",
                "Anomaly logged, {user}. Recalibrate.",
                "{user}, outside parameters. Brief pause.",
            ],
            [
                "Surge level {count}: the signal strengthens.",
                "Surge {count}. The model predicted this.",
                "Surge level {count}. Knowledge ascends.",
            ],
            [
                "Signal count: {count} followers.",
                "{count} entries in the archive. Power follows.",
                "{count} followers. The pattern persists.",
            ],
        ],
        [
            [
                "SIGNAL LOCKED: {user}!",
                "NEW ENTRY IN THE ARCHIVE: {user}!",
                "SIGNAL RECEIVED: {user}!",
            ],
            [
                "{user}: month {count} added to the archive!",
                "MONTH {count}! {user}, THE ARCHIVE REMEMBERS!",
                "PATTERN CONFIRMED: {user}, MONTH {count}!",
            ],
            [
                "{count} ARRIVALS FROM {user}! THE PATTERN GROWS!",
                "RAID INBOUND! {user} WITH {count}!",
                "{user} BRINGS {count} SIGNALS! THE ARCHIVE EXPANDS!",
            ],
            [
                "{user}, pattern flagged. Back soon.",
                "{user}, anomaly logged. Short pause.",
                "{user}, recalibrating you. Back soon.",
            ],
            [
                "SURGE {count}! THE SIGNAL SURGES!",
                "SURGE {count}! OFF THE CHARTS!",
                "SURGE {count}! KNOWLEDGE ASCENDS!",
            ],
            [
                "{count} SIGNALS IN THE ARCHIVE!",
                "{count} FOLLOWERS! THE PATTERN PERSISTS!",
                "{count}! THE ARCHIVE OVERFLOWS!",
            ],
        ],
    ],
    [
        [
            [
                "Welcome in, {user}!",
                "Hey {user}, welcome! Grab a seat.",
                "Glad you made it, {user}!",
            ],
            [
                "{user} subscribed. Thank you!",
                "Thank you, {user}! Month {count}!",
                "Month {count} for {user}. You're the best.",
            ],
            [
                "{user} and {count} friends just arrived. Say hi!",
                "{count} friends from {user}! Everybody say hi!",
                "Look who's here: {user} with {count} friends!",
            ],
            [
                "{user}, a quick breather, okay?",
                "{user}, short break. Snack time?",
                "{user}, take five. We'll save your seat.",
            ],
            [
                "Surge level {count}! Nice.",
                "Surge level {count}! Look at us go.",
                "Surge level {count}! Lucky chat.",
            ],
            [
                "{count} followers! Thank you!",
                "{count} followers! You all rock.",
                "{count} followers! Best table in town.",
            ],
        ],
        [
            [
                "There's a seat for you, {user}. There always was.",
                "Ring the bell: a seat for {user}.",
                "Pull up a chair, {user}. Fortune's warm tonight.",
            ],
            [
                "{user}, your fortune's in the hall now. Month {count}.",
                "Month {count}, {user}. The bell rings for you.",
                "Generous and bold, {user}. Month {count}.",
            ],
            [
                "Open the doors, {user} brought {count}!",
                "{user} rolls in with {count}. More chairs!",
                "The bell rings: {user} and {count} at the crossroads!",
            ],
            [
                "Easy, {user}. Take a breather.",
                "Your seat's saved, {user}. Take a breather.",
                "Sit one out, {user}. The table will wait.",
            ],
            [
                "Fortune climbs: Surge level {count}.",
                "Surge {count}. The table's hot.",
                "Surge {count}. Fortune favors the bold.",
            ],
            [
                "{count} seats at the table now.",
                "{count} at the crossroads. Ring the bell.",
                "{count} followers. One open seat became a crowd.",
            ],
        ],
        [
            [
                "A SEAT AT THE TABLE FOR {user}!",
                "RING THE BELL FOR {user}!",
                "WELCOME TO THE TABLE, {user}!",
            ],
            [
                "Dice rolled for {user}: month {count}! Fortune smiles!",
                "MONTH {count}! THE BELL RINGS FOR {user}!",
                "JACKPOT! {user}, MONTH {count}!",
            ],
            [
                "{user} BRINGS {count} TO THE PARTY!",
                "{count} AT THE DOOR WITH {user}! LET THEM IN!",
                "THE BELL IS RINGING! {user} AND {count} FRIENDS!",
            ],
            [
                "Easy, {user}! Catch your breath.",
                "Whoa, {user}! Quick breather!",
                "{user}, sit tight! Back in a flash!",
            ],
            [
                "SURGE LEVEL {count}! JACKPOT ENERGY!",
                "SURGE {count}! THE BELL WON'T STOP!",
                "SURGE {count}! FORTUNE FAVORS THE BOLD!",
            ],
            [
                "{count} AT THE TABLE! FORTUNE SMILES!",
                "{count} FOLLOWERS! RING EVERY BELL!",
                "{count}! THE TABLE KEEPS GROWING!",
            ],
        ],
    ],
    [
        [
            [
                "Welcome, {user}.",
                "Welcome in, {user}. Enjoy the stream.",
                "Good to have you, {user}.",
            ],
            [
                "Thanks for the support, {user}.",
                "Thank you, {user}. Month {count}.",
                "Month {count}, {user}. Appreciated.",
            ],
            [
                "Welcome, {user} and company ({count}).",
                "{user} arrives with {count}. Welcome.",
                "Welcome, everyone from {user}'s stream ({count}).",
            ],
            [
                "{user}, a short pause.",
                "{user}, a brief timeout.",
                "{user}, back shortly.",
            ],
            [
                "Surge level {count}.",
                "Surge level {count}. Well played.",
                "Surge level {count} reached.",
            ],
            [
                "{count} followers.",
                "{count} followers. Thank you.",
                "{count} followers now. Steady.",
            ],
        ],
        [
            [
                "Noted. Welcome under the Accord, {user}.",
                "The watch grows: {user}.",
                "Under the Accord, all are welcome. Even {user}.",
            ],
            [
                "The Accord thanks you, {user}. Month {count}.",
                "Month {count}. The watch notes it, {user}.",
                "Steady support, {user}. Month {count}.",
            ],
            [
                "{user} and a company of {count} cross under the Accord.",
                "{count} cross the border with {user}. The Accord holds.",
                "A company of {count} under {user}'s banner. Welcome.",
            ],
            [
                "The Accord holds, {user}. Take a breath.",
                "The watch sees all, {user}. A short pause.",
                "Even-handed, {user}. A short pause.",
            ],
            [
                "The Accord notes Surge level {count}.",
                "Surge level {count}. The watch approves.",
                "Surge {count}. Every light gets its turn.",
            ],
            [
                "{count} now keep the watch.",
                "{count} under the Accord.",
                "{count} followers. The grey wolf watches.",
            ],
        ],
        [
            [
                "TO THE WATCH: {user}!",
                "THE WATCH WELCOMES {user}!",
                "UNDER THE ACCORD: {user}!",
            ],
            [
                "Pledged to the Accord: {user}! Month {count}!",
                "MONTH {count}! THE WATCH SALUTES {user}!",
                "THE GREY WOLF HOWLS FOR {user}! MONTH {count}!",
            ],
            [
                "{user} CROSSES WITH {count}! THE ACCORD HOLDS!",
                "{count} CROSS UNDER THE ACCORD WITH {user}!",
                "THE WATCH OPENS THE GATE FOR {user} AND {count}!",
            ],
            [
                "{user}, the Accord pauses you.",
                "{user}, the watch calls a pause.",
                "{user}, the Accord asks a short pause.",
            ],
            [
                "SURGE {count}! THE WATCH RISES!",
                "SURGE {count}! THE WATCH IS AWAKE!",
                "SURGE {count}! EVERY LIGHT BURNS BRIGHT!",
            ],
            [
                "{count} STAND THE WATCH!",
                "{count} UNDER THE ACCORD!",
                "{count}! THE GREY WOLF HOWLS!",
            ],
        ],
    ],
];
/// Occasional quips per bot (PYRE, ECHO, FAVOR, VOLK) and personality (chill, battle, event),
/// docs/LORE.md "Quips" (a draft for Joe's approval; the feature is off by default).
pub const QUIPS: [[[&str; 4]; 3]; 4] = [
    [
        [
            "Stay warm, everyone. Glad you're here.",
            "Hydrate. Even the forge needs water.",
            "Good company makes a good fire.",
            "Take a breath and stretch. We'll keep the fire lit.",
        ],
        [
            "The forge is hot. Keep swinging.",
            "Ashes behind us, ground ahead.",
            "Every ember counts. Stay loud.",
            "From ashes, we rise.",
        ],
        [
            "THE FIRE IS ROARING TONIGHT!",
            "FORGE IT, FIGHT IT, FINISH IT!",
            "EVERY EMBER, ALL IN!",
            "FROM ASHES, WE RISE!",
        ],
    ],
    [
        [
            "Noting a good mood in here. Carry on.",
            "Reminder: water, posture, kindness.",
            "The archive appreciates you being here.",
            "Questions are welcome. Patterns start with one.",
        ],
        [
            "Knowledge ascends. Power follows.",
            "Data suggests we're winning. Keep going.",
            "Every message is a signal. Send yours.",
            "The pattern persists.",
        ],
        [
            "SIGNAL STRENGTH: MAXIMUM!",
            "THE ARCHIVE WILL REMEMBER TONIGHT!",
            "PATTERN LOCKED. HYPE CONFIRMED!",
            "KNOWLEDGE ASCENDS!",
        ],
    ],
    [
        [
            "There's always another seat at this table.",
            "Stretch break? The bell will still be here.",
            "Fortune check: you're in a good chat.",
            "New here? Say hi. We don't bite. Mostly.",
        ],
        [
            "Fortune favors the bold. And the generous.",
            "Roll the dice, friends. The table's hot.",
            "Every voice is a coin in the pot.",
            "Ring the bell, raise a cheer.",
        ],
        [
            "THE BELL IS RINGING!",
            "JACKPOT ENERGY IN HERE!",
            "EVERYONE GETS A SEAT TONIGHT!",
            "FORTUNE FAVORS THE BOLD!",
        ],
    ],
    [
        [
            "All quiet under the Accord. Enjoy the stream.",
            "A short reminder: be kind, stay hydrated.",
            "The watch continues. Welcome, everyone.",
            "Good stream. Carry on.",
        ],
        [
            "The grey wolf watches.",
            "The Accord holds. Play fair, play hard.",
            "Every light gets its turn.",
            "Ground may be taken. A voice may never be silenced.",
        ],
        [
            "THE WATCH IS AWAKE!",
            "UNDER THE ACCORD, WE CELEBRATE!",
            "EVERY LIGHT GETS ITS TURN!",
            "THE GREY WOLF HOWLS!",
        ],
    ],
];
/// The events a streamer can turn off or give their own line, in `Event` order.
pub const EVENTS: [&str; 6] = ["follow", "sub", "raid", "timeout", "surge", "milestone"];
/// (bot, personality) as table indexes; anything unknown is VOLK, chill.
fn voice(bot: &str, personality: &str) -> (usize, usize) {
    (
        ["pyre", "echo", "favor", "volk"]
            .iter()
            .position(|k| *k == bot)
            .unwrap_or(3),
        ["chill", "battle", "event"]
            .iter()
            .position(|k| *k == personality)
            .unwrap_or(0),
    )
}
/// A quip for this bot and personality; `turn` picks which one (the caller passes the time).
pub fn quip_line(bot: &str, personality: &str, turn: i64) -> &'static str {
    let (b, p) = voice(bot, personality);
    QUIPS[b][p][turn.rem_euclid(4) as usize]
}
/// Says a quip in the channel's bot voice (commands::tick decides when).
pub async fn quip(app: &App, channel: &str) -> Res<()> {
    let (key, _) = bot(app, channel).await?;
    let text = quip_line(
        key,
        &voice_settings(app, channel).await?.personality,
        chrono::Utc::now().timestamp() / 60,
    );
    say(app, channel, text).await
}
/// The bot's own lines for an event, in this personality.
pub fn lines(bot: &str, personality: &str, event: Event) -> [&'static str; 3] {
    let (b, p) = voice(bot, personality);
    LINES[b][p][event as usize]
}
/// `template` (one of `lines`, or the streamer's own) filled in.
pub fn fill(template: &str, user: &str, count: i64) -> String {
    template
        .replace("{user}", user)
        .replace("{count}", &count.to_string())
}
/// One of the bot's lines for an event; `turn` picks which.
pub fn line(
    bot: &str,
    personality: &str,
    event: Event,
    user: &str,
    count: i64,
    turn: usize,
) -> String {
    fill(lines(bot, personality, event)[turn % 3], user, count)
}
/// A burst of names as one: "Ana", "Ana and Bo", "Ana, Bo and Cy", "Ana, Bo, Cy and 4 others".
pub fn names(all: &[&str]) -> String {
    match all {
        [] => String::new(),
        [one] => (*one).to_string(),
        [first @ .., last] if all.len() <= 3 => format!("{} and {last}", first.join(", ")),
        _ => {
            let rest = all.len() - 3;
            let others = if rest == 1 { "other" } else { "others" };
            format!("{} and {rest} {others}", all[..3].join(", "))
        }
    }
}
/// How the channel's bot speaks: personality, cross-faction greetings, the events turned off and
/// the streamer's own lines.
struct Voice {
    personality: String,
    rivalry: bool,
    off: Vec<String>,
    lines: sqlx::types::Json<std::collections::HashMap<String, String>>,
}
async fn voice_settings(app: &App, channel: &str) -> Res<Voice> {
    let row: Option<(String, bool, Vec<String>, sqlx::types::Json<_>)> = sqlx::query_as(
        "SELECT bot_personality,bot_rivalry,bot_off,bot_lines FROM chat_settings WHERE channel_id=$1",
    )
    .bind(channel)
    .fetch_optional(&app.db)
    .await?;
    let (personality, rivalry, off, lines) = row.unwrap_or_else(|| {
        (
            "chill".into(),
            false,
            Vec::new(),
            sqlx::types::Json(Default::default()),
        )
    });
    Ok(Voice {
        personality,
        rivalry,
        off,
        lines,
    })
}
/// The bots' factions: PYRE speaks for Myria, ECHO for Aetheron, FAVOR for Glint; VOLK for none.
fn bot_faction(bot: &str) -> Option<&'static str> {
    match bot {
        "pyre" => Some("myria"),
        "echo" => Some("aetheron"),
        "favor" => Some("glint"),
        _ => None,
    }
}
/// What one faction says of another (docs/LORE.md "Rivalries"): jabs, never contempt.
pub fn rivalry(speaker: &str, about: &str) -> Option<&'static str> {
    Some(match (speaker, about) {
        ("myria", "aetheron") => "They study the fire. We walk through it.",
        ("myria", "glint") => "They crown the loudest.",
        ("aetheron", "myria") => "Brave, and wasteful.",
        ("aetheron", "glint") => "A crowd is not an argument.",
        ("glint", "myria") => "They'd rather win alone.",
        ("glint", "aetheron") => "Clever, and lonely.",
        _ => return None,
    })
}
/// The bot's line for an event (unless the streamer turned that event off): their own line when
/// set, else one of the personality's. `users` is one person, or a burst of follows or subs; a
/// name the channel's banned words or link rule refuses is left out, not the whole line.
async fn announce(
    app: &App,
    channel: &str,
    event: Event,
    users: &[String],
    count: i64,
    raider_faction: Option<&str>,
) -> Res<()> {
    let voice = voice_settings(app, channel).await?;
    let kind = EVENTS[event as usize];
    if voice.off.iter().any(|e| e == kind) {
        return Ok(());
    }
    let rules = crate::commands::rules(app, channel).await?;
    let shown: Vec<&str> = users
        .iter()
        .map(String::as_str)
        .filter(|n| moderation::code_allowed(&rules.0, rules.1, n))
        .collect();
    if shown.is_empty() && users.iter().any(|n| !n.is_empty()) {
        return Ok(());
    }
    let (key, _) = bot(app, channel).await?;
    let template = match voice.lines.get(kind) {
        Some(own) => own.as_str(),
        None => lines(key, &voice.personality, event)
            [chrono::Utc::now().timestamp_subsec_nanos() as usize % 3],
    };
    let mut text = fill(template, &names(&shown), count);
    // Cross-faction greetings, when the streamer turned them on: a rival's raid gets a jab.
    if let (Some(speaker), Some(about)) = (bot_faction(key), raider_faction)
        && let Some(jab) = rivalry(speaker, about)
        && voice.rivalry
    {
        text = format!("{text} {jab}");
    }
    say(app, channel, &text).await
}

// ---- Event lines (a transactional outbox) ----

/// Records a follow or a sub month for the bot to greet (in the caller's transaction, so nothing
/// is announced for a change that rolls back). `user` is the person's account.
pub async fn record(
    db: &mut PgConnection,
    channel: &str,
    kind: &str,
    user: &str,
    count: i64,
) -> Res<()> {
    sqlx::query("INSERT INTO bot_events(channel_id,kind,name,count) SELECT $1,$2,display_name,$4 FROM channel_users WHERE id=$3")
        .bind(channel).bind(kind).bind(user).bind(count as i32)
        .execute(db).await?;
    Ok(())
}
/// A counted raid (the raider's name, faction and how many arrived).
pub async fn record_raid(
    app: &App,
    channel: &str,
    raider: &str,
    faction: Option<&str>,
    arrivals: i64,
) -> Res<()> {
    sqlx::query(
        "INSERT INTO bot_events(channel_id,kind,name,count,faction) VALUES($1,'raid',$2,$3,$4)",
    )
    .bind(channel)
    .bind(raider)
    .bind(arrivals as i32)
    .bind(faction)
    .execute(&app.db)
    .await?;
    Ok(())
}
/// Follower counts the bot celebrates, each once per channel.
pub const MILESTONES: [i64; 10] = [10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000];
/// After a follow starts counting (in its transaction): a milestone line the first time the
/// counted total reaches one.
pub async fn follower_milestone(db: &mut PgConnection, channel: &str) -> Res<()> {
    let followers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM follows WHERE following_id=$1 AND counted_at IS NOT NULL",
    )
    .bind(channel)
    .fetch_one(&mut *db)
    .await?;
    if !MILESTONES.contains(&followers) {
        return Ok(());
    }
    let first = sqlx::query(
        "INSERT INTO bot_milestones(channel_id,followers) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(channel)
    .bind(followers)
    .execute(&mut *db)
    .await?
    .rows_affected();
    if first == 1 {
        sqlx::query(
            "INSERT INTO bot_events(channel_id,kind,name,count) VALUES($1,'milestone','',$2)",
        )
        .bind(channel)
        .bind(followers as i32)
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}
/// A Surge reaching a new level.
pub async fn surge_level(app: &App, channel: &str, level: i16) -> Res<()> {
    sqlx::query("INSERT INTO bot_events(channel_id,kind,name,count) VALUES($1,'surge','',$2)")
        .bind(channel)
        .bind(i32::from(level))
        .execute(&app.db)
        .await?;
    Ok(())
}
/// Every few seconds: greets recent events on live channels; anything else is dropped.
pub async fn drain(app: &App) -> Res<()> {
    let events: Vec<(String, String, String, i32, Option<String>, bool)> = sqlx::query_as("WITH batch AS (DELETE FROM bot_events e WHERE id IN (SELECT id FROM bot_events ORDER BY id LIMIT 500)
        RETURNING e.id,e.channel_id,e.kind,e.name,e.count,e.faction,
        e.created_at>now()-interval '2 minutes' AND EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=e.channel_id AND b.state='LIVE') AS live)
        SELECT channel_id,kind,name,count,faction,live FROM batch ORDER BY id")
        .fetch_all(&app.db)
        .await?;
    // A burst of follows, or of subs at the same month (a gift bomb), becomes one line.
    type Group = (String, String, i32, Option<String>, Vec<String>);
    let mut groups: Vec<Group> = Vec::new();
    for (channel, kind, name, count, faction, live) in events {
        if !live {
            continue;
        }
        if matches!(kind.as_str(), "follow" | "sub")
            && let Some(group) = groups
                .iter_mut()
                .find(|g| g.0 == channel && g.1 == kind && g.2 == count)
        {
            group.4.push(name);
            continue;
        }
        groups.push((channel, kind, count, faction, vec![name]));
    }
    for (channel, kind, count, faction, names) in groups {
        let event = match kind.as_str() {
            "follow" => Event::Follow,
            "sub" => Event::Sub,
            "surge" => Event::Surge,
            "milestone" => Event::Milestone,
            _ => Event::Raid,
        };
        // One channel's failure never drops the rest of the batch (it is already deleted).
        if announce(
            app,
            &channel,
            event,
            &names,
            count.into(),
            faction.as_deref(),
        )
        .await
        .is_err()
        {
            eprintln!("bot_event=announce outcome=dropped");
        }
    }
    Ok(())
}

// ---- AutoMod ----

/// What one AutoMod strictness level allows. Each rule (caps, repeats, spam) is off (0) or one
/// of three levels: relaxed (1), standard (2, the original rules) and strict (3).
pub struct Level {
    /// Caps: at least this many letters (mentions and emote codes aside), over this percent capitals.
    caps_letters: usize,
    caps_percent: usize,
    /// Repeats: a run of this many of the same character (digits aside, so numbers pass), or the
    /// same message again (ignoring case, spacing and punctuation) within `repeat_seconds`, when
    /// it's at least `repeat_min` characters (short lines like "gg" are often said twice).
    run: usize,
    repeat_min: usize,
    repeat_seconds: i32,
    /// Spam: over this many symbols or emoji (each emoji counts once, however it's built),
    /// making up over this percent of the message.
    symbols: usize,
    symbol_percent: usize,
}
pub const LEVELS: [Level; 3] = [
    Level {
        caps_letters: 15,
        caps_percent: 85,
        run: 15,
        repeat_min: 15,
        repeat_seconds: 15,
        symbols: 25,
        symbol_percent: 50,
    },
    Level {
        caps_letters: 10,
        caps_percent: 70,
        run: 10,
        repeat_min: 10,
        repeat_seconds: 30,
        symbols: 15,
        symbol_percent: 30,
    },
    Level {
        caps_letters: 8,
        caps_percent: 55,
        run: 7,
        repeat_min: 6,
        repeat_seconds: 60,
        symbols: 8,
        symbol_percent: 20,
    },
];
fn level(n: i16) -> Option<&'static Level> {
    LEVELS.get(usize::try_from(n).ok()?.checked_sub(1)?)
}
/// Which AutoMod rule a message breaks at these levels, if any, before the database check for a
/// repeated message. `emotes` are the message's words that are emote codes, which caps ignores.
pub fn rule(
    body: &str,
    caps: i16,
    repeats: i16,
    spam: i16,
    emotes: &[String],
) -> Option<&'static str> {
    if let Some(l) = level(caps) {
        let letters: Vec<char> = body
            .split_whitespace()
            .filter(|w| !w.starts_with('@') && !emotes.iter().any(|e| e == w))
            .flat_map(str::chars)
            .filter(|c| c.is_alphabetic())
            .collect();
        let upper = letters.iter().filter(|c| c.is_uppercase()).count();
        if letters.len() >= l.caps_letters && upper * 100 > letters.len() * l.caps_percent {
            return Some("caps");
        }
    }
    if let Some(l) = level(repeats) {
        let chars: Vec<char> = body.chars().collect();
        if chars.windows(l.run).any(|w| {
            !w[0].is_whitespace() && !w[0].is_ascii_digit() && w.iter().all(|c| *c == w[0])
        }) {
            return Some("repeats");
        }
    }
    if let Some(l) = level(spam) {
        let marks: Vec<&str> = body
            .graphemes(true)
            .filter(|g| !g.trim().is_empty())
            .collect();
        let symbols = marks
            .iter()
            .filter(|g| {
                g.chars()
                    .next()
                    .is_some_and(|c| !c.is_alphanumeric() && !".,!?'\"()-:;@#".contains(c))
            })
            .count();
        if symbols > l.symbols && symbols * 100 > marks.len() * l.symbol_percent {
            return Some("spam");
        }
    }
    None
}
const WARNINGS: [(&str, &str); 3] = [
    ("caps", "Please ease off the caps."),
    ("repeats", "Please don't repeat yourself."),
    ("spam", "Please go easy on symbols and emojis."),
];
/// Runs before a message is stored (moderation::check_send); channel roles are exempt. A first
/// strike in an hour warns (the message is refused); later ones time out per the channel's ladder,
/// announced by the bot and logged for moderators, who can lift it.
pub async fn automod(app: &App, channel: &str, user: &auth::User, body: &str) -> Res<()> {
    let settings: Option<(i16, i16, i16, Vec<i32>)> = sqlx::query_as("SELECT automod_caps,automod_repeats,automod_spam,automod_ladder FROM chat_settings WHERE channel_id=$1")
        .bind(channel).fetch_optional(&app.db).await?;
    let Some((caps, repeats, spam, ladder)) = settings.filter(|s| s.0 > 0 || s.1 > 0 || s.2 > 0)
    else {
        return Ok(());
    };
    let mut broken = rule(body, caps, repeats, spam, &[]);
    // All-caps emote codes aren't shouting: looked up only when caps would hold the message.
    if broken == Some("caps") {
        let words: Vec<&str> = body.split_whitespace().collect();
        let emotes: Vec<String> = sqlx::query_scalar("SELECT code FROM channel_emotes WHERE status='VISIBLE' AND code=ANY($1)
            UNION SELECT code FROM outside_emotes WHERE (channel_id=$2 OR channel_id IS NULL) AND code=ANY($1)")
            .bind(&words).bind(channel).fetch_all(&app.db).await?;
        broken = rule(body, caps, repeats, spam, &emotes);
    }
    if broken.is_none()
        && let Some(l) = level(repeats)
        && body.trim().chars().count() >= l.repeat_min
    {
        // Messages are stored under the chat's first channel, so this covers shared co-stream chat
        // there too. Letters and numbers only: "buy now" and "BUY NOW!!" are the same message.
        let same: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_messages WHERE channel_id=$1 AND author_id=$2 AND deleted_at IS NULL
                AND created_at>now()-make_interval(secs=>$4)
                AND coalesce(nullif(regexp_replace(lower(body),'[^[:alnum:]]+','','g'),''),lower(body))=coalesce(nullif(regexp_replace(lower($3),'[^[:alnum:]]+','','g'),''),lower($3)))")
            .bind(channel).bind(&user.id).bind(body.trim()).bind(f64::from(l.repeat_seconds)).fetch_one(&app.db).await?;
        broken = same.then_some("repeats");
    }
    let Some(broken) = broken else {
        return Ok(());
    };
    if moderation::role_of(app, channel, user).await?.is_some() {
        return Ok(());
    }
    let mut tx = app.db.begin().await?;
    let strikes: i64 = sqlx::query_scalar("SELECT count(*) FROM automod_strikes WHERE channel_id=$1 AND user_id=$2 AND at>now()-interval '1 hour'")
        .bind(channel).bind(&user.id).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO automod_strikes(channel_id,user_id,rule) VALUES($1,$2,$3)")
        .bind(channel)
        .bind(&user.id)
        .bind(broken)
        .execute(&mut *tx)
        .await?;
    let seconds = ladder
        .get(strikes as usize)
        .or(ladder.last())
        .copied()
        .unwrap_or(0);
    let warning = WARNINGS.iter().find(|w| w.0 == broken).map_or("", |w| w.1);
    if seconds <= 0 {
        tx.commit().await?;
        return Err(Fail::bad(format!(
            "The channel bot held your message. {warning}"
        )));
    }
    sqlx::query("INSERT INTO channel_restrictions(channel_id,user_id,kind,until) VALUES($1,$2,'timeout',now()+make_interval(secs=>$3)) ON CONFLICT(channel_id,user_id,kind) DO UPDATE SET until=greatest(channel_restrictions.until,EXCLUDED.until), created_at=CASE WHEN EXCLUDED.until>channel_restrictions.until THEN now() ELSE channel_restrictions.created_at END")
        .bind(channel).bind(&user.id).bind(f64::from(seconds)).execute(&mut *tx).await?;
    let detail = json!({"rule": broken, "seconds": seconds, "by": "bot"});
    let reason = format!("AutoMod: {broken}");
    moderation::log(
        &mut tx,
        channel,
        channel,
        Role::Owner,
        "automod_timeout",
        Some(&user.id),
        None,
        detail,
        &reason,
    )
    .await?;
    let cleared = moderation::clear_recent(&mut tx, channel, &user.id).await?;
    tx.commit().await?;
    moderation::publish_cleared(app, cleared);
    let name: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM channel_users WHERE id=$1")
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    announce(
        app,
        channel,
        Event::Timeout,
        &[name.unwrap_or_else(|| user.username.clone())],
        0,
        None,
    )
    .await?;
    let length = if seconds >= 120 {
        format!("{} minutes", seconds / 60)
    } else {
        format!("{seconds} seconds")
    };
    Err(Fail::denied(format!(
        "The channel bot timed you out for {length}. {warning}"
    )))
}
#[derive(Deserialize)]
pub struct AutoMod {
    caps: i16,
    repeats: i16,
    spam: i16,
    ladder: Vec<i32>,
}
/// PUT /api/me/automod
async fn set_automod(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<AutoMod>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    for (field, n) in [
        ("caps", input.caps),
        ("repeats", input.repeats),
        ("spam", input.spam),
    ] {
        if !(0..=3).contains(&n) {
            return Err(Fail::field(
                field,
                "Choose off, relaxed, standard or strict.",
            ));
        }
    }
    if input.ladder.is_empty()
        || input.ladder.len() > 5
        || input
            .ladder
            .iter()
            .any(|s| *s != 0 && !(10..=1_209_600).contains(s))
    {
        return Err(Fail::field(
            "ladder",
            "Use 1–5 steps: a warning (0) or 10 seconds to 14 days.",
        ));
    }
    sqlx::query("INSERT INTO chat_settings(channel_id,automod_caps,automod_repeats,automod_spam,automod_ladder) VALUES($1,$2,$3,$4,$5) ON CONFLICT(channel_id) DO UPDATE SET automod_caps=$2,automod_repeats=$3,automod_spam=$4,automod_ladder=$5")
        .bind(&user.id).bind(input.caps).bind(input.repeats).bind(input.spam).bind(&input.ladder).execute(&app.db).await?;
    Ok(Json(json!({"automod": settings(&app, &user.id).await?})))
}
/// The AutoMod settings for Creator Studio: each rule's level (0 off to 3 strict) and the ladder.
pub async fn settings(app: &App, owner: &str) -> Res<Value> {
    let row: Option<(i16, i16, i16, Vec<i32>)> = sqlx::query_as("SELECT automod_caps,automod_repeats,automod_spam,automod_ladder FROM chat_settings WHERE channel_id=$1")
        .bind(owner).fetch_optional(&app.db).await?;
    let (caps, repeats, spam, ladder) = row.unwrap_or((0, 0, 0, vec![0, 60, 600]));
    Ok(json!({"caps": caps, "repeats": repeats, "spam": spam, "ladder": ladder}))
}

// ---- Giveaways ----

/// `!giveaway start KEYWORD`, `!giveaway end` and `!giveaway cancel` (owner and moderators), and
/// entries: a viewer who types the keyword while watching (a Counted session) enters once.
/// Returns true when the message was a giveaway command.
pub async fn giveaway(app: &App, channel: &str, user: &auth::User, body: &str) -> Res<bool> {
    let words: Vec<&str> = body.split_whitespace().collect();
    if words
        .first()
        .is_some_and(|w| w.eq_ignore_ascii_case("!giveaway"))
    {
        if moderation::role_of(app, channel, user).await?.is_none() {
            return Ok(true);
        }
        let text = match words.get(1).map(|w| w.to_ascii_lowercase()).as_deref() {
            Some("start") => match words
                .get(2)
                .filter(|k| (1..=25).contains(&k.chars().count()))
            {
                None => "Start a giveaway with !giveaway start KEYWORD.".to_string(),
                Some(keyword) => {
                    let started = sqlx::query("INSERT INTO giveaways(id,channel_id,keyword,started_by) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                        .bind(profiles::new_id()).bind(channel).bind(keyword).bind(&user.id).execute(&app.db).await?;
                    if started.rows_affected() == 1 {
                        format!(
                            "Giveaway! Type {keyword} in chat while you're watching to enter. The prize is the streamer's; good luck!"
                        )
                    } else {
                        "A giveaway is already running. End it with !giveaway end.".into()
                    }
                }
            },
            Some("end") => {
                let ended: Option<(Option<String>, i64)> = sqlx::query_as("WITH g AS (SELECT id FROM giveaways WHERE channel_id=$1 AND ended_at IS NULL),
                    w AS (SELECT e.user_id FROM giveaway_entries e JOIN g ON g.id=e.giveaway_id
                          WHERE NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=$1 AND r.user_id=e.user_id AND r.kind='ban')
                          ORDER BY random() LIMIT 1),
                    u AS (UPDATE giveaways SET ended_at=now(),winner_id=(SELECT user_id FROM w),entries=(SELECT count(*) FROM giveaway_entries x WHERE x.giveaway_id=giveaways.id)
                          WHERE id=(SELECT id FROM g) RETURNING winner_id,entries)
                    SELECT (SELECT display_name FROM channel_users c WHERE c.id=u.winner_id),u.entries::bigint FROM u")
                    .bind(channel).fetch_optional(&app.db).await?;
                match ended {
                    Some((Some(winner), entries)) => format!(
                        "The giveaway winner is {winner}! ({entries} entered.) Congratulations!"
                    ),
                    Some(_) => "The giveaway ended with no entries.".into(),
                    None => "There's no giveaway running.".into(),
                }
            }
            Some("cancel") => {
                sqlx::query(
                    "UPDATE giveaways SET ended_at=now() WHERE channel_id=$1 AND ended_at IS NULL",
                )
                .bind(channel)
                .execute(&app.db)
                .await?;
                "The giveaway was cancelled.".into()
            }
            _ => "Giveaways: !giveaway start KEYWORD, !giveaway end, !giveaway cancel.".into(),
        };
        say(app, channel, &text).await?;
        return Ok(true);
    }
    // An entry: the exact keyword, from someone with a Counted session on this live channel.
    if words.len() == 1 && user.id != channel {
        sqlx::query("INSERT INTO giveaway_entries(giveaway_id,user_id)
            SELECT g.id,$2 FROM giveaways g WHERE g.channel_id=$1 AND g.ended_at IS NULL AND lower(g.keyword)=lower($3)
              AND EXISTS(SELECT 1 FROM playback_leases l JOIN broadcasts b ON b.id=l.broadcast_id
                         WHERE b.owner_id=$1 AND b.state IN ('LIVE','RECONNECTING') AND l.viewer_key='u:'||$2 AND l.expires_at>now() AND l.level IN ('counted','trusted'))
            ON CONFLICT DO NOTHING")
            .bind(channel).bind(&user.id).bind(words[0]).execute(&app.db).await?;
    }
    Ok(false)
}

// ---- Command import (Nightbot, Fossabot) ----

/// Common variables, translated; anything else in `$(…)` or `${…}` is flagged.
const VARIABLES: [(&str, &str); 10] = [
    ("$(user)", "{user}"),
    ("$(touser)", "{user}"),
    ("$(channel)", "{channel}"),
    ("$(uptime)", "{uptime}"),
    ("$(game)", "{game}"),
    ("${user}", "{user}"),
    ("${touser}", "{user}"),
    ("${channel}", "{channel}"),
    ("${uptime}", "{uptime}"),
    ("${game}", "{game}"),
];
/// One pasted line, `!name response` or `!name: response`: (name, reply, flagged variables).
pub fn parse_import(line: &str) -> Option<(String, String, Vec<String>)> {
    let rest = line.trim().strip_prefix('!')?;
    let (name, reply) = rest.split_once(|c: char| c.is_whitespace() || c == ':')?;
    let name = name.to_ascii_lowercase();
    let mut reply = reply
        .trim_start_matches([':', ' ', '-', '\t'])
        .trim()
        .to_string();
    for (from, to) in VARIABLES {
        reply = reply.replace(from, to);
    }
    let mut flagged = Vec::new();
    for (open, close) in [("$(", ')'), ("${", '}')] {
        let mut from = 0;
        while let Some(at) = reply[from..].find(open) {
            let start = from + at;
            let end = reply[start..]
                .find(close)
                .map_or(reply.len(), |e| start + e + 1);
            flagged.push(reply[start..end].to_string());
            from = end;
        }
    }
    (!name.is_empty() && !reply.is_empty()).then_some((name, reply, flagged))
}
#[derive(Deserialize)]
pub struct Import {
    text: String,
}
/// POST /api/me/commands-import: a pasted Nightbot or Fossabot command list becomes commands.
async fn import(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Import>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    if input.text.len() > 50_000 {
        return Err(Fail::bad("Paste up to 50,000 characters."));
    }
    let (mut imported, mut skipped, mut flagged) = (Vec::new(), Vec::new(), Vec::new());
    for line in input
        .text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(200)
    {
        let short: String = line.chars().take(80).collect();
        let Some((name, reply, vars)) = parse_import(line) else {
            skipped.push(json!({"line": short, "why": "Not a !command and reply."}));
            continue;
        };
        if let Err(why) =
            crate::commands::store(&app, &user.id, &name, &reply, "everyone", 10).await
        {
            skipped.push(json!({"line": short, "why": why.message}));
            continue;
        }
        if !vars.is_empty() {
            flagged.push(json!({"name": name, "variables": vars}));
        }
        imported.push(name);
    }
    Ok(Json(
        json!({"imported": imported, "skipped": skipped, "flagged": flagged}),
    ))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/automod", put(set_automod))
        .route("/api/me/commands-import", axum::routing::post(import))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surge_and_milestone_lines() {
        assert_eq!(
            line("pyre", "chill", Event::Surge, "x", 3, 0),
            "Surge level 3. Keep it burning."
        );
        assert_eq!(
            line("volk", "event", Event::Milestone, "x", 100, 0),
            "100 STAND THE WATCH!"
        );
        assert_eq!(
            line("favor", "battle", Event::Timeout, "Ana", 0, 0),
            "Easy, Ana. Take a breather."
        );
        assert_eq!(
            line("echo", "battle", Event::Follow, "Ana", 0, 4),
            "Archived: Ana. Welcome to the pattern.",
            "the turn picks one of three"
        );
    }
    #[test]
    fn rules_and_lines() {
        let none: &[String] = &[];
        assert_eq!(
            rule("THIS IS ALL CAPS YELLING", 2, 0, 0, none),
            Some("caps")
        );
        assert_eq!(rule("Hello THERE friend of mine", 2, 0, 0, none), None);
        assert_eq!(rule("noooooooooooooo", 0, 2, 0, none), Some("repeats"));
        assert_eq!(rule("$$$%%%^^^&&&***~~~", 0, 0, 2, none), Some("spam"));
        assert_eq!(rule("normal message", 3, 3, 3, none), None);
        assert_eq!(
            line("volk", "battle", Event::Raid, "Ana", 12, 0),
            "Ana and a company of 12 cross under the Accord."
        );
        assert_eq!(
            line("nope", "nope", Event::Follow, "Ana", 0, 0),
            "Welcome, Ana."
        );
    }
    #[test]
    fn levels_and_false_positives() {
        let none: &[String] = &[];
        // Mentions and emote codes aren't shouting.
        assert_eq!(rule("@BIGSTREAMER welcome back", 2, 0, 0, none), None);
        let hype = vec!["SVERHYPE".to_string()];
        assert_eq!(rule("SVERHYPE SVERHYPE yes", 2, 0, 0, &hype), None);
        // Each level is its own threshold: 10 of 14 letters capitals is 71%.
        let loud = "WHAT A GREAT play";
        assert_eq!(rule(loud, 1, 0, 0, none), None, "relaxed needs 85%");
        assert_eq!(rule(loud, 2, 0, 0, none), Some("caps"), "standard over 70%");
        // Numbers aren't stretched letters; a strict channel catches shorter runs.
        assert_eq!(rule("that's 10000000000 views", 0, 3, 0, none), None);
        assert_eq!(rule("nooooooo", 0, 2, 0, none), None);
        assert_eq!(rule("nooooooo", 0, 3, 0, none), Some("repeats"));
        // An emoji counts once, however many code points draw it.
        assert_eq!(rule(&"\u{2764}\u{FE0F}".repeat(8), 0, 0, 2, none), None);
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        assert_eq!(rule(&family.repeat(3), 0, 0, 2, none), None);
        assert_eq!(rule(&"\u{1F602}".repeat(16), 0, 0, 2, none), Some("spam"));
        // A long message with some symbols in it isn't spam.
        let long = format!(
            "{} ** // <3 ~~ ^^ && %% $$",
            "a normal sentence about the run".repeat(5)
        );
        assert_eq!(rule(&long, 0, 0, 2, none), None);
        assert_eq!(rule("!!!", 0, 0, 0, none), None, "off is off");
    }
    #[test]
    fn bursts_read_as_one() {
        assert_eq!(names(&["Ana"]), "Ana");
        assert_eq!(names(&["Ana", "Bo"]), "Ana and Bo");
        assert_eq!(names(&["Ana", "Bo", "Cy"]), "Ana, Bo and Cy");
        assert_eq!(names(&["Ana", "Bo", "Cy", "Di"]), "Ana, Bo, Cy and 1 other");
        assert_eq!(
            names(&["Ana", "Bo", "Cy", "Di", "Ed"]),
            "Ana, Bo, Cy and 2 others"
        );
        assert_eq!(
            fill("Hi {user}, month {count}", "Ana and Bo", 3),
            "Hi Ana and Bo, month 3"
        );
        // Every line has its three, and burst lines never put a verb right after {user}.
        for b in ["pyre", "echo", "favor", "volk"] {
            for p in ["chill", "battle", "event"] {
                for e in [Event::Follow, Event::Sub] {
                    for l in lines(b, p, e) {
                        assert!(l.contains("{user}"), "{l}");
                        let after = l.split("{user}").nth(1).unwrap_or("");
                        let next = after.split_whitespace().next().unwrap_or("");
                        // Past tense ("subscribed") agrees with any number of names.
                        let word = next.trim_end_matches(|c: char| !c.is_alphabetic());
                        assert!(
                            !next.starts_with(char::is_alphabetic) || word.ends_with("ed"),
                            "{b} {p}: {l}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn imports_translate_and_flag() {
        assert_eq!(
            parse_import("!discord Join us: https://discord.gg/x $(user)"),
            Some((
                "discord".into(),
                "Join us: https://discord.gg/x {user}".into(),
                vec![]
            ))
        );
        let (name, reply, flagged) =
            parse_import("!Hug: $(user) hugs ${random.chatter} $(count)").unwrap();
        assert_eq!(
            (name.as_str(), reply.as_str()),
            ("hug", "{user} hugs ${random.chatter} $(count)")
        );
        assert_eq!(flagged, vec!["$(count)", "${random.chatter}"]);
        assert_eq!(parse_import("not a command"), None);
    }
}
