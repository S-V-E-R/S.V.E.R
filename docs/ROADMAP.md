# Roadmap

S.V.E.R is built one module at a time. Each module is fully specified, built, and tested against its "done when" line before the next one starts.

| # | Module | What it delivers | Status |
| --- | --- | --- | --- |
| 0 | Foundation | The site foundation, shared interface, security boundaries and automated checks | Done |
| 1 | Login | Email and Google, Twitch, Discord sign-in; verification; sessions; 2FA for streamers | Done |
| 2 | Profiles | Channel pages, follows, War Council, the Wall, profile songs, schedules, sponsors and fan art | Done |
| 3 | Live streams | Creator Studio, OBS streaming, playback, chat and moderation, viewbot detection, custom emotes, go-live alerts, raids and hosting. Live delivery is still being prepared and verified. | In progress |
| 4 | Factions | Myria, Aetheron, Glint; membership, weekly genre checkpoints, seasonal rewards, private councils and faction hubs | Done |
| 5 | MAGNet | Fair-rotation discovery, recommendations, stream-to-stream handoff | In progress |
| 6 | Support | Subscriptions, Valor and tributes, channel rewards, co-streams, payouts | Planned |
| 7 | CrowdSync | Interactive boards, Skills, polls and predictions, faction rallies | Planned |
| 8 | VODs and clips | Past broadcasts and clipping | Planned |
| 9 | Beacons | Short vertical videos that lead to live streams | Planned |

Modules 1 to 5 make the site usable. After all nine: Progression (levels, XP, daily orders) and the remaining anti-abuse systems.

The numbered table is also the public roadmap feed. The API embeds it at build time and serves it at `GET /api/roadmap` with `no-store`; the website server-renders it and refreshes every 30 seconds while visible. Update this table when a module starts or passes acceptance, then deploy the API. The page follows that release without a frontend rebuild. Unreleased local edits never appear as live progress. Keep rows 0–9 in order and statuses `Done`, `In progress`, `Started` or `Planned`; the backend tests reject an incomplete or malformed table.

## Site pages

Static and informational pages run alongside the modules. They do not close or reopen a module.

- First batch deployed: About, Help & FAQ, Community Guidelines, Terms of Service, Privacy Policy, Copyright & DMCA, and Contact. Layouts follow [the design system](DESIGN.md): self-hosted Cinzel/Barlow fonts, centered information pages, legal summaries and contents lists, shared navigation and footer. Legacy text is updated for the current content rules and available features. Help uses native expandable answers and includes account recovery, profiles, OBS setup and support.
- Before launch: legal review of the adapted text, confirmation of public contact inboxes, and verification/publication of the complete registered copyright-agent details.
- [Take It Down](TAKE_IT_DOWN.md) is deployed: public request/status forms, staff review, image quarantine, exact-copy blocking, removal notices and playback revocation. The production purge credential is configured; requester/staff email and enrolled-browser push have reached provider acceptance. Full live removal/restoration acceptance, legal/process review and video-frame matching remain open.
- Live-stream chat now has mentions, replies, persistent pins, Broadcaster/Moderator/Staff badges and channel emotes, with Creator Studio uploads and staff media review. The remaining module scope and acceptance work are tracked in [Live streams](LIVE_STREAMS.md).
- Second batch deployed: [Factions](https://sver.tv/factions) and the [live Roadmap](https://sver.tv/roadmap). The faction page explains the three sides, their lore and starting genres; Module 4 adds enrollment, live standings and private member hubs. The roadmap reads the running API's published module table and refreshes every 30 seconds while visible, retaining the last received progress with a warning during outages. Module status changes ship with the API; no frontend rebuild is needed.
- Browse and public category discovery arrive with MAGNet. Faction enrollment, the war map, hubs, weekly board elections and staff genre/category tools are implemented.
- The public homepage and focused watch page follow the shared design: real live/recent channel shelves, a manual spotlight carousel, direct watch links, and a responsive player/streamer/chat layout. Search, weighted MAGNet rotation and recommendations, Beacons, and clips remain with their planned modules; their availability is stated in the interface.

- [SVER Plays](https://sver.tv/sverplays/live) is restored at the founder’s request: a dedicated 24/7 live-testing channel with viewer game controls, using the separate game runtime. This does not start the general CrowdSync module.
- Guilds is implemented after Factions: cross-faction teams, applications, emblems, follows, combined schedules, staff review and team shortcuts. Joe brought co-streaming forward from Support on October 5, 2026: up to four players, accepted invitations and separate or shared moderated chat. Payment features remain in Module 6. See [Guilds](GUILDS.md) for acceptance checks.
