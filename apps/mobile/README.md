# S.V.E.R mobile

iOS and Android client for [sver.tv](https://sver.tv). It is a React Native app on Expo SDK 58, in this repo because the website is already React and TypeScript. Expo Go can run it without a store build. The API stays the Rust service. This app adds one phone registration (`POST` and `DELETE /api/me/push/expo`) on the notification system that already exists.

## What it does

- **MAGNet.** `GET /api/discovery/live` draws every public live stream as a node, in the order the API returns. That order is the fair rotation. Viewer count is a label on the card. Nothing in the app sorts by viewers, followers or payment, and nothing offers a way to pay for a better spot. `GET /api/magnet/global` adds the featured stream and the one-line reason. An empty rotation shows `recent` from the same discovery response.
- **Watch.** `GET /api/channels/{username}/live?transport=hls` and the HLS address it returns, played by the platform player (AVPlayer on iOS, ExoPlayer on Android). A heartbeat goes to `POST /api/channels/{username}/live/beat` while playback is advancing, the same call the website uses. Mature streams ask before they play. A stream that only offers WebRTC shows that this build cannot play it.
- **Chat.** `GET` and `POST /api/channels/{username}/chat`, plus the existing socket at `/api/chat/ws`. Sending is a normal text message. Tributes, highlights and Skills are not offered.
- **Log in and log out.** `POST /api/auth/login`, `POST /api/auth/mfa/login`, `GET /api/auth/me` and `POST /api/auth/logout`. The session is the opaque cookie the API already sets (`sver_dev` in development, `__Host-sver` in production). The app stores it in the device secure store and sends it back. The screen follows the signed-in faction: Myria, Aetheron, Glint, or neutral steel when signed out. Colors and type are the tokens in `docs/DESIGN.md` (Cinzel, Barlow, Barlow Condensed). Faction names come from `apps/web/lib/factions.ts`.
- **Alerts.** After sign-in the app asks to register this phone with `POST /api/me/push/expo`. The API then sends through Expo when a followed channel goes live, using the go-live push switch and the per-channel bell (`follows.alerts`). Guild, co-stream, direct message and light notifications use the per-type push switch that already exists. Turning push off on the website stops the phone as well. Log out removes this phone's token. Tapping a go-live alert opens that channel. Other alerts open the matching page on the website. The Alerts screen reads and writes `GET` and `PUT /api/me/notifications/settings`. Nothing in the push copy offers paid placement.

The website's design system forbids glows, so the rest of the chrome stays flat: square frames, beveled buttons, no blur. The MAGNet trail is the exception asked for on this screen: a static halo in the streamer's faction color, not an animated blur.

## Run it

From `apps/mobile`, with Node 22.13+ or 24.3+ and pnpm through corepack:

```powershell
corepack pnpm install
corepack pnpm start
```

Install Expo Go on a phone, join the same network, and scan the QR code. That is the fastest way onto a device. `corepack pnpm android` and `corepack pnpm ios` open a simulator when one is installed (an Android emulator, or Xcode on a Mac). This environment bundled the JavaScript for both platforms (`corepack pnpm bundle`) and did not boot a simulator.

The default API is `https://sver.tv`. Against a local API from `./scripts/dev.ps1`:

```powershell
$env:EXPO_PUBLIC_API_URL = "http://127.0.0.1:8080"
$env:EXPO_PUBLIC_APP_ORIGIN = "http://localhost:3000"
$env:EXPO_PUBLIC_WEB_ORIGIN = "http://localhost:3000"
corepack pnpm start
```

`EXPO_PUBLIC_APP_ORIGIN` must be the API's `APP_ORIGIN`. Mutations and the chat socket are refused when it does not match. A simulator on the same machine can use `127.0.0.1`. An Android emulator uses `http://10.0.2.2:8080` as the API URL and still sends `Origin: http://localhost:3000`. A phone uses your computer's LAN address as the API URL. A standalone Android build (not Expo Go) allows cleartext only when `EXPO_PUBLIC_API_URL` is `http://` at prebuild time.

`SVER_MOBILE_LIVE=1 corepack pnpm test` checks a live discovery card, its HLS field and its chat list against the configured API. The default run skips that and tests cookie handling, rotation order, the faction colors and the notification path rules locally.

## Push on a device

Remote push needs an Expo project and, outside Expo Go on iOS, a development build. This environment bundled the JavaScript and did not deliver a notification to a phone.

1. Create an Expo account and, from `apps/mobile`, run `npx eas-cli init` (or `npx eas init`). That creates the EAS project. Copy its project id (a UUID).
2. For Expo Go and local runs, set `EXPO_PUBLIC_EAS_PROJECT_ID` to that UUID before `corepack pnpm start`. An EAS build also receives the id on its own.
3. On the API host, set `EXPO_ACCESS_TOKEN` to an access token from [expo.dev](https://expo.dev) → Account settings → Access tokens, for the account that owns the project. Leave it out of git. Until it is set, the API stores tokens and does not send. Queued go-live alerts are dropped when the stream ends or after an hour.
4. **FCM (Android development or store builds).** Create a Firebase project, add an Android app with package `tv.sver.app`, and download a Google service account key with the Firebase Cloud Messaging API enabled (FCM V1). Upload it with `npx eas-cli credentials` → Android → Push Notifications. Do not commit `google-services.json`.
5. **APNs (iOS development or store builds).** A paid Apple Developer account is required. Register the test device, then `npx eas-cli credentials` → iOS → Push Notifications, and let EAS create an APNs key. The app id is `tv.sver.app`.
6. **Expo Go.** From SDK 53, Android Expo Go does not receive remote push. iOS Expo Go can, once the project id is set. For Android, make a development build: `npx eas-cli build --profile development --platform android`, install it, and point `EXPO_PUBLIC_API_URL` at the API.
7. Sign in on the phone, allow notifications, and open Alerts. It should say this phone is registered. Follow a channel with the bell on, leave the go-live push switch on, and have that channel go live. The phone should show "{name} is live" and open the channel when tapped. A channel you muted, or a push switch you turned off, does not send.

Staff browser push (`VAPID_*`) is a different system and is not used for this app.

## Checks

```powershell
corepack pnpm typecheck
corepack pnpm lint
corepack pnpm test
corepack pnpm bundle
```

`bundle` writes the iOS and Android JavaScript bundles under `dist/` (gitignored). It does not produce an App Store or Play Store binary.

## Not in this build

- Google, Twitch, Discord and Kick sign-in. Those are browser redirects; the password and authenticator paths are the ones the API exposes to a client that can store a cookie.
- A Turnstile widget. If a playback heartbeat returns `needs_turnstile`, the app says so and does not invent a token. The website can finish that check.
- WebRTC / WHEP playback, emote images, GIFs, raids, the up-next countdown, and MAGNet's own moving player. Chat text and the channel HLS stream are the reusable parts.
- Going live from the phone. Ingest is RTMP from OBS. There is no mobile publish endpoint.
- Store release. No EAS project, signing keys or store listings. `/credits` on the website still lists the API and the website only; `scripts/credits.mjs` does not scan this folder yet.
- The word "streak" is not used here. On S.V.E.R it means a daily login streak only, and this app does not show one.

## Suggested next steps

1. **Going live.** WHIP or SRT from the phone, after the load test, into the current transmux path. RTMP from a phone app is a poor fit for the ingest already documented.
2. **Store release.** An EAS build, the Apple and Google listings, and a pass of `scripts/credits.mjs` so the mobile dependencies appear on `/credits`. OAuth can follow once a system browser can complete the existing callback and return to the app. The push credentials above are part of that build.
