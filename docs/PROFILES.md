# Module 2: Profiles

**Closed — October 3, 2026, 7:31 PM ET.** Joe accepted Module 2 on staging: he confirmed that everything looks good, including the setup on his own channel (JoeTheChode) built with the parts picker, and closed Profiles. Under the closure rule it is not reopened for polish; improvements go on a later list and bugs are still fixed. See "Closure" under "Acceptance".

Approved — Oct 3, 2026. The user (Joe) approved this specification, including every rule first drafted as "Proposed", on October 3, 2026. Implementation progress is recorded in `ChangeLog.md`; this document is not evidence that any part is implemented, migrated or deployed. It follows the closure rule: specify, build, then test against "Done when".

Sources: `docs/PLATFORM_PLAN.md` (2. Profiles), `AGENTS.md`, `docs/LOGIN.md`, `docs/OPERATIONS.md`, the user's Module 2 decisions in `ChangeLog.md`, and the read-only legacy triage in ignored `tmp/module2-legacy-triage.md` (legacy code under `C:\Streaming\SVER` and `C:\Streaming\Website`).

Every rule that the review draft marked **Proposed** was approved by the user on October 3, 2026, and those markers have been removed. The approved items are listed in "Decisions approved October 3, 2026". Everything else is the plan, an earlier user decision, or carried-over legacy behavior. Where they conflict, the user's decisions win over the plan.

## Boundaries and legacy carryover

Same stack as Login: the Rust/Axum service, SQLx/Postgres and the Next.js client in this repository. Profiles adds public channel pages, profile editing, follows, the legacy MySpace-style features, user safety tools and a second legacy import. It reuses Login's session cookie, exact-Origin check on JSON mutations, `rate_limits` table, step-up authentication, error shape (`{"error": "..."}` with optional `Retry-After`) and deletion lifecycle.

User decisions that supersede `PLATFORM_PLAN.md`:
- Channel pages live at root `/username` (`sver.tv/JoeTheChode`), not `/s/username`.
- v1 includes the legacy profile features the plan deferred: War Council (Top 8), profile song, The Wall, mood/status, schedule, sponsors, streaming setup, a reduced set of custom page blocks, the user card and fan art.
- Follower and following lists are **public**.
- Internal accounts `admin`, `support` and `SVER` keep sign-in but have no public channel page.
- When the legacy profile display name differs from the account display name, the profile display name wins.
- Legacy follows, War Council picks, social links and wall posts are imported from the legacy dump in this module, and the 3 avatars and 2 banners are re-hosted from the backups.
- The Wall requires user blocking, reports and an admin review queue in this module.

| Area | Label | Treatment |
|---|---|---|
| Channel page, routing | REWRITE | One read endpoint, root route, new visual language |
| Username format, reserved list | REWRITE | One code list covering every route; route-walk test |
| Rename, history | REWRITE | 60-day interval, 30-day hold, `/oldname` redirect (legacy freed names instantly) |
| Display name, avatar, banner | REWRITE | One display-name field; server-side image processing; no external image URLs |
| Bio, mood/status | PORT | Plain text; legacy limits tightened |
| Social links | REWRITE | Three legacy systems become one, max 5 |
| Follows, public lists | PORT | Case-insensitive lookup, block-aware, legacy side effects removed |
| War Council (Top 8) | PORT | One table; the redundant `topStreamerIds` and `TOP8` block config are dropped |
| Profile song | PORT (player) / ARCHIVE (yt-dlp proxy, HTML scraper, direct audio URLs) | Official embeds plus oEmbed only |
| The Wall | PORT | Legacy policy and moderation, plus reply delete, un-react, reports |
| Schedule, sponsors, streaming setup | PORT | Bounded lists |
| Custom page blocks | REWRITE | Four typed panel types replace 19 untyped JSON blocks |
| User card | REWRITE | Basic card; stats, badges and mod section wait for later modules |
| Fan art | REWRITE | Uploads through our pipeline only, owner approval, reports |
| Blocking, reports, review queue | REWRITE | Legacy channel blocks and AutoMod concepts become platform-wide tools |

ARCHIVE (stays out of v1): username negotiation/escrow/decay, vanity `@slug` URLs, the yt-dlp audio proxy and generic metadata scraper, direct audio-file songs, presence ("online"/"watching X"), ads, shop/storefront, Posts and Community tabs, LFG/collab, raid and chat-popout settings, the viewer passport/claim CTA and migration banners, featured-stats pickers, card frames and cosmetic badges, Subscribe/Tip/Message buttons, and the legacy `WallMessage` model. The faction-hub "War Council" territory vote belongs to Module 4 and will be renamed there.

Not done in this module: email or push notifications of any kind, live state, faction theming of other users, VODs/clips, Beacons, achievements and account-level bans. Their placeholders are defined in "Later-module stubs".

## URLs and routing

- Static web routes replace `apps/web/app/[screen]`: `apps/web/app/{login,signup,oauth-signup,forgot,reset,verify,mfa,account}/page.tsx` (the seven auth screens plus the OAuth signup completion screen added in Module 1). Behavior and redirects stay identical, and the existing navigation check must still pass. Only then add `apps/web/app/[username]/...`. Next.js gives static segments priority, but the reserved-name rule below is what guarantees that no user ever shadows a route.
- Channel routes:
  - `/{username}` is the channel page (Home).
  - `/{username}/wall`, `/{username}/schedule`, `/{username}/about`, `/{username}/fan-art` (only when the owner has fan art enabled), `/{username}/followers` and `/{username}/following`.
  - `/{username}/rewards` is a reserved stub page with no tab (P8).
  - `/{username}/live` redirects with 302 to `/{username}` until Module 3 defines it.
  - Any other sub-path returns 404.
- Lookup is case-insensitive. A request whose casing differs from the stored username gets a 308 to the canonical casing, keeping the sub-path and query string.
- Legacy and planned aliases:
  - `/s/{name}` and `/u/{name}` return 308 to `/{name}` (sub-path kept).
  - `/@{name}` returns 308 to `/{name}`.
  - `/watch/{name}` returns 302 to `/{name}` (Module 3 retargets it to the watch experience).
  - Legacy `?tab=wall|schedule|about` maps to the matching sub-path; `?tab=showcase` and the other legacy tabs (beacon, shop, posts, community) map to `/{name}`.
- Renamed users: during the 30-day hold, `/{oldname}` and `/{oldname}/<sub-path>` return **302** to the same path under the current username, with `Cache-Control: no-store`. A temporary redirect is used because the name is released when the hold ends; a cached 301 would send visitors to whoever claims it next.
  - Holds resolve to a user ID, not a name, so a chain of renames always lands on the current name.
  - After the hold, `/{oldname}` behaves like any unclaimed name (404).
- 404 for: unknown names, deleted accounts (including the 14-day grace period and `legacy_deletion_hold`), internal accounts, and admin-restricted channels. All four return the same page: "This channel doesn't exist." Nothing reveals which case applies.
- Signed-in `/` keeps Login's current redirect to `/account` until Module 5 builds the homepage.
- Server-rendered metadata:
  - title `Display Name (@username) | S.V.E.R`
  - Open Graph title, avatar image and description (the bio, or "Channel on S.V.E.R")
  - canonical URL `https://sver.tv/{username}`
  - Staging keeps `X-Robots-Tag: noindex, nofollow`.
- No shared caching of channel HTML or API responses. The Login rule that authenticated HTML must never be served to another visitor applies to every profile route.

## Usernames

### Format
- 3-25 ASCII letters, digits or underscores; unique regardless of case.
- Additions, applied to new signups and renames only: no leading or trailing underscore, and not all digits. All 42 imported names comply. Existing names are never revalidated.
- The signup rule (`apps/api/crates/sver/src/security.rs::signup_identity`) and the rename rule share one validator.

### Reserved names
- **One code-maintained list** (a single Rust module, `apps/api/crates/sver/src/reserved.rs`, used by signup, rename and channel resolution) holds:
  1. every top-level web route and public file or directory name
  2. every legacy top-level route that can be a valid username
  3. planned module routes
  4. staff, brand and faction names
  5. the existing abuse list
- Matching uses the existing case-insensitive exact match plus the leet-compacted match. The substring blocks (admin, moderator, support, sverstaff, sverofficial and the abuse list) stay as they are.
- Minimum route set:
  - Current routes: login, signup, forgot, reset, verify, mfa, account, api.
  - Planned: settings, studio, admin, following, followers, browse, search, category, categories, genre, genres, watch, live, factions, faction, territory, territories, magnet, beacons, beacon, clips, clip, videos, video, vods, vod, notifications, messages, inbox, logout, help, support, terms, privacy, guidelines, dmca, about, contact, status, report, reports, s, u, user, users, channel, channels, embed, popout, chat, overlay, auth, oauth, static, assets, media, cdn, img, images, uploads, public, robots, sitemap, favicon, manifest, www, mail, email, home, discover, dashboard.
  - Staff and brand: admin, administrator, support, moderator, mod, staff, sver, svertv, official, security, system, root, null, undefined.
  - Factions: aetheron, myria, glint.
  - Legacy segments: accessibility, activity, advertise, appeals, blog, cart, challenges, checkout, costream, creator, crowdsync, dev, dock, drops, emotes, feed, founder, founders, fund, gifting, goals, guilds, hype, icons, leaderboards, marketplace, markets, offline, onboarding, orders, predictions, presents, remote, shop, squad, store, storefronts, surge, team, transparency, trust, unsubscribe, vanity, wallet. The full legacy list is in the triage; segments with hyphens or dots, or shorter than 3 characters, can never be usernames.
- **Route-walk test (required):** a test run by `./scripts/dev.ps1 test` fails if any of these isn't reserved:
  - a top-level folder under `apps/web/app`, ignoring `(group)` folders, `_private` folders and dynamic `[param]` folders, which aren't paths
  - a top-level entry in `apps/web/public`, without its file extension
  - the first path segment of every `source` in a `next.config.ts` redirect or rewrite
  - The same test fails if `apps/web/app` gains a second root dynamic segment.
- Reserved names cannot be claimed at signup or rename (error: "That username isn't available."). The message is the same as for a taken name, so the list can't be probed.

### Internal accounts
- `admin`, `support` and `SVER` are internal. So is any imported account whose preserved legacy `isSystemAccount` is true. The import rehearsal reports how many accounts that adds; if it is more than these three, the import stops for user review.
- Stored as `profiles.internal = true`. Internal accounts:
  - sign in and use the site normally as viewers
  - have no channel page (their names are reserved routes, or 404)
  - can't be followed or added to a War Council, and have no wall
  - are left out of public follower/following lists and the user card
  - When they appear elsewhere (for example as a wall post author), they show as plain unlinked text `@name`.
- Their usernames are never changed by this module.

### Rename, history and hold
- Changing the username is a sensitive change, because the username is a sign-in identifier: primary authentication within five minutes plus a fresh MFA code when MFA is enabled, exactly as Login defines it.
- The new name passes format, reserved, availability and hold checks. Availability is case-insensitive against `users.username` and every active hold except the user's own.
- **Interval:** one rename per 60 days, counted from the last rename. A case-only change (`joethechode` to `JoeTheChode`) is allowed at any time, starts no interval and creates no hold. Imported accounts start with no interval (legacy cooldowns were not exported).
- On success, in one transaction under the user-row lock:
  - update `users.username`
  - insert `username_history(user_id, old_username, new_username, changed_at, reason='rename')`
  - insert `username_holds(handle_canonical, user_id, released_at = now + 30 days)`
  - Login sessions are not revoked
