import assert from "node:assert/strict";
import test from "node:test";
import { mentions, merge, outsideRole, retryDelay, subBadge } from "./chat-rules.ts";
import type { ChatMessage } from "./types";

const line = (seq: number, over: Partial<ChatMessage> = {}): ChatMessage => ({ id: `m${seq}`, seq, body: "hi", created_at: "2026-11-20T12:00:00Z", author: { username: "a", display_name: "A" }, ...over });

test("merge keeps seq order, replaces by id and keeps only the last 100", () => {
  const many = Array.from({ length: 120 }, (_, i) => line(i));
  const merged = merge(many.slice(0, 60), [...many.slice(60), line(5, { body: "edited" })]);
  assert.equal(merged.length, 100);
  assert.equal(merged[0].seq, 20);
  assert.equal(merged.at(-1)?.seq, 119);
});

test("sub badges follow the website's milestones", () => {
  assert.deepEqual([1, 2, 3, 8, 9, 12, 23, 24, 37].map(subBadge), ["1 month", "1 month", "3 months", "6 months", "9 months", "1 year", "1 year", "2 years", "3 years"]);
});

test("outside roles read as the platform's own words", () => {
  assert.equal(outsideRole({ platform: "twitch", name: "x", role: "moderator" }), "Twitch mod");
  assert.equal(outsideRole({ platform: "youtube", name: "x", role: "subscriber" }), "YouTube member");
  assert.equal(outsideRole({ platform: "kick", name: "x", role: "subscriber" }), "Kick sub");
  assert.equal(outsideRole({ platform: "twitch", name: "x", role: "broadcaster" }), "Twitch broadcaster");
  assert.equal(outsideRole({ platform: "twitch", name: "x", role: null }), null);
});

test("reconnect delay doubles with jitter and caps at 30 s", () => {
  assert.equal(retryDelay(0, 0.5), 1000);
  assert.equal(retryDelay(3, 0), 4000);
  assert.equal(retryDelay(10, 1), 45000);
  assert.equal(retryDelay(10, 0.5), 30000);
});

test("a mention highlights only the viewer, case-insensitively", () => {
  assert.equal(mentions(line(1, { mentions: ["Joe"] }), "joe"), true);
  assert.equal(mentions(line(1, { mentions: ["Joe"] }), "ann"), false);
  assert.equal(mentions(line(1), null), false);
});
