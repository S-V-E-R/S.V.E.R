# Channel and transparency additions

Approved by Joe on October 6, 2026, after a review of Glimesh's open-source code (github.com/Glimesh/glimesh.tv, MIT licensed, shut down in July 2023). Glimesh had these and S.V.E.R didn't. Everything else it built, S.V.E.R already covers (fair homepage, hosting and auto-host, chat gates, emote review, gift opt-out, data export, OBS health). Pronouns were considered and not taken.

**When:** right after VODs and clips (Module 8) closes, before Beacons (Module 9). The mature label comes first, because VODs, clips and Beacons all inherit it. The credits page can ship sooner, as its own small change, because it is a license obligation.

| Addition | What it is |
| --- | --- |
| [Mature label](#mature-label) | A stream label for content not suited to younger viewers, with a warning screen, and no access for under-18 accounts |
| [Stream language](#stream-language) | The language a stream is in, with a language filter in Browse |
| [Pop-out chat](#pop-out-chat) | Chat on its own page, for a second window, an OBS dock or an on-stream overlay |
| [Channel editors](#channel-editors) | People who can change a stream's title, category and labels without being moderators |
| [Keep a gifted subscription](#keep-a-gifted-subscription) | Turning a gifted month into a paid subscription that starts when the gift ends |
| [Signature emote](#signature-emote) | One approved emote per channel that works in every chat |
| [Open data](#open-data) | A public page of weekly platform numbers, including how money is split |
| [Credits](#credits) | The open-source software S.V.E.R uses, with its licenses |

## Mature label

The Community Guidelines (`/guidelines`) already ban sexual content, graphic real-world violence and gore. The label covers what is allowed but not suited to younger viewers: violent or horror games, strong language, mature themes. Accounts can be 13 or older, so this protects under-18 viewers.

**Setting it**
- Creator Studio → Stream details has a **Mature** switch, with a one-line explanation of what it's for and what it doesn't permit.
- It is saved on the channel, so it stays on for the next stream until it's turned off. It can be changed while live; the change takes effect within seconds for current viewers.
- When the chosen game's catalog entry has a mature age rating (ESRB M or AO, PEGI 18, from the existing Wikidata catalog worker), Studio switches the label on when that category is picked and says why. The streamer can turn it off.
- The broadcast records whether it was labeled at any point. VODs, highlights and clips from a labeled broadcast are labeled too. Beacons have their own Mature switch when that module is built.

**What viewers see**
- **Under 18:** a labeled stream, VOD or clip doesn't play. The player is replaced by a short panel ("This stream is labeled mature, so it isn't available on your account") with Up next. Labeled streams are left out of every list for them: home, Browse, search, the sidebar, Up next and MAGNet lanes. Chat is closed to them too.
- **Adults:** the first time they open a labeled channel, a framed warning covers the player with **Watch** and **Go back**. The choice is remembered for that channel for 30 days. Settings → Preferences has "Don't warn me about mature streams" (default off).
- **Signed out:** the same warning, remembered for the browser session only.
- Labeled stream cards carry a **Mature** tag next to LIVE (dark background, `--ink` text). Their still image shows normally to adults and signed-out visitors.
- Embeds show the warning first, the same way.

**Fairness:** MAGNet's rotation doesn't change. Labeled streams are removed after ordering, for under-18 viewers only, the same way blocks are applied.

**Raids, hosts and alerts**
- A raid or host into a labeled channel doesn't move under-18 viewers; they get Up next instead. The raid confirmation tells the raider the target is labeled mature.
- Under-18 followers don't get go-live alerts for a labeled broadcast.
- A channel run by an under-18 account can use the label; that only limits who can watch.

**Enforcement**
- A new report reason: **Should be labeled mature**.
- Staff can switch the label on for a broadcast and lock it until that broadcast ends (Admin → Live streams; audited).
- Repeatedly leaving the label off after staff lock it follows the existing strike rules. Content the guidelines ban is still removed whether or not it's labeled.

**Built so far (October 8, 2026, migration 0054):** the Studio switch (saved on the channel; a broadcast labeled at any point stays labeled, and its VOD and clips inherit it), `/live` refusing playback to signed-in under-18 accounts (with Up next), chat history, socket and posting closed to them, the warning screen before anything plays for adults (30 days per channel) and signed-out visitors (the session), and the Mature tag in the player. Then (same day): labeled live streams leave home, the live list, Up next, search, the following list and a MAGNet lane's featured slot for under-18 accounts, after ordering (`streams::mature_hidden`); cards carry the Mature tag; raids leave under-18 viewers behind and tell the raider; under-18 followers get no go-live alert; a host's under-18 viewers get the blocked panel. Then the catalog rating (the Wikidata worker stores ESRB M/AO and PEGI 18 as `game_catalog.mature`; picking such a game in Studio switches the label on and says why; migration 0055), the **Should be labeled mature** report reason, and the staff lock (Admin → Live streams, `POST /api/admin/streams/{id}/mature`, audited as `lock_mature`; the owner can't switch it off until that broadcast ends). Settings → Preferences has "Don't warn me about mature streams" for adults (migration 0056). Embeds exist only for clips, which already have their own 18+ confirmation, so the live embed warning waits until a live embed is built.

**Done when**
1. An under-18 account can't play, find or chat in a labeled stream, VOD or clip, through any list, link, embed, raid or host.
2. Adults and signed-out visitors see the warning once per channel (30 days, or the session for guests), and the preference turns it off.
3. A mature catalog rating switches the label on in Studio, and VODs and clips inherit it.
4. MAGNet rotation tests still pass, with the under-18 filter applied after ordering.
5. The report reason and staff lock work and are audited.

## Stream language

- Studio → Stream details has a required **Language** field. It defaults to the browser's language, from a list of about 40 languages by ISO 639-1 code, plus "Other". It is saved on the channel.
- Stream cards show the language as a small chip, but only when it isn't one of the viewer's languages.
- Settings → Preferences → **Languages I watch in**: several can be picked. It defaults to the browser's languages.
- Browse and category pages get a language filter next to the faction filter: All, My languages, or one language. Home shows all languages unless the viewer turns on "Only show streams in my languages" in the same settings.
- Filters only narrow a list. The order is still MAGNet's rotation.
- `GET /api/discovery/live` accepts `language=`, and cards include `language`.
- The site itself stays in English. Translating the interface is a later phase.

**Done when:** language is saved and shown, the filters and the home setting narrow lists without reordering them, and the API returns and filters by language.

**Built October 9, 2026 (migration 0057).** Studio's Language field (40 languages and Other; the browser's language by default), cards with a language chip when it isn't one of the viewer's, Settings → Preferences → Languages I watch in (up to 10; the browser's until chosen) and "Only show streams in my languages" for home, and Browse's All / My languages filter. `GET /api/discovery/live` takes `language=` as a code, a comma list or `mine` (with `fallback=` for the browser's languages), and cards include `language`. Streams without a language yet are left out only when a language filter is on.

## Pop-out chat

- `sver.tv/{username}/chat` shows only the channel's chat: no top bar, sidebar or footer, in the owner's faction theme. It works from 280 px wide, so it fits a narrow window.
- Same chat, same rules. Signed-out visitors can read; signing in works as usual. Moderators keep their actions.
- A **Pop out** button in the chat header opens it in a 400 × 700 window.
- `?dock=1` is a compact version for an OBS custom browser dock: no header, smaller padding. Creator Studio → Chat explains how to add it in OBS (View → Docks → Custom Browser Docks). Email sign-in works inside the dock. Google may refuse provider sign-in in an embedded browser, and Studio says so.
- `?overlay=1` is a read-only version for an OBS browser source: transparent background, no input box, messages fade after 30 seconds (an owner setting, 10 to 120 seconds). It shows only what chat already shows publicly. It honors deletions and timeouts.
- The page can't be framed by other sites, like the rest of S.V.E.R (`X-Frame-Options: DENY`); OBS loads it as a top-level page.

**Done when:** the pop-out, dock and overlay pages work at 280 px and in OBS, moderation actions work in the pop-out, and the overlay removes deleted messages.

## Channel editors

- An owner can appoint up to **5** editors, with step-up authentication, the same way as moderators. Editors must have a verified account in good standing.
- Editors can change the title, category and language, and switch the Mature label **on**. Only the owner can turn it off.
- Editors can't see the stream key, start or stop the stream, see money, appoint people or change channel settings. Someone can be both a moderator and an editor.
- Editors edit from an **Edit stream info** button on the watch page, and the player menu lists "Channels you edit".
- Edits use the same revision check (a stale edit gets 409) and record who made them; Studio shows "Edited by @name".
- Removing an editor takes effect on their next action.

**Done when:** an editor can make exactly those changes and nothing else, every edit is attributed, and removal is immediate.

## Keep a gifted subscription

- A viewer with an active gifted month sees **Keep your subscription** on the channel and on `/wallet`.
- They choose a tier and pay by card through Stripe Checkout. The first charge is on the day the gift ends, then monthly. Nothing is charged twice for the same days, and the badge's month count carries on.
- Cancelling before the gift ends means nothing is charged.
- A notification three days before a gifted month ends offers the same thing (in-site only; added to the [NOTIFICATIONS.md](NOTIFICATIONS.md) catalog).
- The under-18 guardian confirmation and the $50 monthly cap from [SUPPORT.md](SUPPORT.md) apply.

**Done when:** a gifted viewer converts with the first charge at the gift's end, cancels before it with no charge, and the ledger and badge stay correct.

**Built October 9, 2026 (migration 0060).** A card subscription started while a gifted or Valor month is running becomes a Stripe trial ending when that month ends (Checkout says nothing is charged today and when the first charge is), so cancelling before then charges nothing. The $0 trial invoice turns on renewal without adding a month; the first paid invoice adds the next. Stripe needs a trial to end at least 48 hours out, so a month ending sooner gets up to two extra free days. The reminder is an in-site notification three days before (`sub_ending`, once per ending month). `/wallet` lists active subscriptions with Keep your subscription for non-card ones.

## Signature emote

- Each channel can pick one of its open emotes (not a subscriber emote) as its **signature emote**.
- Once staff approve it in the existing emote review (`/admin/media`), anyone can use it in any channel's chat, written as `username/Code` (for example `kilnworks/Walnut`). It shows the owner's crest on hover.
- The emote picker has a **Signature** tab listing the signature emotes of channels the viewer follows.
- Channel owners can turn off other channels' signature emotes in their own chat (Studio → Chat; default allowed).
- Reports, removal, banned words and strikes work as for other emotes. Removing the emote, or rejecting it in review, ends cross-channel use immediately.

**Done when:** an approved signature emote renders in other channels only in the `username/Code` form, the owner's opt-out works, and removal stops it everywhere.

## Open data

A public page at `/open-data`, linked from About and the footer, showing how S.V.E.R is doing and where the money goes.

**Weekly charts, from launch:**
- accounts (total and new)
- channels that streamed, and hours streamed
- the share of live streams that reached MAGNet's top row within their cycle (the fairness promise, measured)
- members per faction

**Monthly charts:**
- money from subscriptions, gift subs and tributes, split into the creators' share and S.V.E.R's share
- payouts sent

**Privacy rules:** no per-person or per-channel numbers. A weekly figure below 10 is shown as "fewer than 10". If fewer than 10 creators were paid in a month, that month's money is combined with the next.

**How it's built:**
- A nightly job writes the figures to a `public_stats` table.
- The page reads only that table and is cached.
- Each chart has a CSV download and a one-line note on how it's counted.
- Charts use the theme tokens.

**Done when:** the page shows the charts from real data, the thresholds hold, and the CSVs match the charts.

## Credits

- `/credits`, linked from the footer and About, lists the open-source software S.V.E.R uses: the Rust crates and npm packages that ship, plus SRS, PostgreSQL and the Cinzel and Barlow fonts (SIL Open Font License). Each entry has its name, version, license and a link. The full license texts are on the same page.
- It also says S.V.E.R itself is open source under the AGPL-3.0, with a link to the source.
- `scripts/credits.mjs` builds the list from `cargo metadata` and `pnpm licenses list --json` into `apps/web/app/credits/credits.json`. CI fails if a dependency changed without regenerating it.
- If code from Glimesh, Mixer or another MIT project is reused, it is listed here with its copyright notice.

**Done when:** every shipped dependency appears with its license text, and CI catches a stale list.

**Built October 8, 2026.** 371 shipped packages (Rust crates reachable from the API on Linux through normal dependencies, and the website's production npm packages; per-platform native build tools are left out so the list is the same on every machine) plus SRS, PostgreSQL, Cinzel and Barlow. Where a crate offers a choice that includes MIT, S.V.E.R uses MIT and keeps only that notice; identical texts are stored once. The vendored SRS, PostgreSQL and font license texts are in `scripts/credits/`. CI runs `node scripts/credits.mjs --check` in the Web job.

## Considered and not taken

- **Pronouns** on profiles (Joe, October 6).
- **Holding new channels off the homepage** until they have 10 hours of streaming and have been live for 15 minutes. Glimesh did this; S.V.E.R gives new creators a head start instead.
- **A paid platform supporter subscription** with cosmetic perks. Support (Module 6) is already the funding model.
- **Country-based edge selection.** The CDN already routes viewers to the nearest location.
