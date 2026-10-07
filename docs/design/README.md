# Mockups

These are the approved page designs from the S.V.E.R design canvas, rendered at 1440 px wide. **Build pages to match them.** [DESIGN.md](../DESIGN.md) gives the rules (tokens, type, components). These pictures show how the rules come together on each page: layout, order, density, and where the framed panels go.

| Mockup | Route | Notes |
| --- | --- | --- |
| [main.webp](mockups/main.webp) (signed in, Aetheron) and [main-signed-out.webp](mockups/main-signed-out.webp) | `/` | Built: shell, front-line banner with the real season standing, MAGNet rotation, Live now, From your faction (signed in, per MAGNET.md), Just went live, Territories, Latest clips. The Beacons shelf joins after Live now with Module 9. The sidebar's level/XP and daily orders arrive with Progression (Phase 2). Following · live is in the sidebar rather than a home row. |
| [signup.webp](mockups/signup.webp), [signup-choose-side.webp](mockups/signup-choose-side.webp) | `/signup`, `/welcome` | The mockup's three steps grew into the onboarding wizard (legacy parity, approved October 4, 2026): Account, Your side (with each faction's story), the welcome to your faction, Profile, Follow, Ready. The email confirmation reminder is on Ready. Profile and Follow can be skipped; the side can't. |
| [login.webp](mockups/login.webp) | `/login`, `/mfa` | Full-width provider buttons. |
| [about.webp](mockups/about.webp) | `/about` | |
| [factions.webp](mockups/factions.webp) | `/factions` | |
| [roadmap.webp](mockups/roadmap.webp) | `/roadmap` | |
| [legal.webp](mockups/legal.webp) | `/terms`, `/privacy`, `/guidelines`, `/dmca`, `/take-it-down` | |
| [plays.webp](mockups/plays.webp) | Plays watch page | The controller board under the player ([PLAYS.md](../PLAYS.md)). |

## Rules for building UI

1. **Find the mockup first.** If the page has one, match it: the same sections in the same order, the same layout at 1440 px, the same components. Text can follow the current specs.
2. **Placeholders in the mockups are not real data.** Never show invented numbers (viewer counts, standings, XP). Leave a section out until its module supplies real data, and add it in the mockup's position when it does.
3. **No mockup?** Build from [DESIGN.md](../DESIGN.md), reuse the components and patterns of the closest mocked page, and say so in the pull request.
4. **Framed panels with corner brackets (`.frame`) are rare on purpose:** the player card, the front-line banner, the rotation carousel, the Beacons shelf, sign-in panels, channel headers and the faction hub header, plus the cards the info-page mockups draw framed (faction cards, roadmap and About cards, the closing call to action). Everything else, including stream tools under the player (Plays, CrowdSync, boards) and the war map, uses a plain `.panel` or no box at all.
5. **The site wears the viewer's faction colors.** Use the theme tokens (`--accent`, `--line`, `--tint`); never hard-code a faction color except in faction identity (crests, faction cards). Channel pages use the owner's faction inside the channel area.
6. **Don't show placeholders for features that don't exist yet** ("Soon" items, disabled links). Add destinations when they work.
7. **Every UI pull request includes screenshots** at 1440 px and 390 px of each page it changes, next to the matching mockup. A deliberate difference from a mockup is listed in the pull request and needs Joe's approval.

The canvas itself lives on claude.ai ("S.V.E.R Homepage Concept"). When Joe changes a design there, the pictures here are re-rendered in the same change that updates the code.
