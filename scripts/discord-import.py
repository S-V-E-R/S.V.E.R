"""One-time import of the legacy Discord bot's data (docs/DISCORD_BOT.md "Data carried over").

Reads the legacy bot's SQLite file (read-only) and prints SQL for the rebuild's database. Rehearse on
a restored copy first; apply in one transaction:

    python3 -I scripts/discord-import.py /path/to/sver-bot.db > discord-import.sql
    psql -v ON_ERROR_STOP=1 -1 -f discord-import.sql

Prints no personal data to the terminal; the SQL file holds Discord IDs, so keep it private and
delete it after the import.
"""
import json
import sqlite3
import sys

# Legacy fixed-offset zone names, as real IANA zones.
ZONES = {"EST": "America/New_York", "CST": "America/Chicago", "MST": "America/Denver",
         "PST": "America/Los_Angeles", "UTC": "UTC", "GMT": "UTC"}


def literal(value):
    """A SQL string literal (standard_conforming_strings is on in Postgres)."""
    return "'" + str(value).replace("'", "''") + "'"


def home(db):
    """guild_config (and the free-games channel) as the home server's settings."""
    db.row_factory = sqlite3.Row
    rows = db.execute("SELECT * FROM guild_config").fetchall()
    if len(rows) != 1:
        sys.exit(f"expected one guild in guild_config, found {len(rows)}")
    g = dict(rows[0])
    channels = {
        "mod_log": "mod_log_channel", "ban_trap": "ban_trap_channel_id", "welcome": "welcome_channel",
        "goodbye": "goodbye_channel", "announcements": "announcement_channel", "events": "events_channel",
        "giveaways": "giveaway_channel", "birthdays": "birthday_channel", "game_chat": "game_chat_channel",
        "stream_help": "stream_help_channel", "clips": "clips_channel", "lfg": "lfg_channel",
        "wow_leaderboard": "wow_leaderboard_channel_id", "go_live": "go_live_channel",
        "stream_promo": "stream_promo_channel", "rank_up": "rankup_channel",
        "achievements": "achievements_channel", "factions_channel": "faction_channel",
        "mod_team": "mod_team_role_id", "restricted": "restricted_role_id", "community": "auto_role_id",
        "birthday_role": "birthday_role_id", "mentor_role": "mentor_role_id", "now_live": "now_live_role_id",
        "myria": "myria_role_id", "aetheron": "aetheron_role_id", "glint": "glint_role_id",
    }
    toggles = {
        "automod": "automod_enabled", "welcome_post": "welcome_msg_enabled", "welcome_dm": "welcome_dm_enabled",
        "goodbye_post": "goodbye_enabled", "alerts_on": "stream_alerts_enabled", "leveling": "leveling_enabled",
        "factions_on": "faction_system_enabled",
    }
    settings = {key: str(g[col]) for key, col in channels.items() if g.get(col)}
    settings.update({key: bool(g[col]) for key, col in toggles.items() if g.get(col) is not None})
    for key, col in {"support": "support_channel_ids", "xp_off": "xp_disabled_channels"}.items():
        ids = json.loads(g.get(col) or "[]")
        if ids:
            settings[key] = [str(i) for i in ids]
    settings["zone"] = ZONES.get(g.get("default_timezone") or "EST", "America/New_York")
    free = db.execute("SELECT channel_id FROM free_games_config WHERE guild_id=?", (g["guild_id"],)).fetchone()
    if free and free[0]:
        settings["free_games"] = str(free[0])
    settings["free_games_on"] = True
    guild = literal(g["guild_id"])
    return [
        "INSERT INTO discord_home(singleton,guild_id,settings) VALUES(true,"
        f"{guild},{literal(json.dumps(settings))}::jsonb) "
        "ON CONFLICT(singleton) DO UPDATE SET guild_id=EXCLUDED.guild_id,settings=EXCLUDED.settings,updated_at=now();"
    ], f"home server settings: {len(settings)}"


def ts(value):
    """A legacy UTC timestamp ('YYYY-MM-DD HH:MM:SS') as a SQL timestamptz, or NULL."""
    return f"{literal(value)}::timestamp AT TIME ZONE 'UTC'" if value else "NULL"


def ts_or_now(value):
    """For columns that must have a time: the legacy value, or now when it's missing."""
    return ts(value) if value else "now()"


