# Module 8: VODs and clips

Specified October 4, 2026, from the plan's decisions and Joe's answers that day. This module builds after CrowdSync (Module 7). Beacons (Module 9) builds on it.

Live streams are recorded so people can catch up. Streamers keep their best moments as Highlights, and viewers clip moments to share. Everything links back to the live channel.

## Principles

- **Transmux only.** Recordings, Highlights and clips reuse the stream's own video segments, the same ones SRS already produces for HLS. Nothing is re-encoded; Beacons (Module 9) are the platform's only re-encode. This keeps CPU near zero and quality identical to the live stream.
- **Private storage.** All media sits in private storage and plays through short-lived signed URLs. A file key is never guessable, and a URL is never public by itself. This fixes legacy's public bucket, where hidden and deleted media stayed reachable.
- **Every setting is enforced.** Visibility, chat replay and clip permissions are checked on the server for every request. Legacy saved several of these settings but never enforced them.
- **Deleting removes everything.** Deleting a recording or clip removes every file it owns (segments, thumbnail, playlist) and purges the CDN cache.

## Recording (VODs)

- **On by default and public.** Streamers can turn recording off, or set VOD visibility to Public, Subscribers or Private (only the streamer and their mods).
- **How it records:** as SRS writes each HLS segment, a worker copies it to private storage and appends it to the broadcast's playlist. Nothing is written to a local recording disk, so the disk can't fill up. Each broadcast (one publish session, including reconnects within the 60-second grace) is one VOD.
- **Durable jobs.** Copying segments, finishing the playlist, thumbnails and expiry are jobs in a Postgres queue with a processing lease, so each runs exactly once even after a crash. No request waits on media work.
- **Thumbnails** are taken from a keyframe of an existing segment. The streamer can pick another frame.
- **Rewind while live.** When recording is on, viewers can scrub back to the start of the current broadcast and jump back to live. When recording is off, the player offers only the short live window, and the UI never shows a longer bar than it can really play.
- **Retention follows the creator tier** in [SUPPORT.md](SUPPORT.md): Scout 24 hours, Trailblazer 48 hours, Pioneer 72 hours, Pathfinder 7 days, counted from when the broadcast ends. A tier upgrade extends every VOD that hasn't expired yet. A retention job deletes the files first, then marks the row expired (legacy's sound design, kept).
- **The VOD page** shows the title, category, faction, date, length, chapters and a "Watch live" button whenever the channel is live. View counts use Counted viewers from Module 3 viewer integrity.
- **Downloads:** streamers and their editors can download their own VODs and Highlights as an MP4. It's assembled from the stored segments with no re-encode. Viewers can't download.

## Chapters and markers

- **Automatic chapters** start at every category change and every MAGNet spotlight moment.
- **Markers:** the streamer and their mods add one with the `!marker` chat command (an optional label) or a dashboard button. Markers become chapters the streamer can rename, merge or remove.
- Chapters show on the VOD's scrub bar and in a list under the player.

## Highlights (kept permanently)

- The streamer cuts sections of a VOD and saves them as **Highlights**. Highlights never expire.
- Cuts snap to segment boundaries (about 1–2 seconds), so no re-encode is needed. A Highlight owns copies of the segments it uses, so it survives after its VOD expires.
- Total Highlight time is capped by tier (approved October 6, 2026): Scout 10 hours, Trailblazer 25, Pioneer 50, Pathfinder 100. A streamer at the cap deletes a Highlight to save another. Moving down a tier never deletes anything; tiers never go down anyway.
- Highlights have their own title, thumbnail and visibility, and appear on the channel's Videos tab ahead of VODs.

## Clips

- **Who can clip:** by default, any signed-in viewer. Streamers can change this to Followers, Subscribers, Mods, or Off. Signed-out visitors can't clip. Accounts with a chat ban or timeout on the channel can't clip it.
- **Length:** 5–60 seconds; the default is the last 30 seconds. The clip editor lets the viewer drag the start and end within the last 2 minutes of the live stream, or anywhere in a VOD or Highlight.
- **Clipping still works with recording off.** The worker keeps a rolling 2-minute window of segments for clipping only, and deletes them as they age out.
- **Rate limits:** per viewer per hour and per channel. Exact numbers live in the private tuning config.
- **Processing:** a background job joins the clip's segments into one MP4 with no re-encode (the 1-second keyframe interval keeps cuts clean) and takes a thumbnail. A single file is what link previews and embeds need.
- **Titles** are up to 100 characters and go through the same automod as chat.
- **Optional approval:** streamers can require approval before clips go public. Clips by the streamer and their mods skip the queue. Every approval and rejection is logged.
- **Clips never expire.** They're short, and they're how channels get found. The streamer can delete any clip of their channel, and the clipper can delete their own.
- **Each clip keeps** its channel, broadcast, category, faction, clipper, and the chat messages from its time window (copied at creation, since chat bodies expire after 7 days).
- **Sharing:** every clip has its own page, an embed player, oEmbed, and preview tags so a link posted to Discord, X or Reddit shows the video, image and title. The player shows the channel name and a link to the live channel. Clips aren't watermarked, because that would need a re-encode.
- **Beacons:** a clip can be turned into a Beacon in Module 9, which makes the 9:16 version. Only clips the streamer approves can become Beacons.

