# Motion: the welcome ceremony and the season reveal

Approved by Joe on October 8, 2026. Two signature moments get choreographed animation with GSAP. Everything else stays CSS, per [DESIGN.md](DESIGN.md): pages stay light, decoration is CSS and inline SVG, and there are no glows. Three.js isn't used. A 3D canvas would compete with the video player for the GPU, and it would need a separate accessible version of anything it draws.

**When:** with the channel and transparency additions ([CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md)), after VODs and clips (Module 8) closes. The license step below comes first.

## Why GSAP, and only here

- Both moments are timed sequences: several elements, in order, with overlaps. That's where CSS keyframes get brittle and GSAP's timelines stay readable.
- GSAP is loaded with `import("gsap")` inside these two components only. Its few kilobytes don't enter the shared bundle, and no other page downloads it.
- Later candidates, each needing its own approval: a raid arriving and CrowdSync board effects. Until then they stay CSS.

## License step (before installing)

- **GSAP's terms:** GSAP has been free for all uses, including commercial, since April 30, 2025, under Webflow's "Standard 'No Charge' GSAP License".
- **The conflict:** that license is not an open-source license. It forbids using GSAP in visual animation builders that compete with Webflow. S.V.E.R is AGPL-3.0, so shipping GSAP inside our front-end bundle needs an **additional permission under AGPL section 7**. It lets S.V.E.R and its forks combine the code with GSAP under GSAP's own terms.
- **Before installing:**
  1. SVER LLC, as the copyright holder, adds that permission to `LICENSE` and `README.md`.
  2. A lawyer should glance at the wording. (Claude isn't one.)
  3. GSAP is added as an npm dependency, never copied into the repo.
  4. It's listed on the credits page with its license.
- **Fallback:** if Joe would rather not add an exception, use **Motion** (motion.dev, MIT-licensed) instead. It has sequences and springs and needs no license change. The rest of this spec applies to either library.

## Rules for both moments

- **The page works without the animation.** The server renders the final state. The animation only plays it in. Without JavaScript, or if the library fails to load, the content is already there.
- **Reduced motion:** with `prefers-reduced-motion: reduce`, nothing moves. The final state shows at once, with at most a 150 ms fade.
- **Short and skippable:**
  - At most 3 seconds.
  - **Skip** (and Escape) jumps to the end.
  - Buttons work from the first frame, and focus goes to the main heading.
- **Cheap to draw:**
  - Animate only `transform` and `opacity`. No filters, blur, canvas or WebGL.
  - Clip-path and stroke-dashoffset are allowed on small inline SVGs.
  - It must hold 60 fps on a 4× CPU-throttled laptop profile.
- **Design rules still apply:**
  - No glows, light bloom or gradient washes.
  - Square corners.
  - Theme tokens for every colour. A faction's colours are used only when it's that faction's moment.
- **Screen readers:** a reader hears the final text once, never letters or lines as they arrive. Split text stays in one accessible label.
- **Sound:** none. Streaming audio is often playing in another tab.

## 1. The welcome ceremony (`/welcome`, the "Welcome to {faction}" step)

The moment someone enlists. It runs in the faction's theme. The sequence is about 2.6 seconds, with overlaps:

| Time | What happens |
| --- | --- |
| 0.0 s | The panel's corner brackets draw in from each corner. |
| 0.2 s | The crest rises 16 px and fades in, with a slight scale from 0.92. |
| 0.5 s | The relic mark draws behind the crest as a thin inline-SVG line (see below). |
| 0.8 s | "Welcome to {faction}" rises line by line. |
| 1.3 s | Title and creed fade in, then the faction's line from [LORE.md](LORE.md). |
| 1.7 s | The lore paragraph fades in. |
| 2.1 s | The four "what changes now" tiles step in, 80 ms apart. |
| 2.4 s | Continue and the battle cry appear. |

The relic mark is drawn with `stroke-dashoffset` and fades to 20% opacity:
- **Myria:** a rising flame.
- **Aetheron:** a crescent with three stars.
- **Glint:** a crown's outline.

It plays once per enlistment, not on Back and forward. A faction switch between seasons plays it again for the new faction.

**Copy fix in the same change.** The "Your side" tile still says the war, map and hub "arrive with Season 1". They're live now, so the tile links to the faction hub and the war map.

## 2. The season reveal (home, once per season)

The first time someone opens the home page after a season ends, the front-line banner opens into a reveal before settling into its normal form. The data is real: `war.season.finished`, `winners` and the final scoreboard.

| Time | What happens |
| --- | --- |
| 0.0 s | The banner expands to about 280 px tall. "Season {n} · {year} AF" fades in. |
| 0.3 s | The three crests drop into a row. |
| 0.6 s | The standings bar fills left to right to the final territory counts, with the numbers counting up. |
| 1.6 s | The winning crest lifts and its faction colour fills the top rule. The line reads "{Faction} holds the light." |
| 2.1 s | The previous champions line and "Next season starts {date}" appear. |
| 2.6 s | The banner settles back to its usual height. |

- **Ties:** joint winners lift together, and the line reads "{A} and {B} share the light."
- **Viewer-specific lines:**
  - A viewer whose faction won also sees "Your side won." and a link to their faction's season rewards.
  - Others see "Next season, the map is redrawn."
- **Once per season:** remembered per browser in `localStorage`, under a key containing the season number. If storage is unavailable, it plays once per visit. A replay button in the banner plays it again.
- **Placement:** it never covers the page. The rotation and Live now stay where they are and usable throughout.

## Done when

1. GSAP (or Motion) is loaded only on `/welcome` and on home during a reveal. Every other route's JavaScript is unchanged in the build output.
2. Both sequences match the timings above (±100 ms), run within 3 seconds, and Skip and Escape jump to the end.
3. With reduced motion, or with JavaScript off, the final state shows at once and the page is fully usable.
4. Screen readers hear each line once. Focus starts on the heading, and buttons work during the animation.
5. Both hold 60 fps on a 4× CPU-throttled profile, using only transform and opacity (plus dash offsets on small SVGs).
6. The reveal uses real season results, handles joint winners, and plays once per season per browser.
7. The license step is done before the dependency lands, and the credits page lists the library.
