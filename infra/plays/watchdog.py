#!/usr/bin/env python3
"""Plays watchdog (docs/PLAYS.md, reliability): run once a minute by sver-plays-watchdog.timer.

The picture is moving when the Plays stream's HLS media sequence advances at the origin. Two
checks in a row without movement restart the game service (at most once every five minutes).
The verdict goes to health.json, which the bridge forwards to Core; Core emails staff admins when
a problem lasts more than two minutes. A restart stays reported for ten minutes, so staff hear
about it even when the restart fixed the picture.
"""
import json
import os
import re
import subprocess
import time
import urllib.request
from pathlib import Path

# Core's own listener on this host; the public site sits behind Cloudflare's bot checks.
SITE = os.environ.get("PLAYS_SITE", "http://127.0.0.1:18080")
CHANNEL = os.environ.get("PLAYS_CHANNEL", "")
ORIGIN_HLS = os.environ.get("PLAYS_ORIGIN_HLS", "http://127.0.0.1:8090/rebuild")
SERVICE = os.environ.get("PLAYS_SERVICE", "sver-plays")
RUN_DIR = Path(os.environ.get("PLAYS_RUN_DIR", "/var/run/sver-plays"))
RESTART_GAP = 300
REPORT_FOR = 600
UNKNOWN = "unknown"  # Core couldn't be asked; restarting the game wouldn't help.


def fetch(url):
    with urllib.request.urlopen(url, timeout=10) as response:
        return response.read(65536).decode()


def media_sequence():
    """The origin playlist's media sequence; None when the stream is gone; UNKNOWN when Core or
    the origin can't be asked (a fetch timing out or a playlist mid-rotation isn't a stopped game)."""
    try:
        live = json.loads(fetch(f"{SITE}/api/channels/{CHANNEL}/live"))
    except (OSError, ValueError):
        return UNKNOWN
    found = re.search(r"/([0-9a-f]{32})\.m3u8", ((live.get("playback") or {}).get("hls")) or "")
    if not found:
        return None
    try:
        playlist = fetch(f"{ORIGIN_HLS}/{found.group(1)}.m3u8")
    except (OSError, ValueError):
        return UNKNOWN
    sequence = re.search(r"#EXT-X-MEDIA-SEQUENCE:(\d+)", playlist)
    return int(sequence.group(1)) if sequence else None


def runner_report():
    """The runner's own heartbeat (plays-runner.py writes runner.json each second): its main loop
    ticks, the mode and a hash of the last picture. None when this runner doesn't write one."""
    try:
        return json.loads((RUN_DIR / "runner.json").read_text())
    except (OSError, ValueError):
        return None


# The game's own audio keeps the stream's segments coming even when its main loop hangs, so the
# runner's heartbeat must be fresh too. In AI mode the picture must change within three checks
# (the AI presses A after 20 seconds without one); in chat mode a still screen is people deciding.
RUNNER_STALE = 90
STILL_CHECKS = 3


def check(state, sequence, now, restart, runner=None):
    """One pass. Returns the new state; calls restart(state) when the picture has stopped."""
    if sequence == UNKNOWN:
        return state
    still = state.get("still", 0)
    frame = state.get("frame")
    runner_ok = True
    if runner is not None:
        runner_ok = now - float(runner.get("at", 0)) <= RUNNER_STALE
        same = runner.get("frame") is not None and runner.get("frame") == frame
        still = still + 1 if runner.get("mode") == "rl" and same else 0
        frame = runner.get("frame")
        runner_ok = runner_ok and still < STILL_CHECKS
    moving = sequence is not None and sequence != state.get("sequence") and runner_ok
    failures = 0 if moving else state.get("failures", 0) + 1
    problem = state.get("problem")
    restarted_at = state.get("restarted_at", 0)
    if failures >= 2 and now - restarted_at >= RESTART_GAP:
        restarted_at, failures, still = now, 0, 0
        problem = f"The Plays picture stopped advancing at {time.strftime('%H:%M', time.gmtime(now))} UTC; the game was restarted."
        restart({"sequence": sequence, "failures": failures, "problem": problem, "restarted_at": restarted_at, "frame": frame, "still": still})
    elif failures >= 2:
        problem = "The Plays picture is still not advancing after an automatic restart."
    elif moving and now - restarted_at >= REPORT_FOR:
        problem = None
    return {"sequence": sequence, "failures": failures, "problem": problem, "restarted_at": restarted_at, "frame": frame, "still": still}


def main():
    if not CHANNEL:
        raise SystemExit("PLAYS_CHANNEL is required")
    RUN_DIR.mkdir(parents=True, exist_ok=True)
    path = RUN_DIR / "watchdog.json"
    try:
        state = json.loads(path.read_text())
    except (OSError, ValueError):
        state = {}

    def restart(next_state):
        # Saved first, so a restart that outlasts the timeout can't lose restarted_at and let
        # the next run restart again straight away.
        path.write_text(json.dumps(next_state))
        print(f"plays_watchdog restart service={SERVICE}", flush=True)
        try:
            subprocess.run(["systemctl", "restart", SERVICE], check=False, timeout=90)
        except (subprocess.TimeoutExpired, OSError) as error:
            print(f"plays_watchdog restart_slow ({type(error).__name__})", flush=True)

    state = check(state, media_sequence(), time.time(), restart, runner_report())
    path.write_text(json.dumps(state))
    health = RUN_DIR / "health.json"
    health.write_text(json.dumps({"problem": state["problem"]}))
    health.chmod(0o644)
    if state["problem"]:
        print(f"plays_watchdog problem={state['problem']}", flush=True)


if __name__ == "__main__":
    main()
