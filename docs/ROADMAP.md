# Roadmap

S.V.E.R is built one module at a time. Each module is fully specified, built, and tested against its "done when" line before the next one starts.

| # | Module | What it delivers | Status |
| --- | --- | --- | --- |
| 0 | Foundation | The site foundation, shared interface, security boundaries and automated checks | Done |
| 1 | Login | Email and Google, Twitch, Discord sign-in; verification; sessions; 2FA for streamers | Done |
| 2 | Profiles | Channel pages, follows, War Council, the Wall, profile songs, schedules, sponsors and fan art | Done |
| 3 | Live streams | Creator Studio, OBS streaming, playback, chat and moderation, viewbot detection, custom emotes, go-live alerts, raids and hosting. Closed October 12, 2026 after a real OBS acceptance run: direct and CDN latency, reconnect, viewer counts, captions and clip previews. | Done |
| 4 | Factions | Myria, Aetheron, Glint; membership, weekly genre checkpoints, seasonal rewards, private councils and faction hubs | Done |
| 5 | MAGNet | Fair-rotation discovery, recommendations, stream-to-stream handoff, including genre, faction and co-stream lanes and MAGNet chat. Closed October 15, 2026 after acceptance with three live streams: rotation, suggestions, stream-end handoff, search and empty states. | Done |
| 6 | Support | Subscriptions, Valor and tributes, channel rewards, co-streams, payouts. Live on Stripe since October 6, 2026. | Done |
| 7 | CrowdSync | Interactive boards, Skills, polls and predictions, faction rallies, emote combos, Surge, the OBS bridge and game SDKs for JavaScript, Unity and Unreal. Closed October 15, 2026 after the Unity and Unreal SDKs compiled and their example games ran end to end. | Done |
| 8 | VODs and clips | Past broadcasts, permanent Highlights, clipping and chat replay. Closed October 15, 2026 after production acceptance: recording, rewind, expiry with files removed, chapters, Highlights, live and VOD clips, approval, deletion and chat replay. | Done |
| 9 | Beacons | Short vertical videos that lead to live streams, with comments, faction tags and copyright notices. Closed October 15, 2026 after production acceptance of Beacons made from clips; video uploads stay closed for now. | Done |

Modules 1 to 5 make the site usable. Several later systems shipped early at the founder's direction: Progression (levels, XP, daily orders, channel loyalty ranks), achievements and chat cosmetics, Raven's Eye staff analytics, native alerts and overlays, linked chat and restreaming, and the developer platform. Ads and viewer payouts are designed and run on a test-only ledger.

The numbered table is also the public roadmap feed. The API embeds it at build time and serves it at `GET /api/roadmap` with `no-store`; the website server-renders it and refreshes every 30 seconds while visible. Update this table when a module starts or passes acceptance, then deploy the API. The page follows that release without a frontend rebuild. Unreleased local edits never appear as live progress. Keep rows 0–9 in order and statuses `Done`, `In progress`, `Started` or `Planned`; the backend tests reject an incomplete or malformed table.

The iOS and Android app in `apps/mobile` is a client of this API. It is not one of the modules above: it does not change discovery, sessions, playback or chat. See `apps/mobile/README.md` for what that client does and what it leaves for later.

## Site pages

Static and informational pages run alongside the modules. They do not close or reopen a module.

- First batch deployed: About, Help & FAQ, Community Guidelines, Terms of Service, Privacy Policy, Copyright & DMCA, and Contact. Layouts follow [the design system](DESIGN.md): self-hosted Cinzel/Barlow fonts, centered information pages, legal summaries and contents lists, shared navigation and footer. Legacy text is updated for the current content rules and available features. Help uses native expandable answers and includes account recovery, profiles, OBS setup and support.
- Before launch: legal review of the adapted text, confirmation of public contact inboxes, and verification/publication of the complete registered copyright-agent details.
- [Take It Down](TAKE_IT_DOWN.md) is deployed: public request/status forms, staff review, image quarantine, exact-copy blocking, removal notices and playback revocation. The production purge credential is configured; requester/staff email and enrolled-browser push have reached provider acceptance. Video-frame matching is built: likely copies of removed media in live streams, recordings, clips and Beacons are held for staff review. Full live removal/restoration acceptance and legal/process review remain open.
- Live-stream chat now has mentions, replies, persistent pins, Broadcaster/Moderator/Staff badges and channel emotes, with Creator Studio uploads and staff media review. The remaining module scope and acceptance work are tracked in [Live streams](LIVE_STREAMS.md).
- Second batch deployed: [Factions](https://sver.tv/factions) and the [live Roadmap](https://sver.tv/roadmap). The faction page explains the three sides, their lore and starting genres; Module 4 adds enrollment, live standings and private member hubs. The roadmap reads the running API's published module table and refreshes every 30 seconds while visible, retaining the last received progress with a warning during outages. Module status changes ship with the API; no frontend rebuild is needed.
- Browse and public category discovery arrive with MAGNet. Faction enrollment, the war map, hubs, weekly board elections and staff genre/category tools are implemented.
- The public homepage and focused watch page follow the shared design: real live/recent channel shelves, a manual spotlight carousel, direct watch links, and a responsive player/streamer/chat layout. Search, MAGNet rotation and handoff, clips and Beacons are built; their remaining acceptance is tracked in the module rows above.

- The [Discord bot](DISCORD_BOT.md) is being ported from the legacy community bot onto the platform, in seven steps; step 1 (the Gateway connection, slash commands and staff settings) is live; step 2 (levels, ranks, quests and achievements), step 3 (factions and S.V.E.R sync), step 4 (streamers: go-live alerts on four platforms, profiles, schedules, collab and LFG), step 5 (community: onboarding, birthdays, events, polls, giveaways, mentors, free games, the changelog and more) and step 6 (moderation) are built.
- [SVER Plays](https://sver.tv/sverplays/live) is restored at the founder’s request: a dedicated 24/7 live-testing channel with viewer game controls, using the separate game runtime. This does not start the general CrowdSync module.
- [The Wall](WALL.md) is built: 1,000 seats on `/wall`, bought once, that buy a tile on that page and nothing else. Sales stay closed until the Wall terms, tax and prices are settled.
- [Platform supporters and Founders](SUPPORTERS.md) are built: Wolfsworn ($5) and Greycloak ($25) monthly card subscriptions to S.V.E.R on `/supporters`, cosmetic only, with numbered Founder marks for the first 500 paid Greycloak accounts. Managed from `/wallet` and `/admin/supporters`; new subscriptions pause with the "Supporter sales" switch.
- Guilds is implemented after Factions: cross-faction teams, applications, emblems, follows, combined schedules, staff review and team shortcuts. Joe brought co-streaming forward from Support on October 5, 2026: up to four players, accepted invitations and separate or shared moderated chat. Payment features remain in Module 6. See [Guilds](GUILDS.md) for acceptance checks.
