# S.V.E.R mobile

iOS and Android client for [sver.tv](https://sver.tv). It is a React Native app on Expo SDK 58, in this repo because the website is already React and TypeScript. Expo Go can run it without a store build. The API stays the Rust service; this app does not add endpoints.

## What it does

- **MAGNet.** `GET /api/discovery/live` draws every public live stream as a node, in the order the API returns. That order is the fair rotation. Viewer count is a label on the card. Nothing in the app sorts by viewers, followers or payment, and nothing offers a way to pay for a better spot. `GET /api/magnet/global` adds the featured stream and the one-line reason. An empty rotation shows `recent` from the same discovery response.
- **Watch.** `GET /api/channels/{username}/live?transport=hls` and the HLS address it returns, played by the platform player (AVPlayer on iOS, ExoPlayer on Android). A heartbeat goes to `POST /api/channels/{username}/live/beat` while playback is advancing, the same call the website uses. Mature streams ask before they play. A stream that only offers WebRTC shows that this build cannot play it.
- **Chat.** `GET` and `POST /api/channels/{username}/chat`, plus the existing socket at `/api/chat/ws`. Sending is a normal text message. Tributes, highlights and Skills are not offered.
- **Log in and log out.** `POST /api/auth/login`, `POST /api/auth/mfa/login`, `GET /api/auth/me` and `POST /api/auth/logout`. The session is the opaque cookie the API already sets (`sver_dev` in development, `__Host-sver` in production). The app stores it in the device secure store and sends it back. The screen follows the signed-in faction: Myria, Aetheron, Glint, or neutral steel when signed out. Colors and type are the tokens in `docs/DESIGN.md` (Cinzel, Barlow, Barlow Condensed). Faction names come from `apps/web/lib/factions.ts`.

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

`SVER_MOBILE_LIVE=1 corepack pnpm test` checks a live discovery card, its HLS field and its chat list against the configured API. The default run skips that and tests cookie handling, rotation order and the faction colors locally.

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
- Push notifications. Staff browser push exists for Take It Down. There is no device-token API for a phone, so this app does not pretend to register one.
- Going live from the phone. Ingest is RTMP from OBS. There is no mobile publish endpoint.
- Store release. No EAS project, signing keys or store listings. `/credits` on the website still lists the API and the website only; `scripts/credits.mjs` does not scan this folder yet.
- The word "streak" is not used here. On S.V.E.R it means a daily login streak only, and this app does not show one.

## Suggested next steps

1. **Push.** A device registration endpoint (token, platform, the existing notification preferences) and delivery through the same Postgres job queue. Do not reuse the staff Take It Down push credential for viewers.
2. **Going live.** WHIP or SRT from the phone, after the load test, into the current transmux path. RTMP from a phone app is a poor fit for the ingest already documented.
3. **Store release.** An EAS build, the Apple and Google listings, and a pass of `scripts/credits.mjs` so the mobile dependencies appear on `/credits`. OAuth can follow once a system browser can complete the existing callback and return to the app.
