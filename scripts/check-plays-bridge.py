"""Tiny adapter contract check: no running game, Redis or network required."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location("bridge", Path(__file__).resolve().parents[1] / "infra/plays/bridge.py")
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)

class Bus:
    def __init__(self): self.values = {}; self.commands = []
    def set(self, key, value, ex):
        assert ex == 30
        self.values[key] = json.loads(value)
    def publish(self, channel, body):
        self.commands.append((channel, json.loads(body)))
        return 1

bus = Bus()
config = {"viewers_key": "test:presence", "command_channel": "test:command"}
with contextlib.redirect_stdout(io.StringIO()):
    bridge.deliver(bus, config, {"viewers": 2, "round": 1, "command": "up"})
assert bus.values["test:presence"]["authenticatedTrusted"] == 2
assert bus.commands == [("test:command", {"command": "up", "windowNumber": 1})]
bridge.deliver(bus, config, {"viewers": 0, "round": 2, "command": None})
assert len(bus.commands) == 1
for bad in [{"viewers": 1, "command": "reset"}, {"viewers": -1, "command": "a"}]:
    try: bridge.deliver(bus, config, bad)
    except ValueError: pass
    else: raise AssertionError("Malformed command accepted")
assert len(bus.commands) == 1
print("Plays bridge: command whitelist, presence and idle behavior passed.")
