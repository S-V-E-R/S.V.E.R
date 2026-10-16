# S.V.E.R Ads: our own network of approved ads

Decided by Joe on October 16, 2026. The long-term goal is gaming ads from a third-party provider ([ADS.md](ADS.md)). Until S.V.E.R's audience is large enough to interest one, **S.V.E.R runs its own ad network**: advertisers buy campaigns directly, staff approve every ad, and **every campaign gets 100% of the verified impressions it paid for**.

Joe's decisions:

- **Pricing:** a fixed rate card. Staff set a price per 1,000 verified impressions (CPM) for each placement. There is no auction.
- **Buyers:** self-serve. Advertisers sign up, upload their ad, choose impressions, dates and targeting, and pay upfront. A campaign starts only after staff approve its ad.
- **Placements:** streamer ad breaks, Beacons feed cards and labelled site placements.
- **Shortfall (hybrid):** a campaign keeps running until 100% is delivered, even past its end date. Once the end date has passed, the advertiser may withdraw instead and is refunded for every impression not yet delivered.

Joe approved this spec as written, including every **Proposed** default, on October 16, 2026.

**Build step 1 built October 16, 2026** (migration 0111, `adnet.rs`, `/advertise`, Staff → Ad network): the rate card, advertiser accounts, ad uploads (pictures through the image pipeline; videos probed and re-encoded to 720p H.264/AAC with a poster frame) and staff review, quotes with admission control (held checkouts count against inventory), Stripe checkout into each campaign's prepaid ledger account, late payments, cancellation, withdrawal after the end date, staff refunds, and a full refund when an ad is never approved within 30 days. **Ad sales stay off** behind the "Ad sales" switch. Uploads are limited to 11 MB (the server's 12 MB request limit), so a 30-second video should be exported at about 3 Mbps. Stripe Tax waits for the tax decision in step 4: with it off, the checkout total is the quoted price. 

