# Ads, ad revenue sharing and Ad Valor (Phase 4)

**October 16, 2026: until a third-party provider is worth it, S.V.E.R runs its own network of approved ads with guaranteed delivery; see [ADVERTISERS.md](ADVERTISERS.md).** The split and Ad Valor below carry over to it.

Decided by Joe on October 10, 2026: **design first, no ads yet.** This spec, the split and a test-data ledger are built; no ad provider is chosen, no ad is shown and no ad money moves. Joe picks the provider, and approves going live, before any of the "Before ads go live" list starts.

## What ads are on S.V.E.R

- **Ad breaks, never pre-rolls.** An ad plays only during a break in a live stream. A viewer who opens a stream is never made to wait for an ad first.
- **Who sees them:** signed-in and guest viewers of channels that can earn (docs/SUPPORT.md "Who can earn"). Subscribers of a channel never see ads on it. Accounts under 18 see only ads the provider marks as suitable for teens, or none if the provider can't promise that.
- **What the streamer controls:** whether ads run at all (off by default), and when a break starts (manually, from Creator Studio or the Stream Deck/API later). There is no automatic mid-stream break.
- **Ad breaks never get in the way** (Joe, October 16, 2026). Nothing covers the stream or stops it. During a break, the stream shrinks to the top-left of the player and keeps playing, muted, with captions switched on for the break when the stream has them. The ad plays in the bottom-right corner, labelled "Ad", and a note says when the stream comes back to full size. Chat, rallies, the board and every other control keep working. Streamers are told to turn on captions in OBS so muted viewers can follow along.
- **Turning ads off.** Any viewer can turn ads off, from the break itself or from Settings → Preferences ("Show me ads"). Before it happens they're told what it costs: no ads anywhere on S.V.E.R and no Ad Valor. An account with ads off is never served an ad in any placement (`users.ads_off`), so it earns nothing from ads. Guests turn ads off for their browser. Streamers still earn from everyone else.
- **Nothing buys visibility.** Ads never change MAGNet, browse order, faction influence or creator tiers.

## The split

Every ad segment's revenue, as the provider reports it, is split by the streamer's creator tier (docs/SUPPORT.md "Creator tiers"):

| Tier | Streamer | S.V.E.R | Viewers (Ad Valor) |
| --- | --- | --- | --- |
| Scout | 65% | 30% | 5% |
| Trailblazer | 70% | 25% | 5% |
| Pioneer | 75% | 20% | 5% |
| Pathfinder | 80% | 15% | 5% |

- Amounts are tenths of a cent, like the rest of the ledger. Rounding always goes to S.V.E.R's share, so a split never creates money.
- **The viewers' 5%** is divided equally among the viewers counted for that segment. If nobody is counted, S.V.E.R keeps it.
- **Who is counted:** viewers with a Trusted playback session (docs/LIVE_STREAMS.md "Viewer integrity") who are signed in, aren't the streamer and aren't subscribed to the channel. They are captured when the break starts, because playback leases expire. Guests watch ads but earn no Ad Valor.

## Ad Valor

- A viewer's Ad Valor is a USD balance in the ledger (`usd:adv:<user>`), separate from Purchased and Engagement Valor. It shows to the viewer in dollars and cents.
- **Spend:** on subscriptions, like Purchased Valor (designed; built when ads go live).
- **Cash out:** 18+ only (docs/PLATFORM_PLAN.md "Age rules"), through the same Stripe Connect Express onboarding streamers use, with a minimum and fee chosen when it is built. Not transferable.
- A refunded or clawed-back ad payment from the provider posts reversing entries, as Stripe refunds do; a negative Ad Valor balance is recovered from future Ad Valor.

## What's built now (test data only)

- `ads.rs`, migration 0090: `ad_segments` and `ad_segment_viewers`. The only book allowed is `test`; its ledger accounts start with `adtest:` (`adtest:usd:ads`, `adtest:usd:earnings:<user>`, `adtest:usd:adv:<user>`, `adtest:usd:platform`), so payouts, the Money page, open data and Raven's Eye never count them.
- Staff → Ads (`/admin/ads`): start a test break on a live channel (captures who would be counted), then settle it once with synthetic revenue up to $1,000. Both are step-up and audited (`ad_test_break`, `ad_test_settle`). The page lists segments, their splits and the test balances.
- No viewer- or streamer-facing UI yet: test balances must never look like real money.

## Before ads go live

1. Joe picks the ad provider and signs its agreement (server-side ad insertion for HLS/WebRTC, a revenue report per break or per impression, teen-safe filtering).
2. The Terms of Service, privacy policy and refund policy add ads and Ad Valor; a lawyer reviews them.
3. A migration allows the `live` book; settlement reads the provider's report instead of staff input, and posts to the real accounts.
4. Creator Studio: an ads switch (off by default) and a "Run ad break" button; viewers see Ad Valor in Settings.
5. Ad Valor spend on subscriptions and 18+ cash-out.
