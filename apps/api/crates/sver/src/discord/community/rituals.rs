//! Rituals on the home server (docs/DISCORD_BOT.md "Community"): birthdays in each member's own
//! time zone (the birthday role, 2× XP for their day and a post), server anniversaries (100 XP a
//! year and a post), seasonal events announced once at the start and once at the end, and
//! spotlights and hero nominations (`/rituals`).
use super::{field, form, name_of, text, updating};
use crate::discord::{
    call,
    commands::{ANSWERED, Interaction, embed, private},
    factions, home,
    levels::{self, Member, Source},
    quests, role, send,
    streamers::{self, answer, later},
};
use crate::{App, profiles::Res};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use reqwest::Method;
use serde_json::{Map, Value, json};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
fn days_in(month: u32, year: i32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next.and_then(|d| d.pred_opt()).map_or(31, |d| d.day())
}
/// The date a month/day falls on in `year`; Feb 29 is Feb 28 outside leap years.
fn on(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, day.min(days_in(month, year)))
}
/// Days from `today` to the next `month`/`day` (0 when it's today).
fn days_until(today: NaiveDate, month: u32, day: u32) -> i64 {
    [today.year(), today.year() + 1]
        .into_iter()
        .filter_map(|y| on(y, month, day))
        .find(|d| *d >= today)
        .map_or(366, |d| (d - today).num_days())
}
fn server_zone(settings: &Map<String, Value>) -> Tz {
    settings
        .get("zone")
        .and_then(Value::as_str)
        .and_then(streamers::zone)
        .unwrap_or(Tz::UTC)
}
/// A command's group and subcommand (`/rituals spotlight nominate`) and the options under them.
fn grouped(i: &Interaction) -> (&str, &str, &Value) {
    let first = &i.data["options"][0];
    let name = first["name"].as_str().unwrap_or_default();
    match first["type"].as_u64() {
        Some(2) => (
            name,
            first["options"][0]["name"].as_str().unwrap_or_default(),
            &first["options"][0]["options"],
        ),
        _ => ("", name, &first["options"]),
    }
}
fn option<'a>(options: &'a Value, name: &str) -> Option<&'a Value> {
    options
        .as_array()?
        .iter()
        .find(|o| o["name"] == name)
        .map(|o| &o["value"])
}

// ---- Birthdays ----

