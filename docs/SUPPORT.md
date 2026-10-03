# Module 6: Support

Scoped October 3, 2026 by Joe. Builds after Module 5 (MAGNet), so the launch set (live, stable, chat, factions, MAGNet) isn't held up by payments work. Not started.

This module lets viewers support streamers with money and loyalty, and gets streamers paid. It follows the closure rule: specify, build, then test against "Done when". Items marked **Open** need Joe's decision before building.

## Scope

- Monetization eligibility and payout setup
- Subscriptions (three tiers, gift subs, subscriber badges, emotes and chat mode)
- Purchased Valor and cheers
- Engagement Valor (per-channel loyalty points) and channel rewards
- Co-streams as a squad view
- Payouts

Not in this module: Ad Valor and ad revenue sharing (Phase 4, with ads), Progression (Phase 2), alerts and overlays for cheers and subs (Phase 3), merch.

## Who can earn

Any account that is verified, has authenticator 2FA, and has finished Stripe Connect onboarding including the tax form. There is no follower count or hours threshold. Accounts aged 13 to 17 (by date of birth) can earn only through an Express account owned by a parent or legal guardian, as Stripe requires: the guardian completes onboarding, accepts the Connected Account Agreement, and receives the payouts. A restricted or banned account stops earning new revenue while restricted; balances already earned stay owed to it under the Terms.

## Payments and the ledger

- Stripe Checkout for Valor packs and subscriptions; Stripe Connect Express for payouts. Card data never touches S.V.E.R servers.
- Every money movement is a double-entry ledger row in Postgres (AGENTS.md). Balances are derived from the ledger, never stored only as a mutable number.
- Stripe webhooks are signature-checked, stored, and processed idempotently by event ID; a replayed or out-of-order event never double-credits.
- Refunds and chargebacks post reversing entries. A chargeback on a Valor purchase can push the buyer's Purchased Valor balance negative, which locks spending until settled.
- Amounts are integer cents in USD; Valor earnings accrue in tenths of a cent and round down only at payout. **Open:** other currencies.

## Subscriptions

- Tiers: $4.99, $9.99 and $24.99 a month. The streamer keeps 90% of the full price; S.V.E.R pays the card fees out of its 10%.
- Benefits:
  - Subscriber badge showing months subscribed: 1, 3, 6, 9, 12, then each further year.
  - Subscriber emotes: 5 slots at tier 1, 5 more at tier 2, 5 more at tier 3, on top of the channel's 10 open emotes from Module 3.
  - Subscriber-only chat mode the owner or a moderator can switch on.
  - No ads on that channel once ads exist.
- Gift subs: one month to a named viewer, or 5, 10 or 20 one-month gifts to random signed-in chatters in that channel who allow gifts (a setting, default on). A gifted month never auto-renews.
- Paid by card (auto-renews monthly) or by Purchased Valor (one month at a time).
- Cancel anytime; benefits run to the end of the paid month. Upgrading tiers takes effect immediately with Stripe's proration.
- Buyers aged 13 to 17: before their first purchase a parent or guardian confirms, at checkout, that they are the cardholder and consent to the purchase. Purchases by an account under 18 are capped at $50 a month (**Proposed** amount). A parent can report an unauthorized purchase through support for a refund, which reverses the Valor or subscription.

## Purchased Valor

- Bought by card in packs, priced at 99¢ per 100 Valor with discounts on larger packs. The smallest pack is 500, because Stripe's fixed 30¢ fee makes smaller packs lose money. Starting price list (**Proposed** for the larger packs):

  | Pack | Price | Per 100 |
  | --- | --- | --- |
  | 500 | $4.99 | 99.8¢ |
  | 1,500 | $14.49 | 96.6¢ |
  | 5,000 | $46.99 | 94.0¢ |
  | 10,000 | $89.99 | 90.0¢ |
  | 25,000 | $219.99 | 88.0¢ |

- The streamer earns **0.8¢ per Valor** spent on them (cheers and Valor-paid subscriptions). S.V.E.R pays the card fees and keeps the rest; every pack in the list stays above cost.
- Spent as **cheers** in chat (minimum 10 Valor; the message is highlighted and shows the amount) and on subscriptions.
- No expiry. Not transferable between accounts. Viewers can't cash it out. Purchases are non-refundable except where the law or the refund policy says otherwise.
- Cheers obey chat rules: a cheer message that breaks banned-word or link rules is rejected before any Valor moves. A cheer to a channel that banned or blocked the viewer is refused.

## Engagement Valor

- A separate balance for each channel, earned by verified viewers in that channel only. No cash value; it can't be bought, transferred or spent elsewhere.
- Earned by real playback (playback leases, not open tabs) and by chatting, plus a one-time bonus for following. Rates live in the private tuning config with safe example values in the repo.
- Owners create rewards: name, cost, cooldown, per-stream limit, and optional text the viewer must enter. Built-in reward: highlight my message.
- Redemptions go to a Creator Studio queue where the owner or a moderator marks each done or refunds it. Refunds return the points.
- A channel ban freezes earning and spending in that channel.

## Co-streams (squad view)

- A live owner invites 1 to 3 other live owners. When an invitee accepts, their stream joins the squad.
- The squad page shows the streams side by side (stacked on phones). Viewers choose which stream they hear and whose chat they use; the others play muted.
- Each stream keeps its own viewer count, chat, moderation, subs and cheers. No revenue sharing in this module.
- Anyone can leave at any time; the squad ends when its host leaves or goes offline. Blocks and bans between members prevent invites.
- No video mixing or re-encoding: each stream is delivered exactly as in Module 3.

## Payouts

- Monthly, with no minimum: every available balance is paid out. S.V.E.R pays Stripe's payout fees (currently $2 per paid account per month plus 0.25% + 25¢ per payout).
- A hold period covers chargebacks before revenue becomes available. **Proposed:** 30 days.
- Tax reporting through Stripe. Payout setup lives in Creator Studio.

## Before launch

- Terms of Service and a refund policy for paid features, reviewed by a lawyer.
- Stripe account in the company's name, with Connect enabled.

## Done when

An eligible streamer finishes payout setup; a viewer subscribes at each tier by card and by Valor, gifts subs, buys Valor and cheers; every transaction balances in the ledger and survives webhook retries, refunds and chargebacks; a streamer creates rewards and fulfills redemptions; a squad of up to 4 streams plays together; a test payout reaches a Stripe test account.