def rows(db, sql):
    db.row_factory = sqlite3.Row
    return [dict(r) for r in db.execute(sql).fetchall()]


FACTIONS = {"myria", "aetheron", "glint"}


def levels(db):
    """Members and factions, XP and stats, the rank ladder's roles, running multipliers."""
    out = []
    members = rows(db, "SELECT user_id, faction, joined_at FROM members")
    for m in members:
        faction = literal(m["faction"]) if m["faction"] in FACTIONS else "NULL"
        out.append("INSERT INTO discord_members(discord_user,faction,joined_at) VALUES("
                   f"{literal(m['user_id'])},{faction},{ts(m['joined_at'])}) ON CONFLICT(discord_user) DO UPDATE SET faction=EXCLUDED.faction,joined_at=EXCLUDED.joined_at;")
    ranks = {r["base_name"]: r for r in rows(db, "SELECT * FROM rank_thresholds")}
    xp = rows(db, "SELECT * FROM xp")
    for x in xp:
        rank = ranks.get(x["current_rank"] or "", {}).get("rank_order", 0) if x["total_xp"] else 0
        out.append(
            "INSERT INTO discord_xp(discord_user,total,rank,messages,reactions,voice_minutes,watch_minutes,stream_minutes,day,day_xp,message_at,reaction_at) VALUES("
            f"{literal(x['user_id'])},{int(x['total_xp'] or 0)},{int(rank)},{int(x['message_count'] or 0)},{int(x['reactions_given'] or 0)},"
            f"{int(x['voice_minutes'] or 0)},{int(x['watch_minutes'] or 0)},{int(x['stream_minutes'] or 0)},"
            f"{literal(x['daily_xp_date']) + '::date' if x['daily_xp_date'] else 'current_date'},{int(x['daily_xp'] or 0)},"
            f"{ts(x['last_message_xp_at'])},{ts(x['last_reaction_xp_at'])}) ON CONFLICT(discord_user) DO NOTHING;")
    for r in ranks.values():
        roles = {k: str(r[c]) for k, c in [("none", "no_faction_role_id"), ("myria", "myria_role_id"),
                                           ("aetheron", "aetheron_role_id"), ("glint", "glint_role_id")] if r[c]}
        out.append(f"UPDATE discord_ranks SET roles={literal(json.dumps(roles))}::jsonb WHERE rank={int(r['rank_order'])};")
    live = rows(db, "SELECT * FROM xp_multipliers WHERE active=1 AND (expires_at IS NULL OR expires_at > datetime('now'))")
    for m in live:
        kind = m["type"] if m["type"] in {"event", "role", "channel", "user", "faction"} else "event"
        target = "NULL" if kind == "event" else literal(m["target_id"])
        out.append("INSERT INTO discord_multipliers(name,factor,kind,target,ends_at) VALUES("
                   f"{literal(m['name'])},{float(m['multiplier'])},{literal(kind)},{target},{ts(m['expires_at'])});")
    return out, f"members {len(members)}, xp {len(xp)}, rank ladder {len(ranks)}, running multipliers {len(live)}"


def progress_map(value):
    """Legacy {"type","target"} / {"type":"compound","all":[...]} as {kind: number}."""
    v = json.loads(value or "{}")
    parts = v.get("all", []) if v.get("type") == "compound" else [v]
    return parts


