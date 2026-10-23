# OBS

How S.V.E.R fits into OBS Studio without a desktop app of its own. Part 1 is built; part 2 is the plan for a native OBS plugin. Related: [DEVELOPER_PLATFORM.md](DEVELOPER_PLATFORM.md) §1 (device sign-in) and §7, [OVERLAYS.md](OVERLAYS.md), [CHANNEL_ADDITIONS.md](CHANNEL_ADDITIONS.md) "Pop-out chat".

## Part 1: no install (built October 22, 2026)

- **Studio dock, `/dock`.** Added in OBS as a Custom Browser Dock (View → Docks → Custom Browser Docks). It shows the on-air state (the same wording as Creator Studio, from `lib/obs.ts`), viewers, followers, uptime, incoming bitrate and the input-health warnings (codec, keyframe interval, B-frames, bitrate). It changes the title and category, marks a highlight while live, and sends a test alert to the open overlays. Everything goes through endpoints Studio already uses: `GET`/`PATCH /api/me/stream`, `GET /api/channels/{name}/overlay`, `POST /api/channels/{name}/marker` and `POST /api/me/overlays/test`. It refreshes every 5 seconds and pauses while the dock is hidden. The site's header and footer are left off, like pop-out chat.
- **Signing in from a dock.** `/dock` sends a signed-out visitor to `/login?next=/dock`; sign-in and the two-factor step go back to `next` when it's a path on this site (it must start with one `/`), otherwise home. Google sign-in may refuse to run inside OBS, so the dock instructions say to use email.
- **Chat dock** was already there: `/{name}/chat?dock=1`.
- **Scene collection download.** Creator Studio → Alerts & overlays, right after making a link (the only time the link is shown): a `SVER.json` for OBS's Scene Collection → Import, built in the browser (`sceneCollection` in `lib/obs.ts`). It's set for a 1920 × 1080 canvas: a "Live" scene with the "S.V.E.R overlays" scene nested on top, which holds the alert box (its sound goes through OBS to the stream and the streamer's monitor), goal bar, recent events (hidden) and the chat overlay. On Windows "Live" also starts with Game Capture of any fullscreen application. A display or window capture can't be preset, because OBS needs the device's id, so on Mac and Linux the streamer adds their own. The default desktop audio and microphone come with it (no desktop audio on Mac, where OBS has none by default). Loaded in OBS 32.2.2: both scenes, all four browser sources, the game hook and both audio devices came up with no errors. OBS's importer only needs `name` and `sources`.
- **OBS's service list.** Drafted, not yet sent: an entry in obs-studio's `plugins/rtmp-services/data/services.json`, so streamers pick "S.V.E.R" under Service instead of Custom, and OBS applies S.V.E.R's recommended settings itself: 1-second keyframes, B-frames off, at most 8 Mbps video and 160 Kbps audio, H.264 and AAC only, x264 `tune=zerolatency`. Opening that pull request is Joe's call. OBS's checker pings listed servers and removes ones that stop answering, so the ingest address must stay up.

## Part 2: native OBS plugin (plan, not started)

Only worth building for what a web page can't do. A plugin can't replace the SVER Desktop work: OBS owns capture and encoding, so encoder failover, the on-disk replay ring and capture without injection stay with the desktop app.

**What it adds**
1. **One-click setup.** "Connect S.V.E.R" in the plugin's menu starts device sign-in (`sver.tv/go`, §1), then fills Settings → Stream with the server and the stream key. With the service-list entry above, it picks the "S.V.E.R" service; until then, Custom.
2. **The docks built in.** The plugin adds `/dock` and chat as docks itself through obs-browser, so nobody pastes URLs.
3. **Highlight hotkey.** An OBS hotkey (bindable on a Stream Deck through OBS) that calls the marker endpoint. Later: send OBS's stream time so the marker lands exactly, not when the request arrives.
4. **Output health.** Dropped frames, congestion and the encoder OBS is using, sent every 10 seconds while live, so Studio and the dock can say "OBS is dropping frames" (the server only sees what arrives).

**Decision for Joe first.** The plan reserves `stream:key` for SVER Desktop only (§1), so one-click setup needs either:
- (a) **Recommended:** offer `stream:key` to first-party apps (SVER Desktop and this plugin) only. The `/go` approval for it asks for a fresh authenticator code, the same check as showing the key in Studio. The plugin calls a new `POST /api/v1/stream/key` to show the key (it never creates or replaces one), or
- (b) leave keys out: the plugin signs in for the docks, hotkey and health, and the streamer still pastes the key once.

**Build**
- C++17 on the official `obs-plugintemplate` (CMake, GitHub Actions builds for Windows, macOS and Ubuntu), against libobs and obs-frontend-api from OBS 31 or later, with Qt 6 from OBS. Talks to the API with Qt Network; no new dependencies.
- The refresh token goes in the operating system's credential store (Windows Credential Manager, macOS Keychain, libsecret on Linux), never in OBS's config files. The capture study found OBS's own sign-ins keep tokens there in plain text.
- Lives in `integrations/obs-plugin` with the other integrations; registered as an official app in Settings → Developer like the Stream Deck plugin.
- Tests: the sign-in and API client run against the fake server in `integrations/test` in CI; the OBS side is checked by hand on each OBS major version, because plugins break when OBS changes its interfaces.

**Steps**
1. Windows: sign-in, the built-in docks and setup (after option a or b is chosen). Server: `POST /api/v1/stream/key` with its scope, if (a).
2. Highlight hotkey and output health (a new `POST /api/v1/stream/health`, shown in Studio and the dock).
3. macOS and Linux builds, installers (Windows installer, macOS .pkg, Linux .deb and the Flatpak extension) and update notices.

**Costs:** a Windows code-signing certificate (Azure Trusted Signing is about $10 a month) and an Apple Developer account ($99 a year) to sign and notarize. Without signing, Windows SmartScreen and macOS Gatekeeper warn on install.
