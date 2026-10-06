# Module 7: CrowdSync

Scoped October 3, 2026 by Joe. Builds after Module 6 (Support), which brings the Engagement Valor that board presses spend. **Status: phases 1 to 3 built October 6, 2026 (boards; polls, predictions and counters; Skills, Faction Rally, emote combos and Surge), and part 2 (the integration gateway, OBS bridge and Game SDKs) built the same day; see "Implementation status". The Unity and Unreal SDKs still need a compile check and an example game in those engines.**

CrowdSync is how viewers change what happens on stream. It brings back the idea of interactive boards and Skills from an earlier platform, rebuilt from the legacy CrowdSync design (whose in-page board worked) without its flaws: game and bridge access to server internals, no protection against bots, no account for video delay, and money that could buy attention.

It follows the closure rule: specify, build, then test against "Done when". Numbers marked **Proposed** are defaults Joe can change.

## The first board

The S.V.E.R Plays controller is the first CrowdSync board. It is built when Plays moves to the new API (right after Live streams), in this module's board format, with buttons, live vote shares and the control status bar ([PLAYS.md](PLAYS.md)). This module generalizes it: the builder, templates, Engagement Valor costs and outputs for every streamer.

## Parts

1. **Boards:** streamer-built panels of controls under the player. Presses cost the channel's Engagement Valor (or are free) and trigger on-stream effects.
2. **Skills:** premium animated effects a viewer buys with Purchased Valor, shown on stream and in chat.
3. **Polls and predictions:** one poll system; predictions use Engagement Valor only.
4. **Faction Rally and emote combos:** free, crowd-made moments.
5. **Game SDK (part 2):** games react to board input. Starts once boards are stable; same module.

## Rules that apply everywhere

- **Only real, signed-in viewers act.** Pressing, voting, predicting and rallying need a verified account with a Counted or Trusted playback session on that broadcast (Module 3 viewer integrity). Guests can see the board but not press.
- **Nothing here affects MAGNet or faction influence beyond normal rules.** Presses, Skills and predictions never raise a stream's MAGNet chances. A rally counts toward influence only as ordinary chat participation, under its existing cap.
- **No gambling.** Predictions use earned Engagement Valor only, never Purchased Valor or money.
- **No spending leaderboards.** Nothing ranks people by how much they spent.
- **Fair for every viewer, whatever their delay.** Server events carry the broadcast's stream time. Each viewer's player shows an effect when its own playback reaches that moment, so a CDN viewer (3–5 seconds behind) sees the effect in sync with the video, the same as a WebRTC viewer (about 1 second). Timed windows (polls, prediction locks, Plays votes) close by stream time, not wall-clock time, so delayed viewers get the same window.
- **One place for effects.** If the streamer's OBS overlay is connected, effects appear only in the video; otherwise they appear on the page over the player, synced to stream time. Never both.
- **Light pages.** The board panel loads only when opened, shares the existing chat WebSocket, respects reduced-motion settings, and has a "reduce effects" toggle.
- **Channel rules apply.** Channel bans and timeouts block pressing; text inputs go through the channel's banned-word and link rules.

## Boards

- **Building:** Creator Studio has templates, a builder (grid of controls across one or more screens), a safe test mode, and a publish checklist. Boards have drafts and published versions; editing never changes the live board until the streamer publishes.
- **Controls:** button, label, text input (up to 200 characters, **Proposed**), goal (a shared progress bar the crowd fills), and joystick (rate-limited). Mouse controls are deferred.
- **Cost and limits per control:** Engagement Valor cost (0 = free), per-viewer cooldown, an optional per-stream limit, and who can use it (everyone signed in, followers, subscribers, moderators).
- **Outputs (what a press does):**
  - On-stream effect: preset animations, stickers and sounds from the S.V.E.R library or the streamer's uploads (reviewed like emotes).
  - OBS overlay: a browser source URL with a private token, showing effects inside the video.
  - OBS scene and source changes through the S.V.E.R bridge, a small open-source app on the streamer's PC that connects to S.V.E.R with a scoped token. It never connects to S.V.E.R's database or servers directly.
  - Webhook to the streamer's own HTTPS endpoint, signed, with private-network addresses blocked.
- **Board moderators:** the streamer can let channel moderators run the board, block a viewer from the board, and panic-disable all sounds and effects with one click.
- **Rate limits:** per account and per network across all controls, including joystick input (**Proposed:** at most 10 joystick updates per second per viewer).
- **Reliability:** the Engagement Valor charge and the press are recorded in one database transaction, and delivery to outputs goes through a Postgres-backed outbox, so a crash can never charge without a press or lose a press. No Redis.