def quests(db):
    """Quests, their progress, achievements (already announced), and faction points."""
    out = []
    periods = {"daily": "daily", "weekly": "weekly", "monthly": "monthly", "one_time": "once"}
    defs = rows(db, "SELECT * FROM quest_definitions")
    names = {}
    for q in defs:
        needs = {p["type"]: int(p["target"]) for p in progress_map(q["requirements"]) if "type" in p}
        reward = json.loads(q["rewards"] or "{}")
        names[q["id"]] = q["title"]
        faction = literal(q["faction"]) if q["faction"] in FACTIONS else "NULL"
        limit = int(q["max_completions"]) if q["max_completions"] else "NULL"
        out.append("INSERT INTO discord_quests(name,description,period,needs,xp,points,role,faction,max_completions,ends_at,active) VALUES("
                   f"{literal(q['title'])},{literal(q['description'] or '')},{literal(periods.get(q['recurrence'], 'once'))},"
                   f"{literal(json.dumps(needs))}::jsonb,{int(reward.get('xp', 0))},{int(reward.get('factionContribution', 0))},"
                   f"{literal(reward['role']) if reward.get('role') else 'NULL'},{faction},{limit},{ts(q['end_date'])},{'true' if q['active'] else 'false'});")
    progress = rows(db, "SELECT * FROM quest_progress")
    for p in progress:
        if p["quest_id"] not in names:
            continue
        have = {x["type"]: int(x.get("current", 0)) for x in progress_map(p["progress"]) if "type" in x}
        key = "once" if p["period"] == "all_time" else p["period"]
        done = ts(p["completed_at"] or p["created_at"]) if p["completed"] else "NULL"
        out.append("INSERT INTO discord_quest_progress(quest_id,discord_user,period_key,progress,completed_at) "
                   f"SELECT id,{literal(p['discord_id'])},{literal(key)},{literal(json.dumps(have))}::jsonb,{done} "
                   f"FROM discord_quests WHERE name={literal(names[p['quest_id']])} ON CONFLICT DO NOTHING;")
    rename = {"sver_partner": "sver_streamer"}
    unlocked = rows(db, "SELECT user_id, achievement, unlocked_at FROM achievements")
    for a in unlocked:
        key = rename.get(a["achievement"], a["achievement"])
        # The legacy bot marked them all announced; they aren't posted again.
        out.append("INSERT INTO discord_achievements(discord_user,key,unlocked_at,announced_at) VALUES("
                   f"{literal(a['user_id'])},{literal(key)},{ts_or_now(a['unlocked_at'])},{ts_or_now(a['unlocked_at'])}) ON CONFLICT DO NOTHING;")
    # Each voice minute (and watched minute) was recorded twice by the legacy bot: keep the rows with
    # a source, which are the ones its XP engine wrote.
    points = rows(db, "SELECT discord_id, faction, type, amount, created_at, json_extract(metadata,'$.source') AS source FROM faction_contributions")
    kept = 0
    for p in points:
        kind = "holiday" if p["source"] == "holiday" else p["type"]
        if p["faction"] not in FACTIONS or kind not in {"chat", "voice", "stream_support", "event", "quest", "giveaway", "holiday"}:
            continue
        if kind in {"voice", "stream_support"} and p["source"] is None:
            continue
        kept += 1
        out.append("INSERT INTO discord_points(discord_user,faction,kind,points,at) VALUES("
                   f"{literal(p['discord_id'])},{literal(p['faction'])},{literal(kind)},{max(0, int(p['amount'] or 0))},{ts_or_now(p['created_at'])});")
    return out, f"quests {len(defs)}, progress {len(progress)}, achievements {len(unlocked)}, faction points {kept} of {len(points)}"