**Build step 2 built October 16, 2026** (migration 0112, `adserve.rs`, `components/Ads.tsx`):
- **Serving:** in ad breaks (an overlay over the player, which keeps the stream running muted behind it), as a feed card after every 8th Beacon, and in a labelled slot on Home and Browse.
- **Pacing:** the campaign furthest behind its even daily schedule is served first; a campaign at or ahead of today's target waits. One past its end date is served first until whole (the make-good).
- **Tickets and verification:** every ad served gets a single-use ticket. It counts only when the session is still Counted or Trusted (breaks) or a signed-in account in good standing (feed and site), the ad was visible and advancing for 2 s of video or 1 s of a picture (checked against the server's clock), and it isn't a repeat: the same ad once per 10 minutes, a campaign 3 times a day. Otherwise it's rejected with its reason (expired, hidden, too_short, unverified_session, frequency_cap, campaign_full). The last impression marks a campaign delivered, and a campaign never counts more than it paid for.
- **Clicks:** go through `/api/ads/click/{ticket}`.
- **Streamers:** turn ads on in Creator Studio (only channels that can earn) and start 30, 60 or 90-second breaks, at least 10 minutes apart.
- **Forecast:** after 7 days of data it becomes measured (eligible viewers a day times the verified rate) instead of staff's estimate.
- **Known limits:** players find a new break at their next `/live` poll (up to 15 seconds), and feed and site genre targeting reads the channels a viewer follows.

Next: step 3, settlement and the revenue dashboards.

## How the guarantee works

S.V.E.R can only promise impressions it can actually serve. Three rules keep the promise.

1. **Sell only what exists (admission control).** When an advertiser picks impressions, dates and targeting, the checkout quotes them only if the forecast covers it:
   - **Forecast:** verified impressions per placement, genre and day, from the last 14 days of real delivery. **Proposed:** before there's history, staff set a starting estimate.
   - **Safety margin:** sellable = forecast × 70% (**Proposed**), minus impressions already promised to running and accepted campaigns in the same window.
   - If the request doesn't fit, the advertiser sees the most that fits and the soonest dates that would fit the full amount. They can't buy more than that.
2. **Pace delivery.** Each campaign aims for an even daily share between its start and end. When choosing which ad to serve, campaigns furthest behind schedule go first; a campaign ahead of schedule waits.
3. **Make it good.** A campaign that reaches its end date unfinished keeps running at top priority until delivered, at no extra charge. From the end date on, its dashboard offers **Withdraw and refund the rest**: the undelivered impressions are refunded to the original card (Stripe partial refund), and the campaign stops.

The forecast, committed and sellable numbers are on the staff page, so staff can see how close inventory is to sold out.

## What counts as an impression

An impression counts only when a real, watching person saw the ad. The checks reuse viewer integrity ([LIVE_STREAMS.md](LIVE_STREAMS.md) "Viewer integrity") and Beacons view counting ([BEACONS.md](BEACONS.md)):

- **Who:** a playback session at the Counted or Trusted level, or a signed-in session that passed the same checks for the feed and site. Sessions marked Pending, Excluded or flagged as automation never count.
- **Seen:** the page is visible and the ad is on screen. A video ad counts after 2 seconds of advancing playback with at least half of it in view; an image counts after 1 second with half in view (**Proposed**, following the common industry measures). The player reports this with signed, single-use impression tickets, so a replayed or forged report is rejected.
- **Once:** at most one impression per person per ad per 10 minutes, and a frequency cap per person per campaign per day (**Proposed: 3**). Duplicates from the same browser or account across tabs count once.
- **Proof for advertisers:** the dashboard shows impressions served, impressions verified and impressions rejected, with the reason (hidden tab, too short, session not verified, frequency cap, automation). Only verified impressions are billed.

## Placements

| Placement | Format | Where | Who earns |
| --- | --- | --- | --- |
| Streamer ad break | Video, 15 or 30 s, 16:9 | Plays in the player during a break the streamer starts; the stream keeps running behind it | Streamer, viewers' pool, S.V.E.R ([split](#the-split)) |
| Beacons feed card | Video or image, 9:16, up to 30 s | A card marked "Ad" every 8 Beacons in the feed (**Proposed**) | Viewers' pool, S.V.E.R |
| Site placement | Image, 16:9 | One labelled slot on Home and Browse, outside every shelf | Viewers' pool, S.V.E.R |

- **Ad breaks stay as designed in [ADS.md](ADS.md):** never a pre-roll; ads are off for a channel until the streamer turns them on; the streamer starts each break (30, 60 or 90 s) from Creator Studio, the Stream Deck plugin or the API. Subscribers of a channel never see its ads.
- **No placement touches discovery.** Ads never change MAGNet, Browse order, search, faction influence or creator tiers, the same separation as the Wall ([WALL.md](WALL.md)). Ads are always labelled "Ad", with the advertiser's name.
- **Under 18:** only ads approved as teen-safe in review.

## Targeting

Kept simple so the forecast stays honest:

- **Placements:** any of the three.
- **Genres:** all, or chosen genres from the territory map ([FACTIONS.md](FACTIONS.md)). For feed cards and the site slot, genre means the genres the viewer watches most.
- **Language:** all, or chosen stream languages.
- **Audience:** all ages (teen-safe ads only) or 18+.

No personal-data targeting, no tracking across other sites and no third-party pixels.

## The split

Each verified impression's price (the campaign's CPM ÷ 1,000) is split when it's verified, in tenths of a cent like the rest of the ledger. Rounding always goes to S.V.E.R, so the split never creates money.

| Placement | Streamer | Viewers | S.V.E.R |
| --- | --- | --- | --- |
| Ad break | By creator tier: 65% Scout, 70% Trailblazer, 75% Pioneer, 80% Pathfinder ([SUPPORT.md](SUPPORT.md) "Creator tiers") | 5% | The rest |
| Beacons feed card | none | 5% | 95% |
| Site placement | none | 5% | 95% |

