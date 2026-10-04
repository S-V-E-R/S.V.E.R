# Module 9: Beacons

Specified October 4, 2026, from the plan's hardened decisions and the legacy review. This module builds after VODs and clips ([VODS_CLIPS.md](VODS_CLIPS.md)). It is the last core module, and Phase 2 starts when it closes.

Beacons are short vertical videos that lead people to creators and their live streams. A Beacon is a light that shows where to go, so success means viewers who join a live stream or follow a creator, not time spent scrolling.

## Who posts

- Anyone who has streamed on S.V.E.R at least once, including guardian-approved accounts aged 13–17.
- **Limit:** up to 10 Beacons per creator per day.
- A creator posts to their own channel only. Viewers can't post Beacons, but a viewer's approved clip of a channel can become one of that channel's Beacons (see below), with the clipper credited.

## Sources

- **From a clip:** the creator picks one of their channel's approved clips ([VODS_CLIPS.md](VODS_CLIPS.md)), then drags a 9:16 crop window over it. The crop they last used is remembered. A clip from a VOD or Highlight works the same way. The Beacon links back to its clip and broadcast.
- **Upload:** MP4, MOV or WebM, up to 200 MB, 5–60 seconds.
  - The upload goes straight to private storage through a signed upload URL, then the creator marks it complete.
  - The server probes the file itself. File type, length and size come from the probe, never from the browser, and anything that isn't a real video in an allowed format is rejected.
- **Title:** up to 120 characters, and goes through automod.
- **Category:** chosen from the same list as streams. A Beacon from a clip inherits the clip's category.

## Processing (the one re-encode)

- Beacons are S.V.E.R's only re-encode ([AGENTS.md](../AGENTS.md)). It's cheap at 60 seconds or less.
- **One job per Beacon** in the Postgres job queue, with a processing lease, so it runs exactly once and recovers after a crash.
- **Output:** 9:16 H.264 and AAC MP4s at 1080×1920 and 720×1280, each set up for fast playback start, plus a thumbnail.
  - If the source isn't 9:16, it's framed by the creator's crop.
  - Uploads are padded rather than stretched.
- **Metadata is stripped.** Location, device and editing data are removed from every output.
- **Watermark (proposed):** the public copies carry a small "@username · sver.tv" mark, so a Beacon reposted to another app still points back to the creator. The creator can download a clean copy. Since Beacons are re-encoded anyway, the watermark costs nothing extra.
- Before publishing, every upload is checked against the Take It Down blocklist of removed images ([TAKE_IT_DOWN.md](TAKE_IT_DOWN.md)). The same check runs on clip sources.
- A Beacon moves through the statuses Draft, Processing, Ready, Published and Removed. If processing fails, the creator gets the reason and can retry.

## The feed

- **Layout:** one 9:16 video at a time, centered on desktop and full screen on phones ([DESIGN.md](DESIGN.md)).
  - Swipe or use the arrow keys to move between Beacons.
  - Videos autoplay muted until tapped, then stay unmuted for the rest of the session.
- **The right-side rail shows:**
  - the creator's crest and name, with a link to their channel
  - their faction badge
  - a like button
  - the view count
  - a **Live now** button whenever the creator is streaming, which jumps to the stream
  - a share button
  - a report option
- **Live now row:** a row of live creators who have recent Beacons sits at the top of the feed.
- **What's in the feed:** a mix of three sources:
  - creators you follow
  - creators in your faction
  - a **fair rotation** of everyone else, so every eligible creator gets turns and new creators aren't buried
- **Ordering:** newest first within each source. The rotation hands out turns the way MAGNet does: every creator gets a turn before anyone gets a second one.
- **What never affects order:** money, viewer counts, view counts or like counts. This is the same rule as MAGNet. The legacy paid "visibility boost" isn't coming back.
- **Signed-out visitors** see the fair rotation only.
- **Muting:** viewers can mute a creator from the feed.

## Counts

