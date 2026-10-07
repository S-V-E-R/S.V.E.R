# Community additions

Decided by Joe on October 3, 2026. These are smaller features carried over from the legacy platform, each placed in the step of the build order where it fits. Each section has its own "Done when".

| Feature | When |
| --- | --- |
| Download my data | Right after Live streams closes (with Linked chat) |
| Chat commands and the faction bots (PYRE, ECHO, FAVOR, VOLK) | Right after Live streams closes |
| GIFs in chat | Right after Live streams closes |
| Live captions from OBS | After Live streams closes, once a media test confirms captions pass through |
| Direct messages | With Guilds, right after Factions |
| Discord bot | After Support (Module 6) |
| Surge and counter widgets | With CrowdSync (Module 7) |
| Phone as camera | Phase 3, with alerts and overlays |
| Mobile web | Every module (pages must work on phones); installable web app after MAGNet |

Not coming back: mentorship, team relay, patronage and the social feed. Restreaming from S.V.E.R came back the same day as part of Multistream ([LINKED_CHAT.md](LINKED_CHAT.md)).

## Download my data

- Settings → Privacy → **Download my data** creates a zip of everything S.V.E.R holds about the account: profile, settings, follows, wall posts, chat messages still kept, reports filed, strikes and appeals, Valor and payout ledgers, guild memberships, and sign-in history. Machine-readable (JSON) plus a short readme.
- Ready within 7 days (usually minutes); the link is emailed, works for 7 days and needs the user to be signed in. One request per 24 hours.
- Account deletion is already built (Login module); the Privacy Policy explains both.
- **Done when:** a user requests an export, receives a link, downloads a complete zip that matches their data, and the link expires.

## Chat commands and the channel bot

- **Built-in commands** for viewers and moderators already in specs (`/raid`, `/flag`, `/timeout`, `/ban`, `/slow`, `/poll` …) get one consistent help list (`/help`).
- **Custom commands:** a streamer creates `!name` → reply text (up to 300 characters), with who can use it (everyone, followers, subscribers, moderators) and a cooldown. Variables: `{user}`, `{channel}`, `{uptime}`, `{game}`, `{followers}`.
- **Timed messages:** up to 5 messages posted every N minutes (at least 10) while live, only if chat has had activity since the last one.
- Replies come from the channel's **bot**, a built-in system account shown with a bot badge. No third-party bot is needed; a public bot API can come later.

### The faction bots

Carried over from legacy: each faction has its own bot persona, and a neutral one.

| Bot | For | Tagline |
| --- | --- | --- |
| **PYRE** | Myria | Discipline is the flame that never dies. |
| **ECHO** | Aetheron | The pattern persists. |
| **FAVOR** | Glint | Fortune favors the bold. And the generous. |
| **VOLK** | Neutral (no faction, or the streamer prefers it) | The grey wolf watches. |

- A channel gets its owner's faction bot by default; a streamer can switch to VOLK or to another faction's bot.
- **Personality settings:** Chill (warm, little faction flavor; the default), Battle (full faction personality) and Event (maximum hype for tournaments, charity streams and milestones). Personality shows in event messages (follows, subs, raids, Surge levels, milestones) and optional cross-faction greetings. Random quips are off by default.
- **Moderation help:** the bot enforces the channel's AutoMod settings: excessive caps, repeated messages, symbol or emote spam, and the existing banned-word and link rules. It follows a ladder the streamer configures (warn, then a short timeout, then a longer one), announces actions in the bot's voice, and logs each action for moderators, who can reverse it. It never bans on its own; bans stay with people.
- **Giveaways:** the streamer starts one with a keyword; the bot picks a random winner among eligible chatters (Counted session, not banned), shown publicly. Prizes are the streamer's responsibility; the Terms cover it.
- **Starter timers:** new channels can add starter timed messages (social links, chat rules).
- Bot lore and copy are original to S.V.E.R and live in [LORE.md](LORE.md) ("The faction bots"), including sample lines per event. Legacy PYRE lore borrowed names from a published novel series; that text is not carried over. JINX was renamed FAVOR (Joe, October 6, 2026) because JINX is a well-known League of Legends champion.
- Song requests from the legacy bot are not carried over (music licensing).