/// The birthday form, from onboarding or the role-select panel.
fn birthday_form(from: &str) -> Value {
    form(
        &format!("bday:save:{from}"),
        "Your birthday",
        &[
            ("month", "Month (1-12)", false, true, 2, ""),
            ("day", "Day (1-31)", false, true, 2, ""),
            ("year", "Year you were born (optional)", false, false, 4, ""),
            (
                "zone",
                "Time zone, like America/New_York (optional)",
                false,
                false,
                40,
                "",
            ),
        ],
    )
}
/// Saves a birthday: the answer, or why it can't be saved. Changing a birthday never celebrates it
/// a second time in one year (the legacy bot reset that, so a new date meant 2× XP again).
async fn save_birthday(
    app: &App,
    user: &str,
    month: Option<i64>,
    day: Option<i64>,
    year: Option<i64>,
    zone: Option<String>,
) -> Res<Result<String, String>> {
    let now = Utc::now();
    let (Some(month), Some(day)) = (month.filter(|m| (1..=12).contains(m)), day) else {
        return Ok(Err("The month is a number from 1 to 12.".into()));
    };
    // Any Feb 29 counts: it's celebrated on Feb 28 when the year has none.
    let longest = days_in(month as u32, 2024) as i64;
    if !(1..=longest).contains(&day) {
        return Ok(Err(format!(
            "{} has {longest} days.",
            MONTHS[month as usize - 1]
        )));
    }
    if year.is_some_and(|y| !(1900..=i64::from(now.year())).contains(&y)) {
        return Ok(Err(format!("The year is between 1900 and {}.", now.year())));
    }
    let zone = match zone {
        Some(z) => match streamers::zone(&z) {
            Some(z) => z,
            None => {
                return Ok(Err(
                    "That time zone isn't known. Use a name like America/New_York or Europe/London."
                        .into(),
                ));
            }
        },
        None => server_zone(&home::settings(app).await),
    };
    sqlx::query(
        "INSERT INTO discord_birthdays(discord_user,month,day,year,zone) VALUES($1,$2,$3,$4,$5)
        ON CONFLICT(discord_user) DO UPDATE SET month=$2,day=$3,year=$4,zone=$5",
    )
    .bind(user)
    .bind(month as i16)
    .bind(day as i16)
    .bind(year.map(|y| y as i16))
    .bind(zone.name())
    .execute(&app.db)
    .await?;
    quests::unlock(app, user, "birthday_set").await?;
    Ok(Ok(format!(
        "🎂 Your birthday is set to **{} {day}** ({}).",
        MONTHS[month as usize - 1],
        zone.name()
    )))
}
fn local_today(zone: &str) -> NaiveDate {
    Utc::now()
        .with_timezone(&streamers::zone(zone).unwrap_or(Tz::UTC))
        .date_naive()
}
async fn birthday(app: &App, i: &Interaction) -> Res<Value> {
    match i.sub() {
        Some("set") => {
            let number = |k: &str| i.option(k).and_then(Value::as_i64);
            let saved = save_birthday(
                app,
                &i.user,
                number("month"),
                number("day"),
                number("year"),
                text(i, "zone", 40),
            )
            .await?;
            Ok(private(match saved {
                Ok(s) | Err(s) => s,
            }))
        }
        Some("remove") => {
            let gone = sqlx::query("DELETE FROM discord_birthdays WHERE discord_user=$1")
                .bind(&i.user)
                .execute(&app.db)
                .await?
                .rows_affected();
            Ok(private(if gone > 0 {
                "🎂 Your birthday is removed."
            } else {
                "You haven't set a birthday."
            }))
        }
        Some("upcoming") => {
            let all: Vec<(String, i16, i16, String)> =
                sqlx::query_as("SELECT discord_user,month,day,zone FROM discord_birthdays")
                    .fetch_all(&app.db)
                    .await?;
            // ponytail: every birthday read for the next week; fine for a community server.
            let mut soon: Vec<(i64, String)> = all
                .into_iter()
                .filter_map(|(u, m, d, z)| {
                    let n = days_until(local_today(&z), m as u32, d as u32);
                    let when = match n {
                        0 => "today!".to_string(),
                        1 => "tomorrow".into(),
                        n => format!("in {n} days"),
                    };
                    (n <= 7).then(|| {
                        (
                            n,
                            format!("**{} {d}**: <@{u}> ({when})", MONTHS[m as usize - 1]),
                        )
                    })
                })
                .collect();
            soon.sort();
            if soon.is_empty() {
                return Ok(private("No birthdays in the next 7 days."));
            }
            let lines: Vec<String> = soon.into_iter().map(|s| s.1).collect();
            Ok(embed(
                json!({"title": "🎂 Upcoming birthdays", "description": lines.join("\n"), "color": 0xff_69b4}),
                false,
            ))
        }
        _ => {
            let user = i.user_or_me("user");
            let (name, _) = i.profile(&user);
            let row: Option<(i16, i16, Option<i16>, String)> = sqlx::query_as(
                "SELECT month,day,year,zone FROM discord_birthdays WHERE discord_user=$1",
            )
            .bind(&user)
            .fetch_optional(&app.db)
            .await?;
            let Some((m, d, year, zone)) = row else {
                return Ok(private(if user == i.user {
                    "You haven't set your birthday yet. `/birthday set` adds it.".to_string()
                } else {
                    format!("{name} hasn't set a birthday.")
                }));
            };
            let today = local_today(&zone);
            let ahead = days_until(today, m as u32, d as u32);
            let next = match ahead {
                0 => "🎉 **Today!**".to_string(),
                n => format!("in **{n}** day{}", if n == 1 { "" } else { "s" }),
            };
            let mut fields = vec![json!({"name": "Next birthday", "value": next, "inline": true})];
            if let Some(y) = year {
                // Not yet this year: still the age from last year's birthday.
                let passed = on(today.year(), m as u32, d as u32).is_some_and(|b| b <= today);
                let age = today.year() - i32::from(y) - i32::from(!passed);
                fields.push(json!({"name": "Age", "value": age.to_string(), "inline": true}));
            }
            Ok(embed(
                json!({"title": format!("🎂 {name}'s birthday"), "description": format!("**{} {d}**", MONTHS[m as usize - 1]),
                "color": 0xff_69b4, "fields": fields, "footer": {"text": format!("Time zone: {zone}")}}),
                true,
            ))
        }
    }
}
async fn birthday_saved(app: App, i: Interaction) -> Res<Value> {
    let number = |k: &str| field(&i, k).and_then(|v| v.parse::<i64>().ok());
    let saved = save_birthday(
        &app,
        &i.user,
        number("month"),
        number("day"),
        number("year"),
        field(&i, "zone"),
    )
    .await?;
    let note = match saved {
        Ok(s) | Err(s) => s,
    };
    // From onboarding: the last step is done either way.
    if i.custom_id() == "bday:save:onb" {
        let mut done = super::complete(&app, &i.user, &name_of(&i)).await?;
        let after = done["data"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        done["data"]["content"] = json!(format!("{note}\n{after}"));
        return Ok(done);
    }
    Ok(private(note))
}

type Today = (String, i16, Option<i16>, DateTime<Utc>);
/// Each minute: birthdays that began in their owner's zone are celebrated (once a year), and
/// birthday roles come off once that day is over there.
async fn birthdays(app: &App) -> Res<()> {
    let due: Vec<Today> = sqlx::query_as("WITH b AS (SELECT discord_user,(now() AT TIME ZONE zone) AS local FROM discord_birthdays)
        UPDATE discord_birthdays d SET celebrated_year=extract(year FROM b.local),role_until=((b.local::date+1)::timestamp AT TIME ZONE d.zone)
        FROM b WHERE b.discord_user=d.discord_user AND coalesce(d.celebrated_year,0)<extract(year FROM b.local)
            AND extract(month FROM b.local)=d.month
            AND extract(day FROM b.local)=least(d.day,extract(day FROM make_date(extract(year FROM b.local)::int,d.month,1)+interval '1 month'-interval '1 day'))
        RETURNING d.discord_user,d.celebrated_year,d.year,d.role_until")
        .fetch_all(&app.db).await?;
    let guild = home::guild(app).await;
    let settings = home::settings(app).await;
    for (user, this_year, born, until) in due {
        if let (Some(g), Some(r)) = (&guild, home::id(&settings, "birthday_role")) {
            role(app, g, &user, &r, true).await;
        }
        sqlx::query("INSERT INTO discord_multipliers(name,factor,kind,target,ends_at) VALUES('🎂 Birthday',2,'user',$1,$2)")
            .bind(&user).bind(until).execute(&app.db).await?;
        let Some(channel) = home::id(&settings, "birthdays") else {
            continue;
        };
        let faction = levels::faction(app, &user).await?;
        let mut fields =
            vec![json!({"name": "🎁 Birthday bonus", "value": "2× XP all day!", "inline": true})];
        if let Some(f) = &faction {
            fields.push(
                json!({"name": "Faction", "value": streamers::faction_line(f), "inline": true}),
            );
        }
        let line = match born {
            Some(y) => format!("They're turning **{}** today!", this_year - y),
            None => "Have an amazing day!".into(),
        };
        send(app, &channel, json!({"content": format!("🎂 Happy birthday <@{user}>!"), "allowed_mentions": {"users": [user]},
            "embeds": [{"title": "🎂🎉 Happy birthday! 🎉🎂", "description": format!("Happy birthday <@{user}>! 🥳\n\n{line}"), "color": 0xff_69b4, "fields": fields}]})).await;
    }
    let over: Vec<String> = sqlx::query_scalar("UPDATE discord_birthdays SET role_until=NULL WHERE role_until<=now() RETURNING discord_user")
        .fetch_all(&app.db).await?;
    if let (Some(g), Some(r)) = (&guild, home::id(&settings, "birthday_role")) {
        for user in over {
            role(app, g, &user, &r, false).await;
        }
    }
    Ok(())
}

// ---- Server anniversaries ----

fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}
fn roles_of(member: &Value) -> Vec<String> {
    member["roles"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|r| r.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
/// Once a UTC day: every member whose join date comes round gets 100 XP per year and a post (the
/// legacy bot never ran this). The day counts as done only once every member page was read.
pub async fn anniversaries(app: &App, now: DateTime<Utc>) -> Res<()> {
    let today = now.date_naive();
    let done: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM discord_weekly_runs WHERE job='anniversaries' AND period=$1)",
    )
    .bind(today)
    .fetch_one(&app.db)
    .await?;
    let Some(guild) = home::guild(app).await.filter(|_| !done) else {
        return Ok(());
    };
    let settings = home::settings(app).await;
    let to = home::id(&settings, "birthdays").or_else(|| home::id(&settings, "announcements"));
    let mut after = "0".to_string();
    loop {
        let path = format!("/guilds/{guild}/members?limit=1000&after={after}");
        let Some((200, page)) = call(app, Method::GET, &path, None).await else {
            return Ok(());
        };
        let members = page.as_array().cloned().unwrap_or_default();
        for m in &members {
            let (Some(user), Some(joined)) = (
                m["user"]["id"].as_str(),
                m["joined_at"]
                    .as_str()
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok()),
            ) else {
                continue;
            };
            let joined = joined.to_utc().date_naive();
            let years = i64::from(today.year() - joined.year());
            if m["user"]["bot"] == true
                || years < 1
                || on(today.year(), joined.month(), joined.day()) != Some(today)
            {
                continue;
            }
            let new = sqlx::query("INSERT INTO discord_anniversaries(discord_user,years) VALUES($1,$2) ON CONFLICT DO NOTHING")
                .bind(user).bind(years as i16).execute(&app.db).await?.rows_affected() == 1;
            if !new {
                continue;
            }
            let roles = roles_of(m);
            let member = Member {
                user,
                roles: &roles,
                channel: None,
            };
            let given = levels::award(app, member, 100 * years, Source::Anniversary).await?;
            if let Some(channel) = &to {
                send(app, channel, json!({"content": format!("🎊 <@{user}>"), "allowed_mentions": {"users": [user]},
                    "embeds": [{"title": "🎉 Server anniversary!", "color": 0x58_65f2,
                        "description": format!("<@{user}> is celebrating their **{} server anniversary!**\n\nThank you for {years} year{} in the community! 🙌", ordinal(years), if years == 1 { "" } else { "s" }),
                        "fields": [{"name": "XP bonus", "value": format!("+{given} XP"), "inline": true}]}]})).await;
            }
        }
        match members.last().and_then(|m| m["user"]["id"].as_str()) {
            Some(last) if members.len() == 1000 => after = last.to_string(),
            _ => break,
        }
    }
    sqlx::query("INSERT INTO discord_weekly_runs(job,period) VALUES('anniversaries',$1) ON CONFLICT DO NOTHING")
        .bind(today).execute(&app.db).await?;
    Ok(())
}
async fn anniversary(app: App, i: Interaction) -> Res<Value> {
    let (_, _, options) = grouped(&i);
    let user = option(options, "user")
        .and_then(Value::as_str)
        .unwrap_or(&i.user)
        .to_string();
    let guild = home::guild(&app).await.unwrap_or_default();
    let Some((200, m)) = call(
        &app,
        Method::GET,
        &format!("/guilds/{guild}/members/{user}"),
        None,
    )
    .await
    else {
        return Ok(private("They aren't in the server."));
    };
    let Some(joined) = m["joined_at"]
        .as_str()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.to_utc())
    else {
        return Ok(private("Discord didn't say when they joined."));
    };
    let today = Utc::now().date_naive();
    let date = joined.date_naive();
    let ahead = days_until(today, date.month(), date.day());
    let next = (today + Duration::days(ahead))
        .and_hms_opt(0, 0, 0)
        .map_or(0, |t| t.and_utc().timestamp());
    let passed = on(today.year(), date.month(), date.day()).is_some_and(|d| d <= today);
    let years = (today.year() - date.year() - i32::from(!passed)).max(0);
    Ok(embed(
        json!({"title": "🎉 Server anniversary", "description": format!("<@{user}>"), "color": 0x58_65f2, "fields": [
            {"name": "Joined", "value": format!("<t:{}:D>", joined.timestamp()), "inline": true},
            {"name": "Years in the server", "value": years.to_string(), "inline": true},
            {"name": "Next anniversary", "value": format!("<t:{next}:R>"), "inline": true}]}),
        false,
    ))
}