## Chat replay

- VODs replay chat alongside the video, synced to the broadcast time, while the original messages still exist (7 days, matching the longest retention).
- Highlights and clips carry their own copy of the chat from their time window, so their replay never runs out.
- Deleted and moderated messages never appear in replay. Streamers can turn chat replay off.

## MAGNet auto-clips

- When MAGNet spotlights a channel, it suggests a clip of the moment that triggered the spotlight. Length depends on content type: about 35 seconds for gaming, 60 for creative and music, 45 otherwise.
- Suggestions go to the streamer's approval queue and are never published automatically.
- They use MAGNet's room signals only, never money or viewer count ([MAGNET.md](MAGNET.md)).

## Safety and legal

- **Take It Down** ([TAKE_IT_DOWN.md](TAKE_IT_DOWN.md)) covers VODs, Highlights and clips. A valid request removes the media and every clip and Beacon made from it, and purges the CDN, within 48 hours.
- **Reports** extend Module 2 reports with VOD, HIGHLIGHT and CLIP targets. A reported item is kept, hidden from the public, past its expiry until the case closes; then normal retention resumes.
- **Copyright:** this module adds a copyright removal form, a counter-notice process, and a repeat-infringer policy (approved October 6, 2026): each notice staff uphold is a strike for 12 months, and a counter-notice that restores the material removes it. The third active strike puts a SEVERE copyright strike on the account, restricting the channel indefinitely through account standing, where it can be appealed. SVER LLC's designated copyright agent is registered with the U.S. Copyright Office (DMCA-1081854, filed October 4, 2026). A designation lapses after 3 years, so it must be renewed by October 4, 2029; the admin tools show a reminder 60 days before. The agent's name, mailing address, phone and email must match the filing and appear on `/dmca`.
- Recordings of streams marked 18+ keep the 18+ gate.

## Channel page

The Videos tab placeholder from [PROFILES.md](PROFILES.md) becomes a real tab with Highlights, Past broadcasts and Clips (sorted by newest or most viewed). The home page's "Latest clips" row ([DESIGN.md](DESIGN.md)) shows public clips from live and recent channels.

## Not in this module

- Uploading pre-recorded videos: after launch. When it comes, uploads must be probed and processed like recordings, never served as uploaded.
- Linking VODs from other platforms.
- Muting copyrighted audio automatically or fingerprinting.
- Viewer downloads.
- Vertical (9:16) output: that's Beacons, Module 9.

## Legacy notes

| Legacy | Rebuild |
| --- | --- |
| Single-file FLV recording, re-encoded to one 720p MP4 in a request that could take hours | Segments copied as they're written. No re-encode, no long request. |
| Raw recordings never cleaned up, so the disk grew without limit | Nothing kept on local disk. The clip window deletes as it rolls. |
| Public bucket with guessable keys; the clean clip master could be found | Private storage and signed URLs only. |
| VOD visibility and chat-replay settings saved but never enforced | Enforced on the server for every request. |
| Deleting a clip left source, portrait and thumbnail files behind | Deletion removes every file and purges the CDN. |
| "30-minute rewind" bar over about 2 minutes of real video | Rewind is the real recording, or no longer than the live window. |
| Manual uploads labeled permanent but hard-coded to 48 hours; uploads served unchecked | Uploads deferred until they can be processed properly. |
| Recording webhook secret in the URL query string | SRS webhooks are authenticated without putting secrets in URLs. |
| `autoClipEnabled` stub; MAGNet clips saved to local disk with no retention | MAGNet suggests clips into the approval queue, stored like any clip. |
| Kept: tier retention ladder, retention worker that deletes first, clip permission levels, approval queue with audit log, chat window stored on clips, chapters from hype moments plus markers, crash-recovery processing lease | Carried over as above. |

