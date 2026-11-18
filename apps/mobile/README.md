# S.V.E.R mobile

iOS and Android client for [sver.tv](https://sver.tv). It is a React Native app on Expo SDK 58, in this repo because the website is already React and TypeScript. Expo Go can run it without a store build. The API stays the Rust service. This app adds one phone registration (`POST` and `DELETE /api/me/push/expo`) on the notification system that already exists.

## What it does

- **Navigation.** A bar along the bottom holds the main places, as on the website's phone layout (`docs/DESIGN.md` "Phones"): Home, Browse, Beacons, Following when signed in, and Log in or Account. Watch slides up over the bar and closes back to it. Log in opens as a sheet; Alerts and a category's live list open over the bar.
- **Home.** `GET /api/discovery/home` and `GET /api/magnet/global`. The featured stream comes first with its reason and what is up next in the rotation, then MAGNet's spotlights, the channels you follow that are live, and Live now: every public live stream, in the order the API returns. That order is the fair rotation, and the app never re-sorts it. Viewer count is a label on the card. Nothing in the app sorts by viewers, followers or payment, and nothing offers a way to pay for a better spot. The home feed applies the viewer's "only my languages" setting, so a signed-in viewer can see fewer streams here than a guest. With nothing live, Home shows recently live channels and a way to Beacons.
- **Browse.** `GET /api/discovery/browse`: each genre's categories as tiles, the ones with anything live first, otherwise in the API's order. A tile shows its live count only when it is above zero. The search field looks up channels and categories as you type. A category opens its live streams from `GET /api/discovery/live?category=`, in API order.
- **Following.** The channels you follow, live ones first, with a quiet Unfollow that asks first. Follows that come through a guild are undone on the guild.
- **Beacons.** The short-video feed. Only the Beacon on screen plays, and only while the Beacons tab is in front.
- **Watch.** `GET /api/channels/{username}/live?transport=hls` and the HLS address it returns, played by the platform player (AVPlayer on iOS, ExoPlayer on Android). Upright, the screen stacks the player, the streamer bar (name, title, category, watching count, Follow and report), a Play / Chat switch and the chosen pane. Play holds the Plays controller and CrowdSync, and opens first when the channel has them. Tapping the video shows its controls (mute, rotate, and chat when sideways); they hide again after a few seconds. The app is locked upright everywhere except Watch, which may turn sideways (`expo-screen-orientation`): the video then fills the screen and chat opens as a fixed-width panel on the right, without restarting the video. A heartbeat goes to `POST /api/channels/{username}/live/beat` while playback is advancing, the same call the website uses. Mature streams ask before they play. A stream that only offers WebRTC shows that this build cannot play it. An offline channel shows its bio and other channels that are live now.
- **Chat.** `GET` and `POST /api/channels/{username}/chat`, plus the existing socket at `/api/chat/ws`. Sending is a normal text message. Pinned messages and followers-only or subscriber-only notes show at the top. Scrolling up keeps your place and shows how many new messages arrived. Long-press a message to report or block its author. Tributes, highlights and Skills are not offered.
- **Log in and log out.** `POST /api/auth/login`, `POST /api/auth/mfa/login`, `GET /api/auth/me` and `POST /api/auth/logout`. The session is the opaque cookie the API already sets (`sver_dev` in development, `__Host-sver` in production). The app stores it in the device secure store and sends it back. Signed in, the Account tab shows your crest and side, Alerts, a link to settings on sver.tv, Delete account (which opens the website) and Log out. The screen follows the signed-in faction: Myria, Aetheron, Glint, or neutral steel when signed out. Colors and type are the tokens in `docs/DESIGN.md` (Cinzel, Barlow, Barlow Condensed). Faction names come from `apps/web/lib/factions.ts`.
- **Alerts.** After sign-in the app asks to register this phone with `POST /api/me/push/expo`. The API then sends through Expo when a followed channel goes live, using the go-live push switch and the per-channel bell (`follows.alerts`). Guild, co-stream, direct message and light notifications use the per-type push switch that already exists. Turning push off on the website stops the phone as well. Log out removes this phone's token. Tapping a go-live alert opens that channel. Other alerts open the matching page on the website. The Alerts screen reads and writes `GET` and `PUT /api/me/notifications/settings`. Nothing in the push copy offers paid placement.

The website's design system forbids glows, so the chrome stays flat: square frames, beveled buttons, no blur, no rounded corners (the round Plays A and B buttons are the one exception). Home's Live now trail is a plain line with each streamer's faction diamond.

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

## Android test APK

Every push to `main` that touches `apps/mobile` (or a manual run of the **Android test build** workflow) builds a release APK and publishes it as a GitHub prerelease named `Android test build N`. A pull request that touches the app builds the same APK as a workflow artifact without publishing it. Testers download the `.apk` from the repo's Releases page on an Android phone, allow installs from the browser, and open it. Each build carries the run number as its `versionCode` and is signed with the Expo template's debug key, so a newer build installs over an older one. That key is fine for sideloading and is not a Play Store key. The APK covers arm64 phones and x86_64 emulators and talks to `https://sver.tv`. Push alerts stay off until the Expo project and FCM credentials below exist.

To build the same APK locally (JDK 21, Android SDK): `corepack pnpm exec expo prebuild --platform android`, then `./gradlew assembleRelease` in `android/`. On Windows, build from a short path (`subst S: <repo>`); the native C++ steps fail on long paths.

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
