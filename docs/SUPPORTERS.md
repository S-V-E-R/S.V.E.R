# Platform supporters and Founders

Approved by Joe on October 12, 2026. A monthly subscription to S.V.E.R itself, in the spirit of Glimesh's Supporter and Gold Supporter. All of the money goes to the platform, and the perks are only how you look. The first 500 people at the top tier also become numbered **Founders** for good.

This reverses the "paid platform supporter subscription" entry under "Considered and not taken" in [CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md). It also replaces the legacy Founders program, which no one bought into. That program sold Valor multipliers, Valor bonuses, ad-free viewing, early access and cashable Valor, and none of those carry over.

**When:** with the Wall ([WALL.md](WALL.md)). It reuses Support's Stripe subscriptions (Module 6).

**Built so far (October 12, 2026):** the marks themselves, in `apps/web/components/Supporter.tsx` with styles in `globals.css`. That covers the Wolf mark, both chat badges, the Founder mark, the Wolfsworn and Greycloak player-card frames, the silver name and "Supporting since". Joe asked for them to be made first. The reference image is [mockups/supporters.webp](design/mockups/supporters.webp). They aren't shown anywhere yet; the server decides who gets them once subscriptions are built.

## The tiers

| | **Wolfsworn** | **Greycloak** |
| --- | --- | --- |
| Price | $5 a month | $25 a month |
| Chat and user-card badge | Static Wolf badge | Animated Wolf badge |
| Name | Normal | Shown in the Wolf's silver |
| Player card | Silver frame | Animated silver frame |
| Profile | "Supporting since {month year}" | The same |
| Founder spot | No | Yes, while spots last |

- **The lore:** the Grey Wolf is S.V.E.R itself, and its people wore grey because they belonged to no colour ([LORE.md](LORE.md)). Supporters are the Wolf's own, so their look is ash and silver, which no faction owns.
- **Faction marks stay:** the faction crest and theme are always kept. A supporter's crest still shows beside their name.
- **Months count:** the badge shows total months supported, which carry over between tiers and across breaks.
- **Upgrades and downgrades:** moving up to Greycloak takes effect at once with Stripe's proration. Moving down takes effect at the next renewal.

## Founders

- **Who gets one:** the first **500** accounts to pay a Greycloak month each get a numbered Founder mark, #001 to #500, in the order their first payment settles.
- **It's kept for good:** the mark stays after cancelling or downgrading. It shows on the user card, the profile and the player card. In chat, it shows only while the person is subscribed at either tier.
- **The count is public:** `/supporters` shows "312 of 500 Founder spots claimed" from real data. When all 500 are taken, the page reads "All 500 Founder spots are claimed", and Greycloak carries on without them.
- **Refunds:** if the payment that claimed a spot is refunded or lost to a dispute, the mark is revoked. The next new Greycloak gets the lowest free number. A spot is never sold or transferred.
- **No other way in:** staff can't grant Founder marks. The only way to get one is to pay for a Greycloak month.

## What supporting never gives

- MAGNet placement, home, sidebar, Browse or search position.
- Faction influence or war ground. Platform payments are not a "support" source in [FACTIONS.md](FACTIONS.md), which counts support for broadcasters only.
- XP, levels, Valor of either kind, or Engagement Valor.
- Chat priority, bypassing slow mode or chat gates, or extra emote slots.
- Anything a streamer or moderator controls, such as unbans or skipped queues.

**Keep paid and earned apart:** Progression's level rewards (open Question 1 in [PROGRESSION.md](PROGRESSION.md)) may add earned frames and name colours. Paid and earned looks must never match.
- Reserved for supporters: the Wolf mark, silver player-card brackets, the walking frame line and the silver name.
- The earned level badge already uses a silver border at levels 40 to 59. That stays, because it sits on the small level badge, not the player card or the name.

## Paying

- **How:** card only, through Stripe Checkout as a subscription. Valor can't pay for it, because Valor is creators' money. Platform subscriptions can't be gifted at launch.
- **Renewal terms:** the same California renewal terms as creator subscriptions ([SUPPORT.md](SUPPORT.md)). That means the price and "renews until cancelled" next to the button, the same text above Stripe's pay button, and an acknowledgment email.
- **Cancelling:** cancel anytime from `/wallet`, and perks run to the end of the paid month.
- **Under 18:** the guardian confirmation and the $50 monthly cap apply, counted with the account's other purchases.
- **Who gets the money:** 100% goes to SVER LLC, and the creator split doesn't apply. It's shown as its own line in Open data's monthly money charts ([CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md#open-data)).

## Pages

- **`/supporters`:**
  - What supporting is for and the "never gives" list in plain words.
  - The two tiers, with buttons to subscribe.
  - The Founder count.
  - The Founders list: number, name and crest, in number order. A Founder can hide their name, and their spot then shows as "Founder #123".
  - It's linked from the footer ("Support S.V.E.R"), About and `/wall`. It never appears in home, the sidebar or MAGNet.
- **`/wallet`:** the current plan, the next charge, change tier and cancel.
- **Design:** there's no mockup. It's built from [DESIGN.md](DESIGN.md), reusing Support's tier cards, as `docs/design/README.md` says.
  - The two tier cards use `.frame`.
  - New tokens, the same on every theme: `--wolf` (#D8DEE6), `--wolf-light`, `--wolf-mid` and `--wolf-deep`. The silver name and Founder text pass AA on every theme's surface.
  - **Animation:** CSS only, no glow. Under reduced motion it stays still.
    - Greycloak badge: the Wolf's eyes catch the light for under a second every 4 seconds (opacity only).
    - Greycloak frame: a short silver line walks the card's border every 8 seconds (stroke-dashoffset on a small inline SVG, as [MOTION.md](MOTION.md) allows). With reduced motion it becomes a still, thin silver border.
  - The pull request includes 1440 px and 390 px screenshots.

## How it's built

- **`platform_subscriptions` table:** user, tier, Stripe subscription ID, status, period end, `cancel_at_period_end`, and months supported.
- **`founders` table:**
  - Columns: the number (1–500, primary key), the user, the claiming invoice, claim time, revoke time and whether the name is hidden.
  - The user is unique among active marks.
  - The claim runs in the same transaction as the invoice webhook: it takes the lowest free number under a row lock, so two payments can never get the same number.
- **Badges:** chat badges are decided on the server per message, like the Broadcaster and Moderator badges ([LIVE_STREAMS.md](LIVE_STREAMS.md)). Clients can't supply them.
- **Webhooks:** the existing signed, idempotent Stripe processing handles them, keyed by event ID.
- **Admin:**
  - `/admin/supporters` lists subscribers and Founders. Its actions (refund, revoke a mark) are audited.
  - The "Supporter sales" emergency switch pauses new subscriptions. Existing ones keep running.

## Done when

1. Wolfsworn and Greycloak subscriptions start, renew, upgrade (prorated), downgrade at renewal and cancel to period end. The renewal terms and acknowledgment email match creator subscriptions.
2. Badges, the silver name and the frames show only for active subscribers, are decided on the server, and stay still under reduced motion.
3. The first 500 paid Greycloak accounts get unique numbers, with no duplicates under concurrent payments.
   - Marks survive cancellation.
   - A refund or lost dispute revokes the mark and frees the number.
   - The count on `/supporters` is real.
4. Under-18 accounts need guardian confirmation and stay within the $50 cap.
5. Tests show MAGNet ordering, faction influence, XP, Valor and chat rules are identical for supporters and non-supporters.
6. Platform supporter money appears as its own Open data line, with nothing going to creator ledgers.
