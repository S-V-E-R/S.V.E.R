# Notifications

Specified October 4, 2026. One notification system serves every module. Its foundation is built in Live streams (Module 3) together with go-live alerts ([LIVE_STREAMS.md](LIVE_STREAMS.md)), which already need the notification table, the top-bar bell, browser push and email. Each later module adds its own types to the catalog below when it's built. This also schedules the "notification work" [PROFILES.md](PROFILES.md) left unscheduled.

The goal is fewer, better notifications. Each one should be something the person would want to know, and they control how it reaches them.

## Where notifications appear

- **The bell** in the top bar ([DESIGN.md](DESIGN.md)) shows an unread count (up to "99+") and opens a panel with the latest 20.
- **The notifications page** (`/notifications`) lists everything from the last 30 days, with filters (All, Channels, Community, Money, Account) and "Mark all as read".
- **Live updates:** while a page is open, new notifications arrive over the live connection the site already keeps for chat and live state. There's no polling and no separate socket.
- **Browser push** uses standard Web Push with server-held VAPID keys and no outside service. Push messages are encrypted to the browser by the Web Push standard itself. On iPhone and iPad, push works only once the site is installed to the home screen (the installable web app in [COMMUNITY.md](COMMUNITY.md)), and the settings page says so.
- **Email** goes through Resend, using Login's encrypted mail-job queue with idempotency keys.

## Settings (`/settings/notifications`)