- **Viewers' 5%:** goes to the viewer who saw the ad, as Ad Valor, when they're signed in, Trusted and 13 or older; otherwise S.V.E.R keeps it ([ADS.md](ADS.md) "Ad Valor").
- **Streamers** must be able to earn ([SUPPORT.md](SUPPORT.md) "Who can earn"); otherwise the streamer's share goes to S.V.E.R. Earnings join the streamer's existing payout balance (Stripe Connect).
- **The streamer's own views** of their own break never count.

## Money

- **Upfront payment:** the advertiser pays the quoted total with Stripe Checkout (one-time payment, card). **Proposed:** Stripe Tax on, since digital advertising is taxable in some US states.
- **Ledger** (double-entry, like Support):
  - On payment: `usd:stripe` → `usd:ads:prepaid:<campaign>`.
  - On each verified impression: prepaid → `usd:earnings:<streamer>`, `usd:adv:<viewer>` and `usd:platform:ads`.
  - On withdrawal or rejection: prepaid remainder → refund through Stripe.
- **Rejected ads** (never approved) are refunded in full. Each campaign shows its invoice and refunds.
- **Minimum order (Proposed):** 1,000 impressions; the rate card has a minimum spend per campaign.
- **The `test` book** from ADS.md stays for staff testing; real campaigns use a `live` book.

## Advertisers

- **Account:** any S.V.E.R account with a verified email and an authenticator, plus business details (company or person name, website, billing country). Advertising is 18+.
- **Ad review:** staff check every ad before it runs, against the ad rules below. They approve, approve as teen-safe, or reject with a reason; the advertiser can upload a fixed version. Edits to an approved ad go back to review.
- **Ad rules (Proposed):** gaming, creator tools, hardware, software and creator-friendly brands welcome. Not allowed: gambling and betting (including skin gambling and loot-box sites), crypto and NFTs, adult content, politics, weapons, tobacco and vaping, alcohol for under-21 audiences, misleading claims, other streaming platforms. Landing pages must match the ad.
- **Click-through:** an ad can link to one https address. Clicks go through `/ad/click/{id}` (counted for the dashboard, no tracking parameters added by S.V.E.R).

## Dashboards

- **Advertisers (`/advertise`):** campaigns with ordered, delivered and remaining impressions, a daily delivery chart against the pace line, verified vs. rejected impressions with reasons, impressions by placement and genre, clicks, spend, invoices, and Withdraw (after the end date, when not finished). A new campaign shows the live quote and availability.
- **Streamers (Creator Studio → Ads):** the ads switch, Run ad break, breaks run, verified impressions and ad earnings per day.
- **Viewers (Wallet):** Ad Valor balance and history.
- **Staff (`/admin/ads`):** the review queue, the rate card, inventory (forecast, committed, sellable per placement and day), every campaign's pace, rejected-impression reasons, refunds, and ad revenue by placement and split. Raven's Eye and open data get an "Ads" line.

## Build order

1. Rate card, advertiser accounts, ad upload and review, the quote with admission control, Stripe checkout and refunds. Sales stay behind an "Ad sales" switch, off by default.
2. Serving in all three placements, impression tickets and verification, pacing and the make-good rule.
3. Settlement into the ledger, the four dashboards, Raven's Eye and open data.
4. **Before sales open:** advertiser terms, the privacy policy and the Terms of Service updated for ads and Ad Valor, lawyer review, the tax decision. Then Joe opens the switch.

## Done when

1. An advertiser buys 1,000 impressions on a rate-card quote; the campaign doesn't start until staff approve the ad.
2. The quote refuses more impressions than the forecast can deliver in the chosen window.
3. Only verified impressions count, and the dashboard shows served, verified and rejected with reasons. Automated sessions, hidden tabs and repeat views don't count.
4. A campaign that misses its end date keeps running until 100% is delivered; after the end date the advertiser can withdraw and is refunded exactly the undelivered part.
5. Each verified impression's money lands in the right ledger accounts by placement and tier, and the totals match the advertiser's payment minus refunds.
6. Streamers, viewers and staff see their ad revenue; ads never change discovery.
