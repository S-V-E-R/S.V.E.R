"""Checks infra/plays/watchdog.py's decision logic: python scripts/check-plays-watchdog.py"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "infra" / "plays"))
import watchdog  # noqa: E402

restarts = []
restart = lambda _state: restarts.append(1)  # noqa: E731
s = watchdog.check({}, 100, 1000, restart)
s = watchdog.check(s, 160, 1060, restart)
assert (s["failures"], s["problem"], restarts) == (0, None, []), "a moving picture is healthy"
s = watchdog.check(s, 160, 1120, restart)
assert (s["failures"], restarts) == (1, []), "one still check is not enough"
s = watchdog.check(s, None, 1180, restart)
assert restarts == [1] and s["failures"] == 0 and "restarted" in s["problem"], "two restart the game"
s = watchdog.check(s, None, 1240, restart)
s = watchdog.check(s, None, 1300, restart)
assert restarts == [1] and "still not advancing" in s["problem"], "no restart loop within five minutes"
s = watchdog.check(s, 300, 1360, restart)
assert s["problem"], "a restart stays reported for ten minutes"
s = watchdog.check(s, 360, 1180 + 600, restart)
assert s["problem"] is None, "then clears once the picture moves"
before = dict(s)
s = watchdog.check(s, watchdog.UNKNOWN, 3000, restart)
s = watchdog.check(s, watchdog.UNKNOWN, 3060, restart)
assert s == before and restarts == [1], "an unreachable Core never restarts the game"
# The runner's heartbeat: a hung main loop fails even while audio keeps the playlist moving.
restarts.clear()
s = watchdog.check({}, 1, 5000, restart, {"at": 5000, "mode": "chat", "frame": 7})
s = watchdog.check(s, 2, 5060, restart, {"at": 4950, "mode": "chat", "frame": 7})
assert s["failures"] == 1, "a runner heartbeat over 90 seconds old is a stopped game"
s = watchdog.check(s, 3, 5120, restart, {"at": 5110, "mode": "chat", "frame": 7})
assert s["failures"] == 0, "in chat mode a still screen is people deciding"
for n, at in enumerate(range(5180, 5180 + 4 * 60, 60)):
    s = watchdog.check(s, 10 + n, at, restart, {"at": at, "mode": "rl", "frame": 7})
assert restarts == [1], "in AI mode a picture that never changes restarts the game"
s = watchdog.check(s, 20, 5500, restart, {"at": 5500, "mode": "rl", "frame": 8})
assert s["still"] == 0 and s["failures"] == 0, "a new picture resets it"
saved = []
watchdog.check({"failures": 1, "sequence": 5}, 5, 9000, saved.append)
assert saved and saved[0]["restarted_at"] == 9000 and "restarted" in saved[0]["problem"], "the state is saved before restarting"
print("plays watchdog checks passed")
