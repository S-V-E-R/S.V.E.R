# Module 3: Live streams and chat

Started October 3, 2026 at the user's direction. **Status: stream backend and Creator Studio implemented locally; integrated Rust/Postgres/SRS ingest now passes with real synthetic media. Player/accounting, chat/moderation, OBS/browser/CDN and live media acceptance remain open.** Module 1 is closed by user acceptance. Module 2 continues independently; starting this module does not declare Profiles complete.

This document expands Module 3 of [PLATFORM_PLAN.md](PLATFORM_PLAN.md), the security contract in [LOGIN.md](LOGIN.md), and the approved profile/moderation rules in [PROFILES.md](PROFILES.md). Existing plan requirements are identified below. Details marked **Proposed** are implementation defaults for review, not recorded user decisions. Engineering measurements remain open even if the product defaults are accepted.

## Required launch scope

- A verified account with working authenticator MFA can stream from OBS over RTMP, without an application. MFA is optional for viewers.
- A channel has one stream key and at most one active broadcast. Regenerating the key invalidates it immediately and disconnects its publisher.
- Title up to 140 characters and a category are editable before and during a broadcast. A disconnected encoder has **60 seconds** to resume the same broadcast.
- Small audiences use direct SRS WebRTC at about one second. Above a measured capacity threshold, the player switches automatically to delivery through Bunny with a **3–5-second** target. Standard HLS remains a compatibility fallback.
- No video re-encoding. SRS handles the audio conversion needed for WebRTC. Studio shows OBS guidance and warnings for B-frames, keyframe interval and excessive bitrate.
- Viewer counts represent playback sessions, update about every ten seconds, and do not count merely opening chat or the page.
- Verified signed-in users can send chat messages up to 500 characters. Join loads the latest 100 visible messages. Standard emoji are supported.
- Owners appoint channel moderators. Owners/moderators delete messages, time out users for 1 minute through 14 days, and ban/unban. Slow mode is off or 3–120 seconds; link blocking exempts owners/moderators; banned words automatically reject matching messages.
- Launch includes stream and chat reports/moderation. Reuse Module 2's staff permissions, report feedback, strikes and appeals. Its level-three strike opens a separate account-ban review; it never automatically bans an account.

Added October 3, 2026 by Joe: chat mentions, replies, a pinned message, badges and custom emotes; go-live alerts; raids; and hosting. They are specified in "Social features" below.

Viewbot detection was moved into this module by Joe on October 3, 2026 ("Viewer integrity" below).

Deferred by the platform plan: SRT ingest, animated emotes, alerts/overlays, CrowdSync and chat replay. External-platform chat is Linked chat ([LINKED_CHAT.md](LINKED_CHAT.md)), built right after this module closes. VODs/clips are Module 7. Faction influence belongs to Module 4. Subscriptions, Valor (Purchased and Engagement), co-streams and payouts are Module 6, Support ([SUPPORT.md](SUPPORT.md)). Discovery rotation and MAGNet belong to their own modules; this module supplies live state and categories without adding ranking rules.

## Investigation evidence

Read-only inspection on October 3 found:

| Area | Evidence and implication |
| --- | --- |
| Existing media runtime | `srs.service` and `srs-hook-proxy.service` are active on the production server. SRS reports **7.0.136**, with one active stream at inspection. This is inventory, not rebuild playback acceptance. |
| Existing configuration | `/opt/srs/trunk/conf/sver.conf` has `hls_fragment 1`, `hls_window 30`, `hls_wait_keyframe on`, `rtmp_to_rtc on`, and `gop_cache off`. No values were changed. |
| Legacy delivery settings | The legacy backend has SRS ingest/API and Bunny CDN URLs configured. `SRS_LLHLS_URL` and `BUNNY_CDN_TOKEN` are unset in that runtime. This does not establish the complete CDN configuration or prove absence of another packager. |
| Preservation | Both `sver-plays.service` and `sver-plays-obs.service` remain active. Existing media callbacks, streams, recordings and retention jobs must be preserved. |
| Current rebuild | Login provides `authorize_streaming`, recent-authentication/MFA proof, opaque sessions, encryption, rate limits and an existing worker. Module 2 has since added local migration `0004_profiles.sql` and profile/safety APIs and pages; its separate import/deployment work continues. |
| Latency/capacity | Short local capture-to-decode measurements now exist below. Neither the CDN/browser latency target nor the WebRTC capacity threshold is established. |

