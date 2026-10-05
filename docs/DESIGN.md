# Design system

This is the source of truth for how S.V.E.R looks. Build every page against it. If a page needs something this file doesn't cover, follow its spirit and add the rule here in the same change.

Approved by Joe on October 2, 2026 (reference mockup: "S.V.E.R Homepage Concept", version 4). This file gives the rules; the rendered mockups in [design/README.md](design/README.md) show the approved layout of each page. Build pages to match both.

## The idea in one paragraph

S.V.E.R looks like a game client, not a generic streaming site. The layout is the familiar streaming structure (top bar, left sidebar, rows of content) so nobody has to learn it, but the chrome has personality: framed panels with corner brackets, beveled metal buttons, Cinzel headings, and progression on screen. **The whole site takes the color of the signed-in user's faction.** Signed out, it's neutral steel. Ornament is done in CSS and inline SVG only: no glows, no gradient washes, no large decorative images. Pages must stay light on low-end PCs and slow connections.

## Themes

The server sets `data-theme` on `<html>` from the signed-in user's faction: `myria`, `aetheron`, `glint`, or `neutral` when signed out or when the user has no faction yet. It is server-rendered, so there's no flash of the wrong theme. Components use only the tokens below, never raw faction colors, except where they show *another* user's faction (a streamer's crest, an Ally or Enemy turf tag), which uses the fixed faction colors.

| Token | neutral | myria | aetheron | glint | Used for |
| --- | --- | --- | --- | --- | --- |
| `--accent` | `#C9CED6` | `#FF9A1F` | `#A68BFF` | `#E9C35A` | Corner brackets, active nav bar, focus ring, progress fill, links |
| `--accent-light` | `#ECEEF2` | `#FFC56E` | `#D3C6FF` | `#F8E6A8` | Logo, headings, bevel highlight, outline-button text |
| `--accent-dark` | `#5A606C` | `#8A4A06` | `#4B2A9A` | `#7E5F14` | Bevel shadow, progress start |
| `--accent-deep` | `#12151C` | `#2A1604` | `#1C1038` | `#0A1A3C` | Top-of-page tint behind the content |
| `--line` | accent at 32% opacity | | | | Panel borders, rules |
| `--tint` | accent at 7% opacity | | | | Panel fill tint |

Shared tokens, the same in every theme:

| Token | Value | Used for |
| --- | --- | --- |
| `--bg` | `#07080B` | Page background |
| `--surface` | `rgba(8, 9, 13, 0.85)` | Panel base |
| `--ink` | `#F2F0EA` | Body text |
| `--ink-muted` | `#A3A6AE` | Secondary text |
| `--ink-dim` | `#8E929B` | Labels, timestamps |
| `--live` | `#E5262E` | LIVE tag only |
| `--myria`, `--aetheron`, `--glint` | `#FF9A1F`, `#A68BFF`, `#E9C35A` | Other users' faction marks |

Page background: `radial-gradient(90% 60% at 50% 0%, var(--accent-deep), var(--bg) 70%) var(--bg)`. This is the one gradient allowed besides button bevels and progress bars.

Faction crests live in `apps/web/public/factions/{myria,aetheron,glint}.webp` (256 px). Use them at 18 to 56 px as identity marks. Never redraw them, add glows, or use them as large background art on content pages.

## Type

Load fonts with `next/font/google` so they're self-hosted and subset. No `@import` of Google Fonts in CSS, and no Inter, Roboto or Arial as the body face.

| Role | Font | Weight | Size |
| --- | --- | --- | --- |
| Logo, section headings, buttons | Cinzel | 700 to 800 | Logo 28 to 30 px; section headings 22 to 24 px; buttons 15 to 16 px with 0.1em tracking |
| Labels, tags, counts | Barlow Condensed | 600 to 700 | 11 to 16 px, uppercase, 0.1 to 0.2em tracking |
| Body and UI text | Barlow | 400 to 600 | 15 px base, 1.45 to 1.5 line height |

## Components