## Skills

- A catalog of premium animated effects (stickers, full-screen moments, sounds) bought with **Purchased Valor** at fixed prices set by S.V.E.R.
- A Skill plays on stream (through the overlay or on the page, synced as above) and shows in chat like a tribute, with the sender's name and the Skill.
- The streamer earns 0.8¢ per Valor, the tribute rate, and can switch Skill categories (for example, sounds) off for their channel.
- Under-18 buyers follow the Support module's guardian confirmation and $50 monthly cap.
- Skills never affect MAGNet, influence or tiers beyond the earnings they bring.

## Polls and predictions

- **Polls:** one poll system for chat and boards. The owner or a moderator asks a question with 2 to 5 options and a duration; each eligible viewer votes once; results show live and at the end. No paid weighting.
- **Predictions:** the owner or a moderator opens a prediction with 2 to 10 outcomes. Viewers stake **Engagement Valor** on one outcome (**Proposed:** at most 10,000 per prediction). It locks at a set stream time; the owner resolves it, and winners split the pool in proportion to their stakes. Cancelling refunds everyone. Purchased Valor and money can never be staked.

## Faction Rally and emote combos

- **Faction Rally:** a free board control (and a chat command) that lets signed-in faction members rally for their faction. A rally meter on stream shows each faction's share for that stream.
- **Emote combos:** when at least 5 distinct verified accounts send the same emote within 5 seconds, an emote shower plays over the stream (10-second cooldown). It counts distinct accounts, so a single person can't trigger it.

## Surge

Decided October 3, 2026: Surge comes back driven by **participation, not money**.

- A channel event that fills a meter when many **distinct** eligible viewers take part within a short time: chatting, rallying, pressing board controls, paying tribute and subscribing each count as one participation per person per minute, never by amount.
- It starts when enough distinct viewers participate within a minute (**Proposed:** 10, scaled down for small channels so a 5-viewer stream can still trigger one), lasts 5 minutes, and each new level adds time (capped at 10 minutes total).
- Levels 1 to 5 unlock on-stream celebrations (through the overlay or page, synced to stream time) and award Engagement Valor to everyone who took part, capped per day.
- Only Counted or Trusted sessions count, so bots can't start or fill a Surge. It never affects MAGNet. A 30-minute cooldown between Surges.

## Counter widgets

- Board widgets for trackers: a shiny counter (encounters, phase, odds), death counter, win/loss tally and a custom counter. The streamer or moderators update them from the board or a chat command; viewers see them on the board and the overlay.

## Game SDK (part 2)