SRS documents RTMP-to-WebRTC with AAC-to-Opus conversion and WHEP playback. Its HLS documentation warns about segment, encoder and player-buffer latency; an fMP4 setting alone does not establish a working low-latency delivery path. Validate the actual manifest, player and CDN behavior before making latency claims. [SRS WebRTC](https://ossrs.io/lts/en-us/docs/v7/doc/webrtc), [SRS HLS](https://ossrs.io/lts/en-us/docs/v7/doc/hls).

The code-level source map and carryover decisions are in [LIVE_STREAMS_LEGACY.md](LIVE_STREAMS_LEGACY.md). Legacy CodeGraph hit its 45-second queue limit, so bounded reads of the active source, schemas and tests supplied the evidence. No legacy tests were executed.

### Local media protocol proof — October 3

[`scripts/check-media.cjs`](../scripts/check-media.cjs) publishes synthetic H.264/AAC into disposable SRS containers. [`scripts/media-webrtc.py`](../scripts/media-webrtc.py) receives real WebRTC media. The [aggregate receipt](media-proof-local.json) records the latest successful run, pinned image IDs, input settings and measurements. See [local run instructions](OPERATIONS.md#module-3-local-media-proof).

Completed checks: SRS keeps the public stream name separate from the private `?key=` publish parameter; missing, wrong and duplicate credentials fail; playback manifests/segment paths omit the key; a second publisher cannot displace the first; actual HLS video and WebRTC audio/video decode; reconnect changes the media client; SRS kick plus fixture revocation rejects republish; a rotated old key and an unavailable hook both deny publish. The fixture's authorization is deliberately controlled: it does not exercise Login, profile eligibility, database transactions or the required persisted 60-second reconnect lifecycle.

An optional comparison uses the already-installed legacy LL-HLS worker image, unchanged. Its real CMAF parts and blocking reload work, malformed reload parameters fail, and the old publisher route disappears after reconnect. A low-latency receiver starts at the latest independent part and feeds successive parts into FFmpeg. The ordinary FFmpeg HLS receiver uses whole segments and buffers considerably more; neither receiver is a browser player. The worker reads completed one-second SRS HLS segments before producing 500-ms parts, so part duration alone cannot establish end-to-end latency.

Short local runs measured WebRTC around **0.12 seconds p95** and plain HLS around **3.2–3.4 seconds p95**. Part decoding measured **4.76–5.26 seconds p95**; full-segment playback through the LL-HLS reference measured roughly **7.1–12.9 seconds p95**. These are 40-frame capture-to-decode samples, not the sustained capture-to-display acceptance test. Even the fastest part result leaves little margin for CDN/browser delay. The receipt deliberately records `latency_acceptance: false`. Investigate packager/input buffering and qualify the actual browser/CDN path before adopting this worker or claiming the 3–5-second target.

The local SRS image is **7.0.157**, while the inspected live server runs **7.0.136**. Loopback-only ICE candidates are explicitly enabled in the test receiver; this does not prove public UDP/NAT connectivity. OBS UI compatibility, HLS audio decode, browser playback/fallback, Bunny token/cache/query behavior, real bitrate/region coverage, sustained latency and load capacity remain untested. No current publisher, legacy source/configuration, account, production service or database was changed by the proof.

Bunny's official documentation confirms directory-scoped HMAC-SHA256 tokens, inherited path-based authentication for relative media URLs, and `token_ignore_params=true` for queries added by a player. This establishes a documented mechanism, not the live pull-zone configuration. Actual `_HLS_msn`/`_HLS_part` forwarding, unbuffered blocking responses, cache policy, expiry renewal and origin isolation still need a separate staged check. [Bunny advanced token authentication](https://bunny.net/docs/cdn/security/token-authentication/advanced).

### Integrated Rust/SRS ingest proof — October 3

[`apps/api/crates/sver/tests/streams/real_media.rs`](../apps/api/crates/sver/tests/streams/real_media.rs) connects the actual Rust router and isolated Postgres schema to a disposable SRS 7.0.157 instance through an authenticated hook proxy. Creator requests exercise the router in process; SRS callbacks arrive over HTTP. FFmpeg sends synthetic H.264/AAC. The [receipt](stream-ingest-local.json) records real publish rejection, LIVE health, competing-publisher rejection, HLS audio/video decoding, reconnect retaining the broadcast ID/start time, rotation disconnecting and rejecting the old key, and Stop confirming disconnection and refusing auto-reconnect.

This test exposed a real control-path bug: SRS returns 302 for the slashless stream collection URL. The shared HTTP client intentionally refuses redirects, so polling had failed and valid publishers timed out. Inventory now requests `/api/v1/streams/` directly, and the controlled adapter reproduces the redirect. SRS can briefly retain its exclusive publishing token after the old publisher disappears from inventory; the real encoder test retries within the existing 60-second grace, as OBS does, rather than assuming immediate release.

Run explicitly with the local development environment loaded, from `apps/api`:

```powershell
cargo test --test streams real_media::real_srs_ingest -- --ignored --nocapture
```

Prerequisites: local Docker Desktop with `host.docker.internal` reaching host loopback, FFmpeg on PATH, and the already-installed SRS digest in the test plus `nginx:1.28-alpine`. It refuses remote Docker contexts and non-local databases, uses random loopback TCP ports, synthetic accounts and temporary configuration outside the workspace, and cleans its labeled containers/network, files and schema. It is ignored in the ordinary suite/CI because those media prerequisites are separate. No production endpoint, provider or mail service is used. This proof measures no latency and does not qualify OBS UI, WebRTC through the rebuilt player, browsers, CDN delivery, capacity or the live SRS version. The earlier independent WebRTC/LL-HLS protocol receipt remains separate.

## Module 2 integration

Use user IDs as channel ownership keys; never couple ownership to a mutable username. Use the shared canonical-name resolver and the existing hidden-channel rule for unknown, internal, deleted, held or restricted accounts.

Reuse `profiles`, `follows`, `user_blocks`, `war_council`, `staff_roles`, `reports`, `strikes`, `appeals`, `interim_restrictions` and `moderation_actions`. Do not create parallel copies of profile state or a second admin role system. Stream storage uses additive migration `0005_streams.sql`, after Module 2's `0004_profiles.sql`.

- The live player replaces the channel's offline banner in the existing slot. `/{username}/live` becomes a focused player/chat layout; `/watch/{username}` redirects there using the same canonical-name and rename-hold rules.
- `/following` puts live channels first, then retains its existing ordering. User cards and War Council tiles gain actual live indicators. War Council membership may highlight messages visually and never grants moderation authority.
- Pause the profile song while the live player plays; do not start two audio sources. Viewer-selected neutral/faction theme tokens remain shared; faction badges wait for Module 4.
- A Module 2 channel restriction also prevents broadcasting and chat participation under the proposed policy below. Its original rights to account standing and appeals remain intact.
- Feature integration follows the actual Module 2 schema/API as it lands. A prototype may use isolated fixtures, but must not claim those fixtures complete the dependency.

## Stream identity and credentials

**Implemented locally:** a random public media identifier distinct from a 256-bit publishing secret. A playback manifest, segment URL, WHEP exchange or public API response must never reveal that secret. The legacy provider used its stream key as a playback ID; that coupling is not ported.

The implemented OBS contract is a server URL ending in `/rebuild` plus a key field `<public_id>?key=<secret>`. Separate parameter forwarding works in the isolated FFmpeg/SRS proof; OBS UI acceptance remains open. An identifier without the secret fails publish authorization. Disable raw request/query logging on ingress/hooks, redact OBS diagnostics, and never store publisher-supplied paths as trusted URLs.

Store a digest for verification and an encrypted secret for repeat reveal, reusing Login's cryptography with distinct purpose, owner and generation binding. Metadata reads return only creation/revocation dates. Reveal/create/rotate use POST, exact-Origin validation, no-store responses, verified email, an eligible channel, MFA enrollment, recent primary authentication and a fresh authenticator/recovery proof. Reuse Login's `sensitive` and `authorize_streaming`; these checks are server-side.

**User-approved credential rules, October 3:** repeat reveal after recent password/OAuth confirmation and fresh authenticator/recovery proof; revoke on owner Stop, password reset, MFA disable and loss of streaming eligibility. These are implemented locally. Remaining platform-ban features are still proposed below.

- One credential row per owner; rotation changes its generation atomically. No key is created at account signup.
- Rotation commits revocation and a durable publisher-stop task together. Report a pending disconnect honestly if the SRS control call fails; retry it through the worker. Do not silently return a successful stop while the old publisher continues.
- MFA disable, loss of streaming eligibility, deletion, channel restriction or platform ban revoke the key and request immediate stop without the reconnect grace. Re-enrollment/restoration does not resurrect a revoked key.
- Password reset also revokes the publishing credential. Normal logout or an unrelated browser-session revoke does not interrupt OBS.
- Staff cannot reveal another user's key. Stream control is separate from account credential access.
- Rebuild credentials are newly issued. Legacy keys and configurations remain untouched, including sver-plays; there is no silent import, rotation or switch of its ingest.

## Ingest authorization and broadcast state

SRS callbacks are authenticated server-to-server, restricted to the expected media host, and matched against an explicit rebuild vhost/app. A browser Origin exception is limited to these authenticated callback routes. Do not expose the SRS administrative API through the public web proxy. Hook authentication failures and backend/database failures reject publishing.

Port legacy lifecycle locking and stale-callback protection to a Postgres transaction. Validate account eligibility on every publish attempt, including a repeated callback; cached success must not bypass a new revocation. Persist the publisher client ID plus a media-server instance identity so IDs reused after restart cannot match old callbacks.

| State | Entry and transition |
| --- | --- |
| OFFLINE | No open broadcast. An authorized publisher creates STARTING under the channel lock. |
| STARTING | Ingest accepted but media not yet confirmed. **Proposed:** 15-second startup deadline, then end with a diagnostic if no playable media arrives. |
| LIVE | Fresh media confirmed by server observation. Duplicate publish events from the same current publisher are idempotent. |
| RECONNECTING | Current publisher disconnected; persist one deadline at disconnect time + 60 seconds. A new authorized publisher before the deadline resumes the same broadcast ID and start time. |
| ENDED | Deadline elapsed, explicit stop, credential revocation or moderation. Never resurrect this row; a later allowed publish starts a new broadcast. |

One partial unique constraint permits only one STARTING/LIVE/RECONNECTING broadcast per owner. A different publisher is rejected while the current publisher is active. A stale unpublish for a previous publisher cannot end its replacement. Repeated disconnect callbacks do not extend the deadline. At exactly the deadline, the old broadcast has ended; serialize reconnect and expiry under the same lock.

Persist deadlines and recover them after API restart. SRS polling reconciles missing callbacks; a timer in one API process is not authoritative. Broadcast identity survives a reconnect, while media connection identity changes. Old cached segments must never become the new connection's live output; use non-reused segment identities and test this across restart/rotation.

Studio's Stop action closes the broadcast and revokes its key so OBS cannot instantly auto-reconnect against the owner's intent. It shows that consequence on the action and offers a new key after step-up. Media disconnection stays pending until inventory confirms the publisher is absent; a successful control acknowledgement alone is insufficient.

## Metadata, categories and OBS health

- Title: trimmed plain text, 1–140 Unicode characters; reject control characters and blank content. Escape at rendering. **Proposed:** default to "{username}'s stream" until edited.
- Category: required active catalog ID. Reuse the legacy category/genre vocabulary where compatible with the platform's faction genre map; no faction ownership or weighting yet. **Proposed:** begin with a small versioned seed catalog and staff management, without an external game-catalog dependency.
- Owner edits use a monotonic `revision`, returning 409 for a stale edit. A live title/category edit does not restart the broadcast or change its start time. Studio preserves unsaved edits and their revision during polling.
- **Proposed OBS baseline:** H.264/AAC, up to 1080p60, B-frames off, one-second keyframe interval; start testing at 6 Mbps video and 160 Kbps audio. A proposed 8 Mbps warning threshold is provisional until the media test establishes a safe cap. Never claim adaptive quality without actual additional encodes.
- Studio polls health about every five seconds. Derive bitrate and codec data from SRS and use a bounded media probe for keyframe/B-frame measurements if SRS lacks them. Display "Not measured" when data is missing. Never turn a missing measurement into a green pass.
- Report input disconnect, unsupported codec, excessive bitrate, bad keyframe interval and B-frames separately. These warnings are explanatory; load protection may reject an unsupported ingest, but must not silently transcode video.

## Player and viewer accounting (phase 3) — October 3

- `GET /api/channels/{username}/live` is public: `{"live":false}` unless the broadcast is LIVE or RECONNECTING (STARTING is never public). It returns title, category, start time, viewer count, `is_owner` and playback URLs built from the optional `STREAM_WHEP_URL` and `STREAM_HLS_URL`. No publish secret appears in it. Preferred transport is WebRTC when offered; there is no automatic scale switching until both paths pass the media test.
- `POST /api/channels/{username}/live/beat` is the playback lease. The player sends it every ten seconds only while media time advances. Signed-in viewers are keyed by account, guests by a digest of a random first-party browser ID (16–64 characters), never by IP. The owner never counts. A lease counts for 30 seconds after its last beat and is swept by the existing expiry job (`playback_leases`, migration `0006`). Rate limits: 240 beats per IP and 12 per viewer per minute.
- `LivePlayer` replaces the offline banner in the channel player slot and fills `/{username}/live`; `/watch/{name}` now redirects to the live view. It tries WebRTC (WHEP) with an eight-second startup timeout, then HLS (native, or hls.js where Media Source is needed), shows loading/reconnecting/blocked-autoplay/failed states with Retry, retries a dropped transport after three seconds, and uses native controls for keyboard, fullscreen, volume and captions. One video element is reused, so mute/volume survive a transport switch.
- Channel-ban playback denial for signed-in users (decision 1) needs the channel-ban table and lands with phase 4.
- Covered by `tests/streams/playback.rs`: unknown channel, offline/STARTING/LIVE/RECONNECTING/ENDED visibility, URL shape, no secret, guest dedupe and renewal, invalid browser IDs, wrong broadcast, owner exclusion, expiry. Not yet verified in a real browser against SRS (WebRTC/HLS start, fallback timing, autoplay) or with CDN delivery; Following/user-card live badges are not added yet.

## Account bans (phase 4c, part 2) — October 3

- Migration `0013`: `account_bans` (reason, message to the user, private staff note, issuer, optional end, optional related strike, `ACTIVE`/`LIFTED`/`OVERTURNED`). `appeals` now targets a strike *or* a ban (exactly one, enforced by a constraint), so ban appeals reuse the same table, limits and review separation.
- Staff (admin role, MFA, recent sign-in) ban from the admin user page: indefinite, or 1 hour to 365 days. Staff, internal accounts and yourself can't be banned, and an account has at most one ban in force. Issuing a ban in one transaction: restricts the account through `safety::recompute` (channel hidden, public actions refused, stream key revoked, broadcast stopped), ends every session through `auth::invalidate`, writes the audit row and queues the standing email.
- A banned account can still sign in, but the router (`bans::blocked_write`) refuses every write except `/api/auth/*` (security, logout, deletion), standing, strike and ban appeals. The check is on the account, so password reset or another provider can't get around it, and it fails closed on a database error. Reads stay available.
- A timed ban ends by database time with no cleanup job. Old sessions and stream keys are never restored.
- One appeal per ban within 14 days, 1–1000 characters. The issuer can't decide it while another MFA admin exists. Overturning ends the ban and recomputes the restriction. Staff can also lift a ban with a required note. An overturned strike flags any ban that references it for re-review (`review_requested_at`); it never cancels the ban.
- Web: "Account bans" with the appeal form on Settings → Standing, "Ban account" on the admin user page, and an Admin → Bans queue (appeals waiting, bans in force with re-review flags first, Lift).
- Covered by `tests/streams/bans.rs`.

## Stream and chat reports (phase 4c, part 1) — October 3

- Migration `0010` adds `chat_message` and `live_stream` to the Module 2 report targets. Reports, the admin queue, strikes and appeals are reused unchanged. Channel moderator actions never create platform strikes; staff decide that from the queue.
- A chat report is accepted only for a visible message. Its snapshot keeps the body, channel and send time, so the report outlives the seven-day chat expiry. Staff "remove content" tombstones the message and broadcasts the delete to open chats. It is not restored on appeal.
- A live-stream report targets the broadcast ID and is accepted only while the stream is LIVE or RECONNECTING. The snapshot records title, category, broadcast ID, start time and report time; no video is recorded as evidence. Staff "remove content" stops the stream through `streams::revoke`: the key is revoked, the broadcast ends with reason `revoked`, and a durable disconnect is queued for the worker. Video already delivered cannot be recalled, and a stop is never undone.
- Web: Report on other people's chat messages and "Report stream" on the live view, for signed-in viewers only. New labels on the admin queue filter and the reporter's history page.
- Covered by `tests/streams/reports.rs`. Account bans are below; staff username resets for impersonation are still to build.

## Channel moderation (phase 4b) — October 3

- Migration `0008`: `channel_moderators`, `channel_restrictions` (timeout with an end, ban until lifted; a new timeout replaces the old end and never clears a ban), `chat_settings` (slow mode off or 3–120 s, link blocking, up to 200 banned phrases of 1–64 characters) and `channel_moderation_log`.
- `moderation.rs` implements the approved matrix. Roles are rechecked on every request: owner; appointed moderators who are still verified and eligible; staff with the admin role and MFA. Appointing needs the owner with a sign-in in the last five minutes; removal is the owner (same step-up) or staff. Nobody restricts the owner, themselves, a current moderator or staff (the owner removes a moderator first). Moderators cannot delete the owner's, other moderators' or staff messages.
- Actions require a 1–500 character reason and commit with their audit row. Deletes are idempotent tombstones broadcast as `{"type":"delete"}` and logged once. Timeouts are 60 s to 14 days and return a retry time on send.
- Sending applies, in order: ban/timeout, banned phrases (case-folded, whitespace-normalized literal match, no exemption), links (plain-text URL/domain detection; owner, moderators and staff exempt), slow mode (same exemptions, with retry time).
- Decision 1: a channel ban also refuses signed-in playback (`banned: true`, no URLs, heartbeats not counted). Logged-out viewing is unaffected.
- Web: moderator Delete/Timeout/Ban actions in the chat panel (confirmation and reason prompts), Studio → Chat for rules, moderators, active restrictions and the log, and the player's banned and no-playback states.
- Covered by `tests/streams/moderation.rs` and a unit test for link and phrase matching.

## Chat core (phase 4a) — October 3

- `chat_messages` (migration `0007`): client-generated UUID for idempotent retries, a server `seq` for ordering, bodies 1–500 characters, swept seven days after posting by the existing expiry job.
- `GET /api/channels/{username}/chat` returns the latest 100 visible messages; `POST` sends; `/api/chat/ws?channel={username}` is the same-origin socket (Origin checked at handshake, session cookie only, 8 KiB frames, 30 connects per IP per minute, 10,000 sockets per instance). The socket subscribes before loading its snapshot, then forwards only newer `seq`s. A lagging reader is closed with code 4000 ("resync") and the client reconnects and reloads.
- One `send` path for HTTP and socket: session rechecked on every send; verified, eligible (not restricted, deleted or internal) senders only; trimmed plain text, at most four line breaks, no control characters; a block with the channel owner in either direction refuses the send; 2 per second and 20 per ten seconds per account. A retried ID returns the stored message; another author reusing an ID gets 409.
- Blocks between chatters hide each other in their own view (history and live), not for everyone.
- Fanout is one in-process `tokio::broadcast` hub: one API instance only, as specified. The web `Chat` panel sits beside the player on `/{username}/live`; if the socket cannot connect (for example through the Next development rewrite), it polls every four seconds and sends over HTTPS. Staging Nginx gets an upgraded `location = /api/chat/ws`.
- Covered by `tests/streams/chat.rs`, including a real WebSocket over TCP (dev-only `tokio-tungstenite`): origin refusal, snapshot, fanout, socket sends/acks, block filtering, malformed commands. Not yet: moderation (4b), reports/bans (4c), a real-browser run.

## Local backend and Studio implementation — October 3

`apps/api/crates/sver/src/streams.rs` and migration `0005_streams.sql` implement settings, the seeded category catalog, encrypted credentials, broadcasts, retired-publisher records and durable disconnect jobs. Account locks serialize publishing, key changes, expiry and revocation. Each callback binds the public ID and credential generation to SRS's server ID, process/boot service ID and client ID. The exact direct peer and `x-srs-secret` authenticate only the two hook routes; forwarded browser IP headers cannot satisfy that check. Publishing also checks the current private SRS version response, failing closed on an unavailable or different boot.

The five-second worker confirms H.264/AAC and increasing input bytes before marking LIVE, reconciles lost disconnect callbacks, recovers persisted startup/reconnect deadlines, and retries verified publisher disconnects. A late accepted publisher after Stop is detected from its retired identity and queued again. Deleting an account preserves its outstanding disconnect job. Polling failure means unknown media health, not an invented successful stop. Inventory is bounded to fewer than 1,000 streams; pagination and host capacity require qualification before that scale.

`/studio/stream` supplies title/category editing, key create/reveal/rotation, Stop and input health. Primary confirmation reuses password or linked-provider reauthentication; each key operation also requires a fresh MFA/recovery proof. Revealed keys stay only in component memory, hide after 60 seconds or when the tab is hidden, and late responses cannot redisplay a hidden key. Polling preserves unsaved form edits, stale writes return 409, and a stale media observation is labeled unconfirmed. Codec, dimensions and bitrate come from SRS; keyframe/B-frame measurements are explicitly unavailable until a real probe is added. OBS limits and catalog choices remain provisional engineering defaults.

`apps/api/crates/sver/tests/streams.rs` exercises the Rust API against isolated real Postgres and a controlled SRS HTTP adapter. Coverage includes step-up/proof replay, encrypted key binding, CSRF and forged callbacks, competing publishers, out-of-order events, reconnect boundaries, API/SRS restart, stale snapshots, stop/rotation races, failed or merely acknowledged disconnects, and revocation through restrictions, MFA disable, password reset and account erasure. `scripts/check-stream-studio.cjs` checks the component's edits, key lifecycle and pending/stale states. These checks are separate from the controlled-hook real-media harness; neither alone proves integrated Rust/SRS/OBS behavior or browser acceptance. Configuration and validation commands are in [Operations](OPERATIONS.md#module-3-stream-backend-and-studio-local-only).

## Playback and capacity

Use native `RTCPeerConnection` for SRS WHEP, native HLS where supported, and an established HLS browser library where Media Source playback needs one. Reuse the legacy transport fallback behavior, not its multi-provider orchestration layers. Version/pinning decisions follow the media test.

The player has explicit loading, playing, reconnecting, offline and failed states. Preserve mute/volume across a transport switch, stop the retired transport, and show an actionable retry on total failure. Handle autoplay rejection with a play/unmute control. Keyboard operation, captions-track support when tracks exist, fullscreen, mobile layout and reduced motion are acceptance items; no fabricated caption feed is supplied.

**Proposed transport policy:**

1. The server supplies allowed transports and a preferred mode. Direct WebRTC admission is bounded by both a per-broadcast limit and a global server budget, measured with multiple simultaneous broadcasts. A client cannot bypass the budget by choosing WHEP itself.
2. Attempt the preferred transport. On WebRTC connection failure or an eight-second startup timeout, move to the tested CDN/fallback path rather than reconnecting forever.
3. Evaluate audience/capacity about every ten seconds. Exceeding the measured upper boundary for 30 seconds moves viewers to CDN. A lower boundary at 70% of that value for 120 seconds permits a return; no more than one policy switch per viewer per minute. Emergency capacity protection may act sooner.
4. Do not enable automatic scale switching until both delivery paths and switch behavior pass the media test. Test mode can force a path without setting a guessed production viewer threshold.

The accepted CDN objective remains 3–5 seconds, with the original done-when requiring under five seconds for the large-stream test. The existing HLS settings and an unset legacy LL-HLS endpoint do not demonstrate this. First measure the existing stack in an isolated deployment. If it misses the target, evaluate the smallest compatible low-latency packager/CDN configuration; changing the target, vendor or video-transcoding policy is a separate user decision.

Manifest responses must remain fresh at both origin and CDN; successful immutable segments can be cached. Verify actual query forwarding, cache keys, expiry/error caching, CORS, codecs and supported player behavior. A true LL-HLS claim requires observed partial segments and the necessary playlist/server-control behavior, not just short full segments or an fMP4 extension. Bunny settings and protocol compatibility still need direct validation; documentation lookup during triage did not establish them.

Restrict origin access so stopping a stream cannot be bypassed through an old origin URL. Issue playback access only for an eligible active broadcast. A moderation stop denies new playback immediately and terminates active media sessions; previously buffered bytes cannot be recalled. Measure and record the delivery cutoff, including any CDN token/cache lifetime, rather than promising instant disappearance of already delivered video.

## Viewer count

**Proposed:** create one short-lived playback lease per viewer/broadcast, issued by the backend after playback access is allowed. Authenticated viewers deduplicate by user ID across tabs/devices; anonymous viewers deduplicate by a first-party random browser ID across tabs. Do not merge unrelated people solely because they share an IP.

Count only a started player with progressing media and a heartbeat within 30 seconds. Send a heartbeat every ten seconds while playing; pause, ended media and prolonged stalled playback stop renewing it. A transport switch keeps the same lease. The creator's own preview, health probes and chat-only sockets are excluded. Return only aggregate counts publicly; no viewer identity list.

Correlate WebRTC leases with actual SRS sessions where possible. CDN playback heartbeats are client assertions, so corroborate with delivery telemetry when available and bound lease issuance/rates. This excludes obvious page/chat counts without claiming bot-proof unique humans; viewbot detection is specified in "Viewer integrity" below. Expired lease rows are cleaned by the existing worker.

## Chat protocol and persistence

**Proposed:** one same-origin WebSocket endpoint in the existing Rust API, with one channel subscription per connection. Ordinary HTTPS mutations and socket commands call the same validation/permission logic. Use the existing session cookie; never put an account token in a WebSocket URL.

- Validate Origin at handshake; derive user and roles on the server. Guests can read public chat; only verified eligible signed-in accounts can send. Recheck revocation/permissions on writes and close or downgrade stale sessions promptly. Unknown/deleted/restricted channels use the same hidden-channel outcome as profile reads.
- After subscribe, deliver a consistent latest-100 snapshot and then ordered changes with a server cursor, so join cannot miss messages between a history fetch and subscription. Reconnect deduplicates message IDs and reloads after a cursor gap. Message deletes and changed restrictions must also survive a reconnect.
- Message fields: ID, channel ID, author public identity, body, created time and presentation role. No email, IP, credential or internal moderation note is broadcast.
- Body: 1–500 Unicode characters after trimming, no HTML, no executable markup or non-text attachments. Standard emoji remain plain Unicode. **Proposed:** at most four line breaks, 8 KiB input frame cap, two sends per second plus 20 per ten seconds per account across sockets.
- A client-generated message ID makes retrying a successful send idempotent. Success is acknowledged only after persistence. Database failure means no accepted message. Rate/slow-mode reservation and insert must not race across multiple sockets.
- **Proposed:** chat remains available while a public channel is offline. Public history is always capped at the latest 100 visible messages; ordinary message bodies expire after seven days. A report retains its separate snapshot under Module 2's retention rule. No VOD chat replay or archive UI is added.
- Bound connection count, outbound buffers and send rates. Disconnect a lagging reader with a resumable error instead of retaining an unbounded queue. Deployment initially runs one API instance; multi-instance fanout must be established and tested before adding replicas. Do not introduce Redis only to duplicate Postgres state in this first implementation.

## Channel moderation and blocking

**Proposed permissions:**

| Action | Owner | Appointed channel moderator | Platform admin |
| --- | --- | --- | --- |
| Delete a visible chat message | Yes | Yes | Yes, audited |
| Timeout/ban ordinary chatter | Yes | Yes | Yes, audited |
| Change slow mode, link block, banned words | Yes | Yes | Yes, audited |
| Appoint/remove channel moderators | Yes, step-up | No | Remove for safety, audited; owner manages appointments |
| Stop stream or restrict channel | Own stream | No | Yes, staff step-up |
| Reveal/rotate stream key | Own key, step-up | No | No access to another account's secret |
| Platform ban, strike, username reset | No | No | Yes, staff step-up |

A moderator cannot sanction the owner, themselves, another appointed moderator or platform staff. The owner can remove a moderator before sanctioning them. Staff-role changes remain operator-CLI only under Module 2; channel roles never grant `/admin` access. Appointed moderators must have verified accounts in good standing; removal/restriction takes effect on the next action, not after a long permission cache expires.

- Timeout duration is an integer 60–1,209,600 seconds. A new timeout replaces the old end time and does not clear a ban. Expiry is checked against database time without waiting for a cleanup timer.
- Bans last until lifted. **Decided:** channel bans prevent chat and signed-in playback; this requires playback-access enforcement and still cannot prevent logged-out public viewing.
- Actions require a reason of 1–500 characters. Writes and audit records commit together; broadcasts happen after commit. Retrying the same action is idempotent. Delete replaces visible content with a tombstone and does not send the deleted body to new joins.
- Slow mode exempts the owner, channel moderators and acting platform staff. The general abuse/send cap still applies. Display an accurate retry countdown.
- Link blocking examines normalized plain text for URLs/domains, including mixed case and common `www.`/scheme-less forms. Do not treat it as a complete obfuscation detector. Never fetch a link. Owners/moderators are exempt as required by the plan.
- **Proposed banned words:** up to 200 entries of 1–64 characters; Unicode case-folded literal phrase matching with whitespace normalization, no user regex. Reject matching sends before fanout and return a generic rules error. No role exemption for banned words unless the owner changes the list.
- Reuse Module 2's block relationship. A block with the channel owner prevents sending in that channel. Between ordinary chatters, hide each other's messages/history in their own view without muting them for everyone. Moderation reviewers can still inspect reported content under staff permissions. A user blocking a moderator does not defeat moderation.
- All moderation UI uses confirmation for destructive actions, clear duration/reason fields, and accessible focus restoration. Server authorization remains authoritative even when controls are hidden.

## Platform moderation and appeals

Extend Module 2 reports with LIVE_STREAM and CHAT_MESSAGE targets. Snapshot title/category, broadcast ID/time and reported message text as applicable. Do not silently begin recording video evidence: live recording/VOD retention remains separately scoped. Stream reports can be reviewed against a currently live stream; ended content without a recording is explicitly unavailable.

Retain the approved warning, 72-hour restriction, indefinite level-three restriction, 24-hour interim maximum and strike-appeal rules. Channel moderator actions do not themselves create platform strikes. A platform admin decides whether a reported incident merits one.

**Proposed account-ban completion for Module 3:**

- Staff may impose a timed or indefinite participation ban with a required reason and user-facing message. A level-three review requests this decision; it does not preselect an outcome.
- Revoke ordinary sessions, chat connections and stream credentials, stop media, hide the channel, and refuse public-content mutations/follow/report actions while banned. Allow a newly authenticated restricted session only for security, logout, deletion and account-standing/appeal routes. Password reset or alternate OAuth must not bypass the ban.
- Keep ban decisions separate from strike rows. An overturned strike triggers re-review of a related ban; it does not silently cancel an independently reasoned ban. A timed ban expires by database time and does not restore old sessions or publishing keys.
- Give each ban one appeal within 14 days, with the same text limits, review separation, audit visibility and seven-day decision target as strike appeals. Generalize the existing appeal target with migration constraints rather than copy the whole workflow.
- Staff username resets for impersonation select an available neutral replacement under the user lock, preserve identity/relationships, and disable redirect for the impersonating old name while retaining an audit/hold. Do not expose or reassign reserved internal accounts. Users see the reason and can appeal the associated moderation decision.

These proposed account-ban and username-reset semantics extend the approved Module 2 rules and require product review before their implementation. They do not change existing accounts during specification work.

## API and storage outline

Studio routes, categories and the two SRS hook routes below are implemented locally. Other routes are proposed contracts. All authenticated mutations retain exact-Origin checking and server-side ownership checks. Public responses never include publish secrets, email addresses or staff notes.

| Surface | Proposed endpoints |
| --- | --- |
| Studio | `GET/PATCH /api/me/stream`; `POST /api/me/stream/key`, `/key/reveal`, `/key/rotate`, `/stop`; `GET /api/me/stream/health` |
| Viewer | `GET /api/channels/{username}/live`; `GET /api/categories`; `POST /api/playback/{broadcast_id}/sessions`; heartbeat/end routes bound to that playback lease |
| Media | Authenticated `POST /api/internal/srs/publish` and `/unpublish`; bounded private control and health polling; public WHEP proxy limited to authorized read sessions |
| Chat | `GET /api/channels/{username}/chat` (latest 100); same-origin `/api/chat/ws`; message-delete, timeout, ban and settings mutations under `/api/channels/{username}/chat` |
| Roles | Owner-only add/remove under `/api/channels/{username}/moderators`; channel-scoped audit list with no unrelated staff notes |
| Staff | Extend Module 2 report/standing/appeal APIs; add stop-stream, account-ban and username-reset actions to its admin user surface |

New tables: stream settings/credentials, categories, broadcasts, playback leases, chat settings/messages, channel moderators and channel restrictions. Keep active-broadcast uniqueness and credential-generation checks in the database. Add durable pending media-stop records to the worker so external failures do not lose revocation work. Extend existing moderation tables for new targets and ban reviews rather than duplicating them.

On account erasure, remove credentials, leases, moderator memberships and ordinary chat bodies; preserve only the explicitly retained moderation evidence through the existing retention policy, with actor references anonymized as necessary. Stream revocation precedes destructive erasure. Legacy broadcast/chat/settings import is not implicit in the account migration; inventory and define a guarded rehearsal if it is requested. Never activate old credentials or old chat moderation grants through an unreviewed import.

## Viewer integrity (viewbot detection) — approved by Joe, October 3, 2026

Built inside Module 3 on top of the playback leases above, so every number S.V.E.R publishes or pays on is protected from day one. It replaces "viewbot detection remains deferred" in the viewer-count rules. The legacy design was reviewed: its principles carry over, but its implementation never received playback data in production, stored raw IP addresses and showed them to streamers, let a flagged streamer clear their own payout hold, and could freeze a channel's payouts because of bots someone else sent. Those are fixed here.

### Principles

- **Bots aimed at a channel never punish that channel.** Suspicious viewers are simply not counted. Penalties against a streamer (strikes, payout holds, tier removal) need staff review with evidence that the streamer or someone acting for them caused the traffic.
- **Nobody is ranked by viewer count anyway.** MAGNet never sorts by size, which removes most of the incentive to viewbot. Integrity protects what counts do drive: creator tiers, payouts, Ad Valor, faction influence and the public number.
- **No single signal decides.** A shared household, school or mobile network is normal. Decisions need several independent signals to agree.
- **Privacy first.** Raw IP addresses are never stored. Streamers see how many viewers were not counted, never who or why.
- **Fail safe for viewers.** If scoring is down, playback continues; new sessions simply wait to be counted.

### Session levels

Each playback lease gets one level, recalculated as heartbeats arrive:

| Level | Public viewer count | Trusted count (tiers, payouts, Ad Valor, influence) |
| --- | --- | --- |
| **Pending** (first 60 seconds, or until checks pass) | No | No |
| **Counted** | Yes | No |
| **Trusted** | Yes | Yes |
| **Excluded** | No | No |

- **Counted** needs: a passed Turnstile check for the session (invisible for normal browsers; signed-in verified accounts skip it), heartbeats with advancing media, and no exclusion signal.
- **Trusted** additionally needs: a signed-in, email-verified account in good standing, at least 2 minutes of watching with the page visible, and no risk signal above the private threshold. Guests are never trusted, which matches the rule that only verified accounts earn influence and Valor.
- **Excluded** sessions keep playing normally; they just don't count. A session can recover: exclusion is re-evaluated every few minutes, except for hard evidence (below), which lasts for the rest of that session.
- One account counts once per broadcast across all its tabs and devices (already true of leases).

### Signals

Collected from the playback lease heartbeats and the server, then scored by a background job:

- **Account:** signed in, verified, account age, 2FA, standing, follows the channel.
- **Network:** the client IP as reported by Cloudflare (the origin accepts traffic only from Cloudflare), looked up locally in the IPinfo Lite database (ASN and country, free for commercial use with attribution, refreshed daily). Hosting and VPN networks raise risk; residential and mobile networks don't.
- **Concentration:** many sessions on one broadcast from the same network prefix (hashed /24 for IPv4, /48 for IPv6) at once. A whole ISP sharing viewers is normal and never penalized.
- **Playback:** heartbeat regularity (perfectly metronomic timing is a bot signal), media time advancing in step with wall time, page visibility, stalls, and for WebRTC viewers, a matching live connection on SRS. CDN viewers get signed, per-lease playback URLs (Bunny token authentication) so a URL can't be shared to unregistered players.
- **Cohort:** a burst of new sessions joining within seconds that share network, browser build and behavior. A real raid (Module 3 raids) or a MAGNet handoff explains a burst and is never treated as suspicious.
- **Engagement (supporting only):** chat activity from the session's account. Absence of chat is never a penalty on its own.

Weights and thresholds live in the private tuning config loaded at runtime; the repo ships safe example values for tests and local development.

### Counts

| Count | Meaning | Used by |
| --- | --- | --- |
| Raw | All live leases | Operations only, never shown |
| Counted | Counted and Trusted sessions | The public viewer count |
| Trusted | Trusted sessions | Creator tiers, payouts and ad revenue, Ad Valor, faction influence, staff dashboards |

Snapshots of all three are stored every minute per live broadcast.

### Spikes and enforcement

- A sudden jump in sessions that is not explained by a raid, MAGNet handoff or go-live alert puts the new arrivals in a 5-minute provisional window measured from when they were flagged. They count only after it ends and only if they pass.
- Automatic actions are limited to not counting sessions. Optional chat protection during a spike (followers-only for 10 minutes) is offered to the streamer and moderators as a one-click prompt; it is never turned on silently and it always expires.
- A broadcast whose excluded share stays high across several windows opens a case in the staff integrity queue in `/admin`. Staff see aggregate evidence (counts, networks by type, timing patterns), never raw IPs.
- Only staff can act on a case: dismiss, hold payouts for review, pause tier promotion, or issue a strike under the Profiles rules. Every action is audited and can be appealed like other strikes. A streamer can never clear their own case.

### What streamers see

Creator Studio shows the public count and, after the stream, "n viewers were not counted" with a short explanation of why counting can exclude viewers. No identities, networks or reasons per viewer.

### Privacy and retention

- IP addresses are kept only as keyed hashes (network prefix and full address) with a key rotated every 30 days, so they can't be reversed or linked across months.
- Session-level integrity rows are deleted 30 days after the broadcast ends. Per-broadcast snapshots are kept for 1 year; open staff cases keep their evidence until closed plus 1 year.
- The Privacy Policy describes this counting in plain words.

### Storage and API outline

Extend `playback_leases` with level, risk score, flags, hashed network fields and Turnstile result; add `integrity_snapshots` (per broadcast per minute) and `integrity_cases` with audited staff actions. Heartbeats gain visibility, media time and stall fields. The IPinfo Lite file is downloaded by the worker daily and checked before swap; if it is stale, network signals lower their confidence instead of excluding anyone.

Automated coverage adds: guest and signed-in level transitions, Turnstile failure, metronomic heartbeat detection, household and carrier networks not penalized, hosting-network risk, provisional window timing, raid and MAGNet bursts not flagged, recovery from exclusion, counts used by each consumer, no raw IP stored or returned anywhere, staff-only case actions, and retention deletion.

### S.V.E.R Plays

S.V.E.R Plays moves from the legacy backend to this API as soon as Module 3 closes, and becomes the dedicated always-on stream used to monitor live delivery and integrity. Plays stays its own project; it talks to this API only through the internal interface below.

- **Control gate (chat or AI player):** the API publishes, for the Plays channel only, the number of signed-in, verified viewers whose session is Counted or Trusted, refreshed every 5 seconds with a timestamp. The AI player may play only after that number has been 0 for the runner's grace period. A missing or stale value (older than 30 seconds) means chat mode: humans always win. Control goes back to chat as soon as one verified viewer is Counted (about 60 seconds after arriving), without waiting for Trusted.
- **Votes:** a chat vote counts only if the voter's account has a Counted or Trusted playback session on the Plays broadcast. Chat-only accounts and Excluded sessions can't vote. One vote per account per window, as in legacy.
- **Vote window:** scaled from the same verified-viewer count (legacy: 3 to 6 seconds).
- **Faction rewards from votes:** Trusted sessions only, matching the influence rule.
- **Interface:** an authenticated internal endpoint (or a Postgres notification consumed by the Plays bridge) providing the count and timestamp, plus the stream of chat commands from the Plays channel. No Redis is introduced for this.
- **Acceptance:** with Plays running on this API, bots and chat-only accounts can't keep the AI player off or steer votes, and a real viewer takes control back within about a minute.

## Social features (approved by Joe, October 3, 2026)

These were added to Module 3 after the core spec. They reuse the chat, moderation, block and playback-lease rules above. All numbers here were accepted by Joe as defaults on October 3, 2026 and can be tuned later.

### Chat additions

- **Mentions:** `@username` in a message is matched against real accounts on the server and highlighted for the person named. No notification outside chat.
- **Replies:** a message can reply to one visible message in the same channel. The reply shows a one-line quote (first 80 characters) of the original; if the original is deleted the quote reads "Message deleted" and never sends the deleted body.
- **Pinned message:** the owner or a moderator pins one message (an existing visible message or new text up to 500 characters). It stays until unpinned or replaced, survives reconnects, and is cleared if the pinned message is deleted. Pinning is audited like other moderator actions.
- **Badges:** Broadcaster, Moderator and Staff, decided on the server per message. Subscriber and Founder badges arrive with Support and the founders program. The sender's faction crest is separate (Module 4).

### Custom emotes

- Any channel owner can upload up to **10** emotes usable by anyone in that channel's chat. Subscriber-only emote slots come with Support.
- Static PNG or WebP, square, at least 112 px, at most 1 MB upload. Animated files use the first frame (same rule as Profiles images). Served at 28, 56 and 112 px from the media bucket.
- Code: 3 to 20 ASCII letters and digits, case-sensitive, unique within the channel. A message token that exactly matches a code of the current channel renders as that emote; everything else stays text. No cross-channel use in this module.
- Emotes publish immediately. They can be reported (new report target EMOTE); staff can remove one, and removal is audited and can lead to a strike under the Profiles rules. Deleting an emote removes it from future rendering only.
- Banned-word and link rules apply to codes.

### Go-live alerts

- Triggered when a broadcast starts. A reconnect inside the 60-second window is the same broadcast and sends nothing. At most one alert per channel every 6 hours.
- Three delivery methods, each switchable in settings: in-site notifications (the top-bar bell and a notifications list), browser push (standard Web Push with server-held VAPID keys, no outside service), and email through Resend. Email is opt-in; the others default on.
- Per-channel opt-out: a bell next to the Follow button turns alerts off for that channel without unfollowing. Unfollowing removes alerts too.
- Never sent to: the owner, users the owner blocked or banned, unverified or restricted accounts, or deleted accounts. Fan-out runs as Postgres-backed jobs, retried safely without duplicates.
- Email has a one-click unsubscribe and respects the account's notification settings. Push subscriptions that fail permanently are removed.
- In-site notifications are kept 30 days.

### Raids

- A live owner starts a raid from Creator Studio or with `/raid username` in their own chat. The target must be live, not restricted, and accepting raids.
- The target can turn off incoming raids or block raids from specific channels. A target that blocked or banned the raider (or its owner) can't be raided by it.
- Viewers on the raider's stream see a 10-second countdown with Cancel, then their player moves to the target. Signed-in viewers banned from the target stay put.
- The target's chat gets a system line "name is raiding with n", where n counts playback leases that actually started on the target within 60 seconds and came from the raid. The count is never taken from the raider's viewer count.
- One raid per broadcast every 10 minutes; a raid can be cancelled by the raider during the countdown.
- Faction influence for raids into ally or enemy channels is added by Module 4, with weights in the private tuning config.
- Raids are audited (raider, target, time, arrivals).

### Hosting

- An offline channel can host one live channel. Its channel page shows the hosted stream with a "Hosting name" bar and a link to the target.
- Auto-host: a priority list of up to **10** channels chosen by the owner. When the owner is offline, the first live, eligible channel on the list is hosted.
- Hosting stops when the host goes live or the target goes offline; auto-host then moves to the next live channel on the list. When a raider ends their broadcast after a raid, their channel hosts the raid target.
- Targets can opt out of being hosted; the same block and ban rules as raids apply.
- Hosted viewers are counted for the target because they are real playback sessions on the target's broadcast; the host shows no count of its own.

### Storage and API outline

New tables: chat replies and pins (columns on chat messages plus a channel pin row), channel emotes, notification preferences (global and per channel), notifications, push subscriptions, raids, host settings and host state. New report target EMOTE. Endpoints live under `/api/channels/{username}/chat` (pin, unpin), `/api/me/emotes`, `/api/me/notifications`, `/api/me/push`, `/api/me/raids`, `/api/me/hosting`, and a per-follow alert toggle under `/api/channels/{username}/follow`.

Automated coverage adds: mention matching and XSS, reply-to-deleted, pin permissions and reconnect, emote validation and render rules, alert throttling and opt-outs, push failure cleanup, raid eligibility, blocks and arrival counting, and host start/stop transitions.

## Delivery phases and checks

1. **Media proof and concrete deployment design.** Use an isolated SRS instance/vhost with synthetic media and separate ports/output. Validate public-ID/secret separation, callback authentication, one-publisher enforcement, audio/video compatibility, CDN protocol/cache behavior, and fallback. No restart, hook switch or load test against the active sver-plays runtime. Save only aggregate measurements and sanitized configs in the repository.
2. **Stream backend and Studio.** Integrate the finished Profile eligibility contract; implement additive tables, state transitions, credentials, metadata and warnings. Test with real isolated Postgres and a controlled media-server adapter before real ingest.
3. **Player and viewer accounting.** Channel/focused watch layouts, actual transport switching, playback leases and live state in Following/user cards. Test desktop/mobile and network failures.
4. **Chat and moderation.** History/join ordering, rate limits, block filtering, channel roles/actions/settings and platform report/ban/appeal integration. Preserve Module 2's approved strike semantics.
5. **Staged deployment and live acceptance.** Back up current rebuild data/config, identify compatible rollback binaries/migrations, introduce isolated media routing, and validate OBS plus the real delivery path. Preservation and latency are explicit gates.

Media acceptance uses a timestamp burned into synthetic frames plus a receiver-side reference to measure capture-to-display delay, not just playlist distance. Record codec, bitrate, keyframe interval, host/version, client/browser, network region, sample count, p50/p95 delay, startup success, stalls, CPU and egress. **Proposed:** at least 30 observations per path/scenario over ten minutes, then repeat at increasing real playback concurrency and across multiple broadcasts. HTTP requests that fetch playlists alone are not WebRTC viewers. Synthetic load against paid/live infrastructure needs a concrete duration/traffic budget first.

Measure the largest stable direct-WebRTC load with headroom and derive both per-stream and global budgets; do not infer capacity from a configured maximum or the legacy report helper. The CDN run must meet the accepted 3–5-second objective under the selected test load, with the large-stream under-five-second requirement checked explicitly. If no tested configuration meets it, document the failure and resolve the media design before closure.

Required automated coverage:

- Invalid/revoked/leaked-public-ID publish, unverified/no-MFA/restricted/deleted owner, forged hooks, failed backend and key-rotation/publish races all fail safely.
- Concurrent publishers, duplicate/out-of-order callbacks, 59-second and deadline-boundary reconnect, lost callback, API restart, SRS restart and failed-stop retry.
- No publish secret in public API, manifests, segment paths, WHEP, logs, error responses or browser-visible diagnostics; no bypass through a direct origin/ingress alias.
- Media generation freshness after reconnect/rotation; forced WebRTC failure, CDN error, exhausted transports and repeated policy-boundary changes.
- Lease expiry/deduplication, pause/stall, guest playback, own preview exclusion, and switch without double counting.
- Guest/unverified sends, expired sessions, multiple sockets racing slow mode, 500/501 characters, idempotent retry, stored XSS, blocked links/words, deleted-message rejoin and buffer limits.
- Owner/moderator/staff permission matrix, peer protection, immediate demotion, timeout expiry, ban/unban, block filtering, reports, bans and appeals without privilege escalation.
- Regression checks for closed Login and Module 2 Profiles; new top-level routes remain covered by the reserved-name test.

## Product decisions (approved by Joe, October 3, 2026)

1. **Channel bans stop chat and signed-in watching.** A banned signed-in user cannot chat in the channel and is refused playback access while signed in; logged-out public viewing remains possible and is not claimed to be blocked.
2. Approved as proposed: offline public chat stays available; unreported message bodies expire after seven days; public history is the latest 100.
3. Approved as proposed: the moderation permission table, account bans with restricted sessions and one appeal within 14 days, and staff username resets for impersonation.
4. Approved as proposed: the small seeded category catalog, the OBS guidance and the playback-lease viewer-count rules. Capacity thresholds and the final bitrate cap remain measured engineering outcomes.

## Done when

A verified creator with MFA broadcasts from OBS; an anonymous viewer watches through tested direct and CDN paths at the accepted latency targets; a reconnect within 60 seconds retains the broadcast; real playback drives counts; verified users chat; channel moderators can delete, timeout and ban; slow mode/link/word rules work; staff can handle stream/chat reports with the approved standing/appeal behavior; and Studio shows measured OBS warnings. Mentions, replies, pins, badges and custom emotes work; followers get go-live alerts from the channels they choose, with per-channel opt-out; a raid moves viewers after a countdown and reports an accurate arrival count; manual hosting and auto-host start and stop correctly; viewer integrity levels, the three counts and the staff integrity queue work, and no raw IP is stored. Existing accounts, legacy data and sver-plays remain intact. Completing documentation or passing mocked media tests alone does not close this module.
