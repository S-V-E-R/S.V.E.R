# Discord bot

Specified October 19, 2026. The S.V.E.R Discord bot replaces the legacy community bot on the S.V.E.R home server and keeps the streamer bot built in October ([COMMUNITY.md](COMMUNITY.md) "Discord bot"). Everything the legacy bot did comes over, fixed, on the rebuild's own data. Nothing calls the legacy backend.

## Decisions (Joe, October 19, 2026)

- **Two ladders, synced.** Discord keeps its own XP and its 22-rank faction ladder; existing XP is imported. A linked account shows its Discord rank on its S.V.E.R profile, and gets Discord roles for its S.V.E.R level band, faction, supporter tier and streamer status. Discord activity never earns S.V.E.R XP.
- **Influence: linked, verified, capped.** Chat and voice in the home server count toward the faction war only for linked accounts the war already counts (eligible, verified email, in a faction), only in channels staff map to a genre, through the war's normal rules (faction events, caps) with a Discord daily cap of its own. Discord faction points still show in Discord for everyone.
- **Every feature is ported**, including the ones never used, with their bugs fixed.
- **`/rank` is an image card.**

## How it runs

- **One bot.** The S.V.E.R Discord application (`DISCORD_BOT_TOKEN`) that already posts go-lives in streamers' servers. The home server is a staff setting; community features run there. Streamers' servers keep exactly what they have today (go-live posts, subscriber/faction/guild roles).
- **Gateway inside the API.** One Gateway connection, run by the worker only when `DISCORD_GATEWAY=1` and while it holds a Postgres advisory lock, so a second API (rehearsal, rollback, a second container) never runs a second bot. Intents: Guilds, Guild Members, Guild Messages, Message Content, Guild Voice States, Guild Message Reactions, Guild Presences, Guild Moderation, Direct Messages. Members, Message Content and Presences are privileged: they're switched on for the application in the Discord Developer Portal.
- **Resume and recovery.** Resumes after a dropped connection; after a restart it identifies again, and voice sessions are rebuilt from the server's current voice states (the legacy bot lost everyone in voice on every restart). Nothing that matters lives only in memory: lockdowns, raid state, escalation timers, temp rooms and pending onboarding are rows.
- **Slash commands only.** Every legacy `!` command becomes a slash command; admin ones require Manage Server (or Administrator) and are hidden from others by default. Commands are registered on the home server at start.
- **Setup is a staff page** (`/admin/discord`): channels, roles, toggles, word lists, holidays, quests, multipliers, rank ladder, channel→genre mapping. It replaces `!config` and the one-off setup commands.
- **Time zones** are real IANA zones (`chrono-tz`), so birthdays and events follow daylight saving. Legacy EST/CST/MST/PST/UTC/GMT values map to America/New_York, Chicago, Denver, Los_Angeles, UTC and UTC.
- **Private tuning.** Automod thresholds, word lists, anti-raid numbers and the Discord influence cap are anti-abuse tuning: they live in the database and the private server config, not in this repo. The repo ships safe example values for tests.

## Levels and ranks

- **XP sources.** Message: 15–25 XP (random), at most once per 60 s, messages of 5+ characters, not in XP-disabled channels, not commands. Reaction: 5 XP, once per 30 s, not on your own message or a giveaway entry. Voice: 3 XP a minute, plus 2 while someone in the channel is streaming; nothing while alone with no other human, in the AFK channel, or muted and deafened for more than 15 minutes. Streaming: 50 XP per full 30 minutes of a tracked stream. Quests: their reward.
- **Daily cap** 2,000 XP after multipliers (UTC day).
- **Level** = floor(0.1·√XP), so level L needs 100·L² XP.
- **Ranks.** The 22-rank ladder with per-faction names (Initiate … Overlord, Novice … Sovereign, Spark … Inferno; Private … Commander without a faction), thresholds 0 to 600,000, and the existing 88 roles. Promotion only, never demotion. Rank 1 is given to everyone who has XP (the legacy bot only gave it by backfill). Changing faction swaps the rank role to the new faction's version.
- **Multipliers** stack by multiplying: event (everyone), role, channel, user, faction. **Fixed:** a faction multiplier applies only to that faction's members.
- **Rank-up post** in the rank-up channel, with the faction's emoji.
- **`/rank [user]`**: a 950×300 PNG card (faction gradient and accent, avatar, rank, level, progress to the next rank, server position, daily XP) drawn in Rust from the self-hosted fonts; cached 5 minutes.
- **`/leaderboard [xp|voice|messages|streams] [page]`**, 10 a page.
- **`/profile [user]`**: Discord stats, linked platforms, and the S.V.E.R block (level, faction, supporter tier) for linked accounts.