## Done when

1. A broadcast records automatically, plays back as a VOD within a minute of ending, and expires on time for each tier, with its files and CDN cache gone.
2. Viewers can rewind a live broadcast to its start when recording is on.
3. Visibility (Public, Subscribers, Private) is enforced on every playlist and segment URL. A signed URL stops working after it expires.
4. Chapters appear from category changes, MAGNet moments and `!marker`, and the streamer can edit them.
5. A streamer saves a Highlight that still plays after its VOD expires and counts against the tier cap.
6. Viewers clip live (including with recording off) and from VODs, within permissions and rate limits. Approval works, and deleting removes every file. A clip link posted to Discord shows a playable preview.
7. Chat replay works on VODs, Highlights and clips, without deleted or moderated messages.
8. Take It Down, reports and copyright removal remove or hold media as described.
9. No media job re-encodes video, and none blocks a request.

## Implementation and activation

Module 8 is in development. The implementation uses a separate private object store, Postgres jobs, short-lived playback tickets and FFmpeg codec copy. Highlight limits are the approved 10/25/50/100 hours. Production activation and the complete acceptance run remain open.

`VOD_STORAGE` defaults to `disabled`. For production, configure `VOD_STORAGE=s3`, `VOD_SEGMENT_BASE` (the trusted private SRS HTTP origin), `VOD_S3_ENDPOINT`, `VOD_S3_BUCKET`, `VOD_S3_REGION`, `VOD_S3_ACCESS_KEY_ID`, `VOD_S3_SECRET_ACCESS_KEY`, and `VOD_TUNING_FILE` outside the repository. The bucket must have public access disabled and must differ from public profile storage. Its credentials need object read, write, delete and multipart permissions. Configure automatic abortion of incomplete multipart uploads after one day; that covers a process dying between S3 initiation and saving the upload ID.

`compose.videos.yaml` is an optional override added to the existing deployment's Compose files. `SVER_VOD_ENV` points to the external recording environment file, and `SVER_VOD_TUNING_FILE` to a private tuning file mounted read-only in the API. `infra/vods.tuning.example.json` contains development examples, not production anti-abuse settings. Preserve every existing deployment override. The API image already includes FFmpeg. Enable the SRS `on_hls` hook only with the authenticated internal proxy and storage configured.

Before activating recording, run `sver-admin videos probe` with the recording environment. It requires S3 storage and no database connection. It uses fresh synthetic objects to check segment read/write, multipart assembly across a part boundary, range reads, anonymous S3 rejection, multipart abort and deletion. Cleanup is attempted on failure too, and a cleanup failure returns the probe prefix for recovery. Confirm separately that the bucket has neither a public development URL nor a public custom domain; denial at the authenticated S3 endpoint does not prove those settings. This probe does not establish CDN purge or real playback acceptance.

The media worker copies segments through a bounded Postgres spool, streams MP4 output into multipart storage, and checks a processing lease before publishing database state. Retries are idempotent. Copy jobs pin their source segments until the saved media and thumbnail finish. Deletion checkpoints bounded batches and retains short-lived object ownership records to collect delayed writes; held media cannot block cleanup of unrelated files. A queued segment upload keeps its ownership until it completes, even after leaving the clipping window. Live recording playlists publish only the uninterrupted prefix of available segments, and cuts wait for their requested final segment. Playback, byte ranges, thumbnails and downloads are authorized at each request and use `no-store`; neither the bucket nor its keys are public delivery endpoints. Changing visibility or placing a hold revokes outstanding tickets.

Run `./scripts/dev.ps1 test` with local Postgres plus FFmpeg/ffprobe installed. `tests/streams/videos.rs` exercises real encoded segments, playback permissions, retention, independent Highlights, clips, approval, short-clip views, replay redaction, library pagination, deletion retries, reports and copyright workflows. The opt-in `real_srs_ingest` test also checks real SRS recording callbacks, reconnect playback and the timeline of MP4 cuts across reconnects; run it using the prerequisites in [LIVE_STREAMS.md](LIVE_STREAMS.md).

Remaining release gates include private production storage and purge verification, real external clip-preview acceptance, provider failure/scale acceptance and production playback. The repeat-infringer policy is implemented (`copyright_cases.removed_at`, migration `0042_videos`). Production storage is a separate private R2 bucket (`sver-vods`) using the existing storage credentials; the profile media bucket stays separate. Local fault tests cover upload/delete outages, malformed storage responses, delayed segments and cleanup with held media. Module 8 is not closed by local tests alone.
