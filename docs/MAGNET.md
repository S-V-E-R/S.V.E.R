# Module 5: MAGNet

Expanded October 3, 2026 by Joe. Not started. MAGNet is how S.V.E.R moves viewers between live streams. It has three parts:

1. **Discovery:** the homepage, browse, search, watch-page suggestions and the stream-end countdown, all in fair rotation.
2. **MAGNet Hype:** channels a viewer can sit on while MAGNet moves them to whichever stream is having a moment, and gives every stream its turn. It reinvents an idea from an earlier platform's auto-switching channel, with fairness built in.
3. **Spotlights:** short featured slots for first streams, returning creators and staff picks.

It follows the closure rule: specify, build, then test against "Done when". Numbers marked **Proposed** are defaults Joe can change; weights and thresholds for scoring live in the private tuning config, with safe example values in the repo.

## Rules that apply everywhere

- **Never by size.** No list, rotation or score uses viewer count, follower count or "viewers gained". Signals are measured against each stream's own normal level, so a 3-viewer stream can spike as easily as a 3,000-viewer one.
- **No money.** Subscriptions, cheers, gifts or any payment never raise a stream's chances. There are no paid votes or paid boosts.
- **Only real people count.** Every signal comes from Counted or Trusted sessions and verified accounts (Module 3 viewer integrity).
- **Every turn is explained.** Viewers see one line on why a stream is showing; streamers see more detail in Creator Studio.
- **Light pages.** One video player at a time; previews are still images, never a second player.

## Discovery

Carried over from the plan:

- **Homepage, signed out:** live now from all three factions, plus the war map and a sign-up prompt.
- **Homepage, signed in:** Following (live first), Live now, From your faction, Just went live, and the war map.
- **Fair rotation:** Live now and every category list are ordered by rotation. Every few minutes the order reshuffles so each live stream spends time in the top row. Streams in the viewer's faction's home genres get extra rotation weight (the home-turf boost).
- **Browse:** by genre, then category, each in fair rotation with a faction filter.
- **Search:** channels and categories by name, live channels first.
- **Watch page:** suggests other live streams: same genre first, then same faction, then anything live.
- **Stream end:** viewers are offered the next stream (same order) with a 10-second countdown and Cancel. A raid in progress takes priority over this.
- **Nothing live:** never an empty grid; show recently live channels and the war map.

## MAGNet Hype

### Channels

- **Global Hype** at `/magnet`, plus **one Hype lane per genre** (`/magnet/{genre}`), using the genres defined by Module 4. Each channel runs its own engine with its own rotation, hold times and cooldowns.
- Faction Hype channels (one per faction) are a later addition.

### Who can be featured

A stream is eligible when it is:
- live for at least 60 seconds and not reconnecting;
- in a category allowed by the content rules;
- not opted out (streamers are included by default and can opt out in Creator Studio);
- from a channel that isn't restricted and has no open staff integrity case.

S.V.E.R Plays and other system channels are featured only when nothing else is live.

### How a channel decides

The engine ticks every 10 seconds per channel. Each switch is one of two kinds, and they **alternate**:

- **Moment switch:** goes to the eligible stream with the strongest moment right now, if it clearly beats the current stream.
- **Fair-turn switch:** goes to the eligible stream that has waited longest since it was last featured (never-featured streams first), regardless of moment score.

Timing (**Proposed**):
- A featured stream holds for at least **45 seconds**.
- At most **8 minutes** on one stream; then the next switch happens even without a moment.
- At least **2 minutes** between moment switches.
- A stream just featured isn't picked again for **30 minutes**, unless it is the only eligible stream (no dead air).

Because every second switch is a fair turn and a stream can hold at most 8 minutes, every eligible stream in a channel is featured within a bounded time.

If the engine fails, the channel holds its current stream; if that stream ends, it falls back to any live eligible stream, then to the "nothing live" view.

### What counts as a moment

Room signals only at launch, each compared with that stream's own recent baseline:

