# Module 7: CrowdSync

**Closed October 15, 2026 by user acceptance.** Boards, polls, predictions, counters, Skills, rallies, emote combos, Surge, the OBS bridge and the JavaScript SDK were already built and tested; the last item, the engine SDKs, now compiles in Unity 6.6 and Unreal Engine 5.8, and both Crowd Runner example games ran end to end against the gateway.

Scoped October 3, 2026 by Joe. Builds after Module 6 (Support), which brings the Engagement Valor that board presses spend. **Status: phases 1 to 3 built October 6, 2026 (boards; polls, predictions and counters; Skills, Faction Rally, emote combos and Surge), and part 2 (the integration gateway, OBS bridge and Game SDKs) built the same day; see "Implementation status". The Unity and Unreal SDKs were compiled (Unity 6.6, Unreal Engine 5.8) and their Crowd Runner example games run end to end against a local API, October 15, 2026.**

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
- **Reliability:** the Engagement Valor charge and the press are recorded in one database transaction, so a crash can never charge without a press. Webhook delivery goes through a Postgres-backed outbox with retries. Live effects (overlay, page, gateway) are published in-process right after the commit: a crash in that instant can lose one animation, never the press record, and "Game confirms" presses that never reach the game are refunded after 60 seconds. No Redis.

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
- **Board controls with the streamer's own effects (October 11, 2026):** a control's effect can be one of the channel's custom Skills (`custom:<id>`, chosen under "Your Skills" in the editor): pressing it plays that Skill's uploaded image and/or sound, with no Valor charged beyond the control's own cost. Saving or publishing refuses another channel's Skill or one without media, and a deleted Skill plays no effect.
- **Not yet:** the OBS scene bridge and the Game SDK are later phases. Effects sync to an estimated player delay; exact sync to the HLS program date-time is a later refinement if measurements call for it.

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
- **SDKs:** JavaScript/TypeScript (no dependencies; browsers and Node 22+), Unity (`tv.sver.board`, events on the main thread) and Unreal Engine 5 (`SverBoard` plugin, Blueprint events). The example game, Crowd Runner, uses the JavaScript SDK: viewers make the runner jump, steer it and shout; the game disables Jump while airborne and fills the coins goal. The JavaScript SDK and the bridge are tested in CI against fake servers that speak the real protocols, and the gateway by the API suite; the example game was checked in a browser. The Unity and Unreal SDKs compile in Unity 6.6 and Unreal Engine 5.8, and both report a press's id and "Game confirms" flag plus capture/release results. Their Crowd Runner ports (`integrations/example-unity`, `integrations/example-unreal`) were run against a local API: jump, shout, joystick and a confirmed boost reached each game, and each game disabled Jump while airborne and filled the coins goal. Engine builds aren't in CI (no engines on the CI runners).

## Additions from the Mixer review (October 6, 2026)

Hold-then-capture charging for controls a game or the bridge must confirm, viewer groups, an input cap and a game ready state are specified in [DEVELOPER_PLATFORM.md](DEVELOPER_PLATFORM.md), Section 3. The input cap, ready state and pricing guidance (migration 0050) hold-then-capture (migration 0051) and groups (migration 0052) were built October 8, 2026 (`tests/streams/gateway.rs`).

## Creator and developer friendly (specified November 11, 2026)

From a review against MixPlay: the server side holds up, but a developer can't ship a board with a game, connecting a game is copy-paste, and the builder and viewer panel are plain forms. Built in this order, one pull request each. Numbers marked **Proposed** are defaults Joe can change.

### 1. Board library, import/export and game-defined boards