- **A view counts** after 3 seconds of playback that is visible on screen and actually advancing. Each account or guest session counts at most once per Beacon per day.
- Views come only from sessions viewer integrity counts ([LIVE_STREAMS.md](LIVE_STREAMS.md)). Excluded sessions never add views.
- **Likes:**
  - signed-in only
  - one per account
  - can be taken back
  - rate-limited
- **Counter safety:** every counter endpoint needs a session and is rate-limited. Legacy's open click counter isn't repeated.
- **For creators only:** completions (90% watched), follows from a Beacon, and live joins from a Beacon (Live now taps that become a counted live session). These measure whether Beacons do their job.

## Where Beacons appear

- The Beacons feed (`/beacons`) and each Beacon's own page, with preview tags so links show the video on Discord, X and Reddit.
- The **Beacons shelf** on the home page (9:16 cards) and the channel's **Beacons tab**, replacing the stub from [PROFILES.md](PROFILES.md).
- Search results, alongside channels and categories.
- **Charity streams:** a Beacon made from a charity stream ([SUPPORT.md](SUPPORT.md), Shine) carries the Shine label and the charity's donate link. It does not change its place in the feed.

## Content rules and safety

- **Platform content rule:** play, build or make. No reaction videos, gambling or just chatting.
- **Reports:** any signed-in user can report a Beacon. Reports go to the admin review queue (Module 2 reports, target type BEACON), where staff can remove it. A Beacon with an open report keeps playing unless staff hide it.
- **Take It Down:** a valid request removes the Beacon, everything it was made from and every copy within 48 hours, and purges the CDN.
- **Copyright:** claims go through the copyright process in [VODS_CLIPS.md](VODS_CLIPS.md), and repeat infringers lose posting.
- **18+ streams:** a Beacon from an 18+ stream keeps the 18+ gate and only appears in feeds for viewers who have passed it.
- **Deleting:** the creator can delete any of their Beacons, and deleting removes every rendition, the thumbnail and the clean copy, and purges the CDN.

## Not in this module

- Comments, remixing, faction-only feed tabs, Beacons counting toward faction influence, and personalized ranking. These are deferred, as in the plan.
- Push or email alerts for new Beacons, because followers already get go-live alerts.
- Music libraries or editing tools beyond cropping and trimming a clip.

## Legacy notes

| Legacy | Rebuild |
| --- | --- |
| A separate beacon service behind a proxy, which isn't in the repos | Part of the monolith. |
| Clips sent to Beacons fire-and-forget, never retried | Durable jobs with a lease. |
| Paid visibility boost (50 Valor, 1.5× multiplier) | Dropped. Discovery never takes money. |
| Ranking around view counts, seven lanes, embeddings and an "emotional score" | Three sources and a fair rotation. No count-based ranking. |
| Unauthenticated, unthrottled click and like counters | A session and rate limits on every counter. |
| Upload length and type taken from the browser | The server probes every file. |
| No way to report a Beacon | Reports go to the admin queue. |
| Media worker authenticated with the stream webhook secret | Each worker has its own credential. |
| "Beacon" also named the Shine boost and the viewer heartbeat | "Beacon" means only these videos. The heartbeat is called the viewer lease. |
| Kept: approved clips only, the Live now rail, Watch live on every video, counting only visible playback (complete at 90%), the direct-to-storage upload flow, the fairness lane, the watermark with a clean master | Carried over as above. |

## Done when

1. An eligible creator turns an approved clip into a 9:16 Beacon with a chosen crop, and uploads a video that the server probes, re-encodes and strips of metadata.
2. A file that isn't a real video, is too long or is too big is rejected. A match against the Take It Down blocklist never publishes.
3. The feed mixes followed, faction and fair-rotation Beacons, plays muted until tapped, and works with swipe and arrow keys.
4. Ordering never reads money, views or likes.
5. Views count only after 3 seconds of visible, advancing playback from counted sessions. Likes are one per account. Every counter rejects requests without a session and requests over the rate limit.
6. Live now jumps to the stream, and live joins and follows from a Beacon are recorded.
7. A reported Beacon reaches the admin queue and can be removed. Deleting removes every file and purges the CDN.
8. The home shelf, the channel Beacons tab, search and link previews all show Beacons.