**Framed panel.** 1 px `--line` border, background `linear-gradient(180deg, var(--tint), rgba(0,0,0,.25)), var(--surface)`, and 2 px `--accent` L-shaped brackets on all four corners (12 to 18 px long). Used for the player card, daily orders, the front-line banner, the featured carousel and the Beacons shelf. Square corners everywhere; no border-radius on panels or buttons. One way to draw all four brackets with a single pseudo-element:

```css
.frame { position: relative; }
.frame::before {
  content: ""; position: absolute; inset: -1px; pointer-events: none;
  --b: linear-gradient(var(--accent), var(--accent));
  background:
    var(--b) top left / 16px 2px no-repeat,    var(--b) top left / 2px 16px no-repeat,
    var(--b) top right / 16px 2px no-repeat,   var(--b) top right / 2px 16px no-repeat,
    var(--b) bottom left / 16px 2px no-repeat, var(--b) bottom left / 2px 16px no-repeat,
    var(--b) bottom right / 16px 2px no-repeat, var(--b) bottom right / 2px 16px no-repeat;
}
```

**Section heading.** Cinzel 800, `--accent-light`, followed by a small `--accent` diamond (8 px square rotated 45°), a short `--ink-dim` description, and a 1 px `--line` rule filling the rest of the row, with a "View all" link at the end in Barlow Condensed.

**Primary button (beveled).** Background `linear-gradient(180deg, var(--accent-light) 0%, var(--accent) 45%, var(--accent-dark) 100%)`, 1 px `--accent-light` border, `box-shadow: inset 0 1px 0 rgba(255,255,255,.4), inset 0 -2px 0 rgba(0,0,0,.35)`, text `#0A0A0C` in Cinzel. Minimum 44 px tall.

**Outline button.** Transparent dark fill (`rgba(0,0,0,.35)`), 1 px `--accent` border, `--accent-light` Cinzel text. Pressed or selected state fills with `--accent`.

**Tags** (Barlow Condensed 700, uppercase, tight padding, no radius):
- `LIVE`: `--live` background, white text.
- `ALLY`: `--accent` background, dark text. Shown on streams from the viewer's own faction.
- `ENEMY TURF`: dark background, text in the color of the faction that holds the category.
- Viewer count, duration: dark translucent background, `--ink` text.