def moderation(db):
    """Strikes (warnings), the escalation ladder, and the automod and safety settings, merged into
    the home server's settings. The tuning stays out of this file: it's read from the legacy rows."""
    db.row_factory = sqlite3.Row
    guild = db.execute("SELECT guild_id FROM guild_config").fetchone()[0]

    def one(table):
        row = db.execute(f"SELECT * FROM {table} WHERE guild_id=?", (guild,)).fetchone()
        return dict(row) if row else {}

    def listed(row, column):
        return [str(w) for w in json.loads(row.get(column) or "[]") if str(w).strip()]

    settings = {}
    for row in db.execute("SELECT * FROM warning_escalation WHERE guild_id=?", (guild,)):
        settings[f"{row['action']}_at"] = row["strike_count"]
        if row["action"] == "mute" and row["duration_ms"]:
            settings["mute_minutes"] = row["duration_ms"] // 60000
    a = one("automod_config")
    if a:
        def number(key, column, scale=1):
            if a.get(column):
                settings[key] = int(round(a[column] * scale))
        number("new_account_days", "new_account_days")
        number("quarantine_hours", "media_quarantine_hours")
        settings["nsfw"] = bool(a.get("nsfw_filter_enabled"))
        number("mass_spam_count", "mass_spam_threshold")
        number("mass_spam_seconds", "mass_spam_window")
        if a.get("spam_enabled"):
            # The legacy bot read the velocity columns first, then the older ones.
            settings["spam_count"] = a.get("spam_velocity_messages") or a.get("spam_threshold")
            settings["spam_seconds"] = a.get("spam_velocity_seconds") or (a.get("spam_window_ms") or 0) // 1000
        if a.get("caps_enabled"):
            number("caps_percent", "caps_threshold", 100)
            number("caps_length", "caps_min_length")
        if a.get("dupes_enabled"):
            number("dupes_count", "dupes_threshold")
            settings["dupes_seconds"] = (a.get("dupes_window_ms") or 0) // 1000
        if a.get("raid_enabled"):
            number("raid_joins", "raid_join_threshold")
            number("raid_seconds", "raid_window_seconds")
        settings["link_block"] = bool(a.get("links_enabled"))
        settings["dm_on_action"] = bool(a.get("dm_on_action"))
        for key, column in {"allowed_links": "links_whitelist", "banned_words": "banned_words",
                            "immune": "immune_roles"}.items():
            if listed(a, column):
                settings[key] = listed(a, column)
    s = one("safety_config")
    if s:
        for key, column in {"nudges": "nudge_enabled", "dogpile": "dogpile_enabled",
                            "support_guard": "support_guardrails_enabled", "early_warning": "ews_enabled"}.items():
            settings[key] = bool(s.get(column))
        for key, column in {"nudge_count": "nudge_threshold", "nudge_hours": "nudge_window_hours",
                            "dogpile_users": "dogpile_user_threshold", "dogpile_seconds": "dogpile_window_seconds",
                            "support_minutes": "support_unanswered_minutes"}.items():
            if s.get(column):
                settings[key] = int(s[column])
        for key, column in {"nudge_words": "nudge_words", "distress_words": "ews_distress_words",
                            "aggression_words": "ews_aggression_words"}.items():
            if listed(s, column):
                settings[key] = listed(s, column)
    settings = {k: v for k, v in settings.items() if v not in (None, 0)}
    statements = [f"UPDATE discord_home SET settings=settings||{literal(json.dumps(settings))}::jsonb;"]
    warnings = db.execute("SELECT * FROM warnings WHERE guild_id=? ORDER BY id", (guild,)).fetchall()
    for w in warnings:
        # The legacy bot recorded automod strikes under its own id; here automod is no moderator.
        moderator = "NULL" if w["source"] == "automod" else literal(w["moderator_id"])
        statements.append(
            "INSERT INTO discord_cases(user_id,moderator_id,action,reason,active,created_at) VALUES("
            f"{literal(w['user_id'])},{moderator},'warn',{literal(w['reason'] or '')},"
            f"{'true' if w['active'] else 'false'},{ts_or_now(w['created_at'])});")
    return statements, f"moderation settings: {len(settings)}, warnings: {len(warnings)}"


def holidays(db):
    """Faction holidays, replacing the seeded ones of the same name. Legacy rewards
    {xpMultiplier, factionContribution} become the day's factor and each member's points."""
    out = []
    legacy = rows(db, "SELECT faction, name, month, day, rewards, active FROM faction_holidays")
    for h in legacy:
        if h["faction"] not in FACTIONS:
            continue
        reward = json.loads(h["rewards"] or "{}")
        factor = min(10.0, max(0.1, float(reward.get("xpMultiplier", 1))))
        points = max(0, int(reward.get("factionContribution", 0)))
        out.append("INSERT INTO discord_holidays(faction,name,month,day,factor,points,active) VALUES("
                   f"{literal(h['faction'])},{literal(h['name'])},{int(h['month'])},{int(h['day'])},{factor},{points},"
                   f"{'true' if h['active'] else 'false'}) ON CONFLICT(name) DO UPDATE SET faction=EXCLUDED.faction,"
                   "month=EXCLUDED.month,day=EXCLUDED.day,factor=EXCLUDED.factor,points=EXCLUDED.points,active=EXCLUDED.active;")
    return out, f"holidays: {len(out)}"