**Fixed from legacy:** faction points were counted twice; voice credited time spent alone or AFK; quest XP never counted for the faction.

## Factions in Discord

- `/faction join` (with a confirm to switch), the role-select panel and onboarding set the Discord faction. **Synced with S.V.E.R:** for a linked account the S.V.E.R faction wins and Discord follows it; Discord can't move someone's S.V.E.R faction (faction changes on S.V.E.R have their own rules). Unlinked members choose in Discord.
- **Faction roles** (Myria, Aetheron, Glint) and private faction HQ channels, as set up today.
- **Faction points** per season: chat, voice, events, quests, holidays and stream support, shown by `/faction stats` (points and members per faction) and `/faction leaderboard` (top 10 contributors), for the running season or all time when none runs (the legacy `/factions` commands were never registered; they are now).
- **Seasons** start and end with staff `/season start <name> <days>|end|status`, at most one at a time, and end on their own at `ends_at` (the legacy bot never ended them). The faction with the most points inside the season wins (a tie at the top has no winner), gets a 1.5× faction multiplier for 14 days, and the end is posted in the faction channel (or announcements).
- **Holidays**: Myria Founding Day (March 15) and the Iron Trial (September 20), Aetheron Shield Day (May 10) and the Vigil (November 11), Glint Spark Festival (July 4) and Winter Luminance (December 21): a faction multiplier for the day and a contribution for every member, announced in the faction channel. Each runs once on its UTC date; the multiplier lasts until the next UTC midnight. Staff manage them with `/holiday create|list|remove`.
- **Territories**: `/territories` and the "has taken control of" posts read the war map through the `factions` module, no polling of an old API. Holders are compared every 5 minutes; the first look and a new war season only record.
- **Influence** as decided above: `source='discord'` in `faction_influence`, one award per kept message (its id is the event) or voice minute, the war's own checks, faction events and caps, and a Discord daily cap (private tuning: `discord_points`, `discord_daily_cap`). Staff map text and voice channels to genres on the staff page ("Influence channels"); messages in threads don't count.

## Account link and sync

- **Linked automatically.** A Discord account is linked when the person signs in with Discord or links Discord in settings (the `identities` table). No link tokens, no second flow. `/link sver` explains this and sends the link to settings.
- **Roles** (home server, every 15 minutes and right after `/link` or `/faction`): S.V.E.R level bands (staff `/levelroles add <level> <role>|remove <level>|list`; a member gets only the highest band reached), faction, Wolfsworn, Greycloak, Founder, Streamer (has gone live), the last four set on the staff page ("S.V.E.R sync"). Only roles the bot gave are ever taken back (as today); a role the member already had is never recorded as the bot's. Members not in the server are skipped.
- **Nickname**: optional per member (`/link nickname on|off`), off by default. `/link status` shows the link, level, faction and nickname setting.
- **Discord rank on S.V.E.R**: the profile of a linked account can show its Discord rank as a badge (the owner turns it on under User card in profile settings; `GET/PUT /api/me/discord-rank`).
- **Level-ups**: an S.V.E.R level-up of a linked member posts in the rank-up channel ("reached Level N on S.V.E.R").
- **Fixed:** the legacy 10-minute sync failed on every run and never synced anyone; faction conflicts could never be resolved.

## Quests and achievements

- **Quests**: daily, weekly, monthly and one-time, with requirements chat messages, voice minutes, event RSVPs, giveaway entries, stream watching (now tracked), or several at once. Rewards: XP (through multipliers and the cap), faction points, an optional role. Milestones at 5, 10, 25, 50 and 100 completions. `/quests list|progress|board`; staff create and remove them. The six existing quests come over. **Fixed:** a recurring quest paid out only once ever; RSVPs could be farmed; faction-only quests and completion limits weren't enforced.
- **Achievements**: the 16 legacy achievements (S.V.E.R linked / supporter / streamer ("On the Air", in place of the legacy partner badge: there is no partner program), first stream, 10 h streamed, levels 10/25/50/100, voice 10/50/100 h, collab posted, birthday set, faction joined, hero nominated), posted to the achievements channel with rarity colours. **Fixed:** no channel was set, so none were ever posted.

