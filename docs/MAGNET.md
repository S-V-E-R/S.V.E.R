# Module 5: MAGNet

The product name is **MAGNet**. Its channels, chat and Creator Studio labels use that name.

Expanded October 3, 2026 by Joe. **Built October 5, 2026: discovery, spotlights, thumbnails, MAGNet and MAGNet chat (below). Co-streams on MAGNet built October 6, 2026. Live acceptance with real streams is open.** MAGNet is how S.V.E.R moves viewers between live streams. It has three parts:

1. **Discovery:** the homepage, browse, search, watch-page suggestions and the stream-end countdown, all in fair rotation.
2. **MAGNet channels:** channels a viewer can sit on while MAGNet moves them to whichever stream is having a moment, and gives every stream its turn. It reinvents an idea from an earlier platform's auto-switching channel, with fairness built in.
3. **Spotlights:** short featured slots for first streams, returning creators and staff picks.

It follows the closure rule: specify, build, then test against "Done when". All numbers here were accepted by Joe as defaults on October 3, 2026 and can be tuned later; weights and thresholds for scoring live in the private tuning config, with safe example values in the repo.

## Rules that apply everywhere

- **Never by size.** No list, rotation or score uses viewer count, follower count or "viewers gained". Signals are measured against each stream's own normal level, so a 3-viewer stream can spike as easily as a 3,000-viewer one.
- **No money.** Subscriptions, tributes, gifts or any payment never raise a stream's chances. There are no paid votes or paid boosts.
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

Implementation (October 5; `discovery.rs`, migration `0026_discovery.sql`):