- Lets a game receive board input (button presses, joystick, text) and send state back to the board (labels, goal progress, button availability).
- Games connect only through a scoped, token-authenticated WebSocket gateway on S.V.E.R. They never connect to S.V.E.R's database, Redis or internal services.
- First SDKs: JavaScript/TypeScript, Unity (C#) and Unreal (C++), with an example game. Other languages follow on request.
- S.V.E.R Plays can move its votes onto a CrowdSync board once the SDK exists.

## Implementation status

Phase 1, Boards (October 6, 2026; migration 0034, `boards.rs`, `tests/streams/boards.rs`):

- **Studio → Board:** four templates (Hype buttons, Community goal, Shout-outs, Arcade), a builder of up to 4 screens × 24 controls on a 4-column grid, drafts, test mode and a publish checklist. Publishing makes a new version and starts goals over; editing never changes the live board.
- **Controls:** button, label, text input (1–200 characters, through the channel's banned-word and link rules), goal (each press adds its cost, or 1 when free; it closes when full) and joystick (free, relayed live, not stored). Each has a cost in Engagement Valor (0–100,000), a per-viewer cooldown (up to an hour), an optional per-stream limit (counted across everyone), an audience (everyone signed in, followers, subscribers, moderators) and an effect from the library: confetti, hearts, fireworks, stars, rain, shake, spotlight.
- **Who can press:** a verified account with a Counted or Trusted playback lease on the channel's live broadcast. Guests, unverified accounts, excluded sessions, banned or timed-out viewers, viewers blocked from the board and blocked users can't. The owner uses test mode.
- **Reliability:** a press (idempotent by its client ID), its Engagement Valor charge, goal progress and webhook delivery are written in one transaction, under a per-control lock, so a crash never charges without a press.
- **Rate limits:** 10 presses per 10 seconds per account and 50 per network; joystick moves 10 a second per account and 50 per network.
- **Effects:** sent over the existing chat socket with the stream time. Each player waits for its own delay before drawing them (about 0.5 seconds on WebRTC; the distance from the live edge plus 2 seconds on HLS). Captions always show; animations respect reduced motion and the viewer's "Reduce effects" toggle.
- **OBS overlay:** a private browser-source URL (`/overlay/<token>`, only the token's digest is stored, and a new URL replaces the old). While it is connected (checked in within 30 seconds), effects appear only in the video, never also over the player. Test-mode effects reach it too.
- **Webhooks:** the streamer's HTTPS endpoint receives `board.press` events signed like Stripe's (`SVER-Signature: t=…,v1=HMAC-SHA256("t.body")`; the secret is shown once). Delivery goes through the Postgres outbox on its own worker loop. It checks every resolved address and refuses private, loopback, link-local, CGNAT, benchmark, documentation, multicast and NAT64 ranges, then connects to the address it checked (no re-resolution and no redirects). It retries with doubling backoff up to 8 attempts.
- **Running the board:** the owner, staff and (when allowed) channel moderators can pause all effects (panic) and block viewers from the board; each action is in the channel moderation log.
- **Not yet:** effect sounds and streamer-uploaded effects come with Skills (they need the emote-style review); the OBS scene bridge and the Game SDK are later phases. Effects sync to an estimated player delay; exact sync to the HLS program date-time is a later refinement if measurements call for it.

Phase 2, polls, predictions and counters (October 6, 2026; migration 0035, `crowd.rs`, `tests/streams/crowd.rs`):

- **Polls:** the owner or a moderator asks a question with 2 to 5 options for 15 seconds to 30 minutes while live, one at a time. Each real viewer (the same rule as pressing: verified, Counted or Trusted lease, not banned, timed out or blocked) votes once; results update live for everyone. The owner can't vote in their own poll.
- **Predictions:** 2 to 10 outcomes, one at a time. Viewers stake 1 to 10,000 of the channel's Engagement Valor (never Purchased Valor or money), charged in the same transaction as the vote. A prediction locks at its stream time; only the owner (or staff) resolves it. Winners split the whole pool in proportion to their stakes; rounding leftovers go one each to the largest stakes, so nothing is lost or created. If nobody picked the winning outcome, or it is cancelled, every stake is refunded. Predictions left unresolved for a day are cancelled and refunded automatically.
- **Stream-time windows:** votes are accepted 8 seconds after a window ends, and each player closes its own window when its video reaches the end (its measured delay, capped at the grace), so CDN viewers get the same window as WebRTC viewers.
- **Counters:** up to 10 per channel (death counter, shiny counter with encounters, phase and odds, win/loss tally, custom). Managed in Studio → Counters; the owner and moderators update them from the page or in chat (`!deaths`, `!deaths -`, `!deaths +5`, `!deaths =10`, `!record win`, `!record loss`, `!shiny phase`). A viewer's `!deaths` is just a message. Values never go below zero.
- **Display:** a panel under the player shows counters, the running poll and prediction (with live results) and, for the owner and moderators, the controls. The OBS overlay shows counters and live poll results too. Starting, ending, resolving and cancelling are in the channel moderation log.

Phase 3, Skills, Faction Rally, emote combos and Surge (October 6, 2026; migration 0036, `skills.rs`, `surge.rs`, `tests/streams/moments.rs`):

- **Skills:** eight effects at fixed S.V.E.R prices (**Proposed**): stickers (Crown 50, Big heart 50, Trophy 75 Valor), full-screen moments (Starfall 200, Quake 150, Fireworks show 300) and sounds (Chime 30, Fanfare 100; synthesized in the browser, so no audio files). A Skill is a chat message paid in Purchased Valor exactly like a tribute: the same Valor spend, ledger entry and refund rules, and the streamer earns 0.8¢ per Valor. Under-18 guardian confirmation and the $50 monthly cap apply when the Valor is bought. The message shows a Skill badge in chat and the effect plays on stream (overlay or player, never both). Streamers switch categories off in Studio → Board; nothing is sold while the channel's effects are paused.
- **Faction Rally:** a button under the player, a `rally` board control and the `!rally` chat command. A faction member watching the live broadcast (the real-viewer rule) rallies at most once a minute; the meter shows each faction's share for the stream, under the player and in the overlay. A `!rally` message counts toward faction influence only as the ordinary chat message it is; the button and board control count for nothing beyond the meter.
- **Emote combos:** when 5 distinct accounts send the same channel emote within 5 seconds, that emote showers over the stream, with a 10-second cooldown. Counting is in memory on the single API instance.
- **Surge:** chatting, rallying, pressing board controls, tributes, Skills and subscribing each count once per real viewer per minute, never by amount. A Surge starts when enough distinct viewers take part within about a minute (10, scaled to 60% of the current audience for small channels, at least 3; **Proposed**). It lasts 5 minutes; each level (multiples of the starting threshold in distinct participants, up to 5) adds 75 seconds, capped at 10 minutes. Each level plays a celebration. When it ends, everyone who took part (except channel-banned viewers) gets 20 Engagement Valor per level reached, capped at 200 a day per channel (**Proposed**). There is a 30-minute cooldown between Surges. Surge never touches MAGNet.

Part 2, the integration gateway, OBS bridge and Game SDKs (October 6, 2026; migration 0038, `gateway.rs`, `tests/streams/gateway.rs`, [`integrations/`](../integrations/README.md)):

- **Scoped tokens:** Creator Studio → Board → Connections creates up to 10 tokens, each for the **OBS bridge** or a **game**. A token is shown once (only its digest is stored), reaches one channel's board events and state, and can be disconnected at any time: new connections stop at once and open ones close within 30 seconds.
- **The gateway** (`/api/integrations/ws`, token as a bearer header or `?token=` for browsers): sends the published board on connect, then every press (with the viewer's name and text), joystick move, Skill/combo/Surge effect, new version, pause and state change. Clients may send 10 messages a second. Bridges and games never touch the database, Redis or internal services.
- **Games update the board:** a game token can set a control's label, make it unavailable (viewers' presses are refused) and set a goal's progress; viewers' panels update at once, and publishing a new version clears it. Changes are validated and applied in full or not at all.
- **The S.V.E.R bridge** (`integrations/bridge`): a small open-source Node app on the streamer's PC that connects to OBS's own WebSocket server (v5, password handshake) and runs configured steps per control, Skill or Surge: switch scenes, show, hide or toggle sources, enable filters, wait, with per-control cooldowns.
- **SDKs:** JavaScript/TypeScript (no dependencies; browsers and Node 22+), Unity (`tv.sver.board`, events on the main thread) and Unreal Engine 5 (`SverBoard` plugin, Blueprint events). The example game, Crowd Runner, uses the JavaScript SDK: viewers make the runner jump, steer it and shout; the game disables Jump while airborne and fills the coins goal. The JavaScript SDK and the bridge are tested in CI against fake servers that speak the real protocols, and the gateway by the API suite; the example game was checked in a browser. The Unity and Unreal SDKs are written against those engines' APIs but have not been compiled yet (no engine on the build machine), and the engine example games are still to come.

## Not in this module

- Quests, achievements, XP and levels (Progression, Phase 2).
- Auto-highlights based on audio (it needs decoding).
- Marketplace packs for boards, and mouse controls.

## Legacy notes

Reviewed October 3, 2026. Kept: the board, screen and control model, templates, safe test mode, publish checklist, preview tokens, panic disable, overlay tokens, faction rally, emote combos and the command shape. Fixed: games and the bridge connected to the server's Redis directly; Favor charge and press weren't one transaction; guest tokens could press and count toward discovery; joystick input had no limit; effects ignored video delay and could appear twice; two separate poll systems; live boards were edited in place. Changed: Surge is now participation-driven. Dropped: Valor-wagered predictions, paid poll weighting, the money-driven Surge, spending leaderboards, and CrowdSync signals in MAGNet.

## Done when

A streamer builds a board from a template, tests it safely and publishes it; a verified viewer presses controls that spend Engagement Valor and trigger effects in sync with the video for both WebRTC and CDN viewers; the OBS overlay and bridge work with scoped tokens; a webhook fires; guests, banned viewers and excluded sessions can't press; rate limits hold, including joystick; a crash test never charges without a press; a viewer buys and plays a Skill and the streamer is credited; polls, predictions (Engagement Valor only) and their stream-time windows work; a faction rally and an emote combo play; a Surge starts from distinct participants, levels up and awards capped Engagement Valor; counter widgets update from the board and chat; nothing here changes MAGNet selection. Part 2: an example game in each SDK receives presses and updates the board through the gateway.
