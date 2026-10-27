# SVER Desktop

A separate product (`PLATFORM_PLAN.md`, "SVER Desktop and the Rust media engine") that talks to the platform through the public API only ([DEVELOPER_PLATFORM.md](DEVELOPER_PLATFORM.md)). It's what OBS can't be: one app that owns capture and encoding, so it can do encoder failover, an on-disk replay buffer and capture without injection ([OBS.md](OBS.md) part 2). Written October 27, 2026 with **Proposed** defaults, from a static read of TikTok LIVE Studio 1.38.0 for Windows (October 26, 2026). Joe can change any of them.

TikTok LIVE Studio is proprietary and S.V.E.R is AGPL-3.0. This spec takes ideas only: none of TikTok's code, DLLs, models, templates, assets, protocols or endpoints come into SVER.

## What a streamer gets

- **One-click go-live.** Sign in once with the code at `sver.tv/go`. The app gets the stream credential itself, so nobody pastes a stream key into anything. Title and category use the existing revision check: publishing is refused until both are set ([LIVE_STREAMS.md](LIVE_STREAMS.md)).
- **Sources:** display, window, webcam, capture card, microphone, per-app audio, media files, text, and browser sources (SVER overlays work as they do in OBS).
- **Scenes** with a fade transition, a preview, and settings checked before going live.
- **Encoders picked for you.** The app tests upload speed and the hardware encoders, then applies SVER's baseline: H.264, B-frames off, 1-second keyframes, 6 Mbps. If an encoder fails it falls back to another one with the same codec.
- **Three ways out:** WHIP (lowest latency), RTMP (the default), SRT (bad networks). All three already land in SRS under the same publish rules.
- **Chat, alerts and the board** docked in the app from the events socket.
- **Local recording.**

Later parts add the vertical canvas, instant replay, effects, the audio chain and on-device AI (below).

## Rules

- **Public API only.** The app signs in with the device flow and asks for `stream:key` ([DEVELOPER_PLATFORM.md](DEVELOPER_PLATFORM.md) §1). The publish hook doesn't change: SRS treats the app like any other encoder.
- **The server never decodes video.** Everything that looks at pictures or sound runs on the streamer's PC, and only small events leave it, never frames.
- **No surveillance.** No process lists, keyboard or mouse monitoring, or screenshots sent anywhere. Anything that reports a signal about the stream is opt-in and says so in the app.
- **Sandboxed.** Every webview runs under Tauri's capability system: no Node or file access from web content, and the sandbox is never turned off. TikTok's app runs most windows with Node enabled and isolation off, and turns the sandbox off for the whole app; SVER doesn't.
- **Design** follows [DESIGN.md](DESIGN.md) (tokens, fonts, motion).

## How it's built

TikTok's app is three programs behind one window: an Electron UI, a media engine process and an AI process, talking over named pipes, with video handed to the preview as GPU textures that never pass through JavaScript. SVER Desktop keeps that shape and swaps each part for an open one that fits SVER's Rust stack.

```
SVER Desktop (Tauri 2, Rust core) ── device sign-in at sver.tv/go → token with stream:key
  ├─ UI: React views shared with apps/web (Studio panels, chat, board), one webview per floating tool
  ├─ core: typed commands over a local pipe; one state store, mirrored into each view
  ├─ media engine process (Rust, GStreamer through gstreamer-rs)
  │     d3d11screencapturesrc · mfvideosrc · wasapi2src (with per-app loopback)
  │     d3d11compositor (scenes, transitions)
  │     nvh264enc / amfh264enc / qsvh264enc / x264enc (fallback, same codec)
  │     whipclientsink · rtmp2sink · srtsink · mp4mux (recording)
  │     preview: native D3D11 surface under a transparent webview
  └─ AI process (Rust, part 4): ONNX Runtime, whisper.cpp or sherpa-onnx, optional llama.cpp
        reads frames from the engine's shared texture; sends events only
           │ RTMP / WHIP / SRT                    ▲ wss://sver.tv/api/events
           ▼                                      │
        SRS (existing) → publish hook          MAGNet ◄── opt-in moment reports (part 4)
```

- **GStreamer** matches the planned Rust media engine and is LGPL, which works with AGPL. It has screen capture, D3D11 composition, NVENC, AMF and QSV, and WHIP and SRT sinks.
- **libobs** is the fallback if injected game capture is required at launch: GStreamer has no equivalent. It's GPL-2.0-or-later, so combining it with AGPL-3.0 needs a check with counsel first.
- **Tauri** keeps the app small and in Rust. It has no shared-texture preview like Electron's, so the preview is a native surface placed under the webview. If that proves unworkable, Electron is the alternative, with the sandbox on.
- **Crashes stay contained.** If the engine or AI process dies, the UI stays up, says so, and restarts it. A crash of the AI process never stops the stream.

## Platform changes (part 0)

Small API pieces the app needs, built in the main repo before part 1:

- **`stream:key` for first-party apps.** Waits on the decision in [OBS.md](OBS.md) part 2: the recommended option offers the scope to SVER Desktop and the OBS plugin only, with a fresh authenticator code at `/go`, and `POST /api/v1/stream/key` to read the key without creating or replacing one. SVER Desktop uses the same endpoint.
- **A `session` topic** on the events socket that pushes the stream's live state and the server-side input health (codec, keyframe interval, B-frames, bitrate), so the app doesn't poll.
- **The MAGNet input** for app-detected moments (part 4), specified here and built when part 4 starts.