**Stream card.** 16:9 thumbnail with a 1 px border (`--accent` when it's an ally stream, otherwise `rgba(242,240,234,.08)`), tags in the corners, then the streamer's crest (32 px), a two-line title, streamer name and a category chip.

**Progress bar.** Dark track, fill `linear-gradient(90deg, var(--accent-dark), var(--accent))`, no glow.

**Icons.** Inline SVG strokes (`currentColor`), about 18 px. No emoji and no Unicode symbol glyphs (◈ ✦ ⌁) as icons.

## Layout

**Top bar:** Cinzel S.V.E.R logo in `--accent-light` → flexible space → search field (channels, categories, Beacons) → flexible space. Signed in: Valor balances (Phase 2), notifications button with a count badge, and the player chip (crest, username, "Lv 14 · Veteran"). Signed out: Log in, and an "Enlist" primary button.

**Left sidebar (about 268 px; hidden below 960 px):**
1. Player card (signed in): crest with a hex level badge, username, faction title, XP bar. Levels and XP arrive with Progression in Phase 2; until then the card shows the crest, name and faction.
2. Navigation: Home, Browse, Beacons, War map, and the user's faction hub. The active item gets a 3 px `--accent` left bar and a `--tint` background.
3. Daily orders panel (Phase 2).
4. "Following · live" when signed in, "Picked for you" when signed out, marked with the MAGNet mark: three 4×10 px bars in the three faction colors (M.A.G. = Myria, Aetheron, Glint).

**Home, in order:** the front-line banner (faction standing and a call to action written for the viewer's faction; "Pick a side" when signed out) → MAGNet rotation carousel → Live now → Beacons shelf (9:16 cards) → Just went live → Territories (categories, each with the holding faction's crest) → Latest clips → footer.

Factions are a hint on most pages: crests, Ally and Enemy turf tags, territory status and the front-line banner. The faction hub and war map are where they take center stage.

## Pages

Every page uses the same shell (top bar, sidebar, main column) unless noted. Utility pages (auth, settings, account, admin) are calmer: corner brackets only on the outer panel, no banners or tags.

**Landing (home, signed out).** The homepage above, with the signed-out variations: theme `neutral`, sidebar shows "Picked for you", and the front-line banner shows the season standing (a three-part bar), the three crests and an "Enlist" button. Clicking a crest goes to sign-up with that faction preselected. No separate marketing page: the first thing a visitor sees is live streams.

**Sign up, log in, verify, reset, 2FA, OAuth sign-up.** One centered framed panel, 440 px wide, Cinzel title, no sidebar; the top bar shows only the logo and a "Log in" or "Enlist" link. Sign-up is five steps shown as hex-numbered markers (Account, Your side, Profile, Follow, Ready; the onboarding wizard at `/welcome` runs steps 2 to 5). Step 1: provider buttons (Google, Twitch, Discord) as full-width outline buttons, then email, username (with the "sver.tv/username" preview), password (10+ characters), date of birth (13+, never shown) and the Turnstile check. Step 2, "Choose your side", widens to about 1000 px: three crest cards side by side, each with the faction name, epithet, creed and belief; the chosen faction's story appears below them (lore, who belongs, what it rejects, home turf, values). The button reads "Enlist in Myria", and the one-free-switch-in-7-days rule is stated under the cards. It can't be skipped. After enlisting, "Welcome to Myria": crest, title and creed, lore, and what changes now (colors, crest, the factions page, the switch rule). Only real, built benefits are listed. Step 3, Profile: avatar, display name and bio, skippable. Step 4, Follow: up to 8 channels, live first, then the viewer's faction, never ranked by counts, skippable. Step 5, Ready: faction and follow chips, the confirm-email message when unconfirmed (browse and watch now; chat and going live unlock after confirming), quick links, and "Enter S.V.E.R". Log in errors appear as a framed notice with a `--live` left bar and the remaining attempts.

**Channel page (`/username`).**
- Header: a wide banner (16:5) inside a framed panel; avatar (96 px) overlapping its bottom-left edge with the owner's crest beside it; display name in Cinzel, `@username` beneath, faction tag, mood/status line, social links, and Follow (primary) on the right with the follower count.
- If live, the player sits above the tabs with a LIVE tag; offline, the banner carries "Offline" and the next scheduled stream.
- Tabs in Barlow Condensed: Home, Wall, Schedule, About, Fan art (when enabled), Followers, Following. Active tab: `--accent` underline.
- Home tab: War Council (Top 8) as a 4×2 grid of small player cards (avatar, crest, name, live dot), the profile song as a compact one-line player, sponsors as a logo row, then the owner's custom blocks in framed panels.
- Wall: posts in framed panels, newest first, pinned posts on top with a pin mark; replies indented one level.
- **Colors (decided by Joe, October 3, 2026):** inside the channel area (banner frame, tabs, accents, channel panels) the page uses the *owner's* faction theme, so each channel is its owner's territory; the site chrome (top bar, sidebar, footer) stays in the viewer's theme. An owner with no faction yet uses neutral.

**Watch page.** Player first and largest (16:9), chat to its right at 340 px (below the player on narrow screens). Under the player: streamer bar (crest, name, title, category chip, faction tag, Follow), then "Up next" from MAGNet. Chat messages show the sender's crest (14 px) and name in their faction color; moderator actions in a small hover menu. When the stream ends, a framed overlay offers the next stream with a 10-second countdown and Cancel.

**Browse.** Genre tabs across the top (grouped by holding faction, each with its crest), then category tiles at 3:4 with the holder's crest in the corner and the live count. A category page lists its live streams in fair rotation, with a faction filter.

**Following.** Live channels first as stream cards, then offline channels as a compact list (crest, name, last live).

**Settings, account, Creator Studio.** Two columns: a section list on the left in a framed panel, forms on the right. Inputs: dark fill, 1 px `--line` border, `--accent` border on focus, labels in Barlow Condensed above the field. Destructive actions (delete account, regenerate stream key) use an outline button in `--live` and a confirm step. Studio shows the stream key behind 2FA, the recommended OBS settings, and warnings (bitrate too high, B-frames on) as framed notices with a `--live` left bar.

**Faction hub (`/factions/{faction}`).** The one place a crest is shown large (up to 160 px). Header with crest, name, epithet and creed in that faction's theme regardless of the viewer's; season standing; contribution leaderboard as player cards (rank, crest, name, influence); live streams from the faction; territories it holds.

**War map.** Genres as a hex map, clustered by holding faction, each hex edged in the holder's color with the lead percentage; contested hexes get a white edge and a CONTESTED tag. Beside it: standings per faction and a short numbered list of how ground is taken.

**Beacons feed.** One 9:16 video at a time, centered on desktop and full screen on phones, with a right-side rail: creator crest and name, like, view count, and Live now when the creator is streaming. Swipe or arrow keys move between Beacons.

**Info pages (About, Factions, Roadmap, Help).** No sidebar. Top bar with the logo, a short nav (Home, Browse, Factions, Roadmap, About; the current page underlined in `--accent`), Log in and Enlist. Content in a centered column (920 to 1240 px), opening with a small label, a Cinzel headline (50 to 54 px) and a one-paragraph lead, then sections with the standard section heading. End with a framed call-to-action panel. Factions shows each faction in its own colors in a three-column grid (crest 150 px, name, epithet, creed, values, who it's for, home turf, lore, Join button). Roadmap shows the modules on a vertical track with diamond nodes and a status chip (Done filled, In progress outlined bright, Started outlined, Planned dim), then later phases as cards.

**Legal pages (Terms, Privacy, Guidelines, DMCA, Take It Down).** Same top bar, tabs across the top to switch policy. Each policy: title, "Last updated · Effective" dates, a framed "The short version" box with 3 to 5 plain bullets and a note that the full text is what applies, then a sticky "On this page" list on the left and numbered sections on the right. The Take It Down tab is a request page instead: plain-language explanation, a framed "What happens next" list (confirmation, removal of the content and identical copies within 48 hours, follow-up), and a framed request form that works without an account.

**Footer (every page).** Logo, the footer tagline from [COPY.md](COPY.md), links to About, Factions, Roadmap, Help, Terms, Privacy, Guidelines, DMCA, **Take It Down requests** (required to be clearly visible from the homepage) and Contact, and "© SVER LLC".

While pages are being restored, navigation includes only working destinations. The information-page top bar offers Home, Factions, Roadmap, About and Help; Contact stays in the footer. Add Browse when it exists. Contact uses the information-page layout. Before Module 4, faction cards link to an explanation of the planned joining rules; signup does not select or reserve a faction. The information page uses 150 px crests as specified above, while marks elsewhere keep their smaller sizes. Faction cards use their own theme tokens without changing the surrounding site theme, and stack in one column below 960 px. Roadmap status is always written in text as well as distinguished by styling. This does not close the outstanding Take It Down request page and removal-process requirement. Legal contents lists become a normal list above the text below 960 px; keep the short summary and full policy available at every screen size.

**Errors and empty states.** Short and plain ("This channel doesn't exist." / "Nothing live right now."), inside a framed panel, with one useful next step. When nothing is live, show recently live channels and the war map, never a blank grid.

**Phones (below 960 px).** The sidebar becomes a drawer opened from a menu button in the top bar; grids drop to one or two columns; the watch page stacks player, streamer bar, then chat.

### Home and watch before the discovery modules

The homepage is public for guests and members; it never redirects to account security. Keep the approved section order. Until MAGNet, the featured frame is a manual **Live spotlight** carousel over real streams, ordered by start time, with an explicit note that rotation is coming. Previewing a card never starts a playback session. The live grid links directly to `/{username}/live`, and Just went live uses the API snapshot time and a one-hour window. When nothing is live, show recently live channels and the existing faction information page. Channel banners or category labels stand in for live thumbnails; never imply they are captured video frames.

The front-line banner links to the three faction explanations and offers Enlist to guests. Do not invent season standings, select a faction before enrollment exists, or show unearned XP, Valor or active daily orders. Beacons and Latest clips retain compact, plainly labeled empty sections with roadmap links until those modules supply content. Territories uses the real active category catalog and states that control opens with Factions. Search is visibly disabled until MAGNet.

The sidebar keeps the reference's structure in every theme: signed-in player card, Home / Browse / Beacons / War map / Faction hub, framed Daily orders, then Following · live or Picked for you with the full MAGNet wordmark. Before their modules, planned destinations are noninteractive rows marked Soon and Daily orders states that it is coming with Progression. Account tools (My channel, Following, Creator Studio, Settings, Account security) live in the top-bar player menu. Use real profile avatars until factions supply crests; live rows show the creator, category and actual viewer count. Do not manufacture faction membership, ranks, levels, XP or quest progress to fill the reference.

The focused watch page keeps a 16:9 player, the streamer identity and existing Follow/Share/report controls beneath it, a 340 px chat column, and an Up next shelf of other real live streams. Below 960 px, the order is player, streamer, chat, then Up next. Recommendations by genre/faction and the stream-end countdown remain part of MAGNet. Plays/CrowdSync controls are not reproduced before their systems exist. This layout work does not close or start those modules.

## Copy

- Short, direct, specific. Say what something does ("Every live stream gets a turn here, whether it has 3 viewers or 3,000"), not slogans.
- No generic filler taglines or mood lines in the chrome ("A new chapter begins", "Stream • Connect • Belong"). S.V.E.R's own taglines in [COPY.md](COPY.md) are the exception and are used where that file says.
- Never compare S.V.E.R to other platforms.
- Game vocabulary where it fits the war (Enlist, territory, orders, ally), plain words everywhere else.

## Not allowed

- Glows, neon edges, blurred light behind elements, glassmorphism.
- Gradient washes on panels or cards (besides the page tint, bevels and progress bars above).
- Rounded "soft" cards and pill buttons.
- Large decorative images or AI-generated art in the interface.
- Default framework palettes (Tailwind orange, stock blue).

### Module 4 activation

Faction membership supplies the site's server-rendered theme, player-card crest and hub navigation. The War map link is active; Browse and Beacons remain labeled with their planned modules. The front-line banner now uses real weekly ownership and a three-part season bar. Guest crest links preselect the signup choice, and signed-in members can review switching rules at `/choose-faction`. Channel content uses its owner's theme while the shell stays in the viewer's theme. No levels, XP or active daily orders are implied.

The map is inline SVG grouped by home region and colored by current ownership; selected tiles show balanced scores. Keyboard users can select a hex with Enter or Space. Phones use the complete genre board with standing bars. Hub data and the map refresh every 30 seconds while visible. Winning season banners use a small crest and CSS frame alongside the channel's existing uploaded banner.


### Dedicated Plays channel

The dedicated game channel adds a framed Play together panel below its streamer bar, with a keyboard-accessible directional pad, A/B/Start/Select buttons, live aggregate votes and a five-second countdown. Guests can watch; verified members can vote. Disconnected controls disable immediately. On phones, the pad and round status stack without shrinking the buttons. Its profile has a Watch and play link. This is the requested Plays restoration; the general CrowdSync controls remain separate.

### Guilds and co-streams

These routes have no dedicated mockup. They reuse the channel header, plain content panels, Creator Studio navigation, existing live player and chat patterns. Only the guild header uses corner brackets. The viewer's theme stays in place across a cross-faction guild; each member keeps their own crest. Guild emblems appear at 18 px after the faction crest in chat, with an accessible tag label and a text fallback.

The squad page uses a two-column stream grid and a 340 px chat column on wide screens. Chat follows the players on smaller screens, and players stack on phones. Each stream has a labeled audio button; selecting it mutes every other stream. Guild management uses native forms and disclosure controls. Screenshots from `scripts/check-teams.cjs` cover desktop and phone layouts, plus overflow checks at 320 px.
