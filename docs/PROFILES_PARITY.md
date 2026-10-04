# Profiles: legacy feature parity (Module 2)

Audit date: October 3, 2026. Sources: the legacy triage (`tmp/module2-legacy-triage.md`), the legacy code in `C:\Streaming` (profile pages and components, Creator Page Studio tabs, blocks, walls, user card, Prisma models), and the approved spec `docs/PROFILES.md`. Each user-visible legacy profile feature or setting gets one status:

- **SHIPPED**: in 2.0, with where it lives. "(built Oct 3)" marks items added in this audit. Evidence is either a public HTTP check of the deployed stage, a headless-Chrome check (on stage, or locally against the release source), or a code path.
- **STUB**: waits for a later module or phase, as the spec's "Later-module stubs" says.
- **ARCHIVED**: left out by an approved spec decision; the reason is given.
- **MISSING**: not in 2.0 and not in the spec. The audit found 9; Joe decided on October 3 (2:34 PM ET) to build all of them, so none remain (see the end). "(P1–P9)" marks those items, specified in `docs/PROFILES.md`, "Parity additions".

**Counts:** 72 SHIPPED (14 built during the audit and 8 parity additions, all on October 3), 24 STUB, 20 ARCHIVED, 0 MISSING. Total 116.

Stage spot-checks ran on October 3 against `https://sver.tv` with API `sitepages-live-20261003b` and web `m2parity-20261003b`. They used Joe's channel `/JoeTheChode` and `/Staffel`. The parity additions were checked on stage after web and API `m2p2-20261003` went live (signed out, headless Chrome, desktop and phone). The local browser check is `scripts/check-profile-parity.mjs`: 65 checks against synthetic local users, including P1–P9 and a no-uncaught-exceptions check. The link templates have a unit test, `scripts/link-templates.test.mjs`.


## Channel page & routing

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Root channel URL `/{username}`, case-insensitive with canonical casing | SHIPPED | `apps/web/app/[username]/page.tsx`, `GET /api/channels/{u}` | HTTP: `/staffel` 308 → `/Staffel`; tabs 200 |
| `/u/{name}` alias (keeps `?tab`) | SHIPPED | `apps/web/proxy.ts` (also `/s/`, `/@`) | HTTP: `/u/`, `/s/`, `/@` 308 |
| `/watch/{name}` | SHIPPED | `proxy.ts`; Module 3 now owns `/live` | HTTP: 302 → `/Staffel/live` (200) |
| Legacy `?tab=` tabs | SHIPPED | `proxy.ts` maps to sub-paths | HTTP: `?tab=wall` 308 → `/wall` |
| Tabs (Home / Wall / Schedule / About / Fan Art), replacing the header quick-link pills | SHIPPED | `apps/web/components/ChannelTabs.tsx`, one route per tab | HTTP: all 200; fan-art 404 while disabled |
| Not-found page for unknown creators | SHIPPED | `app/[username]/not-found.tsx` (same 404 for unknown, internal, deleted, held, restricted) | HTTP: unknown sub-path 404; `/admin`, `/support`, `/SVER` 404 |
| Page title, Open Graph, canonical | SHIPPED | `lib/channel.ts` `channelMetadata` | HTML: og:title/image/description, canonical, title `Joe (@JoeTheChode) | S.V.E.R` |

## Identity

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Display name | SHIPPED | `/settings/profile` (`profile-settings.tsx`) | API + HTML: Joe's display name |
| @username next to the name | SHIPPED | `ChannelFrame.tsx`, `UserChip.tsx` | HTML |
| Username change with cooldown | SHIPPED | `/settings/profile`; 60-day interval, 30-day hold, `/oldname` 302 (approved REWRITE) | Code path + API tests |
| Avatar upload | SHIPPED | Image pipeline, 64/160/400 WebP | API: 3 sizes; avatar URL 200 image/webp; renders at 112px |
| Banner upload | SHIPPED | Image pipeline (3:1); shown in the player slot | Code path; Joe has none |
| Bio ("Creator Intro") | SHIPPED | `/settings/profile`; plain text, 300 chars (approved PORT) | HTML |
| Mood emoji | SHIPPED | `/settings/profile`; one emoji | API: Joe's mood set |
| Status line | SHIPPED | `/settings/profile`; 80 chars | API: Joe's status set |
| Mood emoji picker with preset emojis | SHIPPED (P2) | `/settings/profile`: 12 presets plus Clear under the Mood field; any single emoji still works | Unit test (every preset passes `text::mood`); local browser: preset → save → shown on header |
| Schedule timezone (was in Basic Info) | SHIPPED | `/studio/channel/schedule` | Code path |