## Part 1: go live

- Tauri shell, device sign-in, the stream credential, go live over WHIP or RTMP.
- Display, window, webcam and microphone sources; scenes with a fade; encoder test and fallback; local recording.
- Chat and alerts docked from the events socket; SVER overlays as browser sources.
- Input-health warnings from the `session` topic, in the same wording as Creator Studio and the OBS dock.

## Part 2: produce

- **Vertical canvas.** A second 1080 × 1920 branch of the compositor. It records locally, and clips upload as Beacons that lead back to the stream ([BEACONS.md](BEACONS.md)).
- **Instant replay.** A ring buffer in the engine. The hotkey drops a marker through the markers API and plays the replay as a scene.
- **Effects.** CrowdSync presses, Skills, Surge levels, raids and hotkeys map to an effect, a sound and a scene or source change, built into the app instead of the OBS bridge. They use the existing capture and release protocol ([CROWDSYNC.md](CROWDSYNC.md)).
- **Audio chain.** Noise suppression (RNNoise or DeepFilterNet), echo cancellation (WebRTC APM), an echo check that plays a test sound, presets stored as data files, and loudness meters (LUFS).
- **SRT** as an output for bad networks.

## Part 3: extend

- **Assistant with typed tools.** Every app action is registered as a tool with a schema, and each one asks permission (allow once, always allow, or ask). The tools call the public API and the engine's commands. Which model runs it, local or hosted, is a separate decision.
- **Plugins.** A manifest plus named UI slots. Plugins run in sandboxed iframes with a postMessage bridge, the same model as CrowdSync custom controls, and only signed bundles load.
- **Phone as camera.** The phone pushes WHIP or SRT to a listener in the app on the local network; no USB driver.
- **Guests in the app.** A guest room over SRS WebRTC, composited in the host's app, so the server still never re-encodes. Co-streams already cover the separate-channels case. LiveKit would be easier but needs Joe's approval (AGENTS.md).

## Part 4: on-device AI

Runs in its own process on the streamer's GPU, and every feature is off until the streamer turns it on. Models download from S.V.E.R's media host with a checksum, chosen by hardware tier.

- **Captions.** Speech recognition (whisper.cpp or sherpa-onnx) sent as a caption track, the same way OBS captions already reach viewers. Nothing is sent to a speech service.
- **Voice triggers.** The same recognizer listens for the streamer's own keywords and fires an effect.
- **Game moments.** Frame classifiers and announcer-audio fingerprints per game, plus an optional vision-language model. The app reports `{event, confidence, stream_ms}` to MAGNet as a moment signal.

### Moments reported by the app

- Signed by the app's token, rate-limited per stream, and listed in the streamer's MAGNet panel.
- A reported moment counts only together with a room signal (distinct verified chatters), the same way the streamer flag counts, so fake events can't farm it.
- MAGNet's rules still apply: never by size, no money, only real people count, and every turn is explained. A turn that used an app-reported moment says so.

## From TikTok's recommendation side

TikTok's ranker runs on its servers and isn't in the app. What the app shows around it, and what SVER does with each piece ([MAGNET.md](MAGNET.md)):

| TikTok | SVER |
| --- | --- |
| A stream or a 10-minute window marked "not eligible for recommendation", with a reason and an appeal | **Take.** A MAGNet-eligible check (no frozen or static picture, healthy input), using the server-side probe. Studio shows the reason. |
| The penalty spreads to co-hosts | **Adapt.** An integrity case on one member applies to a merged co-stream lane, and says so. |
| Reports of every source's type, position and screen share, plus frame snapshots | **Adapt.** Opt-in only, and only coarse "static picture" and "restream" signals worked out on the PC. |
| Server-scored chat order, low messages dropped in busy rooms | **Adapt** (main repo, later). Score by spam and AutoMod signals, never by spend; fold rather than drop, and show viewers that folding is on. |
| Paid boost cards ("Flares") and gift rankings | **Skip.** Breaks "no money". |
| Hourly ranks and leagues that grant exposure | **Skip.** Breaks "never by size". Faction war checkpoints already give competition. |
| Piracy scoring from the process list and input monitoring | **Skip.** Surveillance. Take It Down matching already runs on the server. |

## Not in this module

- Beauty and makeup filters (TikTok's depend on ByteDance's Effect SDK). Background blur and removal can come with part 4.
- Game-hook capture, unless libobs is chosen.
- macOS and Linux builds: Windows first. The engine choices all have macOS and Linux equivalents.
- Server-side chat ranking (main repo, see above).

## Done when (part 1)

1. A new user installs the app, signs in with the code at `sver.tv/go`, and goes live without seeing or copying a stream key.
2. Going live is refused until title and category are set, the same as from OBS.
3. A stream over WHIP and one over RTMP each pass the publish hook and show as live.
4. A disabled or failing hardware encoder falls back to another one with the same codec without ending the stream.
5. Input-health warnings appear in the app within 10 seconds of the server seeing the problem.
6. Revoking the app in account settings stops its next publish.
7. Web content in the app can't reach Node, the file system or other windows.
