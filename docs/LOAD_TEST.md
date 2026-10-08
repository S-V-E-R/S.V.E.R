# Load test plan

Specified October 4, 2026. This plan sets the capacity numbers the specs left open. It runs in Live streams phase 5, "Staged deployment and live acceptance" ([LIVE_STREAMS.md](LIVE_STREAMS.md)). Automatic switching between delivery paths stays off until it passes, and Multistream doesn't ship until the restream budget is set.

The repo is public, so this doc describes the method only. Measured numbers, server details and the resulting limits go into the private tuning config and the private operations notes. Only sanitized aggregate receipts (no hostnames or addresses) are committed, like the existing media proof receipt.

## What it decides

| Number | Used by | How it's set |
| --- | --- | --- |
| **Direct WebRTC budget per broadcast** | The player's transport choice ([LIVE_STREAMS.md](LIVE_STREAMS.md), "server supplies allowed transports") | Measured |
| **Global direct WebRTC budget** (all broadcasts together) | Same | Measured, then headroom applied |
| **CDN path holds 3–5 seconds under load** | The large-stream promise | Measured pass or fail |
| **Restream budget** | Multistream capacity guardrail ([LINKED_CHAT.md](LINKED_CHAT.md)) | From the bandwidth left after viewers |
| **Chat fan-out per channel and in total** | Chat rate limits, raid and MAGNet handoffs | Measured |
| **Bitrate cap for OBS guidance** | Studio's bitrate warning (currently a provisional 8 Mbps) | Confirmed or changed |

## Before anything runs

1. **Confirm the server's guaranteed bandwidth** with the host. The network card's speed is not the guarantee. Every budget below is a share of the guaranteed figure, called **B**.
2. **Joe approves a test window and a spending limit.** Load generators are short-lived rented machines, deleted after each run, and CDN traffic is billed per gigabyte. Each run lists its expected duration and data volume first, as LIVE_STREAMS.md already requires.
3. **An isolated SRS instance** with its own ports and output, as in Live streams phase 1. Nothing touches the running S.V.E.R Plays services, and Plays is watched during every run as the canary.
4. **Abort rules, checked automatically:**
   - Plays stutters or its health check fails.
   - Server CPU stays above 80% for more than a minute.
   - Packet loss rises above 2%.
   - Egress reaches 85% of B.
   - Any real viewer reports a problem.

   The run stops and its generators are deleted.
5. **Off-peak only.** Claude Code runs the server side, because changes on the server go through it and only with Joe's go-ahead.

## Method

**Synthetic streams.** The stream is the test video that already burns a timestamp into every frame (`scripts/check-media.cjs`), published at the OBS baseline: 1080p60 H.264, 6 Mbps video, 160 Kbps audio, a 1-second keyframe interval, B-frames off. A second profile at the provisional 8 Mbps cap checks the bitrate warning.

**Viewers come from outside the server.** Generating load on the same machine would measure nothing.
- **WebRTC viewers:** SRS's own benchmark tool (srs-bench) runs many lightweight WebRTC players per generator machine.
- **Latency:** a small number of real browsers, plus `scripts/media-webrtc.py`, read the burned-in timestamp to measure true delay from capture to screen. Fetching playlists isn't counted as viewing.
- **CDN viewers:** HLS players pulling through Bunny with signed per-lease URLs, as in production.
- **Chat:** scripted socket clients that join, send within the rate limits and stay connected.

**Measured on every step:**
- delay from capture to screen (p50 and p95, at least 30 samples per step over 10 minutes)
- how often playback starts successfully, and stalls per viewer-hour
- server CPU and memory, SRS process load
- outgoing bandwidth, packet loss and retransmissions
- traffic from the server to the CDN (origin pulls)
- chat delivery time (p95) and API response times

## Scenarios

1. **WebRTC, one broadcast.**
   - Increase viewers in steps until one limit is hit: p95 delay above 1.5 seconds, playback success below 99%, a stall rate above the agreed level, CPU above 70%, or bandwidth above the share allowed below.
   - The last step that passed every check, minus 30% headroom, is the per-broadcast budget.
   - Bandwidth limits are also calculated: each WebRTC viewer costs about one stream's bitrate. Generators only need to reach the point where CPU per viewer is clear; the bandwidth ceiling is worked out from B rather than paid for in full.
   - Watch per-core load, not just total CPU. SRS does most of its work on one core per process, so a single process can hit its ceiling while the machine looks mostly idle. If that happens first, repeat the run with several SRS processes sharing the load, and set the budget from the setup that scales best.