## Header

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Follower / Following counts linking to lists | SHIPPED | `ChannelFrame.tsx` | API: 6/8; HTML: follower link |
| Follow button / "Log in to follow" | SHIPPED | `ChannelActions.tsx` | HTML: "Log in to follow" |
| Edit Profile button on own page | SHIPPED | `ChannelActions.tsx` (Edit profile + Creator Studio) | Local browser check |
| Share button (`navigator.share`) / copy link | SHIPPED (P1) | `components/ShareButton.tsx` in the channel header: native share on touch, copy link elsewhere, selectable fallback | Local browser: clipboard holds the canonical URL from `/wall`, share sheet, fallback; stage: button on every channel tab |
| Message button | ARCHIVED | Spec ARCHIVE: Subscribe/Tip/Message buttons | — |
| Subscribe button | STUB | Phase 2 (spec stub table) | Hidden |
| Support / Tip button | STUB | Phase 2 (spec stub table) | Hidden |
| Notification bell (per-follow go-live alerts) | STUB | Notification work / Module 3; legacy preferences not imported | — |
| LIVE badge, live title/category, auto-redirect to `/live` | STUB | Module 3 (spec stub table); M3 now serves `/{u}/live` | Offline label shown |
| Last Live stat | STUB | Module 3 | — |
| Online dot, "Watching @X", last seen | ARCHIVED | Spec ARCHIVE: presence | — |
| VERIFIED / FOUNDER badges | STUB | Phase 3 badges (spec stub table) | Not rendered |
| Cosmetic badges (BadgeDisplay) | ARCHIVED | Spec ARCHIVE: cosmetic badges | — |
| ProfileFrame avatar tiers and glow ring | ARCHIVED | Spec ARCHIVE: card frames | — |
| Faction badge and "{faction} hub" pill | STUB | Module 4; empty slot reserved | HTML: `faction-slot` present |
| Owner-faction page theme / colors | STUB | Module 4 (neutral steel for everyone in M2) | — |
| "Creator Page" / "Welcome to my page" pills, intro card, "Page Vibe" stat | SHIPPED (P9) | Studio → Page header (`/studio/channel/header`), `GET`/`PUT /api/me/header`; bordered pill and muted welcome line, intro panel on Home, "Vibe:" in the counts row; no gradients or glows | API tests (limits, filter, defaults, reset); local browser; stage: default pill on Joe's channel |
| Hosting banner ("hosting @X") | STUB | Module 3 (live state) | — |
| Ad slot | ARCHIVED | Spec ARCHIVE: ads | — |
| Claim / passport CTAs, migration banners | ARCHIVED | Spec ARCHIVE | — |

## Social links

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Social links, 13 platforms | SHIPPED | `/settings/profile`; one system, max 5 (approved REWRITE) | API: Joe's 5 links |
| Type a handle, URL built from a template | SHIPPED (built Oct 3) | `lib/types.ts` `linkUrl`; settings link field | Local browser check + unit test |
| Platform icon and host on each link | SHIPPED (built Oct 3) | `components/PlatformIcon.tsx` (site-styled marks), `ChannelFrame.tsx` | HTML: 5 marks + hosts youtube.com, kick.com, twitch.tv, tiktok.com, discord.gg |
| Linked-account (OAuth) one-click prefill | SHIPPED (P5) | `GET /api/me/link-suggestions`; "From your linked accounts" in `/settings/profile` adds a row, the user saves | API tests (only linked providers, never an existing platform); local browser |
| "Also known as" (linked platform accounts on the user card) | SHIPPED (P6) | Opt-in `profiles.show_linked_accounts`, off by default and never set by the import; shown only on the user card | API tests (off by default, import off, unlink, blocks); stage: 0 accounts opted in |

