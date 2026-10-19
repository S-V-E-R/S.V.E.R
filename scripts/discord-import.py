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


SECTIONS = [home]


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: discord-import.py LEGACY_SQLITE_FILE")
    db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
    print("-- Legacy Discord bot import (docs/DISCORD_BOT.md). Apply with psql -1.")
    for section in SECTIONS:
        statements, summary = section(db)
        print(f"-- {summary}")
        print("\n".join(statements))
        print(summary, file=sys.stderr)


if __name__ == "__main__":
    main()
