"""Checks infra/plays/watchdog.py's decision logic: python scripts/check-plays-watchdog.py"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "infra" / "plays"))
import watchdog  # noqa: E402

restarts = []
restart = lambda: restarts.append(1)  # noqa: E731
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
print("plays watchdog checks passed")
