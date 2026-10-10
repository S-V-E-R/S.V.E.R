# Native alerts and overlays

Phase 3, after the developer platform ([DEVELOPER_PLATFORM.md](DEVELOPER_PLATFORM.md) "Compatibility": StreamElements and Streamlabs alert boxes don't support S.V.E.R). Written October 10, 2026 with **Proposed** defaults, at Joe's direction to keep building until the platform is complete. Joe can change any of them.

## What a streamer gets

- **Creator Studio → Alerts & overlays.** One private link per channel, made and revoked in Studio. Pasted into OBS (or any broadcast app) as a browser source. Like the board overlay's link, it's the only credential, it reaches the channel's alerts and nothing else, and making a new one stops the old one at once.
- **Three widgets on the same link:**
  - **Alert box** (`/overlay/alerts/{token}`): one alert at a time, in order, for follows, subscriptions, tributes, Skills and incoming raids.
  - **Goal bar** (`?widget=goal`): followers or active subscribers toward a target, with a label.
  - **Recent events** (`?widget=events`): the last 5 alerts, newest first.
- **Per alert type:** on or off, a message template (`{name}`, plus `{tier}` and `{months}` for subs, `{valor}` and `{message}` for tributes, `{skill}` for Skills), how long it shows (3–15 seconds). Tributes also have a minimum (10 Valor or more) and whether the attached message shows.
- **Sound:** chime, horn or none, with a volume, made in the browser (Web Audio), so there are no sound files to host or review.
- **Test buttons** play a sample of each alert on open overlays only. Nothing is recorded, nothing reaches viewers or webhooks.
- Saving in Studio updates open overlays straight away.

## Rules

- Alerts come from the same records as the live events API (Section 2), so anything the events API leaves out also stays off the overlay: people the streamer blocked, chat refused by moderation, Valor that didn't move.
- Names are S.V.E.R usernames. A tribute's message is the chat message that carried it, which already passed the channel's chat rules.
- Background is transparent; motion is fades and short slides only (no glows), per DESIGN.md.
- The overlay page loads nothing from other sites (uploaded alert images and sounds come from S.V.E.R's media host).

## Uploaded images and sounds (part 2)

- Each alert type can have its own **image** (PNG, JPEG or WebP up to 2 MB, resized to fit 512 pixels and re-encoded as WebP) and its own **sound** (MP3, OGG or WAV up to 1 MB; the first 10 seconds are re-encoded by the server as mono 16-bit WAV, which OBS's browser source plays without proprietary codecs). Nothing uploaded is served as is.
- An alert with its own sound plays it at the overlay volume; otherwise the chime or horn plays.
- Review: images pass the same checks as every upload (known-abuse fingerprints at upload, Take It Down removal). They appear only in the streamer's own broadcast, so reports about them go through the stream. Sounds aren't images and aren't fingerprinted; staff can remove one through the media records like any upload.
- Images and sounds change only through their upload and remove buttons; saving the other settings keeps them. Open overlays update at once.
- The overlay loads those files from S.V.E.R's media host and nothing else.

## Text-to-speech (part 2b)

- Off by default. When on, a tribute alert reads its message aloud as "{name} says: {message}" (up to 200 characters), after the alert's sound, at the overlay volume.
- Only tribute messages are read: they already passed the channel's banned words, link rules and moderation as chat messages. Names come from usernames, so they're safe to read.
- The browser source speaks with the streaming PC's own installed voices (the Web Speech API). Nothing is sent to a speech service and there is no cost; if the PC has no voices, nothing is read.

## Not in this part

The StreamElements/Streamlabs custom-widget adapter comes next.

## Done when

1. A follow, a subscription, a tribute over the minimum, a Skill and an incoming raid each play their alert, with the template filled in, once, in order.
2. A disabled type, a tribute under the minimum and a blocked person's event never show.
3. The goal bar moves when someone follows or subscribes; the recent list shows the last 5.
4. A revoked or replaced link stops working at once; test alerts reach only open overlays.

## As built (October 10, 2026; `overlays.rs`, migration 0082)

See the module comment in `apps/api/crates/sver/src/overlays.rs`. Covered by `tests/streams/overlays.rs`.
