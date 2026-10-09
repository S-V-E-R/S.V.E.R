# The Wall

Approved by Joe on October 9, 2026, after looking at the Million Dollar Homepage and people selling ad space on their walls. The Wall is a page of 1,000 seats that anyone can buy once and keep for the life of the site. It's the one place on S.V.E.R where money buys a spot, and that spot is only on this page.

**When:** as its own small change, built next after the current channel additions step. It reuses the Stripe setup from Support (Module 6). Sales open only once everything under [Before sales open](#before-sales-open) is done.

## What it is, and what it isn't

- **What a seat is:** a tile on `/wall` with a picture, a name, an optional link and an optional short message. People and brands can both buy one.
- **What it buys:** the tile, and nothing else.
  - Wall money never touches discovery. There's no MAGNet placement, home or sidebar placement, Browse or search ranking, faction points, war ground, badges, cosmetics or chat perks.
  - The page says so, in plain words.
- **What it isn't:**
  - It isn't the paid platform supporter subscription with cosmetic perks that [CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md) lists as not taken. A seat is bought once and gives no perks.
  - It isn't channel sponsors, which are logos a streamer chooses for their own profile ([PROFILES.md](PROFILES.md)).
  - It isn't creator support. Subscriptions, Valor and tributes ([SUPPORT.md](SUPPORT.md)) still pay creators.

## The grid

The grid is 40 columns by 25 rows, which makes 1,000 cells.

| Size | Cells | Starting price | Limit |
| --- | --- | --- | --- |
| Seat | 1 × 1 | $25 | none |
| Table | 2 × 2 | $90 | 60 tables (240 cells) |
| Hall | 4 × 4 | $300 | 20 halls (320 cells) |

The limits keep at least 440 cells for single seats, so the Wall stays mostly people.

**Prices rise as the Wall fills**, like the Million Dollar Homepage's last pixels:

| Cells sold | Seat | Table | Hall |
| --- | --- | --- | --- |
| 0 to 249 (First light) | $25 | $90 | $300 |
| 250 to 599 | $30 | $110 | $360 |
| 600 to 1,000 | $35 | $125 | $420 |

- The price is set when checkout starts, from the cells sold at that moment.
- A completely full Wall brings in about $22,000 to $35,000, depending on the mix of sizes and when they sell. Joe can change the prices and limits from `/admin/wall` before sales open; after that, only the next stage's prices can change.

## Buying a seat

1. **Who can buy:** a signed-in, verified account aged 18 or older (by date of birth). Under-18 accounts see the Wall but no buy button. One account can buy more than one seat. A brand buys through a person's account.
2. **Pick a spot:** choose the size, then any free spot it fits. The spot is held for 15 minutes while the buyer pays, and shows as "being claimed" to everyone else.
3. **Fill it in:**
   - **Picture:** square PNG, JPEG or WebP. At least 100 px for a seat, 200 px for a table and 400 px for a hall. It goes through the existing media pipeline.
   - **Name:** up to 40 characters.
   - **Link** (optional): `https` only.
   - **Message** (optional): up to 140 characters, shown on hover or tap.
   - **Crest** (optional): the buyer's faction crest in the tile's corner, if they're enlisted. It's for decoration only and adds nothing to the war.
4. **Pay:** a one-time Stripe Checkout payment, not a subscription. It reuses Support's signed, idempotent webhook processing. The buyer gets a receipt email.
5. **Review:** the tile shows as "taken · in review" until staff approve it in `/admin/media`. Approved tiles go live at once.

**If a tile is rejected:** the buyer gets the reason and can resubmit for 30 days. They can also ask for a full refund at any point before approval, which frees the spot. After 30 days with no approved tile, the refund is automatic.

**Changing a tile later:** the owner can change the picture, name, link, message and crest from `/wall/{n}`. Each change is reviewed, and the approved version stays up until the change is approved. The spot and size never change.

## What tiles can show

- Nothing the Community Guidelines (`/guidelines`) ban.
- No gambling or betting, crypto tokens or investment schemes, political ads, adult products, or other people's trademarks without their permission.
- **Links:**
  - Links use `rel="sponsored nofollow ugc"`.
  - If a link later leads somewhere banned, staff remove the link and the tile stays.
  - Reports go through the existing report flow ("Report this seat").
- **Removing a live tile:** a tile that breaks these rules after approval is removed under the Wall terms, and its spot stays empty. Every removal is audited.

## The page

- **`/wall`:**
  - **Header:** a short line on what the Wall is, the rule that money here buys nothing else, and the real count, such as "412 of 1,000 seats taken". It also shows the current price stage and the next step ("Prices rise at 600").
  - **Overview:** the full grid, with taken tiles showing their pictures and free cells showing as open.
  - **Footer and About** link to it.
  - **It never appears** on home, the sidebar, Browse or search.
- **Zoom:**
  - Cells are too small to tap in the overview, so selecting part of the grid opens a zoomed section. There, each cell is at least 44 px on any screen.
  - The overview is 1,000 px wide at 1440 px. At 390 px, it fits the screen width, and zoom is how people pick and inspect tiles.
- **List view:** every tile in grid order, with its name, link and message. It's for keyboard and screen-reader users and for search engines. The overview's tiles are also focusable links.
- **`/wall/{n}`:** one tile's page.
  - It shows the tile, the owner's name, the date it was claimed and where it sits on the Wall.
  - It has a share card: an Open Graph image of the Wall with that tile marked. Each buyer gets something worth posting.
- **When it's full:** the page reads "The Wall is full." and stays up.
- **Design:** there's no mockup. It's built from [DESIGN.md](DESIGN.md), reusing the Territories grid's patterns, as `docs/design/README.md` says.
  - The header is the page's one `.frame`.
  - Square corners and theme tokens. The page uses the neutral theme, not a faction's.
  - The pull request includes 1440 px and 390 px screenshots.

**Copy:** the page talks about building S.V.E.R together and follows [COPY.md](COPY.md): no personal hardship story and no comparisons with other platforms. A suggested header line, tied to the Accord's third term: "Coin can't buy the light. It can help build the hall."

## How it's built

- **`wall_tiles` table:** owner, position and size, status (`held`, `in_review`, `live`, `rejected`, `refunded` or `removed`), price in cents and price stage, picture, name, link, message, crest choice, hold expiry, the Stripe session, and timestamps.
- **`wall_cells` table:** one row per occupied cell, with the cell's position as the primary key. A hold inserts all of its cells in one transaction, so two buyers can never get overlapping spots.
- **Hold expiry:** a job on the Postgres queue frees an unpaid hold after 15 minutes. A payment that arrives after the hold expired is refunded automatically if the spot was taken in the meantime. Otherwise, the hold is restored.
- **`/admin/wall`:** the list of tiles, refunds, removal, prices and limits before sales open, and the sales total. All actions are audited.
- **Emergency switch:** "Wall sales" in `/admin/switches` pauses new holds and checkouts. Live tiles stay up.
- **Open data:** Wall sales appear as their own line in the monthly money charts ([CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md#open-data)).

## Before sales open

These need Joe.
- **Wall terms**, added to the Terms of Service and reviewed by the lawyer. They cover:
  - what "for the life of the site" means
  - what happens if sver.tv shuts down
  - refunds
  - removal for broken rules
  - that a seat is advertising, not a donation, so it isn't tax-deductible
- **Tax:** confirm how Wall sales are treated for SVER LLC, and whether sales tax applies to digital ads in the states buyers are in. Turn on Stripe Tax if it does.
- **The prices and limits** above, confirmed.

## Done when

1. A verified adult can hold a free spot of any size, pay, and see the tile go live after approval. Under-18 accounts can't buy.
2. Two buyers can never get overlapping cells. Unpaid holds expire after 15 minutes, and a late payment for a taken spot is refunded automatically.
3. Prices follow the stage at checkout start, and the size limits hold.
4. Rejection, resubmission, refund before approval, edits with review, and removal all work and are audited.
5. Wall money changes nothing anywhere else on the site. Tests check that MAGNet ordering, the war, badges and profiles are identical for buyers and non-buyers.
6. The overview, zoom and list views work at 1440 px and 390 px, with 44 px targets in zoom and a screen-reader-usable list.
7. Each tile has a page and a share card, and the emergency switch pauses sales without hiding live tiles.
