import assert from "node:assert/strict";
import test from "node:test";
import { ago, uptime, watching } from "./format.ts";

const now = Date.parse("2026-11-18T12:00:00Z");
const before = (minutes: number) => new Date(now - minutes * 60000).toISOString();

test("viewer counts read as watching", () => {
  assert.equal(watching(22), "22 watching");
  assert.equal(watching(0), "0 watching");
});

test("uptime is minutes, then hours and minutes, then days and hours", () => {
  assert.equal(uptime(before(12), now), "12m");
  assert.equal(uptime(before(220), now), "3h 40m");
  assert.equal(uptime(before(2 * 1440 + 300), now), "2d 5h");
  assert.equal(uptime(before(-5), now), "0m");
});

test("ago rounds down to the largest unit", () => {
  assert.equal(ago(before(0), now), "just now");
  assert.equal(ago(before(5), now), "5m ago");
  assert.equal(ago(before(125), now), "2h ago");
  assert.equal(ago(before(3 * 1440 + 60), now), "3d ago");
});
