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
    """The origin playlist's media sequence; None when the stream is gone; UNKNOWN without Core."""
    try:
        live = json.loads(fetch(f"{SITE}/api/channels/{CHANNEL}/live"))
    except (OSError, ValueError):
        return UNKNOWN
    try:
        found = re.search(r"/([0-9a-f]{32})\.m3u8", (live.get("playback") or {}).get("hls") or "")
        if not found:
            return None
        playlist = fetch(f"{ORIGIN_HLS}/{found.group(1)}.m3u8")
        sequence = re.search(r"#EXT-X-MEDIA-SEQUENCE:(\d+)", playlist)
        return int(sequence.group(1)) if sequence else None
    except (OSError, ValueError):
        return None


def check(state, sequence, now, restart):
    """One pass. Returns the new state; calls restart() when the picture has stopped."""
    if sequence == UNKNOWN:
        return state
    moving = sequence is not None and sequence != state.get("sequence")
    failures = 0 if moving else state.get("failures", 0) + 1
    problem = state.get("problem")
    restarted_at = state.get("restarted_at", 0)
    if failures >= 2 and now - restarted_at >= RESTART_GAP:
        restart()
        restarted_at, failures = now, 0
        problem = f"The Plays picture stopped advancing at {time.strftime('%H:%M', time.gmtime(now))} UTC; the game was restarted."
    elif failures >= 2:
        problem = "The Plays picture is still not advancing after an automatic restart."
    elif moving and now - restarted_at >= REPORT_FOR:
        problem = None
    return {"sequence": sequence, "failures": failures, "problem": problem, "restarted_at": restarted_at}


def main():
    if not CHANNEL:
        raise SystemExit("PLAYS_CHANNEL is required")
    RUN_DIR.mkdir(parents=True, exist_ok=True)
    path = RUN_DIR / "watchdog.json"
    try:
        state = json.loads(path.read_text())
    except (OSError, ValueError):
        state = {}

    def restart():
        print(f"plays_watchdog restart service={SERVICE}", flush=True)
        subprocess.run(["systemctl", "restart", SERVICE], check=False, timeout=90)

    state = check(state, media_sequence(), time.time(), restart)
    path.write_text(json.dumps(state))
    health = RUN_DIR / "health.json"
    health.write_text(json.dumps({"problem": state["problem"]}))
    health.chmod(0o644)
    if state["problem"]:
        print(f"plays_watchdog problem={state['problem']}", flush=True)


if __name__ == "__main__":
    main()