- **Library:** a channel keeps up to 20 (**Proposed**) saved boards beside its draft. Studio → Board: "Save draft to library" (named), "Load into draft" (asks before replacing unsaved edits), delete. A library board is just a validated board definition; nothing in it is live.
- **Import and export:** "Export" downloads the draft as a `.sverboard.json` file (`{"sver_board": 1, "name", "board"}`); "Import" reads one into the draft. An imported board goes through the same validation as any save. Effects that belong to another channel (`custom:<id>`) are replaced with "none" on import, and the builder says which controls changed.
- **Game-defined boards:** a game sends `{"type": "define", "id", "name", "board"}` over the gateway and that board becomes the live board while the game is connected, as on MixPlay.
  - **The streamer opts in per connection.** A game token (and an app's `board:control` grant) has "May set my board", off by default, chosen when the token is created and changeable later. Without it, `define` is refused with a clear error.
  - The board is validated like a draft. It may not use the channel's custom Skills as effects (a game can't know them) and every cost is capped at the channel's typical viewer balance when one is known (**Proposed**), so a game can't overcharge a streamer's viewers.
  - The first `define` of a connection saves the streamer's published board aside; when the game disconnects (or misses two checks), that board comes back as a new version. The streamer's draft is never touched.
  - A game board is also saved to the library under the game's name, so the streamer can load, edit and publish it as their own.
  - At most one `define` every 10 seconds per connection. Publishing rules are the normal ones: a new version, goals start over, held presses on removed controls are released.
  - Studio shows "Your board is set by <connection name>" with a "Take it back" button, which restores the streamer's board and switches the permission off.
- SDKs: `define(name, board)` in JavaScript, Unity and Unreal; Crowd Runner defines its own board when allowed.

**As built (November 11, 2026; migration 0158, `boards.rs`, `gateway.rs`, `tests/streams/boards.rs`, `tests/streams/gateway.rs`):**

