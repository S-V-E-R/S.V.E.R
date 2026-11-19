# S.V.E.R mobile

iOS and Android client for [sver.tv](https://sver.tv). It is a React Native app on Expo SDK 58, in this repo because the website is already React and TypeScript. Expo Go can run everything except buying Valor without a store build. The API stays the Rust service. This app adds one phone registration (`POST` and `DELETE /api/me/push/expo`) on the notification system that already exists, and the App Store and Google Play purchase endpoints below.

## What it does

- **Navigation.** A bar along the bottom holds the main places, as on the website's phone layout (`docs/DESIGN.md` "Phones"): Home, Browse, Beacons, Following when signed in, and Log in or Account. Watch slides up over the bar and closes back to it. Log in opens as a sheet; Alerts and a category's live list open over the bar.
- **Home.** `GET /api/discovery/home` and `GET /api/magnet/global`. The featured stream comes first with its reason and what is up next in the rotation, then MAGNet's spotlights, the channels you follow that are live, and Live now: every public live stream, in the order the API returns. That order is the fair rotation, and the app never re-sorts it. Viewer count is a label on the card. Nothing in the app sorts by viewers, followers or payment, and nothing offers a way to pay for a better spot. The home feed applies the viewer's "only my languages" setting, so a signed-in viewer can see fewer streams here than a guest. With nothing live, Home shows recently live channels and a way to Beacons.
- **Browse.** `GET /api/discovery/browse`: each genre's categories as tiles, the ones with anything live first, otherwise in the API's order. A tile shows its live count only when it is above zero. The search field looks up channels and categories as you type. A category opens its live streams from `GET /api/discovery/live?category=`, in API order.
- **Following.** The channels you follow, live ones first, with a quiet Unfollow that asks first. Follows that come through a guild are undone on the guild.
- **Beacons.** The short-video feed. Only the Beacon on screen plays, and only while the Beacons tab is in front.
- **Watch.** `GET /api/channels/{username}/live?transport=hls` and the HLS address it returns, played by the platform player (AVPlayer on iOS, ExoPlayer on Android). Upright, the screen stacks the player, the streamer bar (name, title, category, watching count, Follow, Subscribe and report), a Play / Chat switch and the chosen pane. Play holds the Plays controller and CrowdSync, and opens first when the channel has them. Tapping the video shows its controls (mute, rotate, and chat when sideways); they hide again after a few seconds. The app is locked upright everywhere except Watch, which may turn sideways (`expo-screen-orientation`): the video then fills the screen and chat opens as a fixed-width panel on the right, without restarting the video. A heartbeat goes to `POST /api/channels/{username}/live/beat` while playback is advancing, the same call the website uses. Mature streams ask before they play. A stream that only offers WebRTC shows that this build cannot play it. An offline channel shows its bio and other channels that are live now.
- **Chat.** `GET` and `POST /api/channels/{username}/chat`, plus the existing socket at `/api/chat/ws`. Sending is a normal text message. Pinned messages and followers-only or subscriber-only notes show at the top. Scrolling up keeps your place and shows how many new messages arrived. Long-press a message to report or block its author. Under the box, Tribute adds 10 or more Valor to a message (`tribute` on the same `POST`), and the button becomes Pay tribute. Skills opens a sheet of the channel's Skills from `GET /api/channels/{username}/skills`; playing one asks first, then sends a chat message with `skill`. Both go over HTTP, never the socket, and keep one request id per attempt until the server answers, so a retry never pays twice. A message with a tribute shows its Valor and is highlighted. Highlights paid with Engagement Valor are not offered.
- **Log in and log out.** `POST /api/auth/login`, `POST /api/auth/mfa/login`, `GET /api/auth/me` and `POST /api/auth/logout`. The session is the opaque cookie the API already sets (`sver_dev` in development, `__Host-sver` in production). The app stores it in the device secure store and sends it back. Signed in, the Account tab shows your crest and side, Valor, Alerts, a link to settings on sver.tv, Delete account (which opens the website) and Log out. The screen follows the signed-in faction: Myria, Aetheron, Glint, or neutral steel when signed out. Colors and type are the tokens in `docs/DESIGN.md` (Cinzel, Barlow, Barlow Condensed). Faction names come from `apps/web/lib/factions.ts`.
- **Valor.** Account → Valor shows the balance from `GET /api/me/wallet`, the locked notice when the balance is below zero, and Buy Valor. Buy Valor lists the packs that both the server (`store.packs`) and the store return, with the store's own local price. Tapping one asks the server first (`POST /api/me/wallet/store {store, product, guardian_consent}`, which runs the minor checks and returns the account's store token), then opens the store's purchase sheet with that token. Each purchase goes to `POST /api/me/wallet/apple {transaction_id, environment}` or `POST /api/me/wallet/google {token}`, and the app tells the store the purchase is finished only when the server answers `credited` or `void`. Anything else leaves it unfinished, so the store hands it back on the next launch, and the app checks for such purchases each time it opens while signed in. In the US storefront only, the section adds the line "Bonus Valor packs are on sver.tv.", which opens the website's wallet; on Android it also needs the server's `store.google_us_link`. On Android this build checks that Google's external content links program is available and opens the link through Google Play, which shows Google's information screen first. It does not yet report purchases that come from the link: Google requires a fresh external transaction token for each visit (`createBillingProgramReportingDetailsAndroid`), carried through the website's checkout and reported to Google's Play Developer API. Until the server and website do that, keep `STORE_GOOGLE_US_LINK` off. Every "Get Valor" in the app opens the same Buy Valor sheet. The rules, prices and refunds are in `docs/SUPPORT.md` "Store packs".
- **Subscribe and gift.** The Subscribe button opens a sheet from `GET /api/channels/{username}/subscription`, shown only when `can_subscribe` is true and never on your own channel. It subscribes for one month with Valor (`POST /api/channels/{username}/subscription {tier, pay: "valor", id}`), cancels the renewal of a card subscription (`POST /api/channels/{username}/subscription/cancel`) and gifts subs with Valor (`POST /api/channels/{username}/gifts {tier, count, recipient?, pay: "valor", id}`). Each spend asks first. Card payments, upgrades and keeping a gifted subscription by card stay on the website.
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