- **Chat burst:** distinct verified chatters per minute well above the stream's normal. Repeated messages, emote-only spam and brand-new accounts count for less.
- **Follow burst:** new follows per minute above the stream's normal.
- **Raid arriving:** a raid (Module 3) landing on the stream.
- **Streamer flag:** a "Flag this moment" button in Creator Studio and `/flag` in chat. It counts only together with another elevated signal, with a cooldown (**Proposed:** once every 10 minutes).

Not used: viewer count, follower totals, money, faction (Hype channels are faction-neutral; the homepage keeps its home-turf weighting).

Later add-ons: game-API detectors and computer-vision moment detection for specific games, CrowdSync activity, and faction Hype channels. Vision needs frame decoding, so it must be measured against the transmux-only rule before it is added.

### What the viewer sees

- One player showing the featured stream, its title, streamer, category and a one-line reason ("Chat is going off", "Fair turn: hasn't been featured today", "Just raided by …").
- Before a switch: a **5-second countdown** with a still preview card of the next stream and a **Stay** button. Stay opens the current stream on its own channel page and leaves the Hype channel.
- Chat on a Hype channel is the featured stream's own chat, so viewers talk to the streamer they're watching. It changes with each switch.
- Viewers on a Hype channel are ordinary playback sessions on the featured broadcast, so they count for that stream like any other viewer.
- Discovery labels where true: First feature, Returning creator, New creator.

### What the streamer sees

In Creator Studio:
- Live: whether they're featured, on which channel, and why.
- After: feature history with how long each feature lasted and how many Hype viewers stayed to follow or chat (shown to the streamer only, never used for scoring).
- Settings: opt out of MAGNet Hype; the flag-moment button.

### Staff controls

In `/admin`: enable or disable each Hype channel, force a stream onto a channel and release it, emergency stop, and the decision log (each decision with its candidates, signals and reason; kept 7 days). Every staff action is audited.

## Spotlights

- **Automatic:** a stream gets a spotlight slot on the homepage the first time it ever goes live, and when it returns after 30 or more days away.
- **Staff spotlights:** a short reason shown publicly; at most 14 days long; at most one active per channel; a 7-day cooldown before the same channel again; audited.
- Spotlights rotate like everything else; there is no paid spotlight.

## Thumbnails

Preview cards need a still image of each live stream. Generate one keyframe snapshot per live stream about once a minute. This decodes a single frame and never re-encodes or alters the stream.

## Storage and API outline

Tables: Hype channels and their state (current, pending and forced stream; lock and hold times), per-stream feature history and cooldowns, the decision log, streamer MAGNet settings, flag moments, spotlights. Signals are computed from existing chat, follow and raid data plus playback-lease levels; no new tracking of viewers. A Postgres-backed job runs the engines; the realtime switch and countdown events go over the existing WebSocket. No Redis.

## Not in this module

- Computer vision and game-API detectors, CrowdSync signals, faction Hype channels (later).
- Surge (a collective hype event funded by support) belongs with Support or later, and never feeds MAGNet.
- Auto-clips of featured moments belong to Module 7.
- Personalization from watch history.

## Legacy notes

The legacy MAGNet was reviewed on October 3, 2026. Kept: hold, maximum-duration and no-cooldown-when-alone rules, public reasons, creator reasoning, opt-out, spotlight limits, and "game API first, vision second, audio never decides". Dropped: paid votes and money-weighted scoring, viewer-count inputs, the 120-setting configuration, the unused machine-learning model registry, two pre-buffered video players, and the vision pipeline that never ran in production.

## Done when

A signed-out visitor reaches a live stream in one click from the homepage; every live stream reaches the top row within a rotation cycle; the watch page and stream-end countdown move viewers to another live stream; search finds channels and categories; empty states show recent channels. On the Global Hype channel and each genre lane: a chat or follow burst on a small stream triggers a moment switch with a countdown and a reason; fair-turn switches alternate with moment switches and every eligible stream is featured within the bound; hold, maximum time and cooldowns work; opted-out, restricted and integrity-flagged streams never appear; money and viewer count provably have no effect on selection; streamers see their feature history; staff can force, release and stop a channel.
