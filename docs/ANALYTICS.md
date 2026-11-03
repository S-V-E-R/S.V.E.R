# Viewer analytics

Added November 3, 2026 at Joe's direction (`analytics.rs`, migration 0155). The same report exists at three levels:

- **Creators:** Creator Studio → Analytics shows a streamer where their viewers come from, how long they stay, whether discovery brought them, and how often a live card that was seen turned into a visit.
- **Staff:** Raven's Eye shows the same report for the whole platform, plus the top 20 channels by watch hours with each one's share from discovery (`GET /api/admin/analytics`, staff only).
- **Public:** Open data shows six weekly platform figures from it (below).

## Rules

- **Totals only.** A streamer never sees who watched; the smallest thing shown is a count.
- **Real viewers only.** A visit counts once viewer integrity (Module 3) rated its session Counted or Trusted. Pending and Excluded sessions are recorded but never shown.
- **Never used for anything else.** Analytics don't feed discovery, MAGNet, faction influence, creator tiers or money. A source the browser claims can't earn anything, so it can't be gamed for gain.
- **Kept 90 days**, then deleted by the jobs loop. Deleting an account deletes its visits as a viewer.

## Visits

A visit is a run of playback heartbeats (sent every 10 seconds while media advances) by one viewer on one broadcast. It ends when beats stop for 30 seconds, the same lapse as a playback lease, so leaving and coming back is a new visit. Each beat adds the time since the last one, at most 15 seconds, so a visit's length is time actually played. One row per visit in `watch_visits`: broadcast, channel, viewer key (the lease's key, a digest for guests), source, referring site, whether the viewer followed the channel when it started, start, last beat, seconds and whether it counted.

## Sources

The first beat of a visit sets its source; later beats never change it.

| Source | Shown as | How it's known |
| --- | --- | --- |
| `home` | Home page | A click on the homepage (Live Now, rotation, shelves) |
| `magnet_channel` | Watched inside MAGNet | The server: the player is on a MAGNet channel that features this broadcast now |
| `magnet` | MAGNet → your channel | A click from a MAGNet page to the channel |
| `search`, `browse` | Search, Browse | A click on those pages |
| `following` | Following list | The sidebar's "Following · live" or the Following page |
| `alert` | Go-live alerts | Go-live push and email links carry `?src=alert`; the notifications page |
| `suggestion` | Suggested | Up next and the stream-end handoff on another watch page, and the signed-out sidebar picks |
| `raid` | Raids | The server: the beat carries a raid link |
| `host` | Hosts | The stream played inside an offline channel that is hosting it |
| `beacon`, `clip`, `video` | Beacons, Clips, Past broadcasts | A click from those pages |
| `faction` | Faction pages | A click from a faction page or the war map |
| `channel` | Your channel page | A click within the channel's own pages |
| `external` | Other websites | A full page load whose referrer is another site; only its host name is kept (`discord.com`) |
| `direct` | Direct or typed link | No referrer and no click |
| `other` | Other | Anything else |

How the browser knows: the site shell remembers, for one minute, the page (or the nearest section marked `data-src`) of the last internal link clicked and where it led (`lib/view-source.ts`). The player reads it once when it starts, if it led to this channel; otherwise it uses the referrer.

## Impressions

A live card counts as seen when at least half of it is on screen for a second, once per card per page view (`SeenCards`). Pages send them in batches to `POST /api/discovery/seen` (at most 48 cards, 60 requests a minute per address); only broadcasts live at that moment count, and a streamer's own card never counts for them. Stored per broadcast, surface and hour in `discovery_impressions`. Surfaces: the homepage (including the rotation), Browse, MAGNet pages and Up next on watch pages. Search, the Following page and faction pages don't show live cards with a broadcast yet, so they show visits without a click-through rate.

## What the streamer sees

`GET /api/me/analytics?days=7|30|90`, shown in Creator Studio → Analytics:

- **Overview:** watch hours, hours live, unique viewers, visits, average and peak concurrent viewers (integrity snapshots), average and median visit, new and returning viewers (returning means a counted visit before the range, within the 90 days kept), new follows, chatters, the share of watch time from followers and the share of visits signed in.
- **Where viewers come from:** per source, watch hours and their share, visits, viewers, average visit, the share that stayed 5 minutes or more, and follows (a signed-in viewer who wasn't a follower and followed during the visit or within 10 minutes).
- **Did discovery bring viewers?** New arrivals per live hour while on top of Live Now (turns someone saw, docs/MAGNET.md) and while featured on MAGNet, against the rest of the channel's live time, with the ratio. Also how many viewers watched inside MAGNet channels and how many of them then opened the channel.
- **Seen and clicked:** per surface, times the card was seen, visits from that surface and the click-through rate.
- **How long viewers stay:** visits under 1 minute, 1–5, 5–15, 15–60 and over an hour.
- **Other websites:** the top 10 referring sites.
- **By day:** watch hours, the part from discovery (Home, MAGNet, Browse, Search, Suggested, Faction pages) and viewers.
- **Broadcasts:** the last 30 in the range with category, length, watch hours, viewers, peak, average visit, new follows and the top source.

## The platform view

`report(None, from)` is the creator report with every channel included: totals across channels (new viewers are new to S.V.E.R within the 90 days kept), per-minute concurrent viewers summed across live channels, sources, click-through per surface, visit lengths, referring sites and days. The discovery comparison uses each channel's own Live Now and MAGNet windows against that channel's other live time, summed. Instead of one channel's broadcasts it lists the top 20 channels.

## Open data

Weekly, on `/open-data` and in `weekly.csv` (computed with the other figures, `open_data.rs`): hours watched, the percent of watch time from discovery, the average visit in minutes, the percent of homepage card views that led to a visit, the percent of MAGNet viewers who then opened the channel, and arrivals per hour while featured (on top of Live Now or on MAGNet) as a multiple of the rate the rest of the time. Like creators' hours, they're hidden until 10 channels had viewers that week, so no single channel's numbers show. Live Now turns are kept 90 days for these figures; Studio → MAGNet still shows 30.

## Known ceilings

- One extra database write per heartbeat. Past a few thousand concurrent viewers, batch visit updates in memory and flush once a minute (`ponytail:` note in `analytics.rs`).
- Live Now turns and MAGNet features that overlap are both counted as exposed time.
- A source is the browser's claim, except MAGNet and raids. It's shown to the streamer only and affects nothing else.
