# Roadmap

S.V.E.R is built one module at a time. Each module is fully specified, built, and tested against its "done when" line before the next one starts.

| # | Module | What it delivers | Status |
| --- | --- | --- | --- |
| 0 | Foundation | The site foundation, shared interface, security boundaries and automated checks | Done |
| 1 | Login | Email and Google, Twitch, Discord sign-in; verification; sessions; 2FA for streamers | Done |
| 2 | Profiles | Channel pages, follows, War Council, the Wall, profile songs, schedules, sponsors and fan art | Done |
| 3 | Live streams | Creator Studio, OBS streaming, playback, chat and moderation, viewbot detection, custom emotes, go-live alerts, raids and hosting. Live delivery is still being prepared and verified. | In progress |
| 4 | Factions | Myria, Aetheron, Glint; the seasonal war over categories; the faction hub | Planned |
| 5 | MAGNet | Fair-rotation discovery, recommendations, stream-to-stream handoff | Planned |
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
- Urgent work in progress: [Take It Down](TAKE_IT_DOWN.md). The anonymous request/status forms, staff review, image quarantine and exact-copy blocking are implemented locally with three-year request-record retention. Removal emails now survive extended outages and security resets; staff can inspect notification outcomes and saved preservation records. Playback authorization has shipped independently: inactive or revoked streams cannot serve saved HLS URLs or start new WHEP playback. Real local SRS/FFmpeg checks also confirm staff removal stops publishers. Deployment and live acceptance of the complete request workflow remain pending; the request forms are not yet available on the public site. This takes priority over the remaining Live streams work.
- Second batch deployed: [Factions](https://sver.tv/factions) and the [live Roadmap](https://sver.tv/roadmap). The faction page explains the three sides, their lore, starting genres and planned war without enrolling accounts. The roadmap reads the running API's published module table and refreshes every 30 seconds while visible, retaining the last received progress with a warning during outages. Module status changes ship with the API; no frontend rebuild is needed.
- Browse and Categories arrive with MAGNet; the war map, enrollment and faction hubs arrive with Factions.
