# Module 4: Factions

Specified October 4, 2026 from the plan's decisions and Joe's answers that day. Builds after Live streams and its follow-on steps (Multistream, community additions). Guilds follow right after ([GUILDS.md](GUILDS.md)).

Three factions fight a seasonal war over the genres people stream. Every account belongs to one. The war is won by effort, not size or money.

## The factions

| | Myria | Aetheron | Glint |
| --- | --- | --- | --- |
| Title | The Vanguard | The Arcane | The Sovereign |
| Creed | Earn everything. Accept nothing. | Always learning. Never finished. | All are welcome. None are forgotten. |
| Values | Discipline, Conviction, Endurance | Curiosity, Mastery, Discovery | Belonging, Trust, Momentum |
| Color | `#FF9A1F` | `#A68BFF` | `#E9C35A` |

Copy, beliefs and lore follow [COPY.md](COPY.md) and the live Factions page. Glint is community-first: its legacy "merchant empire" lore, map names ("Glint Treasury", "Glint Bazaar") and emotes ("stonks", "coin") are rewritten around hosts, gatherings and belonging; the title "The Sovereign" stays. Artists belong to Aetheron. Crest art must be confirmed original before use (legacy notes said it was "inspired by" other games' emblems).

## Membership

- **Choosing a side shipped early** (October 4, 2026, with the UI rebuild): sign-up step 2, the one free switch within 7 days, the switch log, and faction colors and crests across the site. Accounts that existed before choose on their next visit (the sidebar and the home banner send them to the onboarding wizard at `/welcome`). Everything else in this spec still arrives with Module 4.
- **Every account has a faction**, chosen at sign-up step 2 ("Choose your side", [DESIGN.md](DESIGN.md)). This applies to viewers and streamers alike (legacy limited factions to creator accounts).
- Accounts imported from legacy keep their legacy faction; accounts with none choose on their next sign-in.
- **Switching:** one free switch during the first 7 days after choosing; after that, only in the gap between seasons. Every switch is logged.
- Switching never affects guild membership (guilds are cross-faction).
- Influence already earned stays with the faction it was earned for.

## Genres and the map

- **Genres** are groups of categories (games and creative types). Each category belongs to exactly one genre; staff manage the list in `/admin`, and a category can't move to another genre mid-season.
- **Season home turf** (from the Factions page):
  - Myria: FPS & battle royale; Fighting; Sports & racing; Speedrunning; Crafting & making.
  - Aetheron: RTS & MOBA; Strategy & 4X; Card & board; Puzzle & simulation; Art; Education & coding.
  - Glint: Community events; MMOs & RPGs; Co-op & party; Cozy & sandbox; Music.
  - Any other genre starts **neutral**.
- **The war map** is a RISK-style map: each genre is a territory, grouped into each faction's home region, colored by its current holder, with this week's standings when you hover or tap it. Drawn as a light inline SVG (no large images).
- On phones, and for anyone who prefers it, the same data shows as a **genre board**: one tile per genre with the holder's crest and live standing bars.
- Territories have neighbors on the map for flavor; at launch, adjacency has no rules attached. RISK-style rules (for example, only attacking neighboring territories) can be added later without changing the data.

## Influence

- **Sources:** streaming time, watch time, chat, and support. Effort-weighted: streaming and watch time count most, chat and support less. Support counts by the number of distinct supporters, never by dollars. Exact weights live in the private tuning config.
- **Who earns:** verified accounts with Trusted playback sessions only (Module 3 viewer integrity). Chat influence is capped per person per hour; every source has a per-person daily cap, counted in UTC inside the same database transaction as the award.
- **Where it counts:** in the genre of the stream's category. Viewers' watch time and chat count for their own faction in the genre they're watching.
- **Enemy turf:** streaming or watching in a genre held by another faction earns **1.5×**.
- **War council target:** each faction's members vote each week on one target genre; the winning target earns that faction **+10%** influence there for the following week. Votes and tallies are visible only to the faction's members, and voter identities are never shown.
- **No home-capital bonus**, no fortification, sieges or Valor-bought defenses (legacy systems dropped).
- Every award is a row in an append-only influence ledger with an idempotency key, so nothing is counted twice.

## Weekly checkpoints

- Each week (Monday 00:00 UTC) is its own battle. Standings show live all week; genres only change hands at the checkpoint.
- **Size balancing:** a faction's score in a genre is its influence there divided by its number of **active members** (made a trusted contribution in the last 14 days), with a minimum divisor (**Proposed:** 25) so a tiny faction can't win a genre with one person.
- **Flipping:** a challenger takes a held genre when its balanced score beats the holder's by more than the margin (**Proposed:** 5%). Ties and narrow leads go to the holder.
- **Neutral genres** are claimed by the first faction to reach a minimum influence there (tuning) and lead by the margin.
- Weekly influence resets after each checkpoint; the week's results are kept for history and the hub.

## Seasons

- Three months, with a short break between seasons (when switching is open).
- **Winner:** the faction holding the most genres at the final checkpoint. Ties go to the faction with the most genre-weeks held during the season.
- **Rewards:** members of the winning faction get a season badge and profile banner; the faction is featured on the homepage for the next season; bonus Valor once Valor exists (Module 6), credited then if earned earlier. Amounts live in tuning.
- **Rollover:** every genre returns to its faction's **home turf** for the new season; neutral genres return to neutral. All influence starts at zero.
- **Season 1** starts when this module launches; Joe sets the date. The 7-day free switch applies from each person's own choice.
- Season jobs are durable and replayable: a missed checkpoint or rollover runs once when the worker recovers, never twice.

## Faction hub

`/factions/{faction}` for members (other factions see a public version without the private parts):

- The war map or board, this week's standings and the season scoreboard.
- **This week's target** (war council vote, members only).
- **Contribution leaderboard:** members ranked by influence earned this week and season (effort-based; no spending ranks).
- **Suggested streamers:** live members of the faction, in fair rotation (MAGNet rules; never by viewer count).
- **Faction board:** members-only posts (up to 500 characters), reportable, with slow mode; staff and faction-chosen moderators can remove posts.
- Member directory, paged.

## Elsewhere on the site

- **Theme:** the site uses the signed-in user's faction colors; channel pages use the owner's ([DESIGN.md](DESIGN.md)).
- **MAGNet:** home-turf weight in homepage rotation for genres the viewer's faction holds ([MAGNET.md](MAGNET.md)). Hype channels stay faction-neutral.
- **Chat:** faction crests next to names; bots greet cross-faction raiders ([COMMUNITY.md](COMMUNITY.md)).
- **Raids** into ally or enemy channels earn influence under the normal rules and caps (weights in tuning).

## Not in this module

Faction events (Skirmish Hour and similar), faction ranks and titles (Progression, Phase 2), faction prediction markets (dropped), Discord-sourced influence (dropped), faction chat rooms (the faction board covers it).

## Legacy notes

Reviewed October 4, 2026. Kept: faction copy and colors, the append-only ledger with idempotency keys, durable season rollover, an underdog balance (now by active members), the war council and members-only board, the paged directory. Fixed: factions were creator-only; control flipped on every write; season winner was raw influence (favoring the biggest faction); chat and poll influence were never produced; support counted by amount; no session-trust checks; caps checked outside the transaction; the council strategy tally and voter IDs were readable by anyone logged in; guilds required one faction. Dropped: capitals, fortification, sieges, adjacency bonuses, fog of war, Discord-sourced influence, faction prediction markets, wall-post rank requirements, the "Tricolor" event name.

## Done when

New users choose a faction at sign-up and imported users keep theirs; the 7-day and between-season switching rules hold; trusted streaming, watching, chatting and supporting earn influence with caps, enemy-turf and council bonuses; the Monday checkpoint flips genres by size-balanced margin, keeps ties with the holder and claims neutral genres; a season ends with the right winner, rewards and a reset to home turf; the map, board and hub show correct live standings; faction-private data is only visible to members; nothing is counted twice when a job replays.
