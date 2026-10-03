# Module 6: Support

Scoped October 3, 2026 by Joe. Builds after Module 5 (MAGNet), so the launch set (live, stable, chat, factions, MAGNet) isn't held up by payments work. Not started.

This module lets viewers support streamers with money and loyalty, and gets streamers paid. It follows the closure rule: specify, build, then test against "Done when". All open items were decided by Joe on October 3, 2026.

## Scope

- Monetization eligibility and payout setup
- Creator tiers (Scout, Trailblazer, Pioneer, Pathfinder)
- Subscriptions (three tiers, gift subs, subscriber badges, emotes and chat mode)
- Purchased Valor and tributes
- Engagement Valor (per-channel loyalty points) and channel rewards
- Co-streams as a squad view
- Payouts

Not in this module: Ad Valor and ad revenue sharing (Phase 4, with ads), Progression (Phase 2), alerts and overlays for tributes and subs (Phase 3), merch.

## Who can earn

Any account that is verified, has authenticator 2FA, and has finished Stripe Connect onboarding including the tax form. There is no follower count or hours threshold. Accounts aged 13 to 17 (by date of birth) can earn only through an Express account owned by a parent or legal guardian, as Stripe requires: the guardian completes onboarding, accepts the Connected Account Agreement, and receives the payouts. A restricted or banned account stops earning new revenue while restricted; balances already earned stay owed to it under the Terms.

## Payments and the ledger

- Stripe Checkout for Valor packs and subscriptions; Stripe Connect Express for payouts. Card data never touches S.V.E.R servers.
- Every money movement is a double-entry ledger row in Postgres (AGENTS.md). Balances are derived from the ledger, never stored only as a mutable number.
- Stripe webhooks are signature-checked, stored, and processed idempotently by event ID; a replayed or out-of-order event never double-credits.
- Refunds and chargebacks post reversing entries. A chargeback on a Valor purchase can push the buyer's Purchased Valor balance negative, which locks spending until settled.
- Amounts are integer cents in USD, the only currency at launch; Stripe converts foreign cards. Valor earnings accrue in tenths of a cent and round down only at payout.

## Creator tiers

Carried over from legacy and decided by Joe on October 3, 2026. Every streamer starts at Scout.

| Over the last 90 days | Scout | Trailblazer | Pioneer | Pathfinder |
| --- | --- | --- | --- | --- |
| Streams | 0 | 8 | 30 | 120 |
| Stream hours | 0 | 15 | 80 | 300 |
| Average viewers | 0 | 5 | 20 | 60 |
| Followers | 0 | 150 | 800 | 2,500 |
| Subscribers | 0 | 0 | 8 | 40 |
| Unique viewers | 0 | 0 | 300 | 3,000 |
| Days active | 0 | 10 | 40 | 120 |
| **Subscription split to the streamer** | 65% | 70% | 75% | 80% |
| **Ad split: streamer / S.V.E.R / viewers (Phase 4)** | 65 / 30 / 5 | 70 / 25 / 5 | 75 / 20 / 5 | 80 / 15 / 5 |
| **VOD retention (Module 7)** | 24 hours | 48 hours | 72 hours | 7 days |

- A streamer moves up when they meet every requirement for the next tier. Checked weekly, Monday 00:01 Eastern. Tiers never go down.
- Each tier has a badge shown on the channel and in chat.
- Viewer numbers (average and unique viewers) use the Trusted count from Module 3 viewer integrity, so viewbotted viewers never help a streamer climb. A streamer with an open staff integrity case is not promoted until it is closed.
- Valor tributes pay 0.8¢ per Valor at every tier.
- Legacy tier benefits not carried over: the discovery boost (MAGNet never ranks by size), and referrals, affiliate, partnerships and priority support (deferred).
- The ad split is recorded here so the tiers stay in one place; it applies once ads exist (Phase 4). Viewers' 5% becomes Ad Valor.

## Subscriptions

- Tiers: $4.99, $9.99 and $24.99 a month. The streamer keeps 65% to 80% of the full price depending on their creator tier (below); S.V.E.R pays the card fees out of its share. Gift subs use the same split.
- Benefits:
  - Subscriber badge showing months subscribed: 1, 3, 6, 9, 12, then each further year.
  - Subscriber emotes: 5 slots at tier 1, 5 more at tier 2, 5 more at tier 3, on top of the channel's 10 open emotes from Module 3.
  - Subscriber-only chat mode the owner or a moderator can switch on.
  - No ads on that channel once ads exist.
- Gift subs: one month to a named viewer, or 5, 10 or 20 one-month gifts to random signed-in chatters in that channel who allow gifts (a setting, default on). A gifted month never auto-renews.
- Paid by card (auto-renews monthly) or by Purchased Valor (one month at a time).
- Cancel anytime; benefits run to the end of the paid month. Upgrading tiers takes effect immediately with Stripe's proration.
- Buyers aged 13 to 17: before their first purchase a parent or guardian confirms, at checkout, that they are the cardholder and consent to the purchase. Purchases by an account under 18 are capped at $50 a month. A parent can report an unauthorized purchase through support for a refund, which reverses the Valor or subscription.

## Purchased Valor

