#!/usr/bin/env python3
"""Runs beside the existing Plays game, using its already-installed redis package.

Core uses HTTP/Postgres only. This adapter translates to the separate game runtime's
private command bus. Credentials come from systemd, never command-line arguments.
"""
import json
import os
from pathlib import Path
import time
import urllib.request

COMMANDS = {"up", "down", "left", "right", "a", "b", "start", "select"}


def deliver(bus, config, data):
    command = data.get("command")
    if command is not None and command not in COMMANDS:
        raise ValueError("Unexpected command")
    viewers = data.get("viewers")
    if type(viewers) is not int or viewers < 0:
        raise ValueError("Invalid viewer count")
    bus.set(config["viewers_key"], json.dumps({"authenticatedTrusted": viewers, "ts": int(time.time() * 1000)}), ex=30)
    if command:
        # At-most-once input: Core never reissues this round, even if delivery is interrupted.
        subscribers = bus.publish(config["command_channel"], json.dumps({"command": command, "windowNumber": data["round"]}))
        print(f"plays_command round={data['round']} command={command} listeners={subscribers}", flush=True)


def main():
    import redis  # The separate Plays project's existing dependency; not a Core dependency.
    config = json.loads((Path(os.environ["CREDENTIALS_DIRECTORY"]) / "config.json").read_text())
    bus = redis.Redis.from_url(config["redis_url"], decode_responses=True, socket_connect_timeout=3, socket_timeout=3)
    modes = bus.pubsub(ignore_subscribe_messages=True)
    modes.subscribe(config["mode_channel"])
    mode = "chat"
    failed = False
    while True:
        try:
            message = modes.get_message(timeout=0)
            if message:
                candidate = json.loads(message["data"]).get("mode")
                if candidate in {"chat", "rl"}:
                    mode = candidate
            cached = bus.get(config["mode_channel"] + ":current")
            if cached in {"chat", "rl"}:
                mode = cached
            ready = bus.pubsub_numsub(config["command_channel"])[0][1] > 0
            request = urllib.request.Request(config["api_origin"].rstrip("/") + "/api/plays/bridge", data=json.dumps({"ready": ready, "input_mode": mode}).encode(), headers={"Authorization": "Bearer " + config["token"], "Origin": config["site_origin"], "Content-Type": "application/json"})
            with urllib.request.urlopen(request, timeout=5) as response:
                data = json.loads(response.read(65536))
            deliver(bus, config, data)
            if failed:
                print("plays_bridge recovered", flush=True)
            failed = False
        except Exception as error:
            if not failed:
                # Exceptions from URL/Redis clients can include credentials. Log the class only.
                print(f"plays_bridge unavailable ({type(error).__name__})", flush=True)
            failed = True
        time.sleep(1 if not failed else 3)


if __name__ == "__main__":
    main()