- **Library:** Studio → Board → "Library and files" lists the saved boards with "Load into draft" (asks first when there are unsaved edits) and "Delete", and saves the board being edited under a name; saving under a name already used replaces that entry. 20 boards per channel.
- **Files:** "Export" downloads the board being edited as `<name>.sverboard.json`. "Import" checks the file like any save and makes it the draft. Effects that aren't the channel's own Skills (another channel's, or a deleted one) become "none", and Studio lists the controls that changed; the same happens when a library board is loaded.
- **"May set my board":** a tick box on each game connection in Studio → Board → Connections, also offered when the token is created. For an app with `board:control` it is a tick box in Settings → Connected apps; since Section 2 it is also on the approval screen. Off by default. Switching it off while that game's board is live brings the streamer's board back.
- **`define`:** refused without the permission, for bridges, for a restricted channel, for a board that fails the builder's rules or has nothing viewers can use, for any custom Skill effect, and for a cost above the channel's typical viewer balance (when at least 5 recent viewers make one known). A refused board doesn't use up the 10-second allowance, so a game can fix it and send it again. Messages on the gateway may now be 64 KB (the size limit of a saved draft), so a full board fits.
- **Whose board is live:** the streamer's published board is saved aside by the first `define` and comes back as a new version when that game's connection closes, when it has missed two 30-second checks (found within 70 seconds, also after an API restart), or on "Take it back". If the streamer had nothing published, nothing is published again. If the streamer publishes their own draft while a game's board is live, that publish stays and nothing is restored later. A game setting the board and the board coming back are both in the channel moderation log.
- **Library copy:** each connection keeps one library entry, named by the `name` in its `define` and marked "from a game"; a later `define` replaces it. When the library is full the game's board still goes live but isn't saved.
- **Every new version** (the streamer's publish, a game's board, a restored board) now releases held "Game confirms" presses on controls that are no longer on the board straight away, instead of after their 60 seconds.
- **SDKs:** `define(name, board)` in JavaScript (tested against the fake gateway), `Define` in Unity and `DefineJson` in Unreal. The Unity and Unreal additions were not compiled (no engines on this machine or in CI). The browser Crowd Runner defines its own board on every connect and falls back to the streamer's board when it isn't allowed; the Unity and Unreal example games were left as they are.

### 2. "Connect to S.V.E.R" in the SDKs

- The SDKs sign a game in with device sign-in at `sver.tv/go` and the `board:control` scope, so the streamer types a code instead of pasting a token. Tokens from Studio keep working.
- A game registers once as a developer app; players never see a client secret (public client, PKCE or device flow only).
- The approval screen has the "May set my board" choice from Section 1.
- The SDK stores and refreshes the grant; `connect()` falls back to showing a new code when the grant is gone.

**As built (November 11, 2026; migration 0159, `devapps.rs`, `tests/streams/devapps.rs`, `integrations/sdk-js/test.mjs`):**

- **Sign-in in the SDKs:** a game gives its app's client ID instead of a token (`new SverBoard({ clientId, onCode })` in JavaScript, the `clientId` field and `SignInCode` event in Unity, `SignIn(ClientId)` and `OnCode` in Unreal). On connect the SDK asks for a device code with the `board:control` scope, hands the game the code and `https://sver.tv/go` to show, and asks for the tokens no faster than the interval S.V.E.R gives (5 seconds, plus a quarter second). It then connects to the gateway with the access token and the client ID. No SDK takes or sends a client secret. Tokens from Studio work as before.
- **The stored sign-in:** JavaScript keeps the tokens in `localStorage` under `sver-board:<client ID>` in browsers and in memory elsewhere; `store` replaces it (`fileStore(path)` for Node games, or any `{get, set}`). Unity keeps the refresh token in PlayerPrefs (or `LoadSignIn`/`SaveSignIn`), Unreal in a file under the project's Saved folder (or `SignInFile`). An access token within a minute of its hour is renewed with the refresh token before connecting, and a connection S.V.E.R refuses before `hello` renews it before the next try.
- **When the sign-in is gone:** a refresh S.V.E.R refuses (the app was removed in Settings → Connected apps, 60 days unused, a reused token) clears the stored sign-in and shows a new code. Close code 4001 on an open connection clears it too and stops reconnecting, so the next `connect()` shows a code. A code nobody entered in 10 minutes is replaced by a new one. "Cancel" at `sver.tv/go` and an unknown or suspended app are errors for the game to show; the SDK doesn't ask again on its own. `signOut()` (`SignOut` in Unity and Unreal) revokes the grant and forgets it.
- **Approval screen:** `sver.tv/go` and `/oauth/authorize` show a "May set my board" tick box, off by default, when the app asks for `board:control` (not for S.V.E.R Desktop, which connects as the bridge). The choice is stored on the grant when the tokens are issued. Approving the same app again sets it to the new choice; leaving it off while that game's board is live brings the streamer's board back, like switching it off in Settings.
- **Server:** a public client (an app with no secret) could already use device sign-in, refresh and the gateway with `board:control`. One gap for browser games: `POST /api/oauth/device`, `/api/oauth/token` and `/api/oauth/revoke` now answer with `Access-Control-Allow-Origin: *`, so a game on its own site can read them. They take no cookie, only the public client ID and codes.
- **Crowd Runner (browser)** has a "Connect to S.V.E.R" button beside the token field, and "Disconnect". It needs the game's client ID in the address (`index.html#client=…`), because no official Crowd Runner app is registered yet.
- **Not verified:** the Unity and Unreal additions were not compiled (no engines on this machine or in CI), and the example games for Unity and Unreal still take a token. The JavaScript sign-in is tested against a fake of the three endpoints, not yet against sver.tv. The SDK's wait after `slow_down` (5 seconds more, RFC 8628) is not covered by its test.

### 3. Catalog

- **Publishing:** the owner of a developer app submits a listing: name, one-line summary, description, link (store page or site), icon, up to 4 screenshots, and either a board file or "this game sets its own board". Listings are reviewed by staff before they appear and on every change, like emotes; staff can remove one, and Take It Down applies to its images.
- **Browsing:** `/crowdsync` lists approved games and boards, searchable, with each developer's page. No ranking by spend and no paid placement.
- **Installing:** "Add to my boards" copies a listing's board into the streamer's library. For a game that sets its own board it explains how to connect the game.
- Listings show how many channels have installed them (a count, never who).

**As built (November 11, 2026; migration 0160, `catalog.rs`, `tests/streams/catalog.rs`):**

- **Publishing:** Settings → Developer has a "Catalog listing" form under each app, one listing per app: name (3–40 characters), one-line summary (up to 120), description (up to 2,000, plain text), an `https://` link, up to 4 screenshots, and the board. The board is the owner's draft, one of their library boards, a `.sverboard.json` file, or "this game sets its own board". A board must pass the builder's rules and have at least one control; effects that use the owner's own Skills become "none", since no other channel has them. Text passes the platform word filter. The owner can delete the listing; deleting the app deletes it too.
- **Icon:** the listing uses the app's icon as it was when the listing was sent in, so a new icon is reviewed with the next change. There is no separate listing icon.
- **Screenshots** go through the image pipeline (fingerprinted, stored at 400 and 1600 pixels, 5 MB each). One that is uploaded but never sent in is deleted after a day. A Take It Down removal takes the image off every listing that uses it, and a held image is never shown.
- **Review:** every new listing and every change waits at `/admin/catalog` (also counted on the staff home page). Staff approve, reject with a reason, or remove a listing with a reason; the owner sees the reason beside the form and can send the listing in again. Each action is in the audit log (`catalog_approve`, `catalog_reject`, `catalog_remove`). While a change waits or after it is rejected, the public sees the last approved version. Sending in exactly what is already approved needs no second review.
- **Hidden:** a listing isn't public while its app is suspended or its developer's channel is deleted, restricted or internal.
- **Browsing:** `/crowdsync` lists approved listings by first approval, newest first, 30 a page, searched by name. There is no other order. `/crowdsync/{name}` shows the screenshots, description, link, a preview of the board that can't be pressed, the developer's display name linking to their channel, and the count. The address comes from the listing's name and stays the same once the listing has been public. Under-18 developers (and accounts with no birth date) aren't named. Links: the site footer, `/developers` and Studio → Board.
- **Installing:** "Add to my boards" needs a verified email and two-factor sign-in. It copies the board into the channel's library, checked like an imported file, marked "from the catalog". Adding it again replaces that entry instead of making a second one. A full library (20 boards) refuses with a message saying to delete one first. A game that sets its own board has steps for connecting it ("Connect to S.V.E.R", the code at `sver.tv/go`, "May set my board") in place of the button.
- **Count:** distinct channels that added the board. For a game that sets its own board, it also counts channels with the game connected (`board:control`). Never who.
- **Reports:** there is no "Report" button on a listing yet. Adding a report type touches the report queue, strikes, appeals and overturns, so it was left out; staff remove a listing from `/admin/catalog`, and Take It Down requests cover its images.
- **Not verified:** the pages were type-checked and built but not opened in a browser, and there are no screenshots yet.

### 4. Builder and Studio layout

- The builder becomes a visual grid: drag to place and resize on the 4-column grid, click a control to edit it in a side panel, duplicate, undo and redo, and a live preview of the viewer's panel at desktop and phone width.
- Buttons get a style: one of the palette colours and an optional icon from the icon set.
- Studio → Board splits into tabs: Build, Test and publish, Skills, Overlay, and Advanced (webhook, input cap, connections).

**As built (November 11, 2026; no migration, `boards.rs`, `components/BoardBuilder.tsx`, `lib/board-builder.ts`, `tests/streams/boards.rs`):**

- **Grid:** each screen is drawn as viewers see it on the 4-column grid. Drag a control onto another to take its place; drag its right edge to set its width (1 to 4 columns). Below 600 pixels the edge handle is hidden and the width menu in the side panel does the same job. Controls can be reached with the keyboard (Tab, then Enter to pick one), and the side panel has Move earlier and Move later.
- **Side panel:** click a control to edit it: every setting it had before, plus colour and icon, Duplicate (a copy right after it, under a new ID, unless the screen has 24), Test and Remove.
- **Undo and redo:** buttons beside Save draft, and Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y on the Build tab outside a text field (fields keep the browser's own undo). Typing in one field, or one drag of an edge, is one step. The last 100 steps are kept, and saving doesn't clear them; undoing past a save marks the draft unsaved again. Loading a template, a library board or a file is a step too.
- **Preview:** "What viewers see" draws the current screen at desktop width or at phone width (two columns, as viewers' phones show it).
- **Button style:** buttons and text inputs may have `color` (`steel`, `ember`, `violet`, `gold`: the site's neutral, Myria, Aetheron and gold palettes) and `icon` (12 from the site's icon set: heart, music, bell, shield, flag, clock, message, check, cross, ban, lock, search). Both are optional and checked on every save, import, catalog listing and game `define`; other kinds ignore them. Viewers, the catalog preview and the builder draw them. The lists are fixed in `boards.rs` and sent to Studio with the board.
- **Tabs:** Build (builder, library and files), Test and publish (test stage with a Test button per control, checklist, publish, pause), Skills (categories and your own Skills), Overlay, and Advanced (moderators, webhook, input cap, connections). Every tab stays loaded when hidden, so a token or URL shown once survives switching tabs.
- **Checked:** in a browser against a stand-in API: picking, typing as one undo step, undo and redo, duplicate, edge resize, drag to reorder, Ctrl+Z, the phone preview, saving, the tabs, and phone width with no sideways scroll.

### 5. Developer docs, sandbox and press log

- `/developers/crowdsync`: quickstart, the gateway protocol (moved out of the repository README), charging rules, Crowd Runner walkthrough, and the publishing rules for the catalog.
- **Sandbox:** a game token works with the board's test mode: Studio's "Test" presses reach the connected game as real presses marked `test: true`, charge nothing, and need no live stream. The SDKs also ship a local fake gateway (the one CI uses) for offline work.
- **Press log:** Studio → Board → Advanced shows the last 100 presses and gateway messages for the current stream (control, viewer, outcome), and each connection's state.
- SDK packages: npm (`@sver/board`), a Unity package URL and an Unreal plugin zip on each release.

### 6. Viewer panel, analytics and controls

- **Panel:** on desktop the board can dock beside chat or sit under the player (the viewer's choice, remembered); buttons show a cooldown ring and a press ripple. On phones it is a tab beside Play and Chat.
- **Analytics:** Studio → Analytics gets a Board section: presses and distinct viewers per control per stream, Engagement Valor spent, and refunds. Never per-viewer spend.
- **Crowd input:** a joystick can be "crowd average": S.V.E.R sends the game one averaged move 10 times a second instead of every viewer's.
- **New controls:** slider, and a choice button (pick one of up to 4). Click-on-video and custom HTML controls stay deferred ([DEVELOPER_PLATFORM.md](DEVELOPER_PLATFORM.md), Section 4).

## Not in this module

- Quests, achievements, XP and levels (Progression, Phase 2).
- Auto-highlights based on audio (it needs decoding).
- Marketplace packs for boards, and mouse controls.

## Legacy notes

Reviewed October 3, 2026. Kept: the board, screen and control model, templates, safe test mode, publish checklist, preview tokens, panic disable, overlay tokens, faction rally, emote combos and the command shape. Fixed: games and the bridge connected to the server's Redis directly; Favor charge and press weren't one transaction; guest tokens could press and count toward discovery; joystick input had no limit; effects ignored video delay and could appear twice; two separate poll systems; live boards were edited in place. Changed: Surge is now participation-driven. Dropped: Valor-wagered predictions, paid poll weighting, the money-driven Surge, spending leaderboards, and CrowdSync signals in MAGNet.

## Custom Skills

Built October 10, 2026 (Joe's decision: fixed price tiers; `skills.rs`, migration 0091).

- A streamer adds up to 10 Skills of their own in Studio → Board, each with an uploaded image (shown as a sticker on stream; the same image pipeline as every upload, fingerprinted and covered by Take It Down) and/or a sound (re-encoded to at most 10 seconds of mono WAV, as for alert sounds).
- Prices are only the built-in Skills' price points (30, 50, 75, 100, 150, 200 or 300 Valor); the streamer earns the same 0.8¢ per Valor, paid exactly like a tribute. A Skill without an image or sound isn't on sale.
- It counts as a sticker when it has an image, otherwise as a sound, so the category switches and the board pause apply. Names (1–30 characters) pass the platform word filter; chat shows the name on the Skill badge.
- Deleting a Skill takes it off sale at once; past messages keep its name.

## Done when

A streamer builds a board from a template, tests it safely and publishes it; a verified viewer presses controls that spend Engagement Valor and trigger effects in sync with the video for both WebRTC and CDN viewers; the OBS overlay and bridge work with scoped tokens; a webhook fires; guests, banned viewers and excluded sessions can't press; rate limits hold, including joystick; a crash test never charges without a press; a viewer buys and plays a Skill and the streamer is credited; polls, predictions (Engagement Valor only) and their stream-time windows work; a faction rally and an emote combo play; a Surge starts from distinct participants, levels up and awards capped Engagement Valor; counter widgets update from the board and chat; nothing here changes MAGNet selection. Part 2: an example game in each SDK receives presses and updates the board through the gateway.