- Bought by card in packs, with bonus Valor on bigger packs:

  | Price | Valor | Per 100 |
  | --- | --- | --- |
  | $0.99 | 100 | 99¢ |
  | $1.99 | 200 | 99.5¢ |
  | $4.99 | 525 | 95.0¢ |
  | $9.99 | 1,075 | 92.9¢ |
  | $19.99 | 2,200 | 90.9¢ |
  | $49.99 | 5,600 | 89.3¢ |
  | $99.99 | 11,500 | 86.9¢ |

- The streamer earns **0.8¢ per Valor** spent on them (tributes and Valor-paid subscriptions). S.V.E.R pays the card fees and keeps the rest. Every pack covers its costs except the $0.99 pack, where Stripe's 30¢ fee means S.V.E.R loses about 14¢ when all 100 Valor are spent; Joe accepted that as the entry price.
- Spent as **tributes** in chat (a viewer "pays tribute" with a message: minimum 10 Valor; the message is highlighted and shows the amount) and on subscriptions.
- No expiry. Not transferable between accounts. Viewers can't cash it out. Purchases are non-refundable except where the law or the refund policy says otherwise.
- Tributes obey chat rules: a tribute message that breaks banned-word or link rules is rejected before any Valor moves. A tribute to a channel that banned or blocked the viewer is refused.

## Engagement Valor

- A separate balance for each channel, earned by verified viewers in that channel only. No cash value; it can't be bought, transferred or spent elsewhere.
- Earned by real playback (playback leases, not open tabs) and by chatting, plus a one-time bonus for following. Rates live in the private tuning config with safe example values in the repo.
- Owners create rewards: name, cost, cooldown, per-stream limit, and optional text the viewer must enter. Built-in reward: highlight my message.
- Redemptions go to a Creator Studio queue where the owner or a moderator marks each done or refunds it. Refunds return the points.
- A channel ban freezes earning and spending in that channel.

## Co-streams (squad view)

- A live owner invites 1 to 3 other live owners. When an invitee accepts, their stream joins the squad.
- The squad page shows the streams side by side (stacked on phones). Viewers choose which stream they hear and whose chat they use; the others play muted.
- The host picks one of two modes when creating the squad:
  - **Separate:** each stream keeps its own chat, viewers, subs and tributes.
  - **Merged:** one shared chat for the whole squad. Money spent through the squad page while it runs (tributes, gift subs and the first month of new subs) is pooled and split equally among the members live at that moment; each member's share is then paid at their own tier split. Later renewals of a sub go to the channel the viewer picked.
- Each stream keeps its own viewer count and its own moderators. In merged mode, every member's moderators can moderate the shared chat, and a ban in any member's channel blocks that user from the shared chat.
- Anyone can leave at any time; the squad ends when its host leaves or goes offline. Blocks and bans between members prevent invites.
- No video mixing or re-encoding: each stream is delivered exactly as in Module 3.

## Payouts

Decided by Joe on October 3, 2026.

- **Payday every 2 weeks:** the streamer's whole available balance is paid out by standard transfer (arrives in about 2 business days). No minimum and no hold: earnings are available as soon as Stripe settles them.
- **Early Pay:** between paydays, a streamer can withdraw up to **75%** of what they've earned since the last payday, minus anything already withdrawn early. At most one Early Pay per day.
  - **Standard** (about 2 business days): free to the streamer.
  - **Instant** (minutes, to an eligible debit card): Stripe's 1% fee is paid by the streamer and shown before they confirm.
- **The 25% that stays until payday** covers refunds and disputes that arrive during the period. Whatever is left is paid on payday.
- **Taxes:** S.V.E.R doesn't withhold taxes; streamers are independent creators and Stripe handles tax forms. Creator Studio shows a reminder and an optional estimate of how much to set aside.
- S.V.E.R pays Stripe's standard payout fees (currently $2 per paid account per month plus 0.25% + 25¢ per payout).
- Stripe Chargeback Protection is turned on for Checkout payments (S.V.E.R pays its 0.4% fee). It reimburses fraud disputes only, up to its annual cap, and doesn't cover "cancelled subscription" or "not received" disputes or renewals outside Checkout.
- A dispute that isn't reimbursed reverses the streamer's share. If that share was already paid out, the streamer's balance goes negative and is recovered from their future earnings before the next payout or Early Pay.
- Guardian-owned payout accounts (ages 13 to 17) work the same way; payouts go to the guardian's account.
- Payout setup, Early Pay and payout history live in Creator Studio.

## Before launch

- Terms of Service and a refund policy for paid features, reviewed by a lawyer.
- Stripe account in the company's name, with Connect enabled.

## Done when

An eligible streamer finishes payout setup; a viewer subscribes at each tier by card and by Valor, gifts subs, buys Valor and pays tribute; every transaction balances in the ledger and survives webhook retries, refunds and chargebacks; a streamer creates rewards and fulfills redemptions; a squad of up to 4 streams plays together in both modes, and merged-mode revenue splits correctly; weekly tier checks promote streamers and change their split and VOD retention; a payday payout and a standard and instant Early Pay (capped at 75%, once a day) reach a Stripe test account, and a dispute after payout is recovered from later earnings.
