# Mobile app

The S.V.E.R app for iOS and Android (`apps/mobile`): a viewer app on the same Rust API as the website. Streaming, Creator Studio and settings stay on sver.tv. Setup and push credentials are in `apps/mobile/README.md`.

Decided by Joe on October 21, 2026:

- **Expo (React Native, TypeScript)**, one codebase for iOS and Android, built with EAS. Bundle ID `tv.sver.app`. This is the only addition to the locked stack.
- **Viewer app first.** Going live from a phone waits for WHIP/SRT ingest (`docs/DEVELOPER_PLATFORM.md`).

On November 18, 2026 Joe replaced the October rule "no purchases in the app" with the section below.

## Decided by Joe on November 18, 2026: full payments in the app

- **Buying Valor.** The app sells Valor packs through the App Store and Google Play, at the same prices as the website and at a flat 100 Valor per 99¢: no bonus Valor and no upcharge. Bonus Valor stays a website perk. Packs, prices and economics are in `docs/SUPPORT.md` "Store packs".
- **Spending Valor.** Tributes, Skills, Valor subscription months and gift subs work in the app, paid with Purchased Valor. Cards are never charged from the app: subscribing, upgrading or keeping a gifted subscription by card, and platform supporter subscriptions, stay on sver.tv. So do the gift setting and the list of your subscriptions.
- **One link to sver.tv, United States only.** In the US storefront the Buy Valor section has one quiet line, "Bonus Valor packs are on sver.tv.", which opens the sver.tv wallet in the browser. The app reads the storefront from the store (the App Store storefront `USA`, the Google Play country `US`) and never shows the line anywhere else. On Android the line also needs the server switch `STORE_GOOGLE_US_LINK` (default off), which is turned on only after Joe enrolls in Google's US program for external links and the app makes the calls that program requires.
- **Storefronts at launch.** Packs go on sale first in the US and in storefronts with no VAT or low tax. Joe picks the list in App Store Connect and Play Console (`docs/STORE_SETUP.md`); nothing in the code chooses it. Where the store returns no packs, the app says the packs couldn't load and offers Try again.
- **Minors.** The same $50 a month cap and parent or guardian consent as the website, shared with card purchases. The app asks the server before it opens the store's purchase sheet. A purchase that still lands over the cap (for example Ask to Buy approved days later) has already been paid, so it is credited and flagged for staff, never dropped.

How it works:

- **The store's own server API is the only authority.** The app sends the server a transaction id (Apple) or purchase token (Google). The server asks Apple's or Google's API for that purchase with S.V.E.R's own credentials and credits only what the store reports as paid. It never trusts what the phone or a notification says. Notifications from the stores only tell the server to look again.
- **One idempotent credit.** Every claim, notification and refund check goes through one function, under a lock, keyed on the store's purchase id. A replayed or doubled claim credits once. Credits and refunds post to the existing double-entry ledger.
- **The purchase finishes only after the credit.** The app tells the store a purchase is done only once the server has credited it (or found nothing to credit). Anything else leaves it unfinished, so the store hands it back on the next launch.

## How it talks to the API

- The same endpoints and session cookie as the website. The app keeps the cookies the API sets in the device secure store and sends them back. Writes and the chat socket send the site `Origin`; the API's origin check is browser CSRF protection, which a native app gains nothing from bypassing.
- Live playback asks for `?transport=hls` and uses the native player (AVPlayer, ExoPlayer). WebRTC stays web-only.
- The `/live/beat` heartbeat is sent only while media advances. Guests asked for Turnstile are told the view may not count; the app never invents a token.
- Signup (it needs Turnstile), password recovery and account deletion open sver.tv. The stores require a way to start deletion from the app.
- Go-live and existing push kinds reach the phone through Expo push, under the same switches as browser push (`docs/NOTIFICATIONS.md`).

## v1 screens

1. **Log in**: password, then the authenticator or recovery code. Links to create an account, recover a password and delete the account.
2. **MAGNet**: the live rotation in API order, the featured stream, search, and recently live channels when nothing is on.
3. **Watch**: player, title, category, viewers, mature warning, hosting, and chat (read for everyone, send when signed in). Below the player: the SVER Plays controller on Plays channels, and CrowdSync (`docs/CROWDSYNC.md`): the channel's board, polls, predictions, counters, Faction Rally and Surge, with board effects over the video timed to this phone's distance from live. Board presses and predictions spend earned Engagement Valor only. Skills are played from the chat's Skills sheet with Purchased Valor (November 18, 2026, above). Owner and moderator tools (starting polls, resolving, counters, panic) stay on the website.
4. **Alerts**: the go-live push switch and this phone's registration.

## Screens now (November 2026 redesign)

The tab bar holds Home, Browse, Beacons, Following (signed in) and Log in or Account. `apps/mobile/README.md` lists the endpoints each one reads.

- **Home**: the featured stream with its reason and what is up next, MAGNet's spotlights, live channels you follow, and Live now in API order. It reads `/api/discovery/home`, which applies the viewer's language setting.
- **Browse**: genres of category tiles, live ones first, plus search for channels and categories. A category opens its live streams.
- **Beacons**: the short-video feed, with like, follow and report.
- **Following**: followed channels, live first, with Unfollow.
- **Watch**: player, streamer bar (with Subscribe), a Play / Chat switch and the chosen pane (chat has Tribute and Skills), the order in `docs/DESIGN.md` "Phones". The app is upright everywhere else; Watch may turn sideways, where chat becomes a panel beside the video.
- **Account**: crest and side, Valor (the balance and Buy Valor), Alerts, settings and account deletion on sver.tv, and Log out.

## After v1

- Channel pages.
- Provider sign-in (Google, Twitch, Discord, Kick): an app-intent OAuth start whose callback returns `sver://auth?code=…` with a one-time code, exchanged by `POST /api/auth/app-exchange` for a session. Until then a provider account adds a password through Forgot password on the website. Apple requires Sign in with Apple once Google sign-in is offered in the app.
- Emotes, raids and the up-next countdown.

## Done when

An iOS and an Android build installed from EAS can: log in with a password and with 2FA, browse MAGNet and search, open a live stream and play it under 10 seconds behind live, count as a viewer when signed in, read and send chat, receive a go-live push for a followed channel and open it, and log out. Valor packs are bought through the App Store and Google Play only; credits happen only after the server verifies them with the store. `corepack pnpm typecheck`, `lint`, `test` and `bundle` pass in `apps/mobile`.
