# Design system

This is the source of truth for how S.V.E.R looks. Build every page against it. If a page needs something this file doesn't cover, follow its spirit and add the rule here in the same change.

Approved by Joe on October 2, 2026 (reference mockup: "S.V.E.R Homepage Concept", version 4). This file is self-contained; you don't need the mockup to build from it.

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

**Framed panel.** 1 px `--line` border, background `linear-gradient(180deg, var(--tint), rgba(0,0,0,.25)), var(--surface)`, and 2 px `--accent` L-shaped brackets on all four corners (12 to 18 px long; done with pseudo-elements or small absolutely positioned spans). Used for the player card, daily orders, the front-line banner, the featured carousel and the Beacons shelf. Square corners everywhere; no border-radius on panels or buttons.

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

## Copy

- Short, direct, specific. Say what something does ("Every live stream gets a turn here, whether it has 3 viewers or 3,000"), not slogans.
- No filler taglines or mood lines in the chrome ("A new chapter begins", "Stream • Connect • Belong").
- Never compare S.V.E.R to other platforms.
- Game vocabulary where it fits the war (Enlist, territory, orders, ally), plain words everywhere else.

## Not allowed

- Glows, neon edges, blurred light behind elements, glassmorphism.
- Gradient washes on panels or cards (besides the page tint, bevels and progress bars above).
- Rounded "soft" cards and pill buttons.
- Large decorative images or AI-generated art in the interface.
- Default framework palettes (Tailwind orange, stock blue).