- Commands and bot replies follow the channel's banned-word and link rules.
- **Done when:** a streamer creates custom and timed commands, viewers trigger them within their permissions and cooldowns, and the bot replies with variables filled in; each channel gets its faction's bot (or VOLK) with the chosen personality; AutoMod catches caps, repeats and spam and follows the warn-and-timeout ladder, with every action reversible by moderators; a giveaway picks a fair winner.

## GIFs in chat

- A GIF button in chat searches a GIF service (chosen at build time, with an API licence that allows this use). Only the service's general-audience ratings are allowed.
- Each channel can turn GIFs off or limit them to subscribers or followers. Slow mode and timeouts apply.
- GIFs show as a still preview that plays on hover or tap, and respect reduced-motion settings, so chat stays light on low-end PCs.
- **Done when:** a viewer finds and sends a GIF, a channel's GIF setting is enforced, and GIFs don't autoplay for viewers who prefer reduced motion.

## Live captions from OBS

- Streamers who add captions in OBS (closed captions carried in the video stream) get them shown in the S.V.E.R player with a CC button. S.V.E.R doesn't generate captions itself, so there's no audio decoding.
- First a media test: confirm captions survive SRS transmuxing to WebRTC and LL-HLS. If one path drops them, the player shows captions only where they arrive, and the spec is updated.
- **Done when:** a test stream with OBS captions shows them in the player on both delivery paths (or the limitation is documented).

## Direct messages

- Text DMs between people who **follow each other**. A user can also allow DMs from anyone they follow, or from nobody.
- **Under-18 accounts:** adults can message them only with a mutual follow **and** the minor's DM setting allowing it; it is off by default for under-18s.
- Block, report and mute in every conversation; blocking ends the conversation for both. Staff can review reported messages only.
- Text only at first (no images or files), up to 1,000 characters, with link warnings. Each person can delete a conversation from their own view.
- **Privacy (decided October 4, 2026):** DMs are not end-to-end encrypted at launch, because S.V.E.R must be able to act on reports, protect under-18 accounts and meet Take It Down. Instead:
  - Message bodies are encrypted in Postgres with a DM-only key held outside the database (Login's cryptography, its own purpose binding), so a database copy or backup alone reveals nothing. Everything travels over HTTPS.
  - Staff can read only the conversation named in a report, through the report case. Every access is logged, and the reporter sees when their report was reviewed.
  - Messages are deleted automatically 12 months after they're sent (proposed), or sooner when either person deletes their account. Messages attached to an open report or Take It Down request are held until it closes.
  - Later, as its own step after launch: an opt-in "Private" mode for DMs between two adults, using an established protocol (MLS) rather than a home-made one.
- DM notifications follow the notification settings.
- **Done when:** mutual followers exchange messages in real time; non-mutuals and blocked users can't; the under-18 rules hold; reports reach staff; message bodies are unreadable in a database dump without the DM key; staff reads outside a report are impossible and every report read is logged; expired messages are deleted.

## Discord bot

- A S.V.E.R Discord bot that a streamer adds to their server: posts "now live" messages, and syncs Discord roles for their S.V.E.R subscribers, faction and guild.
- Uses Discord's official bot API; the streamer links their Discord account (already a sign-in option).
- **Done when:** go-live posts arrive in a chosen Discord channel and subscriber and faction roles stay in sync.

## Surge and counter widgets

Specified in [CROWDSYNC.md](CROWDSYNC.md).

## Phone as camera

- A streamer opens a S.V.E.R page on their phone, signs in and sends its camera to their own OBS as a browser source, over a private, token-protected connection. Useful for a hands or face cam without extra hardware.
- Built with alerts and overlays in Phase 3. It needs a small media relay; measure its server cost first.
- **Done when:** a phone's camera appears in OBS with under a second of delay and only the streamer can view it.

## Mobile web

- Every page already has to work on phones ([DESIGN.md](DESIGN.md)). After MAGNet, the site becomes an installable web app (home-screen icon, full screen, push notifications already planned for go-live alerts).
- A native iOS and Android app comes after the core modules, as its own project.
