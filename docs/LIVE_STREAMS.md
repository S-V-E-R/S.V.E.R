# Module 3: Live streams and chat

Started October 3, 2026 at the user's direction. **Status: stream backend and Creator Studio implemented locally, alongside the isolated media protocol proof. Player/accounting, chat/moderation, integrated real ingest and live media acceptance remain open.** Module 1 is closed by user acceptance. Module 2 continues independently; starting this module does not declare Profiles complete.

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

Deferred by the platform plan: SRT ingest, custom emotes, alerts/overlays, CrowdSync, external-platform chat, viewbot detection and chat replay. VODs/clips are Module 6. Faction influence and Engagement Valor belong to Module 4. Discovery rotation and MAGNet belong to their own modules; this module supplies live state and categories without adding ranking rules.

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

Correlate WebRTC leases with actual SRS sessions where possible. CDN playback heartbeats are client assertions, so corroborate with delivery telemetry when available and bound lease issuance/rates. This excludes obvious page/chat counts without claiming bot-proof unique humans; viewbot detection remains deferred. Expired lease rows are cleaned by the existing worker.

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
- Bans last until lifted. **Proposed and asked of the user:** channel bans prevent chat, while public viewing remains available. If signed-in playback denial is selected, it requires media authorization enforcement and still cannot prevent logged-out public viewing. No choice has yet been recorded.
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

## Product defaults awaiting review

1. Channel bans stop chat only; signed-in playback denial is an alternative, with public logged-out viewing still possible. The question was asked during startup; record the reply here before implementing it.
2. Offline public chat stays available; unreported message bodies expire after seven days. Public history remains latest 100.
3. Proposed moderation permissions, account-ban restricted-session/appeal behavior and impersonation username resets above.
4. A small seeded category catalog, proposed OBS limits and the playback-count rules above. Capacity thresholds and the final bitrate cap are measured engineering outcomes, not values to guess at review.

## Done when

A verified creator with MFA broadcasts from OBS; an anonymous viewer watches through tested direct and CDN paths at the accepted latency targets; a reconnect within 60 seconds retains the broadcast; real playback drives counts; verified users chat; channel moderators can delete, timeout and ban; slow mode/link/word rules work; staff can handle stream/chat reports with the approved standing/appeal behavior; and Studio shows measured OBS warnings. Existing accounts, legacy data and sver-plays remain intact. Completing documentation or passing mocked media tests alone does not close this module.