## Follows

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Follow / unfollow | SHIPPED | `PUT/DELETE /api/follows/{u}` | Code path; local browser check |
| Following page `/following` | SHIPPED | `app/following/page.tsx` | HTTP: 307 → /login when signed out |
| Unfollow per row + empty state on `/following` | SHIPPED (built Oct 3) | `components/FollowingList.tsx` | Local browser check |
| Public follower / following lists | SHIPPED | `app/[username]/followers`, `/following` | HTTP 200 |
| Follow date on list rows | SHIPPED (built Oct 3) | `components/People.tsx` | HTML: 6 rows with dates |
| Live-first ordering on Following | STUB | Module 3 | — |

## War Council

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Pick up to 8 (search, add, reorder, remove) | SHIPPED | `/studio/channel/war-council` | API: Joe's 6 positions |
| 4x2 square-tile grid (2 columns on phones), crown on #1 | SHIPPED (built Oct 3) | `app/[username]/page.tsx` `CouncilTile`, `profiles.css` | HTML: 6 tiles + crown; browser: 4 columns desktop, 2 phone |
| Faction tile colors | STUB | Module 4 | — |
| TOP8 block and `topStreamerIds` duplicate | ARCHIVED | Spec: redundant copies dropped | — |

## Profile song

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| YouTube / SoundCloud song with official embed | SHIPPED | `components/SongPlayer.tsx`; oEmbed title, artist, re-hosted thumbnail | API: Joe's YouTube song |
| Starts on the visitor's click at the owner's default volume | SHIPPED (built Oct 3) | `SongPlayer.tsx` (`volumeMessages`) | Client build contains it; local browser check confirms the volume message |
| Fetch details button | SHIPPED (built Oct 3) | `/studio/channel/song`, `POST /api/me/song/preview` | Local browser check |
| Edit title and artist | SHIPPED (built Oct 3) | `/studio/channel/song` | Local browser check |
| Default volume slider | SHIPPED | `/studio/channel/song` (default 70) | API: volume 70 |
| Autoplay on page load (toggle) | ARCHIVED | Approved spec: the song never autoplays | — |
| Album-art URL | ARCHIVED | Spec: no external image URLs; thumbnail re-hosted from oEmbed | — |
| Direct audio URL, yt-dlp proxy, scraper, visualizer | ARCHIVED | Spec ARCHIVE | — |
| Spotify link card | ARCHIVED | Embeds limited to YouTube/SoundCloud; owner sees a notice | Code path |

## Wall (guestbook)

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Posts and one-level replies | SHIPPED | `components/Wall.tsx`, `app/[username]/wall` | HTML: wall preview + "Sign the Wall" |
| Like reaction | SHIPPED | `Wall.tsx` | Code path |
| VALOR reaction | STUB | Module 4 (imported VALOR became Like) | — |
| Who can post: anyone / following / mutual / none | SHIPPED | `/studio/channel/wall` | Code path |
| Who can post: subscribers | STUB | Phase 2 | Hidden |
| Require approval, auto-hide links, auto-hide new accounts | SHIPPED | `/studio/channel/wall` (`require_approval`, `hold_links`, `hold_new_accounts`) | Code path |
| Pending queue (approve / reject / block) | SHIPPED | `/studio/channel/wall`; pending count in Studio | Code path |
| Pinned posts | SHIPPED | Up to 3 (approved; legacy allowed 1-10) | Code path |
| Delete posts and replies | SHIPPED | `Wall.tsx` | Code path |
| Links in posts are clickable | SHIPPED (built Oct 3) | `components/Text.tsx` `Linkified` | Local browser check |
| Relative time with exact-time tooltip | SHIPPED (built Oct 3) | `Text.tsx` `Ago` | HTML + headless Chrome on stage: "3 months ago | Jul 9, 2026, 12:13 AM EDT" |
| Wall-post notifications (`wallMutedByDefault`) | STUB | Notification work | — |
| `WallMessage` model | ARCHIVED | Spec ARCHIVE | — |

