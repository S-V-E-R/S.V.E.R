# Fastmarch

**Proposed October 19, 2026. Step 1 (the solo game on the 404 page, no records) built October 19, 2026; the rest is not scheduled.** A small racing game set inside MAGNet. Numbers marked **Proposed** are defaults Joe can change. Where the rest fits in the module order is Joe's call (see "Open questions").

Fastmarch takes its feel from light-trail racing games, following the lore rule: inspiration, not borrowing ([LORE.md](LORE.md)). Every name comes from S.V.E.R's own world.

## The idea

- **The rider is a courier.** In the lore, Ryn Fastmarch, the Courier, was the fastest runner of the war's messages, and her records stood for a century ([LORE.md](LORE.md), founders). Players are couriers carrying a signal through MAGNet. Couriers come from every faction; Ryn is the legend everyone chases.
- **The arena is MAGNet:** lanes of light that carry streams, joined at junctions. It is not an empty floor.
- **The hazard is the Static**, the lore's obscurity, where nobody is seen. Each courier leaves a trail in their faction color (Myria ember, Aetheron violet, Glint gold; steel when signed out). Hitting a trail, a wall or the Static ends the run: "Lost to the Static."
- **The story fits the moment.** A dropped stream or a missing page has fallen into the Static, and the courier runs the signal back out.

## Two versions

### 1. Solo Fastmarch (a standalone page game)

- **Where it appears:**
  - the 404 page ("This page was lost to the Static");
  - the offline page, once the site is an installable web app with a service worker ([COMMUNITY.md](COMMUNITY.md));
  - a channel's offline screen.
- **Play:** the player against two computer couriers, one per other faction. The player turns with arrow keys or WASD, or by swiping on phones. The game speeds up over time, and the score is time survived.
- **Not CrowdSync.** CrowdSync needs a live broadcast and a real playback session, and none exists on these pages. This version needs no account to play.
- **Records:**
  - Signed-in players can post a run to the board for a channel's offline screen and to a weekly board for each faction ("Aetheron holds the fastest run this week").
  - The all-time record is **Ryn's Mark**.
  - Guests play without posting.
- **Honest scores:**
  - The game is deterministic from a seed the server issues.
  - A posted run sends its seed and the tick of each turn, and the server replays it before accepting the score. A browser can't simply claim a time.
  - One post per account per minute (**Proposed**).

### 2. Crowd Fastmarch (a CrowdSync game)

- **Chat rides against the streamer.** The streamer drives one courier on their PC, and chat steers the other.
- **Built on what exists.** It's a game on the integration gateway using the JavaScript SDK, like Crowd Runner ([CROWDSYNC.md](CROWDSYNC.md), Part 2), and it reuses the solo version's track and drawing code.
- **Junction votes, not a joystick.** Viewers watch video 1 to 5 seconds behind the game, so live steering would always arrive late.
  - Chat's courier slows at each junction, and viewers vote left, straight or right.
  - The vote window closes by stream time, like polls, so CDN viewers get the same window as WebRTC viewers.
  - The most votes wins, and a tie goes straight.
- **CrowdSync's rules apply:** only real, signed-in viewers vote; rate limits, bans and the board panic switch all hold.
- **It ships as a board template** ("Fastmarch: chat rides against you"): the streamer adds one browser source and publishes the board.

## Rules

- **No coin buys the light.** Runs, wins and records never affect MAGNet's rotation, browse order or faction influence. That includes the weekly faction board, which is bragging rights only.
- **Rewards are cosmetic.** At most a small, daily-capped XP grant once Progression's levels are live, and perhaps a Ryn's Mark badge.
- **Light pages.** The solo game is a single canvas with no libraries. It loads only on the pages that show it, pauses when its tab is hidden, and never starts by itself: the player presses Play.
- **Accessible.** Keyboard and touch controls, a pause key, and a text label for the score. The faction colors aren't the only cue: each courier is named on the board.

## Build order

1. The solo game on the 404 page against computer couriers, with no records. **Built** (`lib/fastmarch.ts` with `lib/fastmarch.test.mts`, `components/Fastmarch.tsx`). It shows on the site 404 (`app/not-found.tsx`) and on the channel 404 (`app/[username]/not-found.tsx`), where most mistyped links land. A signed-out visitor picks a side to ride for; a signed-in player rides for their faction. On phones the board is small (60 × 36 cells across the screen width); a taller board for portrait screens can come if players ask.
2. Records and server replay, then the channel offline screen. The offline page comes when the service worker ships.
3. Crowd Fastmarch as a CrowdSync game and board template.

## Open questions for Joe

- **Glow.** [DESIGN.md](DESIGN.md) says ornament never uses glows. Do light trails inside the game canvas count? Step 1 uses flat, bright lines on navy with no bloom.
- **Placement.** Is the rest a standalone side project, part of Progression (Phase 2), or a CrowdSync addition?
- **Name.** Should the game be called "Fastmarch" or "the Courier's Run"?

## Done when

**Solo:**
- A visitor on a missing page plays against two computer couriers on desktop and on a phone.
- A signed-in player posts a run that the server replays and accepts, and a tampered run is refused.
- The channel and weekly faction boards show the best runs.
- Nothing changes MAGNet or influence.

**Crowd:**
- A streamer publishes the Fastmarch template and connects the game.
- Real viewers vote at junctions, and CDN and WebRTC viewers get the same window.
- Chat's courier follows the winning turn, and guests and banned viewers can't vote.
