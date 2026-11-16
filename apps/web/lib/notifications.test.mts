import assert from "node:assert/strict";
import test from "node:test";
import { collapseLive } from "./notifications.ts";

const live = (id: string, username: string, read = true) => ({ id, kind: "live", read, channel: { username } });

test("go-lives collapse to the newest row per channel; other notices pass through", () => {
  const rows = collapseLive([live("1", "joe"), { id: "2", kind: "dm", read: true }, live("3", "joe", false), live("4", "ann"), { id: "5", kind: "dm", read: true }]);
  assert.deepEqual(rows.map(r => [r.id, r.times, r.read]), [["1", 2, false], ["2", 1, true], ["4", 1, true], ["5", 1, true]]);
});