def streamers(db):
    """Streaming accounts (alerts setting), stream sessions, streamer profiles, the auto-link
    opt-outs and past spotlights. S.V.E.R accounts aren't carried over: they come from linked
    Discord identities on the rebuild. Live state and old go-live posts start fresh."""
    out = []
    accounts = rows(db, "SELECT user_id, platform, platform_username, platform_id, alerts_enabled FROM stream_accounts WHERE platform <> 'sver'")
    for a in accounts:
        handle, pid = a["platform_username"], a["platform_id"]
        if a["platform"] == "kick":
            handle = pid = (pid or handle).lower()
        # A YouTube channel the legacy bot never resolved gets its id on the first poll.
        out.append("INSERT INTO discord_stream_accounts(discord_user,platform,handle,platform_id,alerts) VALUES("
                   f"{literal(a['user_id'])},{literal(a['platform'])},{literal(handle)},{literal(pid) if pid else 'NULL'},"
                   f"{'true' if a['alerts_enabled'] else 'false'}) ON CONFLICT(discord_user,platform) DO UPDATE SET "
                   "handle=EXCLUDED.handle,platform_id=EXCLUDED.platform_id,alerts=EXCLUDED.alerts;")
    sessions = rows(db, "SELECT user_id, platform, platform_username, started_at, ended_at, duration_minutes FROM stream_sessions")
    for x in sessions:
        out.append("INSERT INTO discord_stream_sessions(discord_user,platform,handle,started_at,ended_at,minutes) VALUES("
                   f"{literal(x['user_id'])},{literal(x['platform'])},{literal(x['platform_username'])},"
                   f"{ts_or_now(x['started_at'])},{ts_or_now(x['ended_at'])},{max(0, int(x['duration_minutes'] or 0))});")
    profiles = rows(db, "SELECT * FROM streamer_profiles")
    array = lambda items: "ARRAY[" + ",".join(literal(i) for i in items) + "]::text[]"
    for pr in profiles:
        collab = json.loads(pr["collab_preferences"] or "{}")
        platforms = [p for p in collab.get("preferredPlatforms", []) if p in {"sver", "twitch", "youtube", "kick"}]
        bio = literal(pr["bio"][:500]) if pr["bio"] else "NULL"
        out.append("INSERT INTO discord_streamer_profiles(discord_user,bio,tags,games,collab,collab_platforms,collab_content,collab_size,collab_notes,featured,last_live_at,created_at) VALUES("
                   f"{literal(pr['discord_id'])},{bio},{array(json.loads(pr['tags'] or '[]')[:10])},{array(json.loads(pr['games'] or '[]')[:20])},"
                   f"{'true' if collab.get('availableForCollab') else 'false'},{array(platforms)},{array(collab.get('contentTypes', [])[:10])},"
                   f"{min(20, max(2, int(collab.get('maxCollabSize') or 4)))},{literal(str(collab.get('notes') or '')[:200])},"
                   f"{'true' if pr['featured'] else 'false'},{ts(pr['last_live_at'])},{ts_or_now(pr['created_at'])}) ON CONFLICT(discord_user) DO NOTHING;")
    optouts = rows(db, "SELECT user_id, platform FROM auto_link_blacklist")
    for o in optouts:
        out.append("INSERT INTO discord_autolink_optouts(discord_user,platform) VALUES("
                   f"{literal(o['user_id'])},{literal(o['platform'])}) ON CONFLICT DO NOTHING;")
    spotlights = rows(db, "SELECT type, discord_id, title, reason, nominated_by, faction, channel_id, message_id, featured, created_at FROM spotlights")
    for x in spotlights:
        faction = literal(x["faction"]) if x["faction"] in FACTIONS else "NULL"
        opt = lambda v: literal(v) if v else "NULL"
        out.append("INSERT INTO discord_spotlights(kind,discord_user,title,reason,nominated_by,faction,channel_id,message_id,featured,created_at) VALUES("
                   f"{literal(x['type'])},{literal(x['discord_id'])},{opt(x['title'])},{opt(x['reason'])},{opt(x['nominated_by'])},{faction},"
                   f"{opt(x['channel_id'])},{opt(x['message_id'])},{'true' if x['featured'] else 'false'},{ts_or_now(x['created_at'])});")
    return out, (f"stream accounts: {len(accounts)}, sessions: {len(sessions)}, profiles: {len(profiles)}, "
                 f"auto-link opt-outs: {len(optouts)}, spotlights: {len(spotlights)}")


SECTIONS = [home, levels, quests, moderation, holidays, streamers]


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: discord-import.py LEGACY_SQLITE_FILE")
    db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
    sys.stdout.reconfigure(encoding="utf-8")  # psql reads UTF-8, whatever the console uses.
    print("-- Legacy Discord bot import (docs/DISCORD_BOT.md). Apply with psql -1.")
    for section in SECTIONS:
        statements, summary = section(db)
        print(f"-- {summary}")
        print("\n".join(statements))
        print(summary, file=sys.stderr)


if __name__ == "__main__":
    main()