- The owner may switch back to a name still held for them once during the hold. That ends the hold and doesn't count as a new rename.
- During the hold, sign-in with the old username fails like an unknown identifier; email sign-in is unaffected.
- After account erasure, the username is held 30 days with no redirect, then released.
- Errors:
  - "You can change your username again on {date}." (409; date in the user's timezone)
  - "That username isn't available." (409)
  - format errors (400)
  - step-up required (403, using Login's reauth flow)

## Identity fields

All profile text is NFC-normalized and trimmed. Control characters, bidi overrides and zero-width characters are rejected (ZWJ is allowed only inside emoji sequences). Text is plain and rendered escaped; nothing is interpreted as HTML or markdown unless a section below says so. A shared text filter (the existing abuse list plus slur list, **Proposed** home `apps/api/crates/sver/src/security.rs`) rejects with "Remove the blocked word and try again." without naming the word. Owner edits apply immediately; there is no pre-publication review.

### Display name
- **Rules:**
  - 1-32 characters (Unicode scalar values) after trimming; internal whitespace runs collapse to one space.
  - Letters and digits in any script, spaces, and `. _ - ' ! ? & ( )` are allowed.
  - Default and fallback: the username.
- **Impersonation:** the name is rejected if its compacted form matches a staff or brand term (admin, moderator, staff, support, sver, svertv, official), unless it equals the owner's own username ignoring case.
- **Display:** always rendered with `@username` directly beneath or beside it, everywhere a display name appears.
- **Storage:** `profiles.display_name`. There is one source of truth; `users` gets no copy.
- **Import:** the legacy `Profile.displayName` wins; otherwise the legacy `User.displayName`; otherwise the username. Imported values are kept verbatim even if a new rule would reject them; the rules apply on the next edit. The rehearsal reports how many would fail.

### Avatar and banner
- **Inputs:** JPG, PNG or WebP, identified by magic bytes; the extension and declared type are ignored.
  - Avatar up to 5 MB; banner up to 10 MB.
  - Decoded size at most 50 megapixels and at most 10,000 px on either side.
  - Animated images use the first frame.
  - Minimum size: avatar 128x128, banner 1200x400.
- **Upload:** a multipart `POST` to the API, with no presigned client uploads, so the server can always decode and inspect the image. Fields: `crop{x, y, width, height}` in source pixels.
  - Avatar crops must be square; banner crops must be 3:1, within 1 px.
  - Without a crop, the server takes a centered crop.
  - Nginx `client_max_body_size` must allow 11 MB on the upload routes only.
- **Processing:** decode, crop, resize with high-quality downsampling, re-encode as WebP (dropping every metadata block, including EXIF/GPS), and store under content-hashed immutable keys.
  - Avatar sizes 64, 160 and 400 px square (the plan requires three sizes).
  - Banner sizes 750x250, 1500x500 and 3000x1000 (never upscaled, so a size larger than the source is skipped).
- **Storage:**
  - A new S3-compatible bucket, separate from legacy R2, served read-only from `media.sver.tv` through Cloudflare with long-lived immutable caching.
  - Credentials live in the external stage env, never in Git.
  - A replaced or removed image is deleted from storage by a background job after the database change commits. Legacy objects are never touched.
- **Defaults:** a static default avatar (steel crest silhouette) and a default banner (solid steel panel with a corner-bracket frame; no gradient).
- **Removal:** the owner can remove either image and get the default back.
- **Errors (all 400 unless noted):**
  - "Use a JPG, PNG or WebP image."
  - "Avatars can be up to 5 MB." / "Banners can be up to 10 MB." (413)
  - "That image is too small." / "That image is too large to process."
  - "We couldn't read that image."
  - "Crop must be square." / "Crop must be 3:1."
- **Rate limit:** 20 image uploads per user per hour.
- External image URLs are never accepted.

### Bio
- Up to 300 characters, plain text, with up to 4 line breaks. URLs render as plain text, never as links; links belong in social links.

### Mood and status
- **Mood:** exactly one emoji grapheme, or empty.
- **Status:** up to 80 characters of plain text, or empty (legacy allowed 100, and the longest imported status is 28).
- Shown under the name on the channel page.
- **Import:** imported values that fail validation are left empty, and the rehearsal reports the count. The preserved legacy JSON keeps the original.

### Social links
- **Limits:** up to 5 links in a user-chosen order, at most one per platform except Website, which can appear up to 2 times.
- **Platforms and allowed hosts:**
  - Twitch `twitch.tv`
  - YouTube `youtube.com`, `youtu.be`
  - Kick `kick.com`
  - TikTok `tiktok.com`
  - Instagram `instagram.com`
  - X `x.com`, `twitter.com`
  - Bluesky `bsky.app`
  - Discord `discord.gg`, `discord.com/invite`
  - Facebook `facebook.com`
  - Patreon `patreon.com`
  - Ko-fi `ko-fi.com`
  - 4th Wall: the user's `*.4thwall.com` store
  - Website: any https host that isn't an IP address, localhost or a sver.tv host
- **Validation:**
  - URLs must be `https://`, at most 2048 characters, with no credentials in the URL.
  - Platform hosts must match exactly or be a subdomain.
  - The settings UI accepts a handle and builds the URL from a template, as legacy did.
- **Rendering:** `rel="nofollow noopener ugc"` and `target="_blank"`, with a platform icon and the host shown. The URL is never fetched server-side.
- Linked OAuth identities (Twitch, Discord) may offer a one-click prefill. Nothing is added automatically.

## Channel page

### Visibility
- The page is public to everyone, signed in or not, unless the channel is unknown, internal, deleted, held or admin-restricted (all of which return 404).
- Blocking does not hide a public page from the blocked user, but it disables every interaction between the two users (see Blocking).

### Header frame (top to bottom)
- **Player area:** in Module 2 this always shows the banner or default banner with an "Offline" label. It's a stub; Module 3 swaps in the live player in the same slot.
- **Identity:**
  - avatar (400 size, with smaller sizes in srcset)
  - display name (Cinzel)
  - `@username`
  - mood emoji and status
  - faction badge slot (empty until Module 4)
- **Bio** and **social links**.
- **Counts:** follower and following counts, each linking to its public list. Counts are exact integers and exclude internal, deleted and held accounts. "Joined {Month YYYY}" is shown too.
- **Buttons:**
  - Follow / Following, or "Log in to follow" when signed out.
  - On someone else's page, a menu with Block and Report.
  - On your own page, an "Edit profile" button instead.
- **Profile song:** a compact player when set.

### Tabs
- **Home:**
  - War Council grid
  - Wall preview: pinned posts, then the latest 3 approved posts, with "Sign the Wall" linking to the Wall tab
  - next 3 schedule occurrences
- **Wall**
- **Schedule:** the full week.
- **About:** custom blocks in the owner's order, sponsors, streaming setup.
- **Fan Art:** only when enabled.
- Followers and Following are list pages, not tabs.
- Empty sections are hidden from visitors, except the Schedule tab, which always exists and shows an empty state ("No streams scheduled this week."). The owner sees each one with an add prompt.
- Each tab is server-rendered with its own URL. Tab content is paginated, never loaded all at once.

## Follows
- **Who can follow:** any signed-in user can follow or unfollow any eligible channel. Email verification is not required (plan: "any signed-in user").
- **Ineligible targets:** yourself (400 "You can't follow yourself."); internal, deleted, held or restricted accounts (404); either side has blocked the other (403 "You can't follow this channel.").
- Follow and unfollow are idempotent and take effect immediately. Counts update in the same transaction.
- **Rate limits** (legacy): 30 follow/unfollow actions per hour and 100 new follows per day, per user. Exceeding them returns 429 with `Retry-After`.
- **Public lists:**
  - `/{username}/followers` and `/{username}/following` are visible to everyone, newest first, 50 per page with a cursor. Each row shows avatar, display name, @username and follow date.
  - Internal, deleted, held and restricted accounts are omitted.
  - For a signed-in viewer, users in a block relationship with that viewer are omitted too.
  - There is no per-user privacy toggle in v1.
- **Following page** `/following`:
  - signed in only; signed-out visitors are redirected to `/login`
  - lists the viewer's followed channels, ordered by follow date (newest first) until Module 3 adds "live first"
  - each row has an Unfollow button
  - empty state: "Channels you follow appear here."
- No notifications, emails, chat messages or activity entries come from follows in this module. Legacy per-follow notification preferences are not imported.
- **Erasure:** follows in both directions are deleted.

## War Council (Top 8)
- **Rules:**
  - Up to 8 members in positions 1-8, with no duplicates and not including yourself.
  - Members must be eligible (not internal, deleted, held or restricted), with no block in either direction at save time.
  - No consent or notification is involved, and members don't have to be followed.
- **Editing:** at `/studio/channel/war-council`: search by username or display name (10 results, eligible users only), add, reorder by drag or move up/down, remove. Saving replaces the whole list in one transaction.
- **Errors:**
  - "Your War Council is full." (409)
  - "You can't add yourself."
  - "That user can't be added." (404 or 403)
- **Display:**
  - A framed 4x2 grid (2x4 on mobile) of square tiles, each with avatar, display name and @username, linking to the member's channel.
  - Position 1 carries a crown mark (a static icon, no glow).
  - The section is titled "War Council".
  - At read time, members who became ineligible are skipped and the remaining members fill the gaps in order. The stored positions are kept, and the owner sees "1 member is no longer available" with a remove action.
- **Blocking:** removes the other user from your War Council and removes you from theirs, immediately.
- **Stubs:**
  - Member tile color uses the viewer's theme accent; the member's faction badge waits for Module 4.
  - "Highlight War Council messages in chat" waits for Module 3.
  - The "Council of War" achievement waits for Phase 3.
- **Import:** legacy `ProfileTop8` picks, with positions compacted to 1..n in the legacy order. Self-references and missing users are dropped and counted. Legacy `topStreamerIds` and `TOP8` block items are not used (both are empty or unused for the 42).

## Profile song
- **Supported sources:** official embeds only.
  - YouTube: `youtube.com/watch?v=ID`, `youtu.be/ID`, `youtube.com/shorts/ID`, `music.youtube.com/watch?v=ID`, where ID is 11 URL-safe characters.
  - SoundCloud: public track URLs on `soundcloud.com` (and `on.soundcloud.com` short links, resolved through oEmbed only).
  - Everything else gets "Use a YouTube or SoundCloud link." That includes playlists, live streams, direct audio files and Spotify (see Import).
- **Metadata:** fetched server-side from the official oEmbed endpoints only (`https://www.youtube.com/oembed`, `https://soundcloud.com/oembed`), with a fixed host, no redirect following, a 5-second timeout and a 64 KB response cap.
  - The oEmbed title and author fill Title and Artist, which the owner can edit (each up to 100 characters, filtered text).
  - The oEmbed thumbnail is copied once into our media bucket as a 400 px WebP, so viewers never hotlink provider images.
  - A provider 401, 403 or 404 means private, removed or not embeddable: "That track can't be embedded."
  - A timeout or 5xx returns 503 "Couldn't reach YouTube/SoundCloud. Try again."
- **Rate limit:** 10 metadata lookups per user per minute.
- **Stored:** provider, video or track ID, canonical URL, title, artist, thumbnail key, default volume 0-100 (default 70), updated time.
- **Player:**
  - A compact card with thumbnail, title, artist and a play button.
  - The provider iframe is created **only after the visitor clicks play**: `youtube-nocookie.com/embed/ID`, or the `w.soundcloud.com/player` widget. Playback then starts in the iframe, and the default volume is applied through the provider's player API.
  - Never autoplays. The legacy autoplay flag is ignored.
  - One player per page; it stops when the visitor navigates away.
  - Keyboard operable, with a visible pause control.
- **Removal:** the owner can clear the song at any time.
- **Import:**
  - The 3 YouTube/SoundCloud records map directly, keeping title and artist. The legacy album-art URL is not hotlinked; the thumbnail is fetched through oEmbed by a post-import job, and if that fails the card shows no art.
  - The Spotify record is not shown publicly. Its owner sees "Spotify links aren't supported. Add a YouTube or SoundCloud track." in song settings, and the original stays in the preserved legacy JSON.
  - Spotify embeds are not supported in v1.

## The Wall
- **Who can post (owner setting `who_can_post`):**
  - **ANYONE**: any signed-in user with a verified email. This is the default.
  - **FOLLOWING**: users the owner follows.
  - **MUTUAL**: users who follow the owner and whom the owner follows back.
  - **NONE**.
  - The owner can always post on their own wall.
  - The legacy SUBSCRIBERS option is hidden until subscriptions exist. On import it maps to NONE; no imported wall uses it.
  - Users in a block relationship with the owner (either direction), restricted users and internal accounts can't post, reply or react: 403 "You can't post on this wall."
- **Posts:**
  - 1-500 characters of plain text, at most 8 line breaks, filtered text.
  - URLs render as `rel="nofollow noopener ugc"` links.
  - No editing (legacy had none).
  - Email verification is required to post, reply or react.
- **Replies:**
  - 1-300 characters; one level deep only.
  - The same permission as posting, except the owner can always reply.
  - Ordered oldest first; the first 3 are shown, the rest load behind "Show more".
- **Reactions:**
  - One Like per user per post, toggled on and off.
  - The legacy VALOR reaction waits for Module 4. Imported VALOR reactions become Likes.
- **Rate limits** (legacy): 10 posts per minute and 50 per day, 20 replies per minute, 30 reaction changes per minute. Exceeding them returns 429 with `Retry-After`.
- **Review (owner settings):**
  - `require_approval`: every post from others is PENDING.
  - `hold_links`: posts containing a URL are PENDING.
  - `hold_new_accounts`: posts from accounts under 7 days old are PENDING. Age is measured from `users.created_at`; imported accounts keep their legacy creation dates.
  - Replies follow the same rules as posts.
  - PENDING content is visible only to the author (marked "Waiting for approval") and the owner.
- **Owner actions:**
  - Pending queue at `/studio/channel/wall`, oldest first, 20 per page, with Approve, Reject and Block author.
  - Block author rejects all that author's pending posts and replies on this wall and creates a user block.
  - Rejected content is visible only to its author (marked "Not approved").
  - Pin up to 3 approved posts (legacy code allowed 1-10). Pinned posts show first, in pin order.
- **Deletion:**
  - An author can delete their own posts and replies; the owner can delete any post or reply on their wall. Deleting is soft and removes the content from every view immediately; deleting a post also removes its replies and reactions from view.
  - Soft-deleted rows are purged after 30 days unless an open report references them.
- **Reports:** any signed-in user can report a visible post or reply (see Reports).
- **Display:**
  - Newest first, 20 per page with a cursor.
  - Each post shows the author's avatar, display name and @username (linked to their channel, or unlinked text for internal or ineligible authors), relative time with an exact-time tooltip, body, Like count, replies, and Delete or Report as permitted.
  - Posts by authors who became ineligible remain visible, unlinked. Admin removal is how bad content leaves a wall.
- **Notifications:** none in v1. The owner sees a pending count in Studio.
- **Import:** legacy WallPost, WallReply and WallReaction rows, keeping their IDs.
  - Status APPROVED, PENDING or REJECTED is kept, as are the legacy deletion timestamp and creation time. Bodies are kept verbatim even if longer than the new limits.
  - Legacy `wallSettings` maps `whoCanPost`, `requireApproval`, `autoHideLinks` and `autoHideNewAccounts` to the new settings.
  - Legacy `pinnedWallPostIds` keeps the first 3 that reference approved, undeleted posts on that owner's wall.

## Schedule
- **Settings:**
  - Owner timezone: a named IANA zone, never a fixed offset. Defaults to the browser's zone at first save.
  - Weekly blocks: up to 21 (legacy allowed 50). Each has a weekday (ISO 1-7), start and end `HH:MM` in 24-hour time, and an optional label of up to 60 characters.
  - A block whose end time is before its start time crosses midnight. Start equal to end is rejected. Blocks on the same weekday may not overlap.
- **One-off events:** up to 20 future events. Each has a title (1-100 characters), start and end, lasts at most 24 hours and must start within the next 365 days. Past events are hidden and purged after 30 days.
- **Display:**
  - Occurrences are expanded with DST-correct zone rules (ported from `@sver/profile-schedule`).
  - They are shown in the viewer's browser timezone, with the owner's zone noted, covering the next 7 days on the Schedule tab and the next 3 occurrences on Home.
  - The empty state is hidden from visitors.
- **Stub:** "live now" versus "scheduled" waits for Module 3. Nothing feeds discovery (MAGNet keeps "stream schedules" deferred).
- **Import:** not imported (the user listed only follows, War Council, links and wall posts). Legacy schedules stay in the dump.

## Sponsors
- **Entries:** up to 10, in owner order, each togglable active or hidden.
- **Fields:**
  - name (1-80)
  - optional description (up to 300)
  - https link (Website host rules)
  - optional discount code (up to 50, shown with a copy button)
  - category: HARDWARE, PERIPHERALS, SOFTWARE, APPAREL, FOOD_DRINK, SERVICES or OTHER
  - optional logo through the image pipeline: JPG, PNG or WebP up to 2 MB, stored as a 256 px WebP fit inside a square
- **Rendering:** links use `rel="sponsored nofollow noopener"`. The section appears on About.
- **Import:** not imported.

## Streaming setup
- **Items:** up to 20, in owner order.
- **Fields:**
  - category: CPU, GPU, RAM, MOTHERBOARD, CAMERA, MIC or PERIPHERALS, picked through the parts picker (see "Setup parts picker" below). OTHER holds entries kept from before the picker. The original 14 categories (CAMERA, MICROPHONE, AUDIO_INTERFACE, HEADPHONES, PC, CPU, GPU, CAPTURE, LIGHTING, MONITOR, KEYBOARD, MOUSE, CONTROLLER, OTHER) were mapped by migration `0011_setup_parts.sql`.
  - name (1-80): a part from SVER's parts list or a custom entry
  - optional note (up to 120)
  - optional https link (`rel="sponsored nofollow noopener"`)
- Shown on About, grouped by category.
- **Import:** not imported.

## Custom page blocks (reduced set)
- **Types:**
  - **About**: a markdown subset: paragraphs, bold, italic, lists, `###` headings, links with https only and `rel="nofollow noopener ugc"`. No images, HTML, tables or code blocks. Up to 2000 characters; sanitized at render.
  - **Panel**: title 1-80 characters plus a body in the same markdown subset, up to 2000 characters.
  - **Quotes**: 1-5 quotes, each up to 280 characters with an optional attribution of up to 80.
  - **Game shelf**: 1-12 game names, each 1-80 characters. Module 3 or 5 may later link them to categories.
- **Limits:** up to 10 blocks, in owner order, each togglable on or off. Each block type validates its configuration strictly; unknown fields are rejected.
- Blocks render on About, in order, after the bio.
- **Out:** the other 15 legacy types. Several of them (SOCIAL_LINKS, TOP8, SCHEDULE, SONG, SPONSORS, FAN_ART, STREAMING_SETUP) are first-class sections now. The rest are stubs or ARCHIVE (see Later-module stubs).
- **Import:** legacy blocks are not imported. The legacy About markdown is empty for all 42 accounts.

## Fan art
- **Setting:** the owner enables submissions, off by default. The Fan Art tab is visible only when it is enabled and there is at least one approved item.
- **Submitters:**
  - Must be signed in with a verified email, eligible and not blocked in either direction.
  - Upload: JPG, PNG or WebP up to 5 MB, max 4096 px on either side after the decode checks. Stored as 400 and 1600 px WebP with metadata stripped.
  - Fields: artist name (1-80, defaults to the submitter's display name), optional https artist link, optional caption (up to 200), and a required checkbox: "I made this or have permission to share it."
- **Limits:**
  - Each submitter can make 5 submissions per day.
  - Each channel can have 20 pending items. When the queue is full, submitters see "This channel's fan art queue is full."
  - Each channel can show 100 approved items.
- **Review:**
  - Every item starts PENDING and is visible only to the submitter and the owner.
  - The owner approves or rejects at `/studio/channel/fan-art`; rejected items are deleted from storage after 7 days.
  - Approved items show in a framed grid with the artist credit.
  - The submitter can withdraw their item at any time. The owner can remove it at any time.
  - Anyone signed in can report an item (copyright is one of the report reasons).
- **Import:** not imported.

## User card
- **Opens from:** clicking or keyboard-activating any avatar or name chip in Module 2 surfaces (lists, War Council, wall posts). Module 3 adds chat.
- **Layout:** a popover on desktop and a bottom sheet on mobile.
- **Contents:** avatar, display name, @username, the first 160 characters of the bio, follower count, "Joined {Month YYYY}", Follow button, "View channel", and a menu with Block and Report.
- **Stubs:** faction badge (Module 4), live indicator (Module 3), moderator actions (Module 3). These are reserved slots with nothing rendered.
- **Ineligible targets:** internal, deleted, held and restricted accounts show no card; their chips are plain text.

## Safety: blocking, reports, strikes, appeals and the admin review queue

### Blocking
- Any signed-in user can block any other non-internal user from the user card, the channel menu, the wall queue, or **Settings > Blocked users** (`/settings/blocked`).
- Blocking is silent: the blocked user is never told. It takes effect immediately.
- A user can have up to 1,000 blocks. Blocking yourself returns 400.
- In one transaction, blocking:
  - removes follows in both directions
  - removes each user from the other's War Council
  - rejects the blocked user's pending wall posts, replies and fan art on the blocker's channel
- While the block exists, neither user can follow the other, add the other to a War Council, or post, reply, react or submit fan art on the other's channel.
- Each user is omitted from the lists the other views (follower/following lists, War Council search).
- Existing approved wall posts stay visible; the owner can delete them.
- Unblocking restores nothing that was removed.
- Module 3 reuses the same block relationship for chat.
- Rate limit: 30 block/unblock actions per hour.

### Reports
- **Who can report:** any signed-in user.
- **Targets:**
  - a channel profile (with a field choice: display name, username, avatar, banner, bio, status, links, song, War Council, sponsors, setup, custom blocks)
  - a wall post
  - a wall reply
  - a fan art item
- **Reasons:** spam, harassment or bullying, hate, sexual content, violence or self-harm, impersonation, private information, copyright, other. An optional note of up to 500 characters.
- **Limits:**
  - One open report per reporter per target (a repeat report updates the note).
  - 10 reports per hour per reporter.
  - Users can't report their own content.
- **Snapshot:** each report stores a copy of the reported text, or retains the reported image keys, at the time of reporting. Moderators see what was reported even after an edit or delete. Snapshotted media is exempt from the cleanup jobs until the report closes. Snapshots are purged 90 days after resolution.
- **Privacy:** the reporter's identity is never shown to the reported user. On submitting, the reporter sees "Thanks. We'll review this." Later feedback is covered in Reporter feedback.

### Reporter feedback
- **My reports page:** `/settings/reports`. It is signed-in only and private to the reporter; no one else, staff included, sees this view.
  - It lists the reporter's reports from the last 180 days, newest first, 20 per page.
  - Each row shows the target type, the reported user's `@username` (or "Deleted user"), the reason the reporter chose, the reporter's own note, the submission date, and a status.
- **Statuses:**
  - "Under review": the report is OPEN.
  - "Action taken": the report closed as ACTIONED by staff.
  - "Closed": the report was dismissed, or closed because the account was erased.
  - "Closed" carries no notice, so reporters aren't pinged about dismissals.
- **Notice:** only when action is taken. "We reviewed your report and took action. Thanks for helping keep S.V.E.R safe."
  - An unread dot appears on the Settings navigation and on the row until the reporter opens `/settings/reports`, which marks every notice as seen.
  - One staff action can close many reports on one target; every reporter of that target gets the notice.
- **Never revealed to the reporter:**
  - what action was taken
  - whether a strike or restriction was issued
  - strike counts
  - appeals and their outcomes
  - the reviewer's identity
  - other reporters, or how many reports there were
  - An overturned appeal never changes the reporter's status, because a change would reveal the appeal.
- **Email:** an opt-in setting on `/settings/reports`, "Email me when action is taken on my reports", default off.
  - It uses Login's encrypted mail-job queue and Resend with idempotency keys, sending at most one digest per reporter per 24 hours.
  - The text is generic and includes no names, content or target: "Action was taken on a report you submitted. See sver.tv/settings/reports."
  - If adding a template to Login's mailer turns out not to fit, v1 ships in-app only and the setting is hidden.
- Unverified email addresses never receive mail.
- Reporter rows are deleted when the reporter's account is erased.

### Strikes
- **Issuing:** a confirmed violation issues a strike.
  - Staff issue strikes from the review queue, as part of a Remove content, Reset field or Restrict channel action, or from an admin user page (`/admin/users/{username}`) without a report.
  - A "Strike" checkbox is checked by default on Remove content and Reset field. Staff may uncheck it for a non-violation cleanup, such as an honest copyright mistake, and must give a note.
  - Dismissing a report never issues a strike.
  - Internal accounts can't receive strikes.
- **Strike contents:**
  - reason (the report reason list)
  - severity STANDARD or SEVERE
  - the content involved: the report snapshot, or a fresh snapshot taken at issue time
  - linked report IDs
  - the penalty applied and its end
  - a message to the user of up to 500 characters
  - issuer, issued time, expiry and status
- **Expiry:** a STANDARD strike stops counting 90 days after issue; a SEVERE strike after 365 days. Expired and overturned strikes stay in the user's history and the staff view, but never count toward the level.
- **Level:** the number of active strikes, capped at 3. Any active SEVERE strike puts the user at level 3. The penalty is applied when a strike is issued, based on the level that strike reaches.

| Level reached | Penalty |
|---|---|
| 1 | Warning. A notice on the account standing page; no feature limits. |
| 2 | Channel restricted for 72 hours (Restrict channel rules), counted from issue, or from the start of a converted interim restriction. |
| 3 | Channel restricted indefinitely until staff lift it or a successful appeal. Once Module 3 account bans exist, a level-3 strike opens an account-ban review in the queue. The ban itself is a separate staff decision, never automatic. |

- **Severe violations skip ahead:** staff may mark a strike SEVERE, which goes straight to level 3, for:
  - hate or credible threats of violence
  - sharing private information (doxxing)
  - impersonating S.V.E.R staff
  - sexual content involving minors
  - Content involving minors is also preserved and handled under the operator's legal reporting obligations, outside this flow.
- **Restrict channel and strikes:**
  - A restriction is normally the penalty of a strike: each strike records its own `penalty_until`, and the account's effective restriction end is the latest end among its active strikes and any interim restriction.
  - **Interim restriction:** staff may restrict a channel for **at most 24 hours** without a strike while investigating urgent reports. When it ends, staff must either issue a strike or lift it.
    - The admin queue lists every open interim restriction with its end time, soonest first.
    - It never runs past 24 hours without a strike: if the time passes with no decision, the restriction ends automatically and the case stays in the queue as **overdue** until staff record a strike or a lift.
    - Both outcomes, and any overdue expiry, are written to `moderation_actions`.
  - **Interim restriction converted to a strike:** when staff issue a strike for the same incident (linked to the interim restriction), the interim restriction closes as converted. The strike's restriction time counts from the **start of the interim restriction**, so the two never stack: a level-2 strike's 72 hours end 72 hours after the interim restriction began. A level-3 restriction is indefinite either way.
  - When a strike expires, the restriction it applied stays in force until its own end. Expiry lowers the level for future strikes but doesn't lift an active penalty early.
  - Overturning a strike removes it from the count and recomputes the effective restriction from the remaining active strikes. That lifts the restriction immediately if nothing else justifies it.
  - Staff may also lift a restriction directly; that is an audited action.
- **What the user sees:**
  - **Account standing page:** `/settings/standing`. It shows the current level, any restriction and its end date, and every strike with its reason, date, the content involved (their own content only), penalty, expiry, status, and appeal status or button.
  - It never shows who reported them, who issued the strike, or other moderation notes.
  - A new strike shows a site-wide banner until the user acknowledges it. A generic email is always sent for a new strike and for each appeal decision, if the email is verified and the mailer fits: "There's an update to your account standing. See sver.tv/settings/standing."
- **During a restriction,** the user can still open the standing page and file appeals.

### Appeals
- **Who and when:** the user can appeal each strike **once**, within **14 days** of its issue time, from `/settings/standing`.
- **Appeal text:** 1-1000 characters of plain text, escaped when rendered and not passed through the word filter, so users can quote the content involved.
- **Rate limit:** 5 appeals per day.
- **Penalty while pending:** the penalty stays in force.
- **Strike-2 timing, stated plainly:** a strike-2 restriction lasts 72 hours, but appeals can be filed for 14 days and have a 7-day decision target. So a strike-2 appeal is usually decided **after the 72 hours have already ended**. Overturning it mainly removes the strike from the count, which lowers the level any future strike reaches. It rarely shortens the restriction itself. The appeal form and the standing page show this on every level-2 strike: "This restriction lasts 72 hours. Appeals are usually decided after it ends. If your appeal succeeds, the strike is removed from your record and no longer counts toward future penalties."
- The user sees "Appeal submitted", then "Strike upheld" or "Strike overturned", plus an optional message from staff of up to 500 characters. The message is always signed "S.V.E.R moderators", never with a staff identity.
- **Errors:**
  - "You've already appealed this strike." (409)
  - "The appeal window for this strike closed on {date}." (409)
  - "This strike can no longer be appealed." (409; already overturned)
  - Appeal length (400)
  - Not your strike (404)
- **Review:** appeals appear on an **Appeals** tab of the admin review queue (`/admin/appeals`), oldest first.
  - Each shows the strike, the content involved, the issuer's note, the linked reports' reasons and notes, the appeal text, and the user's strike history.
  - **Different reviewer:** the API refuses a decision by the strike's issuer ("Another moderator must review this appeal.", 403) whenever another staff account with MFA enabled exists.
  - When the issuer is the only eligible staff member, they may decide it with a required note, and the audit row is flagged `self_review = true`.
  - The decision requires step-up and a staff note.
  - The target is a decision within 7 days. There is no automatic outcome.
- **Outcomes:**
  - **Upheld:** the strike and penalty stay.
  - **Overturned:**
    - the strike's status becomes OVERTURNED and it stops counting
    - the effective restriction is recomputed and lifted as above
    - wall posts, replies or fan art the strike removed are restored to their previous state if they still exist
    - reset profile fields are not restored automatically; the user can enter them again
    - Linked reports are not reopened, and reporter statuses don't change.
- **Audit:** strike issued, strike notice email queued, interim restriction set, restriction lifted, appeal decided (with outcome and `self_review`) and account-ban review opened are all written to `moderation_actions`. Appeal submission is recorded in `appeals`.
- **Logs:** fixed-field only, using the existing `mod_event` format (for example `mod_event=appeal_decided outcome=overturned`).

### Staff roles and the review queue
- **Role model:**
  - A `staff_roles(user_id, role)` table with role `admin`.
  - Granted or revoked only by an operator CLI command run on the server (`sver-admin role grant|revoke`), never through the web. Each change is recorded in `moderation_actions`.
  - Legacy ADMIN roles are not imported automatically. The operator grants the user's own account after the deploy.
- **Staff requirements:** staff must have MFA enabled to use any admin route. Admin mutations require step-up within the last five minutes (Login's sensitive-change rule). Non-staff get 404 on `/admin/*` and its API.
- **Queue page:** `/admin/reports`.
  - Open reports are grouped by target, oldest first.
  - Each group shows report count, reasons, the reporters' notes, the snapshot, the current content and the target's recent moderation history.
  - Filters: target type, reason.
- **Actions:** each action closes every open report on the target and requires a moderator note of up to 500 characters.
  - **Dismiss.**
  - **Remove content:** wall post, reply or fan art. It is hidden for everyone except staff. The author sees "Removed by S.V.E.R moderators."
  - **Reset field:** avatar or banner goes back to the default; display name goes back to the username; bio, status, mood, song, a link, a sponsor, a setup item or a block is cleared.
  - **Restrict channel**, as a strike penalty or an interim restriction of up to 24 hours (see Strikes):
    - the channel page and user card are hidden (404)
    - the user can't edit public profile fields, post, reply, react, submit fan art or file reports
    - the user can still sign in, browse, follow and unfollow, view their account standing and file appeals, and sees a banner explaining the restriction and its end date
    - Lifting a restriction is an action too.
  - Staff can mark any action as also issuing a strike (see Strikes). Account-wide bans arrive with Module 3; until then, level 3 is an indefinite channel restriction. Username resets for impersonation also wait for the Module 3 moderation pass.
- **Audit:** every admin action, including role changes, is written to `moderation_actions(actor_id, action, target_type, target_id, report_ids, note, created_at)`. That table is never exposed outside `/admin`.
- **Logs:** fixed fields only, `mod_event=<action> outcome=<ok|denied>`. They contain no user IDs, content or notes, following Login's audit-event rule.
- **Queue tabs:** Reports (`/admin/reports`) and Appeals (`/admin/appeals`). An admin user page, `/admin/users/{username}`, shows that user's standing, strikes, restrictions and moderation history, and lets staff issue a strike directly (optionally converting an open interim restriction) or set or lift an interim restriction.

## Visual direction
- The game-client look from the homepage concept PDFs (see `AGENTS.md`): framed panels with code-drawn corner brackets, beveled buttons, Cinzel headings (self-hosted, no third-party font requests) and steel neutrals. **No glows, no gradients, no image frames.**
- Theme colors follow the **viewer's** faction:
  - Myria: ember
  - Aetheron: violet
  - Glint: gold on navy
  - Neutral steel when signed out or when the viewer has no faction
- Until Module 4, every viewer gets neutral steel. Channel owners' factions never recolor their page, and their imported legacy faction is ignored until Module 4.
- Theme tokens are CSS variables set once at the layout level from the viewer's session, so profile components never hard-code faction colors.
- Legacy elements are restyled, not copied. The War Council grid, wall cards, the song card and the sponsor and setup lists all use the frame style. The legacy `ProfileFrame` tier rings, animated borders and radial glows are ARCHIVE.
- **Accessibility:**
  - Every control is keyboard operable with visible focus.
  - Image alt text comes from the display name.
  - Text contrast meets WCAG AA in all four themes.
  - Reduced-motion is honored; there are no animated borders anyway.
  - The song player never autoplays.

## Editing surfaces
- **Split** (plan principle 9 puts complex creator tools in Creator Studio, not the viewer interface):
  - `/settings/profile`: username (with the rename rules and cooldown date), display name, avatar, banner, bio, mood/status, social links.
  - `/settings/blocked`: blocked users, with unblock.
  - `/settings/reports`: the user's reports, action-taken notices and the email opt-in.
  - `/settings/standing`: account standing, strikes and appeals.
  - `/account`: unchanged; Login's security settings.
  - `/studio/channel`:
    - Song
    - War Council
    - Wall (settings, pending queue, pins)
    - Schedule
    - Sponsors
    - Streaming setup
    - Blocks
    - Fan Art (setting and queue)
- Studio pages require sign-in (otherwise a redirect to `/login`). They are available to every account, because there is no creator application.
- **Saving:** each section saves on its own and atomically. A failed validation changes nothing in that section and returns field-level errors. This replaces legacy's single save for all seven tabs.
- **Change tracking:** the channel page reflects a saved change immediately, with no caching delay. Concurrent edits are last-write-wins per section, using `updated_at` checks (409 "This changed in another tab. Reload to see the latest." when it is stale).

## Data model (migration `0004_profiles.sql`; `0003` is Login's `0003_oauth_signup.sql`)

IDs are text, matching Login. Imported rows keep their legacy IDs; new rows get random UUIDs. Every user-owned row references `users(id)` with `ON DELETE CASCADE` unless noted. No changes are made to `users`, `identities` or `legacy_account_data` beyond the foreign-key references.

| Table | Key columns and constraints |
|---|---|
| `profiles` | `user_id` PK; `display_name` (not null); `bio`; `mood_emoji`; `status_text`; `avatar_key`; `banner_key`; `internal` bool; `restricted_until` (nullable; `infinity` for indefinite); `who_can_post` enum; `require_approval`, `hold_links`, `hold_new_accounts` bools; `fan_art_enabled` bool; `song_provider`, `song_media_id`, `song_url`, `song_title`, `song_artist`, `song_thumb_key`, `song_volume` (0-100), `song_notice`; `follower_count`, `following_count` (kept in transaction); `created_at`, `updated_at` |
| `username_history` | `id`; `user_id`; `old_username`; `new_username`; `reason` (`rename`, `import`); `changed_at` |
| `username_holds` | `handle_canonical` PK; `user_id` (nullable after erasure); `released_at`; `redirect` bool |
| `social_links` | `id`; `user_id`; `position` 1-5; `platform`; `url`; unique (`user_id`, `position`); per-platform uniqueness enforced in code |
| `follows` | `follower_id`, `following_id` PK pair; `created_at`; check that the two differ; index on `following_id`, `created_at` |
| `user_blocks` | `blocker_id`, `blocked_id` PK pair; `created_at`; check that the two differ |
| `war_council` | `user_id`, `position` 1-8 PK; `member_id`; unique (`user_id`, `member_id`); check `member_id <> user_id` |
| `wall_posts` | `id`; `wall_owner_id`; `author_id`; `body`; `status` APPROVED/PENDING/REJECTED/REMOVED; `pinned_position` 1-3 nullable; `created_at`; `deleted_at`; `moderated_at` |
| `wall_replies` | `id`; `post_id`; `author_id`; `body`; `status`; `created_at`; `deleted_at` |
| `wall_likes` | `post_id`, `user_id` PK pair; `created_at` |
| `schedules`, `schedule_blocks`, `schedule_events` | owner timezone; weekday, start, end, label; title, start_at, end_at |
| `sponsors`, `setup_items`, `profile_blocks` | ordered owner rows; `profile_blocks.type` enum ABOUT/PANEL/QUOTES/GAME_SHELF; `config` jsonb validated per type in code |
| `parts`, `part_submissions` (`0011_setup_parts.sql`) | SVER's setup parts list: `id`; `category`; `brand`; `model`; `norm`; `rank`; `source` SEED/STAFF; `status` ACTIVE/RETIRED; unique (`category`, `norm`). Custom-entry queue: `id`; `category`; `name`; `norm`; `submitted_by`; `status` PENDING/APPROVED/DISMISSED; `part_id`; `reviewed_by`; `reviewed_at`; unique (`category`, `norm`). `setup_items` gains `part_id`, `submission_id` and `legacy_category` |
| `fan_art` | `id`; `channel_id`; `submitter_id`; `image_key`; `artist_name`; `artist_link`; `caption`; `status`; `submitted_at`; `reviewed_at` |
| `reports` | `id`; `reporter_id` (nullable after erasure); `target_type`; `target_id`; `field`; `reason`; `note`; `snapshot` jsonb; `status` OPEN/ACTIONED/DISMISSED; `created_at`; `closed_at`; `reporter_notice` NONE/ACTION_TAKEN; `reporter_seen_at`; one open report per reporter and target |
| `strikes` | `id`; `user_id`; `reason`; `severity` STANDARD/SEVERE; `content_snapshot` jsonb; `report_ids` text[]; `removed_refs` jsonb (content to restore if overturned); `penalty` WARNING/RESTRICT_72H/RESTRICT_INDEFINITE; `interim_restriction_id` (nullable; the converted interim restriction); `penalty_starts_at` (the converted interim restriction's start, otherwise `issued_at`); a level-2 strike sets `penalty_until = penalty_starts_at + 72 hours`; `penalty_until` (nullable; `infinity` for indefinite); `message_to_user`; `issued_by`; `issued_at`; `expires_at`; `status` ACTIVE/OVERTURNED; `acknowledged_at`; `overturned_at` |
| `appeals` | `id`; `strike_id` unique (one appeal per strike); `user_id`; `body`; `created_at`; `status` PENDING/UPHELD/OVERTURNED; `reviewed_by`; `reviewed_at`; `message_to_user`; `staff_note`; `self_review` bool |
| `interim_restrictions` | `id`; `user_id`; `starts_at`; `until` (at most `starts_at + 24 hours`); `created_by`; `resolution` OPEN/CONVERTED/LIFTED; `resolved_at`; `resolved_by`; `overdue_at` (set when it ended with no decision) |
| `profiles` additions | `email_report_updates` bool, default false; `restricted_until`, kept as a cache of the effective restriction computed from active strikes and interim restrictions |
| `staff_roles`, `moderation_actions` | see Safety |
| `media_objects` | `key` PK; `owner_id`; `kind`; `bytes`; `created_at`; `delete_after` (cleanup queue) |
| `import_runs` | `name` PK; `completed_at`; aggregate `counts` jsonb; guards against a second import |

## API (all under `/api`)

Public reads need no session. Mutations need a session and the exact Origin. Responses never include emails, IP addresses or internal flags.

- **Channel reads (public):**
  - `GET /channels/{username}`: header, counts, song, War Council, Home previews, viewer relationship (`following`, `blocked`, `is_owner`) when signed in. Includes `redirect_to` when the name is held, which the web turns into a 302.
  - `GET /channels/{username}/wall?cursor=`
  - `GET /channels/{username}/schedule`
  - `GET /channels/{username}/about`
  - `GET /channels/{username}/fan-art?cursor=`
  - `GET /channels/{username}/followers?cursor=` and `GET /channels/{username}/following?cursor=`
  - `GET /users/{username}/card`
- **Follows:**
  - `PUT /follows/{username}` to follow; `DELETE /follows/{username}` to unfollow
  - `GET /me/following?cursor=`
- **Profile settings:**
  - `GET` and `PATCH /me/profile` (display name, bio, mood, status)
  - `POST /me/username` (step-up)
  - `PUT /me/links`
  - `POST`/`DELETE /me/avatar` and `POST`/`DELETE /me/banner`
- **Song:** `POST /me/song/preview` (oEmbed lookup), `PUT /me/song`, `DELETE /me/song`.
- **War Council:** `GET /me/war-council/search?q=`, `PUT /me/war-council`.
- **Wall settings and pins:** `PUT /me/wall/settings`, `PUT /me/wall/pins`.
- **Wall content:**
  - `POST /channels/{username}/wall` to post
  - `POST /wall/posts/{id}/replies` to reply
  - `PUT`/`DELETE /wall/posts/{id}/like`
  - `DELETE /wall/posts/{id}` and `DELETE /wall/replies/{id}`
  - `GET /me/wall/pending?cursor=`
  - `POST /wall/{posts|replies}/{id}/{approve|reject|block-author}`
- **Studio lists:** `PUT /me/schedule`, `PUT /me/sponsors`, `PUT /me/setup`, `PUT /me/blocks`, with `POST`/`DELETE /me/sponsors/{id}/logo`.
- **Fan art:**
  - `PUT /me/fan-art/settings`
  - `POST /channels/{username}/fan-art` to submit
  - `DELETE /fan-art/{id}`
  - `GET /me/fan-art/pending`
  - `POST /fan-art/{id}/{approve|reject}`
- **Blocks:** `GET /me/blocks`, `PUT /blocks/{username}`, `DELETE /blocks/{username}`.
- **Reports:** `POST /reports`.
- **Reporter feedback:** `GET /me/reports?cursor=`, `POST /me/reports/seen`, `PUT /me/reports/email`.
- **Standing and appeals:** `GET /me/standing`, `POST /me/strikes/{id}/acknowledge`, `POST /me/strikes/{id}/appeal`.
- **Admin:**
  - `GET /admin/reports?cursor=&type=&reason=`
  - `POST /admin/reports/{target_type}/{target_id}/actions`: body `{action, note, strike?: {reason, severity, message_to_user, interim_restriction_id?}}`
  - `GET /admin/users/{username}/standing`
  - `POST /admin/users/{username}/strikes`: body `{reason, severity, message_to_user, interim_restriction_id?}`
  - `POST`/`DELETE /admin/users/{username}/interim-restriction`
  - `POST /admin/users/{username}/restriction/lift`
  - `GET /admin/appeals?cursor=`
  - `POST /admin/appeals/{id}/decision`: body `{outcome: upheld|overturned, staff_note, message_to_user?}`
  - `GET /admin/parts?status=PENDING|APPROVED|DISMISSED`
  - `POST /admin/parts/{id}/decision`: body `{decision: approve|dismiss, brand?, model?}`
- **Setup parts:** `GET /parts?category=&q=` (signed in).
  - `GET /admin/moderation-actions?cursor=`
- **Status codes:**
  - 400 validation, with `{"error", "field"}` (the `field` key is new)
  - 401 signed out
  - 403 not permitted, verification required ("Verify your email to post."), or step-up required
  - 404 not found or hidden
  - 409 conflicts (taken name, cooldown, full list, stale edit, appeal already filed, appeal window closed, strike already overturned)
  - 403 "Another moderator must review this appeal." when a different eligible reviewer exists
  - 400 "An interim restriction can last up to 24 hours." and "Strikes can't be issued to internal accounts."
  - 413 file too large
  - 429 with `Retry-After`
  - 503 when a provider is unavailable
- 404 responses never reveal whether an account exists, is internal, deleted, held or restricted.

## Account deletion and erasure (extends Login)
- **During Login's 14-day deletion grace,** and for `legacy_deletion_hold` accounts:
  - the channel page and user card return 404
  - the account is left out of every list, count, War Council and search
  - its wall posts and replies on other channels show as "Deleted user" without a link, and their bodies stay hidden until the deletion is cancelled
  - Cancelling restores everything.
- **At erasure,** the worker deletes:
  - the profile, media objects (queued for storage deletion), links, song, schedule, sponsors, setup and blocks
  - follows in both directions, recomputing the counts it affects
  - War Council rows the user owns, and rows where they are a member
  - their wall, with all posts and replies on it
  - wall posts, replies and likes they authored elsewhere
  - fan art they submitted or received
  - blocks in both directions
- Strikes, appeals and interim restrictions on the account are deleted. Reports they filed keep the snapshot with `reporter_id` cleared, and their feedback rows go with it. Reports about them are closed as ACTIONED with the note "account erased". `moderation_actions` rows keep the action with `target_id` retained, because they are the audit record.
- Username history is deleted, and the username is held 30 days without a redirect.
- Held legacy accounts are never erased automatically (Login rule).

## Legacy profile import

This import is one-time and insert-only. It applies the same guardrails as the Login import in `OPERATIONS.md`, and the user must authorize the live run at the time it happens. It never reruns the account importer, and it never modifies `users`, `identities` or `legacy_account_data`.

### Sources (private, outside the workspace and Git)
- **Database:** `legacy.dump` (the fresh full legacy backup taken just before the account migration), from `C:\Users\Admin\SVER-dev\migration-20261002-9171fd49`. The server copy is in the root-only migration directory.
- **Media:** the avatar and banner files from the preserved snapshot `C:\Users\Admin\SVER-backups\accounts-20261002\accounts-20261002.tar.gz`, after verifying its checksums. The archive and the legacy R2 objects are never modified.

### Export
1. Restore `legacy.dump` into an isolated Postgres 18 container: no network, no published ports, a database created from `template0`, `pg_restore --exit-on-error --no-owner --no-privileges`.
2. Export these rows to a private JSON file beside the dump:
   - `Follow`: `followerId`, `followingId`, `createdAt`
   - `ProfileTop8`: `userId`, `targetUserId`, `position`
   - `SocialLink`: `userId`, `platform`, `url`, `createdAt`
   - `WallPost`: `id`, `authorId`, `wallOwnerUserId`, `body`, `createdAt`, `deletedAt`, `moderationStatus`, `moderatedAt`
   - `WallReply`: `id`, `postId`, `authorId`, `body`, `createdAt`, `deletedAt`
   - `WallReaction`: `postId`, `userId`, `type`, `createdAt`
3. Extract only the 5 referenced media files into a private temporary directory.
4. Remove the container and its storage after export.
- Everything else remains in the dump.
- Profile fields (display name, bio, mood, status, song, wall settings, pins, internal flag) are derived from the already-imported `legacy_account_data.account` and `legacy_account_data.profile`, which is read-only.

### Mapping

**Accounts**
- **Profiles:** one per imported user, including the 3 accounts without a legacy profile, using the identity-field import rules above.
  - `internal = true` for `admin`, `support`, `SVER`, and any account with legacy `isSystemAccount`. If that set is larger than 3, the import stops.
  - No `username_history` rows are created; legacy history wasn't exported and stays in the dump.

**Relationships**
- **Follows:** imported with their original `createdAt`.
  - Dropped and counted: self-follows, duplicates, and rows referencing users outside the 42.
  - Rows touching internal or held accounts are imported but hidden by the read rules.
  - Counts are computed after insert.
- **War Council:** imported in legacy position order, compacted to 1..n.
  - Self-references and missing users are dropped and counted.
  - Members who are internal or held are imported and skipped at read.
- **Social links:**
  - Platform mapping: legacy enum to the new list (`X` covers twitter.com and x.com, `WEBSITE` stays Website).
  - Unknown platforms with a valid https URL become Website.
  - Dropped and counted: URLs that fail the new rules (not https, wrong host, `javascript:`).
  - `http://` on an allowed host is upgraded to `https://`.
  - If more than 5 remain, the 5 oldest are kept and the rest are counted.

**Wall**
- **Posts and replies:** imported with the same IDs, status, timestamps and soft-deletion time.
  - Bodies are kept verbatim.
  - Dropped and counted: rows whose author or wall owner is missing, and replies whose post is missing.
- **Reactions:** LIKE and VALOR both become a like, at most one per user per post.
- **Wall settings and pins:** mapped as described in The Wall.

**Song**
- YouTube and SoundCloud records map; the Spotify record gets a `song_notice` and no song.
- Thumbnails are fetched afterwards by the post-import oEmbed job, rate-limited to one request per second. A failure leaves no thumbnail.

**Media**
- Each file is checked against the snapshot checksum, then run through the normal image pipeline.
- Avatars get a centered square crop; banners get a centered 3:1 crop.
- New keys are written to `profiles`.
- A file that fails to decode leaves the default image and is counted.
- The legacy URLs remain in `legacy_account_data`.

### Tool and modes (a `profiles` subcommand of `sver-import-check`, or a sibling binary sharing its guards)

**Rehearsal (default)**
- Guards:
  - refuses production mode
  - refuses non-loopback databases
  - refuses any database name other than `sver_rebuild`
  - refuses source paths inside this workspace
- Steps:
  1. Creates a UUID-named schema and applies all migrations.
  2. Loads the accounts from the private `accounts.json` with the existing account-import code path. That is rehearsal only, never against live.
  3. Runs the profile import against the exported JSON and media files, using a local filesystem storage adapter.
  4. Compares the results and removes the schema and stored files, whether it succeeded or returned an error.
- It never starts a web server or mail worker.

**`--check-live` / `--apply-live`**
- Requirements:
  - the Login importer's live guards: production configuration, origin `https://sver.tv`, loopback `sver_stage` on port 15432
  - migration `0004` already applied (live modes never migrate)
  - no `import_runs` row for `legacy-profiles`
  - zero rows in the follows, War Council, links and wall tables for the 42 imported user IDs. The import runs before the Module 2 web release enables these features.
- Check rolls back; apply commits atomically with a ten-second lock timeout.
- Media uploads go to the production bucket under new keys, before the commit. Check mode deletes the objects it uploaded; a failed apply queues them for deletion.
- Each mode requires a new private report filename, as the Login importer does.

**Verification inside the transaction**
- every imported row is compared field by field against the export
- counts are recomputed
- digests of `users`, `identities` and `legacy_account_data` match before and after
- every media key exists in the bucket with the expected byte length and dimensions
- Any mismatch rolls back.

**Live sequence**
1. Focused local tests.
2. Fresh-source rehearsal.
3. Fresh live database backup.
4. `--check-live`.
5. User go-ahead.
6. `--apply-live`.
7. Independent post-commit comparison.
8. Post-import live backup, restored in isolation and verified.
9. Thumbnail job.
10. Web release.

### Output and privacy
- Stdout and logs carry aggregate counts only: rows read, imported, skipped and dropped per category and reason; media processed or failed; internal accounts flagged; display-name, mood and status validation failures.
- They never contain usernames, IDs, emails, URLs, post bodies, file names or keys.
- A private per-row report (dropped row IDs and reasons) is saved beside the private export, outside the workspace and Git. Never print or paste it.
- `ChangeLog.md` and this document record aggregates only.

## Later-module stubs

| Feature | Module 2 behavior | Filled by |
|---|---|---|
| Live state, player, `/{username}/live`, live-first Following, War Council chat highlight, user card live dot and mod actions, chat use of blocks | Offline banner in the player slot; `/live` 302s to the channel; Following sorted by follow date; slots reserved, nothing rendered | Module 3 |
| Faction badge on channel, card and War Council tiles; faction theme for signed-in viewers; VALOR wall reaction | Empty badge slot; neutral steel for everyone; Like only | Module 4 |
| Schedule-aware discovery, similar channels | None | Module 5 |
| Videos, clips, featured or pinned content on the channel page | No section rendered | Module 7 |
| Beacons tab | None | Module 8 |
| Wall SUBSCRIBERS option, Subscribe and Valor tribute buttons | Hidden | Module 6 |
| Badges, achievements, featured stats, card frames | Not rendered; the only stats are follower and following counts and the join date | Phase 3 |
| Channel rewards (`/{username}/rewards`, decision P8) | Reserved sub-path showing "Rewards are coming"; no tab | Module 4 / Phase 2 (economy) |
| Notifications (wall posts, follows, approvals) | None; Studio shows pending counts. Report outcomes and strikes use `/settings/reports`, `/settings/standing` and the generic emails defined in Safety | Notification work (unscheduled) |

## Acceptance

Run against real Postgres, plus a local S3-compatible test bucket or the filesystem adapter, and a controlled HTTP oEmbed fake.

1. **Routing**
   - The split auth routes behave exactly as before, and the navigation check passes.
   - The route-walk test fails when a new unreserved `apps/web/app` folder or `apps/web/public` entry is added, and passes on the real tree.
   - Case variants return 308 to the canonical path.
   - The `/s/`, `/u/`, `/@` and `?tab=` aliases redirect.
   - Unknown, internal, deleted, held and restricted names all return an identical 404.
   - `/admin`, `/support` and `/SVER` never render a channel page.
2. **Usernames**
   - The format, reserved and leet-compacted checks, and the new edge-underscore and all-digit rules, apply at signup and rename.
   - Every reserved route name is refused.
   - Rename requires step-up and enforces the 60-day interval. Case-only changes are exempt.
   - The hold blocks other users and allows the one-time revert.
   - `/{old}` and its sub-paths return 302 to the current name with `no-store`. A chain of renames lands on the current name.
   - The name is released and 404s after 30 days.
   - Sign-in with the old name fails. Concurrent claims of one name produce exactly one winner.
3. **Identity fields**
   - Boundary lengths are accepted or rejected for display name, bio, status, links and block content.
   - Control, bidi and zero-width characters are rejected.
   - Impersonation display names are rejected.
   - The filter rejects without echoing the word.
   - Link host and scheme validation works, including `javascript:`, IP hosts and sver.tv.
   - `@username` is always rendered next to the display name.
4. **Images**
   - Magic-byte detection works: renamed non-images and SVG are rejected.
   - Size, dimension and decompression-bomb limits hold.
   - Crops are validated, and metadata is stripped (EXIF/GPS absent).
   - Three avatar sizes and up to three banner sizes are produced.
   - Old objects are queued for deletion. Removal returns the default.
   - The 413 and rate limits fire.
5. **Channel page**
   - Signed-out visitors see the page with counts and links.
   - The owner sees edit controls; other viewers see Follow, Block and Report.
   - Empty sections are hidden from visitors; the Schedule tab always shows, with an empty state.
   - No authenticated HTML reaches an anonymous follow-up request.
   - The metadata tags are present.
6. **Follows**
   - Follow and unfollow are instant and idempotent. Self-follow and blocked or hidden targets are refused.
   - Counts stay exact under concurrent follows. Rate limits fire.
   - Public lists paginate and omit hidden and blocked users.
   - The Following page requires sign-in.
7. **War Council**
   - The 8 limit, duplicates, self and ineligible members are refused.
   - Ordering persists.
   - Members who become ineligible are skipped at read, and the owner sees the notice.
   - A block removes the member both ways. The search excludes ineligible users.
8. **Song**
   - Accepted and rejected URL forms behave as specified.
   - The oEmbed fake checks for no redirect following, the timeout, the size cap and 401/404/5xx mapping.
   - The thumbnail is re-hosted.
   - The iframe appears only after a click, and there is no autoplay.
   - The Spotify record shows the owner notice and nothing publicly.
9. **Wall**
   - Each `who_can_post` mode works, along with blocks both ways, verification, the restriction state and the internal-account check.
   - The approval, link and new-account holds work, and PENDING and REJECTED visibility is correct.
   - Approve, reject and block-author work; pins are capped at 3.
   - Author and owner deletes work, and like toggles work.
   - Pagination works and the rate limits fire.
10. **Schedule, sponsors, setup, blocks, fan art**
    - Limits and validation hold.
    - DST transitions are correct: a block across a spring-forward and a fall-back weekend in two zones.
    - Overnight blocks work.
    - Fan art: the enable toggle, the queue cap, PENDING visibility, approve, reject, withdraw, and purge of rejected files.
11. **Safety**
    - Block side effects run in one transaction, and the block is silent.
    - Report dedupe, the rate limit, and snapshot persistence after edit or delete work.
    - Non-staff, and staff without MFA, get 404 on admin routes. Step-up is required for admin actions.
    - Every action closes its reports and writes `moderation_actions`.
    - A restriction hides the page and blocks edits, posts and reports until it ends.
    - Logs contain fixed fields only.
12. **Reporter feedback**
    - My reports lists only the viewer's own reports; other users and staff get 404 on its API.
    - Status mapping works: OPEN shows Under review, ACTIONED shows Action taken, DISMISSED and erased show Closed.
    - The notice and unread dot appear only for ACTIONED, every reporter of a multi-report target gets one, and opening the page marks them seen.
    - No response field or rendered text includes the action type, strike, restriction, appeal outcome, reviewer or other reporters.
    - An overturned appeal leaves reporter statuses unchanged.
    - The email setting is off by default. When enabled, at most one generic digest goes out per 24 hours, only to verified addresses, with an idempotency key and no names or content.
13. **Strikes and appeals**
    - Strikes 1, 2 and 3 apply a warning, a 72-hour restriction (ending exactly 72 hours after issue, or after the start of a converted interim restriction) and an indefinite restriction. A SEVERE strike goes straight to level 3.
    - Strikes stop counting after 90 days (STANDARD) or 365 days (SEVERE); expiry doesn't shorten an active penalty.
    - Interim restrictions are capped at 24 hours and need no strike. The queue shows each one's end time. One left undecided ends at 24 hours and is flagged overdue until staff record a strike or a lift. Converting one to a strike for the same incident never stacks the two. The effective restriction is the latest end among active strikes and interim restrictions, recomputed on issue, overturn, lift and expiry.
    - Strikes can't be issued to internal accounts. Unchecking the strike box requires a note.
    - The standing page shows only the user's own strikes and content, never reporters or the issuer. The acknowledgement banner persists until acknowledged.
    - Every level-2 strike's appeal form and standing entry show the plain-language strike-2 timing notice.
    - Appeals: one per strike, within 14 days, 1-1000 characters, 5 per day. A second appeal, a late appeal or an appeal on an overturned strike is refused. Restricted users can still appeal.
    - An issuer deciding their own appeal gets 403 while another MFA-enabled staff account exists, and succeeds with `self_review = true` when none does. Decisions require step-up.
    - Overturning removes the strike from the count, lifts restrictions that no longer apply, restores removed wall or fan art content, and leaves reset fields and reporter statuses unchanged.
    - Every strike, restriction and appeal step writes `moderation_actions`, and logs carry fixed fields only.
    - Erasure removes the account's strikes, appeals and feedback rows.
14. **Erasure**
    - During the grace period the page is hidden and content shows as "Deleted user". Cancelling restores it.
    - After erasure, every listed table is cleared, media is queued for deletion, counts are recomputed, the username is held without a redirect, and reports and audit rows are handled as specified.
    - Legacy-held accounts never enter erasure.
15. **Import**
    - Rehearsal: correct aggregate counts; the guards refuse production, remote and in-workspace sources.
    - A second run is refused.
    - A seeded conflict and an injected mismatch both roll back.
    - The `users`, `identities` and `legacy_account_data` digests are unchanged.
    - Stdout is free of IDs, names, URLs and bodies.
    - All 5 media files are processed, or failures are counted.
    - `--check-live` rolls back cleanly before `--apply-live`.
16. **Visual**
    - Desktop and mobile rendering in neutral steel, with each faction theme applied through the tokens.
    - No gradients, glows or animated borders.
    - Keyboard and contrast checks pass.

Before closing: the live import is completed and verified under the user's authorization. Browser QA of the channel page, settings (including My reports and Account standing), Studio, wall and the admin Reports and Appeals queues is done on staging. The user confirms their own imported channel at `sver.tv/{username}` shows the right display name, avatar, banner, bio, links, War Council and wall.

Status, October 3, 2026: on Joe's instruction, the legacy profile import ran on staging from the newest nightly legacy backup and was verified with aggregate counts only:
- 42 profiles: 39 legacy profiles plus 3 defaults for accounts without one
- 57 of 57 follows
- 6 of 6 War Council picks
- 17 of 18 links: one owner had 6, and the 5 oldest were kept
- 2 of 2 wall posts; no replies or reactions exist
- 3 avatars and 2 banners re-hosted

An independent comparison, plus a post-import backup restored in isolation, matched. Two operator decisions applied to this import:
- Only admin, support and SVER are internal. The 2 accounts with the legacy system flag were imported as public accounts and can be hidden later. This supersedes the stop in decision 3 for this import.
- Images are stored on the server's filesystem until the `media.sver.tv` bucket exists.

One imported banner's centered 3:1 crop was below 1200x400, so the import upscaled that crop to the minimum instead of dropping it. Uploads still refuse small images.

Status, October 3, 2026, 3:15 PM ET: the 9 parity additions (P1–P9) are deployed to staging as API and web `m2p2-20261003` (migration `0009`). Their automated acceptance tests pass locally, and a signed-out headless-Chrome pass on staging shows the Share button, header copy and rewards stub with no client errors.

Before closure, Profiles stayed open until:
- Cloudflare's "SVER Uploads" skip rule covers `/api/me/setup/photos` (setup photo uploads through Cloudflare get 403 until then; the exact change is in `docs/OPERATIONS.md`)
- a signed-in staging browser pass covers settings, Studio (including P3, P4 and P9), the wall and the admin Reports and Appeals queues
- Joe confirms his own channel

### Closure

Closed October 3, 2026, 7:31 PM ET. Joe confirmed on staging that everything looks good, including his JoeTheChode setup built with the parts picker, and said to close Profiles. Staging then ran API and web `m2interfaces-20261003` (migrations through `0012`). Remaining items are later or follow-up work, not closure conditions: the `media.sver.tv` media domain (images stay on the server's filesystem until then), whether to hide the 2 imported accounts with the legacy system flag, and site polish. Operations follow-ups are tracked in `ChangeLog.md`.

## Done when

- Any visitor can open any eligible channel at `sver.tv/{username}`. Internal, deleted and restricted accounts have no page, and every top-level route is a reserved name checked by a failing test.
- A user can:
  - set a username, display name, avatar, banner, bio, mood/status and up to 5 links
  - rename within the 60-day rule, with `/{oldname}` redirecting for the 30-day hold
  - follow and unfollow, and see their Following page and anyone's public follower/following lists
  - curate a War Council, set a YouTube or SoundCloud profile song, and fill in a schedule, sponsors, setup and custom blocks
- Viewers can sign, reply to and like walls under the owner's posting and approval rules, and submit fan art for approval.
- Users can block and report, and see "Action taken" on their own reports without learning the penalty or reviewer.
- Staff can resolve reports from the admin review queue. Confirmed violations issue strikes with the defined warning, 72-hour and indefinite levels and expiry. Interim restrictions never exceed 24 hours, and they never stack with the strike they become.
- Users can see their strikes and appeal each one once within 14 days, and a different staff member (where one exists) can uphold or overturn it, with an overturned strike's penalty lifted and every step audited.
- Legacy follows, War Council picks, social links and wall posts, plus the 3 avatars and 2 banners, are imported and verified with aggregate-only output.
- Later-module features show their defined stubs.
- The 9 parity additions P1–P9 (Joe's decisions of October 3, 2026, 2:34 PM ET) pass their acceptance tests on staging.

## Decisions approved October 3, 2026

The user approved items 1-19 as drafted, together with fixes 20-23.

1. **Routing:**
   - sub-path tabs (`/{u}/wall`, schedule, about, fan-art, followers, following)
   - `/{u}/live` returning 302 to the channel until Module 3
   - `/s/`, `/u/`, `/@` aliases as 308
   - 302 with no-store for hold redirects
2. **Username rules:**
   - no leading or trailing underscore, and not all digits
   - case-only renames are free
   - a one-time revert to your own held name
   - the old name can't be used to sign in during the hold
   - erased names held 30 days
   - Login sessions kept after a rename
3. **Internal accounts:** `isSystemAccount` accounts count as internal (the import stops if that adds any). Internal users can view and follow but can't post or appear in lists.
4. **Display name:** allowed characters and punctuation; staff-term impersonation check; imported values kept verbatim.
5. **Images:**
   - direct multipart upload
   - avatar sizes 64/160/400 and a 3:1 banner (750/1500/3000)
   - minimum dimensions
   - first frame of animated images
   - a new bucket served from `media.sver.tv`
   - centered crops for the imported files
6. **Text limits:** bio up to 4 line breaks; status 80; wall posts up to 8 line breaks; a 50 posts/day cap; replies follow the post review rules.
7. **Links:** platform and host list; Website up to 2 times; http upgraded to https on import; the 5 oldest kept on import.
8. **Song:** no autoplay ever; thumbnails re-hosted; Spotify shows only an owner notice (no Spotify embed in v1).
9. **Wall:**
   - 3 pins
   - deleted rows purged after 30 days
   - VALOR imported as like
   - SUBSCRIBERS mapped to NONE
   - posts by ineligible authors stay visible, unlinked
10. **Schedule, sponsors, setup and blocks:** 21 weekly blocks and 20 events; the sponsor and setup category lists; 10 custom blocks of 4 types with a markdown subset; none of these imported.
11. **Fan art:** off by default; the attestation checkbox; 5 submissions per day, 20 pending, 100 shown; not imported.
12. **Safety:**
    - a 1,000-block cap and the block side effects
    - the report reason list and 90-day snapshot retention
    - a `staff_roles` table granted only by a server CLI, with MFA and step-up required for staff
    - channel restriction as the strongest Module 2 action
13. **Surfaces:** profile identity under `/settings/profile` and channel features under `/studio/channel`; per-section saves; stale-edit 409.
14. **Data model and API:** the table and path names, `0004_profiles.sql` (renumbered because Login added `0003_oauth_signup.sql`), and the added `field` key in error responses.
15. **Erasure:** audit rows keep target IDs; erasure also removes the user's own wall and its posts.
16. **Import:** tool placement; the import must run before the web release enables the features; the live sequence.
17. **Reporter feedback:**
    - `/settings/reports` with the Under review, Action taken and Closed statuses, kept for 180 days
    - a notice only on Action taken
    - overturned appeals never change reporter status
    - an opt-in generic email digest (once per 24 hours) through Login's mailer, or in-app only if the mailer doesn't fit
18. **Strikes:**
    - a strike checkbox checked by default, uncheckable with a note
    - level 1 is a warning, level 2 a 72-hour restriction, and level 3 an indefinite restriction that opens a ban review once Module 3 bans exist
    - expiry of 90 days (STANDARD) or 365 days (SEVERE)
    - the SEVERE list that skips to level 3
    - interim restrictions without a strike (24 hours at most; see fix 20)
    - expiry doesn't shorten active penalties
    - an always-sent generic standing email
19. **Appeals:**
    - one per strike within 14 days, 1-1000 characters, 5 per day, no word filter
    - the penalty stays during review
    - a different reviewer is enforced when one exists, with a `self_review` fallback
    - a 7-day decision target
    - overturning restores removed content but not reset fields
20. **Interim restriction:** 24 hours at most; when it ends, staff must issue a strike or lift it. One left undecided ends at 24 hours and stays in the queue as overdue.
21. **No stacking:** a strike for the same incident counts its restriction time from the start of the interim restriction.
22. **Strike 3:** an indefinite restriction plus a staff ban review once Module 3 bans exist. There is no intermediate step.
23. **Strike-2 appeals:** the spec and the appeal page say plainly that a strike-2 appeal is usually decided after the 72 hours end, so overturning it mainly removes the strike from the count.

## Parity additions: Joe's decisions, October 3, 2026 (2:34 PM ET)

The parity audit (`docs/PROFILES_PARITY.md`) found 9 legacy features that Module 2 neither had nor deferred. Joe decided to build all 9 before closing Profiles. These are his decisions. Each one lists the rules, the edge cases and the acceptance tests. Migration `0009_profile_parity.sql` carries the schema for all of them.

### P1. Share / copy link on channels

Rules:
- Every channel header has a **Share** button next to Follow or Edit, for every viewer (signed out, signed in, owner).
- The shared URL is always the canonical channel URL `{origin}/{username}` (current casing, no sub-path, no query). It isn't the URL of the tab being viewed.
- On a touch device (`pointer: coarse`) with `navigator.share`, the button opens the native share sheet with the title "{display name} (@{username}) on S.V.E.R" and the URL.
- Everywhere else it copies the URL to the clipboard and shows "Link copied" for 2 seconds.
- No server call, no tracking, no share counts.

Edge cases:
- If the user cancels the native sheet (`AbortError`), nothing is shown.
- If the clipboard is refused or missing (permissions, insecure context), the button shows a read-only text field holding the URL, already selected, with "Copy this link".
- After a rename, the button uses the new name, because the page is served under the canonical name.

Acceptance:
- The button renders for signed-out viewers, viewers and the owner.
- Clicking it on desktop puts exactly `https://sver.tv/{username}` on the clipboard (stage origin), even from `/wall`.
- With `navigator.share` on a coarse pointer, the share sheet receives that URL.
- When the clipboard rejects, the selected fallback field appears.

### P2. Mood emoji presets

Rules:
- Under the Mood field in `/settings/profile` there is a row of 12 preset buttons: 🎮 🔥 😎 🎧 ⚔️ 🛡️ 🏆 💀 😴 🍕 🎉 ❤️, plus **Clear**.
- A preset fills the field; nothing is saved until **Save profile**. The preset that matches the field is marked pressed (`aria-pressed`).
- Typing any other single emoji still works. The server rule is unchanged: exactly one emoji, or empty.

Edge cases:
- Presets with a variation selector (⚔️ 🛡️ ❤️) are one emoji and must pass the server rule.
- Clear empties the field; saving then removes the mood.

Acceptance:
- A unit test checks that every preset passes `text::mood`.
- In the browser, picking a preset and saving shows that emoji on the channel header; Clear plus Save removes it.

### P3. Page readiness checklist

Rules:
- Owner only, in Creator Studio. `/studio/channel` (previously a redirect to Song) becomes **Channel overview** with the full checklist.
- The Studio channel navigation gains **Overview** (`/studio/channel`) and **Page header** (`/studio/channel/header`, P9).
- While the checklist is incomplete and not dismissed, every `/studio/channel/*` section page (not the overview itself, which shows the full list) shows a one-line banner: "Page readiness: {done} of 7 done", a link "Finish your page" and **Dismiss**.
- `GET /api/me/readiness` computes the 7 steps on the server. Each step has a key, label, done flag and fix link:
  1. avatar: an avatar is uploaded
  2. banner: a banner is uploaded
  3. bio: the bio isn't empty
  4. links: at least one social link
  5. song: a profile song is set
  6. council: at least one War Council member
  7. schedule: at least one weekly block or upcoming event
- `PUT /api/me/readiness {"dismissed": true|false}` stores or clears `profiles.readiness_dismissed_at`.

Edge cases:
- Dismissed: the banner hides on every device. The overview still shows the checklist, with "Show the reminder again".
- All 7 done: the banner hides by itself, and the overview says "Your page is ready."
- A step that becomes undone again (avatar removed) shows as not done. It doesn't undo a dismissal.
- No endpoint exposes anyone else's readiness, and nothing appears on the public channel.

Acceptance:
- The API reports each step done as it's completed and counts done/total.
- Dismiss persists across sessions and restore works.
- Requests are signed-in only (401 signed out).
- The banner shows on Studio channel pages, disappears after Dismiss and never renders on the channel page.

### P4. Setup photos, title and description

Rules:
- Streaming setup gains a title (0–80 characters, one line) and a description (0–500 characters, up to 6 line breaks, plain text). Both are word-filtered and saved with `PUT /api/me/setup` (optional `title` and `description`; omitted means unchanged) under the existing "setup" revision.
- Up to **3 photos**:
  - `POST /api/me/setup/photos` (multipart `file`, optional `alt` 0–120 characters, filtered)
  - JPG, PNG or WebP, up to 5 MB, at least 64 px and at most 4096 px on each side
  - stored as 400 px and 1600 px WebP that fit inside those sizes (no crop, metadata stripped, first frame only) under the new media kind `setup_photo` (`setup/{hash}/`)
  - the image upload rate limit shared with the other images (20 per hour)
- `PUT /api/me/setup/photos {"photos":[{"id","alt"}]}` sets the order and alt text, and must list exactly the owner's current photos. `DELETE /api/me/setup/photos/{id}` deletes one photo and queues its media for deletion, unless another row still uses the same content-addressed key.
- Photo uploads, reorders and deletes don't bump the "setup" revision, so they never make an open gear or text edit stale.
- Photos are the owner's own content (like the avatar). They need no approval and show at once on About, in the Streaming setup section: title, description, a photo row (400 px; the 1600 px image opens in a new tab), then the gear grouped by category. Photos, title or description alone make the About tab visible.
- **Moderation, as fan art:**
  - report target `setup_photo`, with the Report button on each photo
  - staff "Remove content" sets the photo REMOVED, which hides it from visitors; the owner sees "Removed by moderators" in Studio and can delete it
  - an overturned appeal restores it
  - the existing "setup" field reset also clears the title, description and photos
  - snapshots hold the image key and alt text; channel-field report snapshots now also include `setup_text` (title and description) and `header` (P9)
- **Upload route:** Nginx gives `/api/me/setup/photos` the 11 MB upload body like the other image paths. Cloudflare's "SVER Uploads" skip rule must add this path (exact change in `docs/OPERATIONS.md`). No existing covered pattern fits cleanly.

Edge cases:
- A 4th photo is refused with 409 "You can add up to 3 setup photos." The count is checked under a lock on the owner's profile row, so parallel uploads can't exceed 3.
- The same image uploaded twice is refused with 409 "You already added that photo."
- A REMOVED photo still counts toward the 3 until the owner deletes it, so an appeal can always restore it.
- Restricted users can't upload or edit setup (403). Non-images, oversized files and tiny images are refused with the image error messages.
- Erasure deletes the rows (cascade) and queues the media.
- Photos are public like the rest of About. A block limits interaction, not viewing, so blocked viewers can still see them (as with every other channel section).

Acceptance:
- An upload creates the 400 and 1600 WebP files and shows on About.
- A 4th upload gets 409; a reorder with a wrong set gets 400; delete queues media.
- Title and description are length- and filter-checked.
- Restricted gets 403.
- A report → Remove content hides the photo from About; an overturn restores it; the setup reset clears everything.
- `nginx -t` passes with the new path in the upload location.

### P5. Social link suggestions from linked Twitch and Discord accounts

Rules:
- Login stores the provider's public handle on the identity (`identities.handle`):
  - Twitch: `login`, lowercase `[a-z0-9_]{3,25}`
  - Discord: `username`, `[a-z0-9_.]{2,32}`
  - Google: none
- The handle is set on sign-up, linking and every sign-in with that provider. A value that fails the pattern is stored as no handle.
- `GET /api/me/link-suggestions` returns, for linked providers:
  - Twitch: `https://twitch.tv/{login}`, when a handle is known
  - Discord: `https://discord.com/users/{discord user id}` (the identity's provider subject; always available for a linked Discord account)
  - A platform the user already has a link for is left out.
- The Discord link rule also accepts `https://discord.com/users/{17–20 digits}`.
- `/settings/profile` → Social links shows "From your linked accounts" with an **Add** button for each suggestion. It adds a row to the form; nothing is saved until **Save links**. Suggestions never apply themselves.

Edge cases:
- Identities linked before this release have no Twitch handle until the next Twitch sign-in. The UI says "Sign in with Twitch once to suggest your channel link."
- No suggestion is shown when 5 links already exist (the Add button is disabled with "You already have 5 links").
- Unlinking a provider removes its suggestion.

Acceptance:
- Suggestions come only from linked providers, never for an existing platform, and never saved without the user.
- The Discord users URL is accepted by the validator.
- The handle is updated on sign-in.

### P6. "Also known as" on the user card (opt-in)

Rules:
- `/settings/profile` has a checkbox "Show my linked Twitch and Discord accounts on my user card", stored as `profiles.show_linked_accounts`, **off by default**. Saved with `PUT /api/me/card-settings {"show_linked_accounts": bool}`.
- The migration sets it off for everyone. The legacy import and every other path never set it; only the account owner can turn it on.
- When on, `GET /api/users/{u}/card` includes `also_known_as` (Twitch first, then Discord): Twitch `{platform, handle, url: "https://twitch.tv/{login}"}` and Discord `{platform, handle, url: null}` (text only), for linked identities with a known handle. Otherwise it's an empty list.
- The card shows "Also known as" with the Twitch link (`nofollow noopener noreferrer`) and the Discord name as text.

Edge cases:
- Off, or on with no handles: nothing is shown.
- Unlinking a provider removes it from the card at once.
- Not shown when the viewer and the user block each other (either direction).
- Not shown on the channel page, only on the card.

Acceptance:
- Off by default for new accounts and imported accounts (the import test checks it).
- On shows only providers with handles; off hides them; unlink removes them; a blocked pair sees none.

### P7. Activity feed (Module 2 scope)

Rules:
- Table `activity_events (id, actor_id, kind, subject_id, ref_id, data, created_at)`. Users are foreign keys with cascade. `kind` is text checked against a registry in code (`activity.rs`). Module 5 adds kinds by adding entries there, with no schema change. Unknown kinds are skipped when read.
- Module 2 kinds, recorded when the action succeeds:
  - `follow`: A follows B. One row per pair; a refollow moves it to now.
  - `wall_post`: a post by A on B's wall becomes visible (posted without approval, or approved). Subject B, ref the post. A post on your own wall has subject = actor.
  - `war_council`: A saves a War Council with at least one member.
  - `song`: A saves a profile song (data: title, artist).
  - `schedule`: A saves a schedule with at least one block or event.
- `war_council`, `song` and `schedule` coalesce: a save within 60 minutes of A's previous event of that kind updates it instead of adding a row.
- Removing the song deletes A's song events; resets of the song, schedule or War Council by staff delete those events too.
- `GET /api/channels/{u}/activity?cursor=` lists the channel owner's events, newest first, 20 per page. Each item has kind, time, subject chip and data. Same visibility as the channel: 404 when the channel isn't eligible (unknown, internal, deleted, held or restricted). Blocks don't hide the channel; they filter events as below.
- Read filters (applied at read time, so later changes take effect at once):
  - the subject is eligible (not internal, deleted, held or restricted)
  - no block between the viewer and the subject, or between the actor and the subject
  - follow events only while the follow exists
  - wall_post events only while the post is APPROVED and not deleted
- The Home tab ends with "Recent activity", after the wall preview and Up next: the latest 5 with **Show more**. Visitors see nothing when it's empty; the owner sees "Your recent activity appears here."
- Retention: the profile job purges events older than 90 days. No backfill and no legacy import.

Edge cases:
- Internal accounts can follow, but their events aren't recorded.
- A restricted user can still follow (and that follow is recorded), but their own channel and feed are hidden while restricted. Actions a restriction blocks (posting, Studio edits) record nothing. A restricted subject hides the event in other feeds.
- Erasure deletes every event where the user is actor or subject (cascade).
- Everything shown is already public (follower lists, wall, War Council, song, schedule); there are no notifications.

Acceptance:
- Each kind is recorded; coalescing works within 60 minutes.
- Unfollow hides the follow event. A deleted, pending or removed post hides its event.
- A blocked or restricted subject is hidden; an internal actor records nothing.
- Erasure removes events; pagination works; an unknown kind is skipped.

### P8. `/{username}/rewards` (stub)

Rules:
- A reserved channel sub-path that renders the channel frame with a "Rewards are coming" panel: "Channel rewards arrive with the Valor economy." It has no tab in the tab bar until the economy exists (Module 4 / Phase 2).
- It's a stub, listed in "Later-module stubs". Visibility and 404 rules are the channel's.

Acceptance:
- `/{u}/rewards` is 200 with the placeholder for a visible channel and 404 for unknown or ineligible names.
- The tab bar has no Rewards tab.

### P9. Decorative header copy

Rules:
- The owner edits it in Studio → **Page header** (`/studio/channel/header`), saved with `GET`/`PUT /api/me/header` under the section revision "header":
  - Page label pill: 0–24 characters, one line. Default "Creator Page".
  - Welcome line: 0–80 characters, one line. Default "Welcome to my page".
  - Intro card: a title (0–60, one line) and a body (0–500, up to 6 line breaks, plain text). Shown only when the body isn't empty; with no title, the heading is "About this page".
  - Page vibe: 0–24 characters, one line, shown as "Vibe: {text}" in the counts row. No default.
  - "Show the label and welcome line" toggle, default on.
- Empty label or welcome means "use the default". All text is plain, word-filtered, with no links or markup.
- The channel API returns `header: {label, welcome, intro_title, intro_body, vibe}` with the defaults resolved. `label` and `welcome` are null when the toggle is off.
- Design: the label is a bordered small-caps pill (no fill, gradient or glow), the welcome line is muted text under the handle, and the intro card is a standard panel at the top of Home.
- **Safety:** reportable as the channel field `header`. The staff field reset `header` restores the defaults and turns the toggle on.

Edge cases:
- Imported and new channels show the defaults.
- Whitespace-only input counts as empty.
- A stale save gets 409 like the other sections.

Acceptance:
- Defaults render for a new channel; edits persist and render.
- Clearing restores the defaults; toggle off hides the label and welcome line; the intro is hidden when its body is empty.
- Limits and the filter are enforced; the reset field restores the defaults.

## Setup parts picker: Joe's decision, October 3, 2026 (4:52 PM ET)
Spec addition. Users build their streaming setup by picking parts instead of typing everything.

Decision:
- PCPartPicker has no public API and its terms bar copying its catalog, so SVER keeps its **own** parts list. The seed list was written from general knowledge of popular current and recent parts. Nothing is scraped or copied from PCPartPicker or any other catalog.
- "Just core streaming stuff someone would need to know." Categories are **only**: CPU, GPU, RAM, Motherboard, Camera, Mic, Peripherals.
  - Peripherals is one category covering headsets, keyboards, mice, monitors, capture cards and stream decks.
  - No case, cooler, PSU, storage or other accessories.
- v1 has no prices or store links. The owner's own optional link per item stays as before.

Parts list:
- Each part has a brand and a model. The shown name is "brand model".
- Names are normalized for matching: lowercase, `+` read as "plus", every other non-letter or non-digit becomes a space, spaces collapsed. One part per category and normalized name.
- Seed (migration `0011_setup_parts.sql`, 335 parts): CPU 46 (Intel and AMD), GPU 50 (NVIDIA, AMD and Intel), RAM 36 (common DDR4 and DDR5 kits), Motherboard 46 (popular boards by chipset), Camera 41 (webcams, mirrorless and streaming cameras), Mic 41 (popular USB and XLR mics), Peripherals 75.
- **Audio interfaces in Mic (Joe's decision, October 3, 2026, 5:52 PM ET):** the Mic category also lists audio interfaces and streaming mixers. Migration `0012_mic_interfaces.sql` adds 41 of them, written from general knowledge, including the GoXLR and GoXLR Mini, the Focusrite Scarlett and Vocaster lines, RODECaster Duo and Pro II, Elgato Wave XLR, Universal Audio Volt, MOTU M2/M4, Audient iD and EVO, PreSonus, Behringer UMC, Shure MVX2U and Beacn Mix. That makes Mic 82 parts and the list 376.
  - `parts.kind` is `AUDIO_INTERFACE` for these and null for everything else. The insert is idempotent: re-running it only marks existing names as interfaces.
  - The category shows as **Mic & audio interface**. Search results, Studio items and About entries for interfaces carry an "Audio interface" label.
  - Entries from the old AUDIO_INTERFACE category (already in Mic, with `legacy_category`) show as interfaces too, while they stay custom entries. A linked entry takes its kind from the part.
- Staff add parts by approving custom entries. Parts are never deleted while in use; `RETIRED` hides a part from search and new picks.

Picker (Studio, Streaming setup):
- One search-as-you-type box per category: an ARIA combobox (arrow keys, Enter, Escape) over `GET /api/parts?category=&q=`.
  - Signed in only. Up to 10 results.
  - Every typed word must match. Results are ordered exact match, then name starts with, ends with, word starts with, then shorter names and the seed rank. An empty box shows the category's top 10.
- Picking a part adds it with the canonical name and a "SVER list" tag.
- **Custom entry fallback:** the last option is always "Add “typed text” as a custom entry" unless the text exactly matches a listed part. Typing a listed part's exact name (after normalization) picks that part.
- Each item keeps its optional note and link, can move up or down within its category, and can be removed. 20 items in total, as before. Nothing is saved until Save setup.
- The public About tab groups items in the fixed category order (CPU, GPU, RAM, Motherboard, Camera, Mic, Peripherals, then Other), in owner order within a category. Custom entries show right away, exactly as typed.

Custom entries and staff review:
- A custom entry is saved on the user's setup immediately and queued for staff review as PENDING.
  - One queue entry per category and normalized name. Later users who type the same name share it.
  - A user can create up to 30 new queue entries per 24 hours. Past that, entries still save but aren't queued.
  - Names use the usual setup text rules (1–80 characters, word filter).
- Staff review at `/admin/parts` ("Setup parts" in the admin nav). It reuses the Module 2 admin rules: staff role plus MFA to view (everyone else gets 404), and step-up within the last five minutes for decisions (403 otherwise).
  - Waiting / Added / Dismissed tabs. Each entry shows the category, typed name, who first added it, when, how many setups use it, and similar listed parts.
  - **Add to the parts list:** staff tidy the brand and model, which adds a STAFF part (or reuses an existing part with the same normalized name). Every setup entry linked to that queue entry becomes that part with its canonical name.
  - **Dismiss:** setups keep the entry as typed. The name isn't queued again.
  - A second decision on the same entry gets 409. Both decisions are audited (`part_approved`, `part_dismissed`).
- Studio tags custom entries "Custom · waiting for review" while pending, otherwise "Custom".

Existing data (no one loses what they entered):
- Migration `0011_setup_parts.sql` keeps every existing item's name, note, link and order, and stores the original category in `legacy_category`.
- Mapping: MICROPHONE and AUDIO_INTERFACE → Mic (AUDIO_INTERFACE entries are labeled "Audio interface"). HEADPHONES, CAPTURE, MONITOR, KEYBOARD, MOUSE and CONTROLLER → Peripherals. CPU, GPU and CAMERA are unchanged. PC, LIGHTING and OTHER → Other.
- Other is shown in Studio and on About only when a user has such items. Kept names are read-only; owners can change the note and link or remove the item, and add new items in the seven categories. New items can't use Other.
- Existing items stay custom (untagged) until the owner next saves. That save links an exact match to the listed part and queues the rest like any custom entry.
- The setup title, description and photos are unaffected.

Edge cases:
- A picked part that has since been retired or doesn't match the category gets "Choose a part from the list again."
- Account erasure deletes the user's setup items and the queue entries they started. Other users' entries that shared a deleted queue entry stay on their setups and are queued again on their next save.
- No new upload kinds, prices, store links or affiliate data.

Acceptance:
- Seed counts per category as above. All seven pickers search, pick by keyboard and mouse, and offer the custom entry.
- Custom entries save at once, appear on About, and queue as pending. Staff can approve one (the setups link to the new part) or dismiss it (setups unchanged).
- Non-staff get 404 on the queue and its API. A stale step-up gets 403. Search is 401 when signed out.
- Pre-picker items of all 14 old categories survive the migration with name, note, link and order intact.
- Setup photos and the rest of Profiles still work.

## Deferred to later modules
- Account-wide ban mechanics, which level 3 escalates to, and username resets for impersonation: the Module 3 moderation pass.
- Schedules, sponsors, page blocks, fan art and legacy username history are not imported in Module 2 (approved). They remain in the legacy dump for a later import if needed.
- Whether follower/following lists need a per-user privacy toggle later.
