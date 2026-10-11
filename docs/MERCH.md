# Merch store (Phase 4)

Joe's decision on October 10, 2026: **a platform store.** Written the same day with **Proposed** defaults that Joe can change. Nothing here launches until Joe picks the print-on-demand vendor and signs its agreement ("Before launch").

## What it sells

- **Launch set:** S.V.E.R and faction merch using the three crests Joe confirmed as approved original artwork (October 4, 2026). Shirts, hoodies, mugs and stickers. Copy and lore follow [COPY.md](COPY.md) and [LORE.md](LORE.md).
- **Later, its own decision:** creator merch (a streamer's own designs, with a revenue split by creator tier, like subscriptions). It needs artwork review and copyright handling for uploads, so it doesn't ship with the launch set.
- Buying merch never changes MAGNet, faction influence, creator tiers or progression. No exclusive in-platform perks for buyers: a faction badge or title can't be bought.

## How it works

- **Print on demand:** nothing is stocked. Each order goes to the vendor, which prints, packs, ships and handles carrier tracking. S.V.E.R holds no inventory and never sees addresses beyond what the order needs.
- **Checkout:** Stripe Checkout, as for Valor and subscriptions (docs/SUPPORT.md "Payments and the ledger"), collecting the shipping address and calculating tax. Card data never touches S.V.E.R. Each sale posts double-entry ledger rows: cash in, the vendor's cost as owed to the vendor, the rest to S.V.E.R.
- **Fulfillment:** after payment, the order is sent to the vendor's API idempotently (keyed by the Checkout session), and the vendor's webhook updates the status (submitted, in production, shipped with tracking, delivered, failed). The buyer gets emails at payment and at shipping, and sees orders under Settings → Orders.
- **Refunds and problems:** damaged or misprinted items are replaced or refunded through the vendor's claim process; a refund posts reversing entries. Returns of correct items follow the refund policy, which needs a merch section.
- **Who can buy:** signed-in, verified accounts. Buyers aged 13 to 17 need the same guardian confirmation as other purchases and count toward the $50 monthly cap.
- **Where:** a Store page linked from the footer and each faction's page. Staff manage products, prices and the vendor's product IDs in `/admin`, audited.

## Before launch

1. Joe picks the print-on-demand vendor and signs its agreement (US shipping first; international is its own decision).
2. Product photos or vendor mockups, prices and the shipping policy.
3. The Terms of Service, privacy policy (shipping addresses go to the vendor) and refund policy add merch; a lawyer reviews them.
4. Stripe Tax or the vendor's tax handling is set up for sales tax.

## Done when

1. A buyer pays for a crest shirt, the order reaches the vendor once, and the buyer gets the shipped email with tracking.
2. A replayed Checkout or vendor webhook never creates a second order or a second ledger posting.
3. A refund posts reversing entries and the order shows as refunded.
4. A buyer under 18 is held to the guardian rule and the $50 cap.
