# Multistream: restreaming and Linked chat

Decided by Joe on October 3, 2026. Builds as its own step right after Module 3 (Live streams) closes and before Module 4 (Factions). Merged chat was a goal of the first S.V.E.R and was parked for Phase 3; it moved up, together with restreaming, so streaming on S.V.E.R never costs a multistreamer extra work.


## Why this exists

Feedback from an original S.V.E.R streamer (October 3, 2026): they stopped streaming on S.V.E.R because multistreaming with Twitch was a hassle (two OBS outputs, double upload, two chats). This step removes that hassle so streaming on S.V.E.R costs nothing extra, while S.V.E.R stays the home: S.V.E.R is always the origin of the stream, S.V.E.R chat is where everything comes together, and S.V.E.R-only features (factions, MAGNet, Valor, CrowdSync) live only here.

It has two parts, built together right after Live streams closes: **restreaming** and **Linked chat**.

## Restreaming

- The streamer sends **one** stream from OBS to S.V.E.R, as usual. In Creator Studio they add up to **3 destinations** (Twitch, YouTube, Kick, or any custom RTMP address) with each platform's stream key, and switch each on or off.
- When they go live on S.V.E.R, S.V.E.R forwards the stream to each enabled destination. **Nothing is re-encoded**: the media server relays the same video, so the other platforms get the quality the streamer sent.
- Only verified accounts with 2FA can restream, like streaming itself. Destination keys are encrypted at rest, shown only after a fresh 2FA check, and deleted with the destination or the account.
- If a destination rejects the stream or drops, S.V.E.R retries with backoff and shows the status per destination in Creator Studio ("Twitch: live", "YouTube: reconnecting", "Kick: key rejected"). The S.V.E.R stream is never affected.
- **Capacity guardrail:** a total restream budget, set from the bandwidth load test ([LOAD_TEST.md](LOAD_TEST.md), the same test that sets the WebRTC threshold). Viewers are always served first; if restreaming nears the budget, new restreams wait with a clear notice. Restreaming uses outgoing bandwidth only (one stream's bitrate per destination) and almost no CPU.
- S.V.E.R never adds anything to the video sent to other platforms (no S.V.E.R overlays or "watch on S.V.E.R" messages), so streamers stay within those platforms' rules.
- **Studio insight:** after a stream, Creator Studio shows how S.V.E.R chat and viewers compared with the linked platforms' chat activity, so streamers can see their S.V.E.R community grow.
- **Done when:** a streamer sends one OBS stream and it appears live on S.V.E.R and on two other platforms without re-encoding; a rejected key and a dropped destination show the right status and recover; keys are never exposed in responses or logs; the budget holds and never degrades S.V.E.R viewers.

### Restreaming as built (October 9, 2026)

- Creator Studio → Restream (`/studio/restream`); API `GET|POST /api/me/restream`, `PATCH|DELETE /api/me/restream/{id}` (`restream.rs`, migration 0067). Adding or changing a destination needs a verified account with 2FA, like streaming.
- Twitch and YouTube use their default ingest servers unless the streamer gives one; Kick and custom destinations need the server from the platform (Kick's differs per account). Servers must be `rtmp://` or `rtmps://`, with no credentials or query in the address, and in production must resolve only to public addresses.
- Keys are sealed with the site key and are **write-only**: no response ever returns them, and the streamer replaces a key instead of viewing it (simpler and stricter than "shown after 2FA").
- The relay supervisor runs every 3 seconds inside the API when `RESTREAM_SOURCE` is set (production: `rtmp://127.0.0.1:1936/rebuild`, the media server's local RTMP app). For each enabled destination of a LIVE broadcast it runs `ffmpeg -c copy` from the source to `{server}/{key}`, with ffmpeg's output discarded because it can contain the key. Statuses: Off air, Connecting, Live (up 15 s), Reconnecting (backoff 2, 4… up to 60 s), Key rejected (3 fast failures in a row; retried every 5 minutes, cleared when the key or server is saved) and Waiting for capacity.
- Capacity: `RESTREAM_MAX` relays at once (default 100; at about 8 Mbps each that is under a tenth of the guaranteed 10 Gbps). Over it, destinations wait with a notice and the S.V.E.R stream is unaffected.
- An API restart (a deploy) drops relays for a few seconds; the platforms keep the stream session through short reconnects.
- Tests: `tests/streams/restream.rs` (validation, the 3-destination limit, keys never returned and sealed at rest, and the supervisor going Connecting → Reconnecting → Key rejected against a closed port); unit test `restream::tests`.
- Not yet: the post-stream comparison with linked platforms' chat (it needs Linked chat).

## Linked chat

### What it does

A streamer links their accounts on other streaming platforms in Creator Studio. While they're live on S.V.E.R, chat messages from those platforms appear in their S.V.E.R chat, marked with the platform's icon. The streamer can reply to an outside message from S.V.E.R, and the reply is posted back on that platform from the streamer's own account. Viewers on S.V.E.R see outside messages but can't reply to the other platforms.

Launch platforms: Twitch, YouTube and Kick, each through its official API. More can be added later.

### Linking accounts

- Creator Studio → Linked chat: connect each platform with its official sign-in (OAuth), asking only for the permissions to read the streamer's channel chat and post as the streamer.
- Tokens are encrypted at rest, refreshed automatically, and deleted when the streamer unlinks or deletes their account.
- Only verified accounts with 2FA (the same rule as streaming) can link.
- Linking is per platform and can be turned on or off without unlinking.

### How it runs

- When the streamer's S.V.E.R broadcast goes live, the API connects to each linked, enabled platform's chat for that streamer. It disconnects when the broadcast ends (after the 60-second reconnect window).
- A dropped connection reconnects with backoff. Creator Studio and the streamer's own chat view show a notice such as "YouTube chat disconnected, reconnecting" until it recovers.
- Each platform's rate limits and API quotas are respected. If a platform's quota runs out, linked chat for that platform pauses with a notice instead of failing silently. (YouTube's chat API has a daily quota; measure it during the build and request an increase if needed.)

### Outside messages on S.V.E.R

- **Every outside message carries its platform badge**, placed before the sender's name where S.V.E.R messages show the faction crest: the platform's official icon (Twitch, YouTube or Kick), used under each platform's brand guidelines, with an accessible label such as "From Twitch". The badge is always shown and can't be hidden or faked, so nobody can mistake an outside message for a S.V.E.R account or the reverse.
- If the platform reports the sender's role there (broadcaster, moderator or subscriber), a small text marker shows it next to the badge, for example "Twitch mod". These roles give no permissions on S.V.E.R.
- The sender's display name from that platform, with no S.V.E.R profile link. Hovering or tapping the name shows the platform and a link to their channel on it.
- Plain text only, up to the S.V.E.R chat length; outside emotes appear as their text names.
- Kept like S.V.E.R chat messages (latest 100 on join, 7-day expiry). Only the platform, the outside user's ID and display name, and the text are stored.
- **They never count toward anything:** not viewer counts, Valor, faction influence, MAGNet chat bursts, viewer integrity, tiers or Plays votes. Outside users aren't S.V.E.R accounts.
- **MAGNet:** when the channel is featured, outside messages stay in the channel's chat and don't cross into Hype chat.

### Moderation

- The channel's banned words and link blocking apply to how outside messages are shown on S.V.E.R.
- S.V.E.R channel moderators can hide an outside message on S.V.E.R and mute an outside user on S.V.E.R for the rest of the broadcast or permanently.
- These actions only affect S.V.E.R. Moderation on the other platform stays with that platform's tools. A message deleted on its platform is removed from S.V.E.R when the platform reports the deletion.
- Outside messages can be reported to S.V.E.R staff like any chat message.

### Replies

- Only the streamer can reply out, using a "Reply on Twitch / YouTube / Kick" action on an outside message, or by choosing a platform when sending. The reply is posted from the streamer's linked account and also shows in S.V.E.R chat.
- Moderators and viewers can't post to other platforms.
- If posting fails (permission revoked, rate limit), the streamer sees why.

### Rules on other platforms

Some platforms don't allow showing other platforms' chat inside the stream video itself. Linked chat lives on the S.V.E.R site, not in the video. Creator Studio reminds streamers who simulcast not to put the merged chat on screen in the video they send elsewhere. Later chat overlays (Phase 3) will offer a S.V.E.R-only mode for this.

### Privacy and copy

- The Privacy Policy lists the platforms that linked chat connects to and what is stored.
- Platform names appear only as integrations (icons, "Reply on …", linking screens), never as comparisons.

### Storage and API outline

Tables: linked platform accounts (encrypted tokens, scopes, enabled flag), outside chat users (platform, ID, display name, S.V.E.R mute state), and outside messages stored with S.V.E.R chat using the chat origin field from the MAGNet's spec (origin: platform). Platform connectors run as workers inside the API process, one per live linked channel per platform, with bounded queues. No Redis.

### Linked chat as built (October 9, 2026): Twitch

- **Linking:** Creator Studio → Linked chat (`/studio/linked-chat`) starts the platform's OAuth with the intent `chat` (verified accounts with 2FA). Twitch asks for `user:read:chat user:write:chat user:bot channel:bot` only. Tokens are sealed in `linked_chat_accounts` (migration 0068), refreshed when a reply needs them, and revoked at Twitch and deleted on Unlink. One Twitch account links to one S.V.E.R channel.
- **Receiving:** Twitch EventSub **webhooks** (`channel.chat.message`, `channel.chat.message_delete`) to `POST /api/integrations/twitch/eventsub`, so no connection is held open and an API restart loses nothing (Twitch retries a 5xx). Each delivery is checked with its HMAC-SHA256 signature (a secret derived from the site key) and refused if older than 10 minutes. The supervisor in the 5-second stream loop subscribes when the broadcast goes LIVE and unsubscribes when it ends (not during RECONNECTING); a refused subscription shows "Needs linking again", a failed one retries every 30 seconds with "Twitch chat disconnected, reconnecting". Shared-chat copies of other channels' messages are ignored.
- **Storage:** outside messages are in their own table, `outside_chat_messages`, sharing `chat_messages`' sequence so they interleave in order. Nothing that counts S.V.E.R chat (viewers, Valor, influence, MAGNet, integrity, orders, tiers) reads that table, which is how they "provably don't count"; the test also checks chat rows and Engagement Valor are unchanged. Only the platform, the sender's ID, login, display name and role there, and the text are kept, for 7 days. History merges the newest 100 of both.
- **Showing:** a platform badge before the name ("Twitch", in the platform's color, labelled for screen readers), a role marker such as "Twitch mod", the name linking to their channel on the platform, plain text. The OBS overlay leaves outside messages out.
- **Moderation on S.V.E.R:** banned words and the link rule decide whether an outside message is shown; owners and moderators can hide a message and mute a sender for this stream or permanently (`outside_chat_mutes`, logged in the channel moderation log), and the channel's mutes are listed in Studio. A delete on Twitch hides the message here.
- **Replies:** the owner's "Reply on Twitch" posts with Helix Send Chat Message as their own account (`POST /api/me/linked-chat/reply`, 20 per 30 seconds); Twitch's refusal reason is shown. The reply comes back through EventSub like any message.
- **Tests:** `tests/streams/linked_chat.rs` (signature, challenge, dedupe, counts unchanged, banned words, platform delete, hide, mute and unmute, tokens never returned, unlink).
- **Not yet:** YouTube (needs Google's verification of the `youtube.force-ssl` scope; its chat API is polled against a daily quota) and Kick (needs a Kick developer app); both show "Coming soon" in Studio until Joe registers them. Reporting outside messages to staff, and the post-stream comparison in Studio.
- **Twitch app settings (for activation):** the existing Twitch sign-in app needs no new redirect; the EventSub callback is `https://sver.tv/api/integrations/twitch/eventsub`.

### Done when (Linked chat)

A streamer links Twitch, YouTube and Kick; while they're live, messages from each appear in S.V.E.R chat within a few seconds, each with its platform badge; the streamer replies to each platform from S.V.E.R and the reply appears there; moderators hide and mute outside users on S.V.E.R; disconnects and quota limits show a notice and recover; outside messages provably don't affect viewer counts, Valor, influence, MAGNet or integrity; unlinking deletes the tokens.