- **Fair rotation** (`rotate`): live streams sit in a fixed cycle ordered by start time, and the cycle advances one place every 3 minutes (`ROTATE_SECONDS`), so every stream is first within one cycle. Streams in the viewer's faction's home genres (`faction_genres.home`) take a second place in the cycle, the home-turf boost. Inputs are start time, genre and the viewer's faction only. Unit tests prove every stream reaches the top within the cycle, that the boost doubles a stream's turns, and that viewer counts change nothing; the API test repeats the last check with 40 counted sessions.
- **Public stream** = LIVE, or RECONNECTING inside its grace window, on an eligible (not restricted or deleted) channel. Channels the viewer blocked, or that blocked the viewer, are left out of that viewer's lists.
- **Endpoints:** `GET /api/discovery/home` (following live, live in rotation, the viewer's faction, just went live under 30 minutes, spotlights, recently live in the last 14 days when nothing is live), `GET /api/discovery/live?genre=&category=&faction=`, `GET /api/discovery/browse` (genres → categories with live counts), `GET /api/search?q=` (2–50 characters, literal substring, live channels first, then categories) and `GET /api/channels/{username}/suggestions` (same genre, then same faction, then anything; it works just after a stream ends). The old `/api/streams` and `/api/live` lists are removed.
- **Web:** the home page, `/browse`, `/search` (with the top-bar search box), Up next on the watch page, and the stream-end countdown (`UpNext`: 10 seconds with Cancel, shown only to viewers who saw the stream live; a raid moves them before the stream ends). Cards show the live still and a New creator / Returning creator label.
- **Spotlights:** automatic for a first broadcast ever and for a return after 30+ days away (derived from broadcast history, so no table). Staff spotlights (Admin → Live streams) have a public reason of up to 120 characters, last 1–14 days, allow one active per channel with a 7-day cooldown (serialized per channel), and both creating and ending one are audited.
- **Thumbnails** (`probe.rs`): the OBS-health probe already fetches each live stream's newest HLS segment every 30 seconds; about once a minute ffmpeg decodes one frame to a 640-pixel WebP (the stream is never re-encoded). Each still gets a new key, the previous one is deleted, and ended streams' stills are swept. The API image gains `ffmpeg`.
- Coverage: `discovery` unit tests, `tests/streams/discovery.rs`, `scripts/check-discovery.cjs` and the updated shell check.

## MAGNet channels

### Channels

- **Global MAGNet** at `/magnet`, plus **one MAGNet lane per genre** (`/magnet/{genre}`), using the genres defined by Module 4. Each channel runs its own engine with its own rotation, hold times and cooldowns.
- Faction MAGNet channels (one per faction) are a later addition.

### Who can be featured

A stream is eligible when it is:
- live for at least 60 seconds and not reconnecting;
- in a category allowed by the content rules;
- not opted out. **Every channel is eligible by default**; a streamer can opt out of MAGNet in Creator Studio at any time, effective at the next tick;
- from a channel that isn't restricted and has no open staff integrity case.

S.V.E.R Plays and other system channels are featured only when nothing else is live.

### How a channel decides

The engine ticks every 10 seconds per channel. Each switch is one of two kinds, and they **alternate**:

- **Moment switch:** goes to the eligible stream with the strongest moment right now, if it clearly beats the current stream.
- **Fair-turn switch:** goes to the eligible stream that has waited longest since it was last featured (never-featured streams first), regardless of moment score.

Timing:
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
- **Streamer flag:** a "Flag this moment" button in Creator Studio and `/flag` in chat. It counts only together with another elevated signal, with a cooldown (once every 10 minutes).

Not used: viewer count, follower totals, money, faction (MAGNet channels are faction-neutral; the homepage keeps its home-turf weighting).

Later add-ons: game-API detectors and computer-vision moment detection for specific games, CrowdSync activity, and faction MAGNet channels. Vision needs frame decoding, so it must be measured against the transmux-only rule before it is added.

### What the viewer sees

- One player showing the featured stream, its title, streamer, category and a one-line reason ("Chat is going off", "Fair turn: hasn't been featured today", "Just raided by …").
- Before a switch: a **5-second countdown** with a still preview card of the next stream and a **Stay** button. Stay opens the current stream on its own channel page and leaves the MAGNet channel.
- Chat works as described in "MAGNet chat" below.
- Viewers on a MAGNet channel are ordinary playback sessions on the featured broadcast, so they count for that stream like any other viewer.
- Discovery labels where true: First feature, Returning creator, New creator.

### MAGNet chat

Each MAGNet channel has its **own chat**, separate from every streamer's channel chat. It merges with the featured stream's chat during a feature and detaches when MAGNet moves on.

- **Between features** (countdown, nothing live): MAGNet chat is its own room. During the countdown it shows "Chat joins *streamer* in 5…".
- **During a feature (merged):** MAGNet chat and the featured channel's chat show each other's messages in real time. Messages from the MAGNet side carry the MAGNet mark so the streamer and their community can see who came from MAGNet. Viewers on the channel page and on the MAGNet channel are in the same conversation.
- **After the switch (detached):** the link closes. Messages sent during the merge stay in both histories; new messages stay in the room they were sent in. The streamer's chat is back to just their channel.
- **The channel's rules apply to everything that crosses into it:** its slow mode, link blocking, banned words, followers-only and subscriber-only modes. The channel's moderators can delete MAGNet messages that crossed into their chat and time out or ban those users from their channel.
- **Viewers banned from (or timed out in, or blocked by) the featured channel:** a channel ban means no chat and no signed-in watching (Module 3), and MAGNet doesn't create a way around it. For as long as that channel is featured, the banned viewer:
  - sees a holding card instead of the stream ("This stream isn't available to you. MAGNet moves on in about *n* minutes"), with links to other live streams in the same lane;
  - can read MAGNet chat but can't send, because MAGNet chat is merged into that channel's conversation ("Chat resumes when MAGNet moves on");
  - gets no playback session on that broadcast, so they aren't counted as its viewer.
  
  When MAGNet switches, video and chat come back automatically. A timeout works the same way until it expires. If the viewer has blocked the streamer, they get the same holding card. One banned viewer never stops a stream from being featured. As everywhere, signed-out viewing can't be blocked.
- **Flood protection for small channels:** MAGNet senders get an extra slow mode in the merged channel (one message every 3 seconds per person) on top of the channel's own rules. The streamer can turn off chat merging for their channel and stay in MAGNet; MAGNet viewers then chat only in MAGNet chat.
- **MAGNet chat itself** is moderated by staff and follows the platform chat rules (500 characters, latest 100 on join, 7-day expiry, reports).
- **No feedback loop:** messages from the MAGNet side never count toward the featured stream's chat-burst signal, so being featured can't keep a stream featured.

### Co-streams on MAGNet

Co-streams (Module 6, Support) can be picked up by MAGNet:

- **Separate squads:** each member stays its own candidate. When one is featured, the player shows "Co-streaming with …" with links to the others; MAGNet chat merges with that member's chat only.
- **Merged squads:** the squad is **one candidate**. Its moment signals come from the squad's shared chat and all members' follows; its fair-turn wait is the longest wait among its members. When featured, the MAGNet player shows the squad's focused stream with small tabs to switch between members (still one video player), and MAGNet chat merges with the squad's shared chat. Each member's opt-out applies: an opted-out member is left out of the featured squad.
- **Cooldown:** a featured squad puts all its members on the normal cooldown, so a squad can't take repeated turns through different members.
- **Viewers and money:** MAGNet viewers watching a squad count for the stream they're watching. Support spent through the MAGNet channel while a merged squad is featured follows the squad's pooled split.

### What the streamer sees

In Creator Studio:
- Live: whether they're featured, on which channel, and why.
- After: feature history with how long each feature lasted and how many MAGNet viewers stayed to follow or chat (shown to the streamer only, never used for scoring).
- Settings: opt out of MAGNet; turn chat merging off or on; the flag-moment button.

### Staff controls

In `/admin`: enable or disable each MAGNet channel, force a stream onto a channel and release it, emergency stop, and the decision log (each decision with its candidates, signals and reason; kept 7 days). Every staff action is audited.

### Implementation of MAGNet (October 5; `magnet.rs`, migration `0027_magnet.sql`)

- **Lanes:** `global` plus one per Module 4 genre (`magnet_lanes`, created by the engine). Each lane ticks every 10 seconds with the 5-second media pass. `decide` is a pure function of the lane state and candidates. Its `Candidate` type has no field for viewer count, followers or money, so none can affect selection.
- **Eligibility:** LIVE (not reconnecting) for 60+ seconds, an active category (the lane's genre on genre lanes), not opted out, an eligible channel and no OPEN integrity case. S.V.E.R Plays (`plays_runtime`) is a candidate only when nothing else is eligible.
- **Switching:** moments and fair turns alternate (`last_kind`). The rules: a 45-second minimum hold, an 8-minute maximum (then a fair turn), at least 2 minutes between moments, and a moment must clearly beat the current stream (1.5×). Fair turns go to the stream that has waited longest on that lane (never-featured first). There's a 30-minute cooldown unless only one other stream is eligible, and a lone eligible stream holds. If the featured stream ends or becomes ineligible, the lane falls back at once; if nothing is eligible, it clears. A failed tick holds the current stream. A unit simulation proves every eligible stream is featured within 2 × n × 8 minutes, even beside a stream that's always having a moment.
- **Signals** (each against the stream's own last 30 minutes; hardened October 7, below): distinct verified chatters per minute (accounts under 7 days count half; MAGNet-side messages never count), verified follows per minute, a raid arriving in the last 2 minutes, and the streamer's flag. The flag adds only to another elevated signal and can be used once every 10 minutes (Studio button or `/flag`). Thresholds come from `MAGNET_TUNING_FILE`; the repo holds safe defaults.
- **Viewers:** `/magnet` and `/magnet/{genre}`. A switch is announced 5 seconds ahead with a still of the next stream and Stay. The player plays the featured broadcast, so its sessions count for that stream; leases are tagged with the lane for Studio history only. Viewers banned from, timed out in, or blocked by the featured channel get the holding card (no playback session) with other streams in the lane.
- **MAGNet chat:** a lane's own room stores `chat_messages` with no channel and `origin` = the lane. While a stream is featured and its streamer allows merging, MAGNet messages are sent through that channel's normal chat path, so all its rules apply, plus 3 seconds per MAGNet sender. They're stored in its chat with `origin`, shown with the MAGNet mark, moderatable by its moderators, and excluded from burst signals. The room shows its own messages plus the channel's chat since the feature began, and detaches at the next switch (`/api/magnet/{lane}/chat`, `/api/magnet/{lane}/ws`). Held viewers can read but not send. Room messages can be reported, and staff removal updates open rooms.
- **Studio** (Creator Studio → MAGNet): featured now, opt-out, chat merging, the flag, and 30-day history with MAGNet viewers and who followed or chatted. **Staff** (Admin → MAGNet): lanes, enable/disable, force/release, emergency stop and the decision log, all audited.
- **Co-streams** (October 6): `candidates` folds a live merged squad's eligible members into one `Candidate` whose `members` lists their broadcasts; opted-out or otherwise ineligible members are left out, and a squad with one eligible member is an ordinary candidate. Its chat signal reads the squad's shared chat (members and MAGNet-side messages never count), follows and flags add up across members, a raid on any member counts, and its wait is the oldest `last_featured` among them. The host is shown first; showing any member counts as showing the unit, and staff can force any member. A feature writes one `magnet_features` row per member, so they share the cooldown and each sees it in Studio. `/api/magnet/{lane}` returns the squad: tabs for a merged squad's featured members (one player), links for a separate squad's other members. While a merged squad is featured, MAGNet chat merges with its shared chat under every member's rules (`co_stream` in the room snapshot); a ban, timeout or block by any member, or a shared-chat restriction, gives the holding card. Chat merging needs every member to allow it. Tributes still can't be sent from MAGNet chat, so the pooled split has nothing to route yet.
- Coverage: the `magnet` unit tests (timing, alternation, cooldowns, own-baseline signals, the fairness bound, a co-stream as one unit) and `tests/streams/magnet.rs` (eligibility, countdown and switch, a small-stream chat burst, the no-feedback rule, holding, MAGNet chat merge and detach, channel rules and slow mode, reports and removal, Studio, staff and audit; a merged squad as one candidate without its opted-out member, shared cooldown, tabs, holding on any member's ban, chat merged into the shared chat, a shared-chat moment). `tests/streams/teams.rs` checks MAGNet chat shows a featured squad's shared room, and `scripts/check-discovery.cjs` the tabs and co-stream links.

### Hardening (October 7, 2026; migration `0044_magnet_hardening.sql`)

A review against the auto-switching channel MAGNet reinvents found that its moment detection was its weakest point: whatever it read could be gamed, broke without warning, or rewarded being featured. MAGNet's room signals are hardened the same way:

- **Only real viewers count.** A chatter or new follower counts toward a stream's moment only if viewer integrity counted their session on that broadcast (for a merged co-stream, on any member's) and doesn't exclude it now. Chatting or following without watching does nothing. The baseline follows the same rule, so the comparison stays like for like.
- **MAGNet's own audience never counts.** People MAGNet brought to a stream (sessions tagged with the lane) are left out of its chat and follow signals. Before this, follows from MAGNet viewers raised the featured stream's score and made it harder for another stream to beat it. A session is tagged only if it started while that lane was featuring the broadcast, so a client can't tag any other session.
- **No money in signals.** Tributes and Skills (paid messages) are left out of chat signals.
- **Low-effort chat counts half** (`low_effort_weight`): a chatter whose messages in the minute were all emote-only (only the channel's emotes) or copy-pasted (the same text another chatter sent in that room within a minute). Accounts under 7 days still count half (`new_account_weight`); the two multiply.
- **Raids** count as a moment only if at least one viewer the integrity check doesn't exclude actually arrived with the raid, and only the first raid between two channels (either direction) in `raid_repeat_hours` (24). Friends trading raids can't trade moments.
- **Viewers MAGNet sends don't look like viewbots.** The integrity check's arrival-spike rule leaves out sessions MAGNet brought, so a small stream's MAGNet audience counts for it straight away instead of sitting in a 5-minute provisional window. Other sudden arrivals on the same stream still trigger it.
- **Countdown recovery.** If the next stream ends or becomes ineligible during the 5-second countdown, the countdown is cancelled (`cancelled` in the decision log) and the lane decides again in the same tick. If the current stream ended too, the lane falls back at once instead of showing nothing until the next tick.
- **Ended staff picks are released** automatically (`released` in the decision log), so Admin never shows a force that can no longer take effect.
- **Lane health.** A lane whose ticks fail keeps holding its stream, as before, and records since when (`failing_since`, `failures`). Admin → MAGNet marks a lane as stalled when its ticks are failing or it hasn't ticked for six ticks. One successful tick clears it.
- Coverage: the hardening scenario in `tests/streams/magnet.rs` (non-viewers, MAGNet's audience, copy-paste, emote-only, paid messages, follows, raids with and without arrivals and a raid traded back, countdown recovery, the stale force, stalled lanes and the spike rule).

Considered and not adopted: a split-screen of several moments at once breaks the one-player rule, and computer-vision detection stays a later add-on (game state read from pixels breaks whenever a game changes its interface; game APIs come first).

## Spotlights

- **Automatic:** a stream gets a spotlight slot on the homepage the first time it ever goes live, and when it returns after 30 or more days away.
- **Staff spotlights:** a short reason shown publicly; at most 14 days long; at most one active per channel; a 7-day cooldown before the same channel again; audited.
- Spotlights rotate like everything else; there is no paid spotlight.

## Thumbnails

Preview cards need a still image of each live stream. Generate one keyframe snapshot per live stream about once a minute. This decodes a single frame and never re-encodes or alters the stream.

Cards and the homepage rotation show the full frame inside their 16:9 area, with letterboxing when needed. Visible stills refresh about once a minute; hidden tabs and offscreen cards do no refresh work. Missing, failed or stale captures show the category label. The image endpoint resolves the latest capture when requested, so a lazy image cannot refer to a previously deleted still. It serves only eligible live/reconnecting broadcasts and captures less than three minutes old, with no browser caching and no playback lease.

## Storage and API outline

Chat messages gain an origin (channel or MAGNet channel) so merged messages can be shown, moderated and excluded from burst signals. Tables: MAGNet channels and their state (current, pending and forced stream; lock and hold times), per-stream feature history and cooldowns, the decision log, streamer MAGNet settings, flag moments, spotlights. Signals are computed from existing chat, follow and raid data plus playback-lease levels; no new tracking of viewers. A Postgres-backed job runs the engines; the realtime switch and countdown events go over the existing WebSocket. No Redis.

## Not in this module

- Computer vision and game-API detectors, CrowdSync signals, faction MAGNet channels (later).
- Surge (a collective hype event funded by support) belongs with Support or later, and never feeds MAGNet.
- Auto-clips of featured moments belong to Module 8 (VODs and clips).
- Personalization from watch history.

## Legacy notes

The legacy MAGNet was reviewed on October 3, 2026. Kept: hold, maximum-duration and no-cooldown-when-alone rules, public reasons, creator reasoning, opt-out, spotlight limits, and "game API first, vision second, audio never decides". Dropped: paid votes and money-weighted scoring, viewer-count inputs, the 120-setting configuration, the unused machine-learning model registry, two pre-buffered video players, and the vision pipeline that never ran in production.

## Labels and language (October 6, 2026)

The mature label and stream language ([CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md)) never change rotation order. Language filters narrow a list; mature-labeled streams are removed after ordering for under-18 viewers only, the same way blocks are.

## Done when

A signed-out visitor reaches a live stream in one click from the homepage; every live stream reaches the top row within a rotation cycle; the watch page and stream-end countdown move viewers to another live stream; search finds channels and categories; empty states show recent channels. On the Global MAGNet channel and each genre lane: a chat or follow burst on a small stream triggers a moment switch with a countdown and a reason; fair-turn switches alternate with moment switches and every eligible stream is featured within the bound; hold, maximum time and cooldowns work; opted-out, restricted and integrity-flagged streams never appear; MAGNet chat merges with the featured chat under the channel's rules and detaches on switch, viewers banned from the featured channel get the holding card and can't send until it switches, and merged messages don't count toward the burst signal; merged co-stream squads are featured as one unit; money and viewer count provably have no effect on selection; streamers see their feature history; staff can force, release and stop a channel.
