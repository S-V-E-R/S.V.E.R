# Mobile app

The S.V.E.R app for iOS and Android (`apps/mobile`): a viewer app on the same Rust API as the website. Streaming, Creator Studio, the wallet and settings stay on sver.tv. Setup and push credentials are in `apps/mobile/README.md`.

Decided by Joe on October 21, 2026:

- **Expo (React Native, TypeScript)**, one codebase for iOS and Android, built with EAS. Bundle ID `tv.sver.app`. This is the only addition to the locked stack.
- **Viewer app first.** Going live from a phone waits for WHIP/SRT ingest (`docs/DEVELOPER_PLATFORM.md`).
- **No purchases in the app.** Subscriptions, Valor, tributes, supporters and Wall seats may be shown if the viewer already has them, but there are no buy buttons and no links to buy. Buying stays on sver.tv, which keeps the app clear of store billing and out of the money ledgers.

## How it talks to the API

- The same endpoints and session cookie as the website. The app keeps the cookies the API sets in the device secure store and sends them back. Writes and the chat socket send the site `Origin`; the API's origin check is browser CSRF protection, which a native app gains nothing from bypassing.
- Live playback asks for `?transport=hls` and uses the native player (AVPlayer, ExoPlayer). WebRTC stays web-only.
- The `/live/beat` heartbeat is sent only while media advances. Guests asked for Turnstile are told the view may not count; the app never invents a token.
- Signup (it needs Turnstile), password recovery and account deletion open sver.tv. The stores require a way to start deletion from the app.
- Go-live and existing push kinds reach the phone through Expo push, under the same switches as browser push (`docs/NOTIFICATIONS.md`).

## v1 screens

1. **Log in**: password, then the authenticator or recovery code. Links to create an account, recover a password and delete the account.
2. **MAGNet**: the live rotation in API order, the featured stream, search, and recently live channels when nothing is on.
3. **Watch**: player, title, category, viewers, mature warning, hosting, and chat (read for everyone, send when signed in). Below the player: the SVER Plays controller on Plays channels, and CrowdSync (`docs/CROWDSYNC.md`): the channel's board, polls, predictions, counters, Faction Rally and Surge, with board effects over the video timed to this phone's distance from live. Board presses and predictions spend earned Engagement Valor only. **Skills are not offered in the app**: they spend Purchased Valor, which the stores treat as an in-app purchase. Chat shows a Skill when someone plays one from the website. Owner and moderator tools (starting polls, resolving, counters, panic) stay on the website.
4. **Alerts**: the go-live push switch and this phone's registration.

## After v1

- Following list, Beacons feed, channel pages and follow from the app.
- Provider sign-in (Google, Twitch, Discord, Kick): an app-intent OAuth start whose callback returns `sver://auth?code=…` with a one-time code, exchanged by `POST /api/auth/app-exchange` for a session. Until then a provider account adds a password through Forgot password on the website. Apple requires Sign in with Apple once Google sign-in is offered in the app.
- Emotes, raids and the up-next countdown.

## Done when

An iOS and an Android build installed from EAS can: log in with a password and with 2FA, browse MAGNet and search, open a live stream and play it under 10 seconds behind live, count as a viewer when signed in, read and send chat, receive a go-live push for a followed channel and open it, and log out. No screen has a buy button or a link to buy. `corepack pnpm typecheck`, `lint`, `test` and `bundle` pass in `apps/mobile`.