## Go-live alerts and streamers

- **S.V.E.R streams** post from the platform's own go-live event (the same queue as today's streamer bot), not polling. Faction-coloured embed with title, category, preview and a link; edited to "was live" when the stream ends and removed 4 hours later.
- **Twitch** through EventSub on the rebuild's Twitch app, with a reconciliation poll that pages past 100 accounts. **Kick** through Kick's official API, **YouTube** through the Data API. **Fixed:** Kick and YouTube never worked.
- **Linking**: `/alerts link <platform> <name>`, the streamer panel (its S.V.E.R button now works), and auto-detect of Twitch streaming status with the same opt-out list. `/alerts on|off`.
- **Now Live role** while live on any platform.
- **Stream sessions** and stream XP are recorded when a stream ends.
- **Streamer profiles, directory and search** (`/streamer view|edit|search`), **schedules** (`/schedule`) with the Monday schedule post, **follower milestones** from S.V.E.R follower counts (10 to 10,000), **clip of the week** from S.V.E.R's top clip, and the **weekly Streamer of the Week spotlight**. **Fixed:** the schedule post, milestone checker and clip of the week were never started; the platform search filter found nothing.
- **Collab** (`/collab post|browse|myrequest|history`) and **LFG** (`/lfg`, forum or thread). **Fixed:** collab preferences queried a missing column.
- **As built (step 4):** S.V.E.R go-lives follow the platform's own alert decision (its 6-hour throttle; a throttled stream still gets Now Live and a session), and only for members of the home server. Each account posts at most once per 6 hours; a stream back within 10 minutes turns its old post live again. Twitch uses the rebuild's Twitch app and EventSub webhook, polled every 3 minutes as a check. Kick needs `KICK_CLIENT_ID` and `KICK_CLIENT_SECRET` (an app on Kick's developer portal) and YouTube needs `YOUTUBE_API_KEY`; until they're set those two platforms can't be linked. YouTube reads each channel's public feed and asks the API only which recent video is live (1 quota unit per 50 videos, where the legacy search cost 100 per channel). `/alerts unlink` is new, and unlinking any platform stops auto-detect relinking it. The legacy bot's two schedule systems are one (`/schedule`); `/streamer edit schedule` points to it. Schedule times show in each reader's own time zone.

## Community

- **Welcome**: the public welcome, the Pending role, DM onboarding (rules, faction, platform roles, S.V.E.R link, birthday), Community Member on completion, a fallback in the welcome channel when DMs are closed. **Fixed:** the S.V.E.R step was a "Coming soon" stub.
- **Goodbye**: one post. **Fixed:** every leave posted two.
- **Birthdays** (`/birthday set|view|upcoming|remove`, panel, onboarding): the birthday role, a 2× XP multiplier for the day and a post, in the member's own time zone.
- **Server anniversaries**: 100 XP per year and a post. **Fixed:** they never fired.
- **Seasonal events** (`/rituals event`): announced once at start and once at end. **Fixed:** the start was re-announced every hour.
- **Spotlights and nominations** (`/rituals spotlight`).
- **Events and RSVPs** (`/event`): reminders 60 and 15 minutes before, recurring daily/weekly/biweekly/monthly, real time zones. Events without an end close after 24 hours.
- **Polls** (`/poll`): 2–4 options, one vote each, and they close at their time. **Fixed:** timed polls never closed.
- **Giveaways** (`/giveaway submit` for staff review, staff create/end/reroll/cancel): button or 🎉 entry, fair draw. **Fixed:** they could end up to a day late.
- **Reminders** (`/remind`). **Fixed:** they never fired.
- **Mentors** (`/mentor signup|request|list|complete`).
- **Role-select panel**: platform links, faction, free-games and alert toggles, birthday.
- **Threads and forums**: `/thread create` in managed channels; forum setup and starter posts from the staff page.
- **Temp voice rooms** ("➕ Create Private Room"). **Fixed:** empty rooms were never deleted.
- **Custom commands** (`/command add|remove|list` for staff; members use `/c <name>`).
- **Embeds** (`/embed create|edit|json`).
- **Free games**: Epic, Steam (giveaways and free weekends) and GOG, every 30 minutes, deduplicated, with the Free Games ping role on the first post of a batch. **Fixed:** the GOG source has never found anything; checked against its current API.
- **GitHub changelog**: published releases of S.V.E.R repositories, signed webhooks. **Fixed:** releases were accepted and never posted.
- **WoW and Chromely**: `/wow link|profile|leaderboard|record|rivals|submit` and speedrun record posts, against the Chromely API.

## Moderation and safety

- **Automod**, in order: new-account and unverified media, media quarantine for new joiners, NSFW text/links/files, mass spam (bulk delete and a 24-hour timeout), spam velocity, blocked links (allow list), excessive caps (first one in 5 minutes allowed), duplicates, banned words. A hit deletes the message, records a strike, logs it and checks escalation; immune roles skip it. Message edits are scanned too. **Fixed:** the duplicate check's real window collapsed to the spam window.
- **Escalation ladder** (3 strikes → 1 h mute, 5 → kick, 7 → ban today): staff approve from buttons in the mod log; after 30 minutes untouched it pings the mod team again. **Fixed:** "at least", not exactly, N strikes; a mute approval without a duration failed; the 30-minute timer was lost on restart.
- **Manual moderation** (`/mod warn|warnings|clearwarn|mute|unmute|kick|ban|softban|duration|purge|note|notes|history|protect|unprotect|lockdown|unlock|bantrap`) and `/modpanel`. Every action is logged. **Fixed:** timed bans never lifted; notes were split across two tables; kick, ban and mute weren't logged; mute and timed ban wrote duplicate records.
- **Anti-raid**: a burst of joins locks the server and posts unlock / ban all / auto-unlock buttons. **New-account filter** with approve/kick/ban buttons. **Ban trap** channel.
- **Safety layer** (off until staff turn it on, as today): language nudges, anti-dogpile, early warning signals, unanswered support questions.
- **Tickets / modmail** by DM or `/ticket`: a private thread per ticket, claim, close with reason, notes, priority and category, inactivity warning at 5 days and close at 7, transcripts. **Fixed:** tickets could never be configured; the one-per-hour limit didn't work.
- **Mod log** for automod, warnings, escalations, actions, deletes, bulk deletes, edits, joins and leaves. Deleted-message evidence is kept 30 days.
- **Health**: `/health` and the staff page show the Gateway, permissions and role position.
- **Tuning** is on the staff page (Automod, Escalation and Safety): numbers and word lists, each check off while its numbers or list are empty. The import carries the legacy values over; the legacy bot's built-in NSFW words and sites were code, not data, so staff enter those.

## Data carried over

A one-time import from the legacy bot's SQLite database, rehearsed on a copy first: XP and stats, members, factions and onboarding state, the rank ladder and its 88 role IDs, multipliers, holidays, quests and progress, achievements, faction points, birthdays, stream accounts, sessions and the auto-link opt-outs, streamer profiles, spotlights, managed threads, free games already posted, warnings and the escalation ladder, automod and safety settings, guild channel/role settings. Discord IDs are matched to S.V.E.R accounts through linked Discord identities. The legacy file is archived with the legacy database afterwards.

## Build order

Each step is its own PR and release.

1. **Core**: Gateway, slash command framework, the home-server settings, the staff page shell, the data import (rehearsed), health.
2. **Levels and ranks**: XP, voice, multipliers, ranks, rank card, leaderboard, profile, quests and achievements.
3. **Factions and sync**: faction roles, seasons, holidays, territories, influence, S.V.E.R role/level sync, Discord rank on profiles.
4. **Streamers**: go-live alerts on all four platforms, Now Live, sessions, profiles, schedules, milestones, clip of the week, spotlights, collab, LFG.
5. **Community**: welcome and onboarding, goodbye, birthdays, anniversaries, events, polls, giveaways, reminders, mentors, panels, threads and forums, temp rooms, custom commands, embeds, free games, GitHub changelog, WoW/Chromely.
6. **Moderation**: automod, escalation, manual moderation, anti-raid, new-account filter, ban trap, safety, tickets, mod log.
7. **Switch-over**: stop the legacy bot, run the final import, start the Gateway on the home server, check every feature live, then retire the legacy backend and archive its database read-only.

## Done when

On the home server, for one test member and the real community: XP and ranks match the imported totals and promote with the right faction role; a linked member gets their level, faction and supporter roles and their Discord rank shows on S.V.E.R; Discord chat in a mapped channel adds capped influence for a verified linked member and none for others; a S.V.E.R and a Twitch go-live each post and clear; a free game, a birthday and a welcome post; an automod hit, an escalation approval, a ticket and a raid lockdown work end to end; and the legacy bot and backend are stopped with nothing lost.