// ---- Seasonal events ----

fn theme(name: &str) -> (&'static str, u32) {
    match name {
        "winter" => ("❄️", 0x7e_cff5),
        "spring" => ("🌸", 0x7e_d17e),
        "summer" => ("☀️", 0xf5_b642),
        "fall" => ("🍂", 0xe0_7b39),
        _ => ("🌟", 0x58_65f2),
    }
}
/// "2026-12-01 to 2026-12-31" in `zone`: from the first day's midnight to the end of the last.
fn window(text: &str, zone: Tz) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let (a, b) = text.split_once(" to ")?;
    let parse = |d: &str| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok();
    let (start, end) = (parse(a)?, parse(b)?.succ_opt()?);
    let at = |d: NaiveDate| {
        d.and_hms_opt(0, 0, 0)?
            .and_local_timezone(zone)
            .earliest()
            .map(|t| t.to_utc())
    };
    let (start, end) = (at(start)?, at(end)?);
    (start < end).then_some((start, end))
}
async fn season_created(app: App, i: Interaction) -> Res<Value> {
    if !i.staff() {
        return Ok(private("Only staff can do that."));
    }
    let (Some(name), Some(dates)) = (field(&i, "name"), field(&i, "dates")) else {
        return Ok(private("A seasonal event needs a name and dates."));
    };
    let zone = server_zone(&home::settings(&app).await);
    let Some((starts, ends)) = window(&dates, zone) else {
        return Ok(private(
            "Dates look like 2026-12-01 to 2026-12-31, the end on or after the start.",
        ));
    };
    let theme_name = field(&i, "theme")
        .map(|t| t.to_lowercase())
        .filter(|t| ["winter", "spring", "summer", "fall"].contains(&t.as_str()))
        .unwrap_or_else(|| "custom".into());
    let factor = match field(&i, "factor") {
        None => None,
        Some(f) => match f.trim_end_matches(['x', '×']).parse::<f64>() {
            Ok(f) if (0.1..=10.0).contains(&f) => Some(f),
            _ => {
                return Ok(private(
                    "The XP multiplier is a number from 0.1 to 10, like 2 or 1.5.",
                ));
            }
        },
    };
    let name: String = name.chars().take(80).collect();
    let description: String = field(&i, "description")
        .unwrap_or_default()
        .chars()
        .take(500)
        .collect();
    let id: i64 = sqlx::query_scalar("INSERT INTO discord_seasonal_events(name,description,theme,starts_at,ends_at,factor,created_by) VALUES($1,$2,$3,$4,$5,$6::float8::numeric,$7) RETURNING id")
        .bind(&name).bind(description).bind(&theme_name).bind(starts).bind(ends).bind(factor).bind(&i.user)
        .fetch_one(&app.db).await?;
    Ok(private(format!(
        "{} **{name}** (#{id}) runs from <t:{}:f> to <t:{}:f>. It's announced when it starts and when it ends.",
        theme(&theme_name).0,
        starts.timestamp(),
        ends.timestamp()
    )))
}
async fn season_stats(app: &App, id: i64) -> Res<Option<(String, Vec<Value>)>> {
    let event: Option<(String, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT name,starts_at,least(ends_at,coalesce(ended_at,ends_at)) FROM discord_seasonal_events WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&app.db)
    .await?;
    let Some((name, from, to)) = event else {
        return Ok(None);
    };
    let (done, people): (i64, i64) = sqlx::query_as("SELECT count(*),count(DISTINCT discord_user) FROM discord_quest_progress WHERE completed_at>=$1 AND completed_at<$2")
        .bind(from).bind(to).fetch_one(&app.db).await?;
    let top: Vec<(String, i64)> = sqlx::query_as("SELECT discord_user,sum(points)::int8 FROM discord_points WHERE at>=$1 AND at<$2 GROUP BY 1 ORDER BY 2 DESC LIMIT 5")
        .bind(from).bind(to).fetch_all(&app.db).await?;
    let mut fields = vec![
        json!({"name": "Participants", "value": people.to_string(), "inline": true}),
        json!({"name": "Quests completed", "value": done.to_string(), "inline": true}),
    ];
    if !top.is_empty() {
        let lines: Vec<String> = top
            .iter()
            .enumerate()
            .map(|(n, (u, p))| format!("{}. <@{u}>: {p} points", n + 1))
            .collect();
        fields.push(json!({"name": "Top contributors", "value": lines.join("\n")}));
    }
    Ok(Some((name, fields)))
}
type Season = (i64, String, String, String, DateTime<Utc>, Option<f64>);
/// Each minute: seasonal events that began are announced (and their multiplier starts), and ones
/// that ended are announced with their stats. Each is claimed first, so it's posted once (the
/// legacy bot re-announced the start every hour).
async fn seasons(app: &App) -> Res<()> {
    let settings = home::settings(app).await;
    let to = home::id(&settings, "announcements").or_else(|| home::id(&settings, "events"));
    let started: Vec<Season> = sqlx::query_as("UPDATE discord_seasonal_events SET started_at=now() WHERE started_at IS NULL AND ended_at IS NULL AND starts_at<=now() AND ends_at>now()
        RETURNING id,name,description,theme,ends_at,factor::float8")
        .fetch_all(&app.db).await?;
    for (id, name, description, theme_name, ends, factor) in started {
        let mut fields = vec![
            json!({"name": "Ends", "value": format!("<t:{}:R>", ends.timestamp()), "inline": true}),
        ];
        if let Some(f) = factor {
            let multiplier: i64 = sqlx::query_scalar("INSERT INTO discord_multipliers(name,factor,kind,ends_at) VALUES($1,$2::float8::numeric,'event',$3) RETURNING id")
                .bind(format!("{name} (seasonal)")).bind(f).bind(ends).fetch_one(&app.db).await?;
            sqlx::query("UPDATE discord_seasonal_events SET multiplier_id=$2 WHERE id=$1")
                .bind(id)
                .bind(multiplier)
                .execute(&app.db)
                .await?;
            fields.push(
                json!({"name": "Bonus", "value": format!("{f}× XP for everyone"), "inline": true}),
            );
        }
        let (emoji, color) = theme(&theme_name);
        let description = if description.is_empty() {
            "A seasonal event is on!".to_string()
        } else {
            description
        };
        if let Some(to) = &to {
            send(app, to, json!({"embeds": [{"title": format!("{emoji} {name} has begun!"), "description": description, "color": color, "fields": fields}],
                "allowed_mentions": {"parse": []}})).await;
        }
    }
    let ended: Vec<(i64, String, bool)> = sqlx::query_as("UPDATE discord_seasonal_events SET ended_at=now() WHERE ended_at IS NULL AND ends_at<=now() RETURNING id,theme,started_at IS NOT NULL")
        .fetch_all(&app.db).await?;
    for (id, theme_name, was_on) in ended {
        let (Some(to), true) = (&to, was_on) else {
            continue;
        };
        if let Some((name, fields)) = season_stats(app, id).await? {
            send(app, to, json!({"embeds": [{"title": format!("{name} has ended!"), "description": "Thanks to everyone who took part!",
                "color": theme(&theme_name).1, "fields": fields}], "allowed_mentions": {"parse": []}})).await;
        }
    }
    Ok(())
}
async fn season_command(app: &App, sub: &str, options: &Value) -> Res<Value> {
    let id = option(options, "id").and_then(Value::as_i64).unwrap_or(0);
    match sub {
        "create" => Ok(form(
            "ritual:season",
            "New seasonal event",
            &[
                ("name", "Name", false, true, 80, ""),
                ("description", "Description", true, false, 500, ""),
                (
                    "dates",
                    "Dates (2026-12-01 to 2026-12-31)",
                    false,
                    true,
                    40,
                    "",
                ),
                (
                    "theme",
                    "Theme: winter, spring, summer, fall or custom",
                    false,
                    false,
                    10,
                    "",
                ),
                (
                    "factor",
                    "XP multiplier for everyone (optional, like 2)",
                    false,
                    false,
                    5,
                    "",
                ),
            ],
        )),
        "end" => {
            // Not started yet: it's simply off, with nothing to announce.
            let ended = sqlx::query("UPDATE discord_seasonal_events SET ends_at=CASE WHEN started_at IS NULL THEN ends_at ELSE now() END,ended_at=CASE WHEN started_at IS NULL THEN now() END WHERE id=$1 AND ended_at IS NULL")
                .bind(id).execute(&app.db).await?.rows_affected();
            if ended == 0 {
                return Ok(private(
                    "No running or upcoming seasonal event has that number.",
                ));
            }
            sqlx::query("UPDATE discord_multipliers m SET ends_at=now() FROM discord_seasonal_events e WHERE e.id=$1 AND m.id=e.multiplier_id AND (m.ends_at IS NULL OR m.ends_at>now())")
                .bind(id).execute(&app.db).await?;
            seasons(app).await?;
            Ok(private(format!("Seasonal event #{id} is over.")))
        }
        "stats" => Ok(match season_stats(app, id).await? {
            Some((name, fields)) => embed(
                json!({"title": format!("📊 {name}"), "color": 0x58_65f2, "fields": fields}),
                false,
            ),
            None => private("No seasonal event has that number."),
        }),
        _ => {
            type Row = (i64, String, String, DateTime<Utc>, DateTime<Utc>, bool);
            let rows: Vec<Row> = sqlx::query_as("SELECT id,name,theme,starts_at,ends_at,started_at IS NOT NULL FROM discord_seasonal_events WHERE ended_at IS NULL ORDER BY starts_at LIMIT 10")
                .fetch_all(&app.db).await?;
            if rows.is_empty() {
                return Ok(private("No seasonal events are running or coming up."));
            }
            let lines: Vec<String> = rows
                .iter()
                .map(|(id, name, t, from, to, on)| {
                    let (when, at) = if *on { ("ends", to) } else { ("starts", from) };
                    format!(
                        "{} **{name}** (#{id}) · {when} <t:{}:R>",
                        theme(t).0,
                        at.timestamp()
                    )
                })
                .collect();
            Ok(embed(
                json!({"title": "🌟 Seasonal events", "description": lines.join("\n"), "color": 0x58_65f2}),
                false,
            ))
        }
    }
}

// ---- Spotlights ----

async fn nominate(app: &App, i: &Interaction, options: &Value) -> Res<Value> {
    let Some(target) = option(options, "user").and_then(Value::as_str) else {
        return Ok(private("Pick who to nominate."));
    };
    let reason: String = option(options, "reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(500)
        .collect();
    if target == i.user {
        return Ok(private("Nominate someone else."));
    }
    if i.data["resolved"]["users"][target]["bot"] == true {
        return Ok(private("Bots can't be community heroes."));
    }
    if reason.is_empty() {
        return Ok(private("Say why they deserve it."));
    }
    // One nomination a day each, so a spotlight stays special.
    let recent: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_spotlights WHERE kind='hero' AND nominated_by=$1 AND created_at>now()-interval '1 day')")
        .bind(&i.user).fetch_one(&app.db).await?;
    if recent {
        return Ok(private(
            "You've nominated someone in the last day. Try again tomorrow.",
        ));
    }
    let faction = levels::faction(app, target).await?;
    let settings = home::settings(app).await;
    let to = home::id(&settings, "go_live").or_else(|| home::id(&settings, "announcements"));
    let id: i64 = sqlx::query_scalar("INSERT INTO discord_spotlights(kind,discord_user,title,reason,nominated_by,faction,channel_id) VALUES('hero',$1,'🦸 Community Hero',$2,$3,$4,$5) RETURNING id")
        .bind(target).bind(&reason).bind(&i.user).bind(&faction).bind(&to).fetch_one(&app.db).await?;
    quests::unlock(app, &i.user, "hero_nominated").await?;
    let card = json!({"title": "🦸 Community Hero", "color": levels::color(faction.as_deref()),
        "description": format!("<@{target}> has been nominated as a community hero!\n\n**Why:** {reason}"),
        "footer": {"text": format!("Nominated by {}", name_of(i))}});
    if let Some(to) = &to
        && let Some(m) = send(
            app,
            to,
            json!({"embeds": [card.clone()], "allowed_mentions": {"parse": []}}),
        )
        .await
    {
        sqlx::query("UPDATE discord_spotlights SET message_id=$2 WHERE id=$1")
            .bind(id)
            .bind(m)
            .execute(&app.db)
            .await?;
    }
    Ok(embed(card, false))
}
async fn history(app: &App, options: &Value) -> Res<Value> {
    let user = option(options, "user").and_then(Value::as_str);
    type Row = (
        String,
        String,
        Option<String>,
        Option<String>,
        DateTime<Utc>,
    );
    let rows: Vec<Row> = sqlx::query_as("SELECT kind,discord_user,title,reason,created_at FROM discord_spotlights WHERE $1::text IS NULL OR discord_user=$1 ORDER BY created_at DESC LIMIT 10")
        .bind(user).fetch_all(&app.db).await?;
    let icon = |k: &str| match k {
        "streamer" => "🎬",
        "hero" => "🦸",
        _ => "⭐",
    };
    let lines: Vec<String> = rows
        .iter()
        .map(|(k, u, t, r, at)| {
            let mut line = format!(
                "{} **{}**: <@{u}> · <t:{}:D>",
                icon(k),
                t.as_deref().unwrap_or(k),
                at.timestamp()
            );
            if let (Some(r), true) = (r, user.is_some()) {
                line.push_str(&format!("\n{r}"));
            }
            line
        })
        .collect();
    let description = if lines.is_empty() {
        "No spotlights yet.".to_string()
    } else {
        lines.join("\n")
    };
    Ok(embed(
        json!({"title": "🌟 Spotlights", "description": description, "color": 0x58_65f2}),
        false,
    ))
}
async fn streamer_now(app: App, _i: Interaction) -> Res<Value> {
    Ok(private(match streamers::streamer_spotlight(&app).await? {
        Some(user) => format!("Streamer of the Week posted for <@{user}>."),
        None => "No spotlight: nobody streamed this week, one went up in the last 6 days, or no go-live or announcements channel is set.".into(),
    }))
}

async fn rituals(app: &App, i: &Interaction) -> Res<Value> {
    let (group, sub, options) = grouped(i);
    let staff_only = matches!(
        (group, sub),
        ("event", "create" | "end") | ("spotlight", "streamer")
    );
    if staff_only && !i.staff() {
        return Ok(private("Only staff can do that."));
    }
    match (group, sub) {
        ("spotlight", "nominate") => nominate(app, i, options).await,
        ("spotlight", "history") => history(app, options).await,
        ("spotlight", _) => {
            later(app, i, true, streamer_now);
            Ok(ANSWERED)
        }
        ("event", sub) => season_command(app, sub, options).await,
        _ => {
            later(app, i, false, anniversary);
            Ok(ANSWERED)
        }
    }
}

// ---- Commands and forms ----

pub fn definitions() -> Vec<Value> {
    let user = |d: &str, required: bool| json!({"type": 6, "name": "user", "description": d, "required": required});
    let id = json!({"type": 4, "name": "id", "description": "Its number from the list", "required": true, "min_value": 1});
    vec![
        json!({"name": "birthday", "description": "Birthdays: a shout-out and 2× XP on your day.", "options": [
            {"type": 1, "name": "set", "description": "Set your birthday", "options": [
                {"type": 4, "name": "month", "description": "Month (1-12)", "required": true, "min_value": 1, "max_value": 12},
                {"type": 4, "name": "day", "description": "Day (1-31)", "required": true, "min_value": 1, "max_value": 31},
                {"type": 4, "name": "year", "description": "The year you were born (optional)", "min_value": 1900, "max_value": 2100},
                {"type": 3, "name": "zone", "description": "Your time zone, like America/New_York (the server's by default)", "max_length": 40}]},
            {"type": 1, "name": "view", "description": "A member's birthday", "options": [user("Whose (you by default)", false)]},
            {"type": 1, "name": "upcoming", "description": "Birthdays in the next 7 days"},
            {"type": 1, "name": "remove", "description": "Remove your birthday"}]}),
        // The staff-only parts are checked when they're used; the rest is for everyone.
        json!({"name": "rituals", "description": "Spotlights, anniversaries and seasonal events.", "options": [
            {"type": 2, "name": "spotlight", "description": "Community spotlights", "options": [
                {"type": 1, "name": "nominate", "description": "Nominate a community hero", "options": [
                    user("Who", true),
                    {"type": 3, "name": "reason", "description": "Why they deserve it", "required": true, "max_length": 500}]},
                {"type": 1, "name": "history", "description": "Past spotlights", "options": [user("Only this member's", false)]},
                {"type": 1, "name": "streamer", "description": "Post the Streamer of the Week now (staff)"}]},
            {"type": 1, "name": "anniversary", "description": "A member's server anniversary", "options": [user("Whose (you by default)", false)]},
            {"type": 2, "name": "event", "description": "Seasonal events", "options": [
                {"type": 1, "name": "create", "description": "Create a seasonal event (staff)"},
                {"type": 1, "name": "list", "description": "Running and upcoming seasonal events"},
                {"type": 1, "name": "end", "description": "End a seasonal event now (staff)", "options": [id.clone()]},
                {"type": 1, "name": "stats", "description": "A seasonal event's stats", "options": [id]}]}]}),
    ]
}

pub async fn command(app: &App, i: &Interaction) -> Option<Value> {
    Some(match i.name() {
        "birthday" => answer(birthday(app, i).await),
        "rituals" => answer(rituals(app, i).await),
        _ => return None,
    })
}

/// The birthday form (from onboarding or the panel) and the seasonal-event form.
pub async fn component(app: &App, i: &Interaction) -> Option<Value> {
    let id = i.custom_id();
    Some(match id {
        "bday:open:onb" | "bday:open:panel" => {
            birthday_form(id.rsplit(':').next().unwrap_or_default())
        }
        "bday:save:onb" => {
            updating(app, i, birthday_saved);
            ANSWERED
        }
        "bday:save:panel" => answer(birthday_saved(app.clone(), Interaction::parse(&i.raw)?).await),
        "ritual:season" => answer(season_created(app.clone(), Interaction::parse(&i.raw)?).await),
        _ => return None,
    })
}

/// Each minute: birthdays and seasonal events; every 15 minutes, the day's anniversaries.
pub async fn minute(app: &App) -> Res<()> {
    birthdays(app).await?;
    if factions::due("anniversaries", 15) {
        anniversaries(app, Utc::now()).await?;
    }
    seasons(app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_and_windows() {
        let d = |y, m, n| NaiveDate::from_ymd_opt(y, m, n).unwrap();
        assert_eq!(days_in(2, 2025), 28);
        assert_eq!(days_in(2, 2024), 29);
        assert_eq!(days_in(12, 2025), 31);
        // Feb 29 falls on Feb 28 outside leap years.
        assert_eq!(on(2025, 2, 29), Some(d(2025, 2, 28)));
        assert_eq!(days_until(d(2025, 2, 28), 2, 29), 0);
        assert_eq!(days_until(d(2026, 10, 20), 10, 19), 364);
        assert_eq!(days_until(d(2026, 10, 20), 10, 21), 1);
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(12), "12th");
        assert_eq!(ordinal(22), "22nd");
        let ny: Tz = "America/New_York".parse().unwrap();
        let (a, b) = window("2026-12-01 to 2026-12-31", ny).unwrap();
        assert_eq!(a.to_rfc3339(), "2026-12-01T05:00:00+00:00");
        assert_eq!(b.to_rfc3339(), "2027-01-01T05:00:00+00:00");
        assert!(window("2026-12-31 to 2026-12-01", ny).is_none());
    }
}