- **By type:** each type in the catalog has its own in-site, push and email switches, with the defaults shown in the catalog.
- **Required notices:** some notices can't be turned off: account security, money, legal requests, and account standing. They are marked "Required" with the reason.
- **By channel:** the bell next to Follow turns a channel's go-live alerts off without unfollowing (already in Live streams). The same page lists every muted channel so they're easy to undo.
- **Quiet hours:** a time range (in the person's time zone) when push and email wait, then arrive as one summary afterward. In-site notifications still collect.
  - Off by default for adults.
  - On from 10 PM to 8 AM by default for under-18 accounts.
  - Required security notices ignore quiet hours.
- **Email digest:** instead of separate emails, a daily summary at a time the person picks. Required notices always send right away.
- **Message previews:** whether push and email show the text of a DM. Off by default, so a lock screen shows "New message from *name*" only.

## Rules every notification follows

- **No notification from someone you've blocked**, or from a channel or guild you've muted.
- **Grouping:** related notifications merge into one ("Ana and 11 others followed you", "6 clips are waiting for approval") instead of stacking up. A group updates in place and becomes unread again when something new joins it.
- **Limits:** a cap on push messages per person per hour and per day. Above the cap, they wait for the next summary. Exact caps live in the private tuning config.
  - Go-live alerts keep their existing limit: one per channel every 6 hours.
- **Email is only for things worth an inbox:**
  - Optional emails have a one-click unsubscribe (including the header mail apps use) that turns off that type.
  - Optional emails end with S.V.E.R's postal address, as CAN-SPAM requires of commercial email: SVER LLC's registered DMCA agent address from `/dmca`, unless the server sets `MAIL_POSTAL_ADDRESS`. Change both together if the address moves.
  - Unverified addresses never get mail.
  - Product news is a separate opt-in that is off by default, and under-18 accounts never get it.
- **No sensitive content in push or email:** no report details, strike reasons, payment amounts beyond the person's own, or other people's private information. Those link to the page that shows them after sign-in.
- **Failed push subscriptions are removed.** Bounced or complaining email addresses stop receiving optional email until re-verified.
- **Retention:** in-site notifications are deleted after 30 days. Delivery records (sent, failed, bounced) are kept 30 days for troubleshooting, then deleted.
- **Accessibility:** the unread count is announced to screen readers, and every notification is readable without its icon.

## Catalog

Defaults: **S** in-site, **P** push, **E** email. **Required** means always on for the channels listed.

| Module | Notification | Who gets it | Default |
| --- | --- | --- | --- |
| Login | New sign-in from a new device or place; password, email or 2FA changed; account deletion requested | The account owner | S + E, Required |
| Profiles | New follower (grouped) | Channel owner | S |
| Profiles | New wall post, or a reply to your post | Wall owner, post author | S |
| Profiles | Report outcome | Reporter | S; E opt-in (the existing generic digest) |
| Profiles | Strike or appeal decision | The account | S + E, Required (the existing generic email) |
| Live streams | A followed channel went live | Followers, minus muted channels | S + P + E |
| Live streams | Incoming raid, or someone is hosting you | Streamer | S, plus a toast in Studio while live |
| Take It Down | Request received, status changes, outcome | Requester (by email; may have no account) | E, Required |
| Take It Down | A request about your content | Uploader | S + E, Required |
| Multistream | A restream destination failed or disconnected | Streamer | S + P, plus a Studio toast |
| Factions | Weekly checkpoint results; season won or lost | Members | S |
| Factions | War council vote is open; switching window is open | Members | S |
| Guilds | New application | Guild leaders and officers | S + P |
| Guilds | Your application was accepted or declined; you were invited | Applicant or invitee | S + P |
| Direct messages | New message (grouped per conversation) | Recipient | S + P |
| MAGNet | You're up next, or you're in the spotlight now | Streamer | S + P, plus a Studio toast |
| Support | New subscriber, gift sub, tribute (grouped) | Streamer | S |
| Support | Receipt; subscription renewing, renewed, failed or ended | Buyer | E, Required (receipts); S for the rest |
| Support | Guardian approval needed for an under-18 purchase or payout account | Guardian (by email) | E, Required |
| Support | Payday sent, Early Pay sent, payout problem, chargeback | Streamer | S + E, Required |
| Support | Creator tier promotion | Streamer | S + E |
| Support | Your gifted month ends in 3 days; keep the subscription ([CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md)) | Gift recipient | S |
| CrowdSync | A prediction you joined was resolved | Participant | S |
| VODs and clips | Clips waiting for approval (grouped) | Streamer and mods | S |
| VODs and clips | Your clip was approved, published or removed | Clipper | S |
| VODs and clips | Highlight storage nearly full | Streamer | S |
| VODs and clips | Copyright claim or counter-notice about your content | Uploader | S + E, Required |
| Beacons | Beacon published, processing failed, or removed | Creator | S |

Not notified: chat mentions (they're highlighted in chat only, per Live streams), new Beacons from followed creators (go-live alerts cover it), and likes or views.

## Staff alerts

Staff alerts use the same system but are a separate group that only staff accounts see:

- new Take It Down requests (hourly repeats after 24 hours, as in [TAKE_IT_DOWN.md](TAKE_IT_DOWN.md))
- new reports and appeals
- viewbot integrity cases
- Plays health alerts ([PLAYS.md](PLAYS.md))
- the DMCA agent renewal reminder 60 days before it's due ([VODS_CLIPS.md](VODS_CLIPS.md))

Staff alerts can't be turned off while someone holds the staff role.

## Data

- **notifications:** recipient, type, grouping key, actors (up to a few, plus a count), target, read time, created time.
- **notification preferences:** global and per channel, per type and channel. This already exists in Live streams and gains the per-type columns.
- **push subscriptions:** already in Live streams.
- **deliveries:** one row per push or email attempt, with status, for troubleshooting.

Delivery runs as jobs in the Postgres queue, with a lease and idempotency keys, so a notification is never sent twice. No Redis.

## Legacy notes

Legacy kept notification preferences and a notifications list, but most types were stubs. Wall-post notifications were muted by default and never sent. The rebuild keeps one catalog, so a type exists only once a module actually sends it.

## Done when

1. The bell, the panel and `/notifications` show live unread counts and grouped notifications, and mark as read across tabs.
2. Each type obeys its settings. Required notices can't be turned off. Per-channel mutes and blocks always suppress.
3. Push works in desktop browsers and in the installed web app on phones, and failed subscriptions are cleaned up.
4. Email respects one-click unsubscribe, verification and the digest setting.
5. Quiet hours hold back push and email and send one summary afterward. Under-18 accounts have quiet hours on by default.
6. DM previews stay hidden unless turned on.
7. Hourly and daily push caps hold under a burst (for example, a raid plus many follows).
8. No notification is ever sent twice after a worker restart.
9. Notifications older than 30 days are deleted.