2. **WebRTC, many broadcasts.** Repeat with 5, then 20 simultaneous broadcasts with viewers spread across them. Many broadcasts cost more CPU than one broadcast with the same number of viewers, so the global budget comes from this run, again with 30% headroom.
3. **CDN path under load.**
   - A few hundred CDN viewers on one broadcast, enough to prove two things:
     - p95 delay stays within 3–5 seconds and under 5 for a large stream
     - origin traffic stays flat as viewers are added, which shows the CDN is absorbing them
   - Bunny's own capacity isn't what's being tested, so there's no need to pay for thousands of simulated viewers.
4. **Switching.** Push a broadcast past its WebRTC budget. Viewers past the limit get the CDN path, nobody is counted twice, and nobody is dropped. Also: forced WebRTC failure, CDN errors, and the 8-second fallback.
5. **Bursts:**
   - **Raid:** 500 viewers arrive within 10 seconds.
   - **Go-live alert:** thousands of sign-ins and page loads within a minute, measured on the API and database.
   - **MAGNet handoff:** a spotlight sends a channel's room to another channel.

   The site stays up, chat delivers within its target, and the viewer-integrity provisional window behaves.
6. **Restreaming.** Push to a local stand-in for each platform on a generator machine, never to a real platform at volume. Measure outgoing bandwidth and CPU per restream, then run one real end-to-end test per platform (YouTube, Twitch, Kick) with a test channel.
7. **Chat.** One channel with thousands of connected chatters at a busy message rate, plus many quieter channels at once. This sets the chat fan-out limits and confirms the rate limits hold.
8. **WHIP and SRT ingest** (October 8, 2026, before they were offered). On an isolated SRS (same image as production, its own ports, never the live media server), 20 WHIP publishers (H.264 + Opus, pion) and 20 SRT publishers (MPEG-TS from FFmpeg), each 720p30 at about 6.5 Mbps, ran together: all 40 connected, every one reported H.264/AAC (SRS turns WHIP's Opus into AAC) and wrote HLS, and SRS used about 55% of one core (about 1.4% per publisher, linear from 20 to 40). Budget with 30% headroom: 28 simultaneous WHIP/SRT publishers on one SRS process, far above current use. FFmpeg 8's own WHIP muxer fails SRS's DTLS handshake; OBS uses a different WebRTC stack that SRS supports.

## Turning results into budgets

- **Viewers come first.** Direct WebRTC, plus origin traffic to the CDN, plus restreams, must stay under **70% of B** at the measured peak. The other 30% is headroom for bursts, retransmissions, VOD and clip uploads, and everything else on the server.
- **Restream budget:** whatever is left of that 70% after the global WebRTC budget and observed origin traffic. LINKED_CHAT.md's guardrail queues new restreams when it's reached.
- **Bitrate cap:** if 8 Mbps streams cause stalls on ordinary connections, or squeeze the budgets too far, Studio's warning moves down to match.
- **Where they're kept:** every budget goes into the private tuning config, with the date and test run noted in the private operations notes. The code ships safe example values.

## When to run it again

- After an SRS upgrade, a server or host plan change, a bitrate cap change, or a CDN configuration change.
- When real peak usage reaches half of any budget, so the next limit is known before it's hit.
- Before any event expected to draw an unusually large audience.

## Done when

1. B is confirmed with the host and recorded privately.
2. The per-broadcast and global WebRTC budgets are measured with 30% headroom and loaded from the private config.
3. The CDN path meets 3–5 seconds (under 5 for a large stream) under the test load, with origin traffic flat. If it can't, the failure is documented and the media design is fixed before Live streams closes.
4. Switching moves viewers past the budget to the CDN with no double counting or drops.
5. Raid, go-live and MAGNet bursts pass. Chat fan-out limits are set.
6. The restream budget is set and Multistream's guardrail uses it.
7. Plays was never disturbed, every generator was deleted, and spending stayed within Joe's limit.
8. A sanitized aggregate receipt is committed.