## Buying Valor on a device

In-app purchases need a native build: `corepack pnpm exec expo run:ios --device` (a Mac with Xcode) or `corepack pnpm exec expo run:android --device`, or an EAS build. Expo Go and the sideloaded Android test APK can't buy: Expo Go says "Buying Valor needs the App Store or Google Play version of the app.", and the test APK can't reach Google Play, so its packs don't load. Spending Valor works in every build.

The API has to have the store keys set (`docs/SUPPORT.md` "Store packs", configuration). Without them the app shows that buying isn't available yet.

- **iOS:** create a sandbox tester in App Store Connect and sign in with it on the phone (on recent iOS, Settings → Developer → Sandbox Apple Account). Development and TestFlight builds buy in the sandbox, and nothing is charged.
- **Android:** add the tester's Google account as a license tester in Play Console, and install a build from the internal testing track. Google Play answers only for a build it knows, and a license tester is not charged.
- **Test accounts:** the production server credits sandbox and license-tester purchases only for the S.V.E.R accounts listed in `STORE_TEST_USERS`, up to `STORE_TEST_MONTHLY_VALOR` each month. Anyone else gets "Test purchases aren't accepted on this account." A development server accepts only test purchases.

The full setup, including App Store Connect and Play Console, is in `docs/STORE_SETUP.md`.

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
- Buying Valor by card, card subscriptions, upgrades, keeping a gifted subscription by card, platform supporter subscriptions, the gift setting and the list of your subscriptions. Those stay on the website.
- Store release. No EAS project, signing keys or store listings yet; `docs/STORE_SETUP.md` is the checklist. `/credits` on the website still lists the API and the website only; `scripts/credits.mjs` does not scan this folder yet.
- The word "streak" is not used here. On S.V.E.R it means a daily login streak only, and this app does not show one.

## Suggested next steps

1. **Going live.** WHIP or SRT from the phone, after the load test, into the current transmux path. RTMP from a phone app is a poor fit for the ingest already documented.
2. **Store release.** An EAS build, the Apple and Google listings, and a pass of `scripts/credits.mjs` so the mobile dependencies appear on `/credits`. OAuth can follow once a system browser can complete the existing callback and return to the app. The push credentials above are part of that build.