## Schedule

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Weekly blocks and one-off events | SHIPPED | `/studio/channel/schedule`, `app/[username]/schedule` | HTTP: tab 200, empty state; stage has 0 schedules |
| Next streams on Home | SHIPPED | `components/Schedule.tsx` `Occurrences` | Code path |

## Sponsors

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Sponsors: logo, description, link, category, active toggle | SHIPPED | `/studio/channel/sponsors`, About tab | Code path; stage has none |
| Discount code with copy button | SHIPPED (built Oct 3) | `components/CopyButton.tsx` | Local browser check |
| `sponsored nofollow` on sponsor links | SHIPPED (built Oct 3) | `app/[username]/about/page.tsx` | Local browser check |

## Streaming setup

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Gear list with links (peripherals) | SHIPPED | `/studio/channel/setup`, About tab; since Oct 3 (4:52 PM ET) picked through the setup parts picker, with custom entries for anything not listed | API tests and local browser check (parts picker) |
| CPU / GPU / RAM / storage specs | SHIPPED | Parts picker categories CPU, GPU, RAM and Motherboard (storage is out by Joe's decision of Oct 3; older entries such as PC stay under Other) | API tests (migration keeps all 14 old categories) |
| Grouped by category | SHIPPED (built Oct 3) | `about/page.tsx` with `groupByCategory` (`lib/setup-parts.ts`), fixed picker order, Other last | Local browser check |
| Setup photos (up to 3, lightbox), setup title and description | SHIPPED (P4) | `/studio/channel/setup`; media kind `setup_photo` (400/1600 WebP), report target `setup_photo` with Remove/overturn like fan art; the 1600 px image opens in a new tab instead of a lightbox | API tests (limits, moderation, appeal, reset, erasure); origin upload route 401 (not 413); uploads through Cloudflare need the WAF rule change |

## Blocks

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| ABOUT, PANEL, QUOTES, GAME_SHELF | SHIPPED | `/studio/channel/blocks`; About tab | Code path |
| Block on/off toggle and ordering | SHIPPED | `/studio/channel/blocks` | Code path |
| Game shelf box art, status, note | ARCHIVED | Approved spec: names only | — |
| Quote context | SHIPPED | As the quote attribution | Code path |
| SOCIAL_LINKS, TOP8, SCHEDULE, SONG, SPONSORS, FAN_ART, STREAMING_SETUP blocks | SHIPPED | Now built-in sections | See rows above |
| FOLLOW_STATS block | SHIPPED | Header counts | HTML |
| STREAM_STATUS block | STUB | Module 3 | — |
| HIGHLIGHTS block | STUB | Module 6 | — |
| SUBSCRIBE block | STUB | Phase 2 | — |
| BADGES, TROPHY_CASE blocks | STUB | Phase 3 | — |
| GOAL_BAR block | ARCHIVED | Approved spec: outside the reduced block set; no later owner | — |

## Fan art

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Gallery | SHIPPED | `app/[username]/fan-art` (full size opens in a new tab instead of a lightbox) | HTTP: 404 while disabled (as specified); stage has 0 items |
| Submit form with permission checkbox | SHIPPED | Fan Art tab | Code path |
| Owner approval queue | SHIPPED | `/studio/channel/fan-art`; pending count | Code path |

## User card

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Card: avatar, name, bio, followers, joined, Follow, View channel, Block/Report | SHIPPED | `components/UserChip.tsx`, `GET /api/users/{u}/card` | API shape OK |
| Bottom sheet on mobile | SHIPPED (built Oct 3) | `profiles.css` + Close button | Local browser check (phone) |
| Live dot and Watch live | STUB | Module 3 (slot wired) | — |
| Moderator section | STUB | Module 3 | — |
| Card stat/badge customization page, featured stats and badges, Stats Visibility toggles | ARCHIVED | Spec ARCHIVE: featured-stats pickers | — |

## Creator Page Studio

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Creator Studio with per-section pages and "View channel" | SHIPPED | `/studio/channel/*` | HTTP: 307 → /login when signed out |
| Unsaved-changes / last-saved indicator, Preview | SHIPPED | Each section saves on its own with a status message; changes appear at once; "View channel" | Code path |
| Page Readiness checklist / profile completion banner | SHIPPED (P3) | `/studio/channel` overview checklist (7 steps) and a dismissible banner on Studio channel pages; owner only | API tests; local browser (banner, Dismiss, restore, absent from the channel page) |

## Other channel content

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Activity feed | SHIPPED (P7) | "Recent activity" on Home, `GET /api/channels/{u}/activity`; follows, wall posts, War Council, song and schedule changes; kind registry in `activity.rs` for Module 5 | API tests (blocks, restrictions, internal, erasure, pagination); stage: API 200, empty until new events |
| Similar streamers | STUB | Module 5 | — |
| Featured video, trailer, pinned and recent content | STUB | Module 6 | — |
| Beacon tab | STUB | Module 7 | — |
| `/{u}/rewards` | STUB (P8) | Reserved sub-path with "Rewards are coming", no tab; filled by the economy (Module 4 / Phase 2) | Stage: 200 with the placeholder, no Rewards tab, unknown name 404 |
| Shop / storefront tabs and pages | ARCHIVED | Spec ARCHIVE | — |
| Posts / Community tabs | ARCHIVED | Spec ARCHIVE | — |
| LFG / open-to-collab status | ARCHIVED | Spec ARCHIVE | — |
| Raid settings, chat-popout settings | ARCHIVED | Spec ARCHIVE | — |
| Vanity `@slug` URLs, name negotiation/escrow/decay | ARCHIVED | Spec ARCHIVE | — |

## Safety

| Legacy feature or setting | 2.0 status | Where / reason | Evidence |
|---|---|---|---|
| Blocking (legacy per-channel blocks) | SHIPPED | Site-wide blocks, `/settings/blocked` | Code path |

## Did not exist in legacy

Legacy profiles had none of the following, so there is nothing to carry over:
- profile view counts or a visitors list
- user-chosen profile themes or colors (the only theme was the owner's faction colors; see Header)
- custom CSS
- free-form layout options beyond block order (block order is SHIPPED)

VOD view counts belong to Module 6. The guestbook is the Wall (SHIPPED). The setup parts picker (SVER's own parts list plus the staff review queue at `/admin/parts`) is new and has no legacy counterpart; see docs/PROFILES.md, "Setup parts picker".

## MISSING: resolved

The audit's 9 MISSING items went to Joe with recommendations. On October 3, 2026 (2:34 PM ET) he decided to build all 9 before closing Profiles. Their rules, edge cases and acceptance tests are in `docs/PROFILES.md`, "Parity additions" (P1–P9). Eight are SHIPPED and `/{u}/rewards` is a defined STUB, all deployed to stage in `m2p2-20261003`.

| Item | Decision | Status |
|---|---|---|
| Share / copy-link button on the channel header | P1 | SHIPPED |
| Mood emoji picker with presets | P2 | SHIPPED |
| Page Readiness checklist / profile completion banner | P3 (owner only, dismissible) | SHIPPED |
| Setup photos (up to 3) and setup title/description | P4 (new media kind, reports and moderation like fan art) | SHIPPED; uploads through Cloudflare wait on the WAF rule change |
| OAuth linked-account prefill for social links | P5 (suggest only, the user confirms) | SHIPPED |
| "Also known as" on the user card | P6 (opt-in, off by default, never on for imported users) | SHIPPED |
| Activity feed on the channel | P7 (Module 2 events; Module 5 adds kinds) | SHIPPED |
| `/{username}/rewards` | P8 | STUB |
| Decorative header copy | P9 (owner-editable, defaults) | SHIPPED |
