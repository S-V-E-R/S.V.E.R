# Module 7: CrowdSync

Scoped October 3, 2026 by Joe. Builds after Module 6 (Support), which brings the Engagement Valor that board presses spend. Not started.

CrowdSync is how viewers change what happens on stream. It brings back the idea of interactive boards and Skills from an earlier platform, rebuilt from the legacy CrowdSync design (whose in-page board worked) without its flaws: game and bridge access to server internals, no protection against bots, no account for video delay, and money that could buy attention.

It follows the closure rule: specify, build, then test against "Done when". Numbers marked **Proposed** are defaults Joe can change.

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

## Game SDK (part 2)

- Lets a game receive board input (button presses, joystick, text) and send state back to the board (labels, goal progress, button availability).
- Games connect only through a scoped, token-authenticated WebSocket gateway on S.V.E.R. They never connect to S.V.E.R's database, Redis or internal services.
- First SDKs: JavaScript/TypeScript, Unity (C#) and Unreal (C++), with an example game. Other languages follow on request.
- S.V.E.R Plays can move its votes onto a CrowdSync board once the SDK exists.

## Not in this module

- Surge (a collective hype event); its legacy version was money-driven and needs its own decision.
- Quests, achievements, XP and levels (Progression, Phase 2).
- Auto-highlights based on audio (it needs decoding).
- Marketplace packs for boards, and mouse controls.

## Legacy notes

Reviewed October 3, 2026. Kept: the board, screen and control model, templates, safe test mode, publish checklist, preview tokens, panic disable, overlay tokens, faction rally, emote combos and the command shape. Fixed: games and the bridge connected to the server's Redis directly; Favor charge and press weren't one transaction; guest tokens could press and count toward discovery; joystick input had no limit; effects ignored video delay and could appear twice; two separate poll systems; live boards were edited in place. Dropped: Valor-wagered predictions, paid poll weighting, money-driven Surge, spending leaderboards, and CrowdSync signals in MAGNet.

## Done when

A streamer builds a board from a template, tests it safely and publishes it; a verified viewer presses controls that spend Engagement Valor and trigger effects in sync with the video for both WebRTC and CDN viewers; the OBS overlay and bridge work with scoped tokens; a webhook fires; guests, banned viewers and excluded sessions can't press; rate limits hold, including joystick; a crash test never charges without a press; a viewer buys and plays a Skill and the streamer is credited; polls, predictions (Engagement Valor only) and their stream-time windows work; a faction rally and an emote combo play; nothing here changes MAGNet selection. Part 2: an example game in each SDK receives presses and updates the board through the gateway.
