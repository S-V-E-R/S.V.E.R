import assert from "node:assert/strict";
import test from "node:test";
import { blocker, counterText, type Control } from "./board-rules.ts";

const button = (over: Partial<Control> = {}): Control => ({
  id: "a", kind: "button", label: "A", cost: 0, cooldown_seconds: 0, per_stream_limit: null, audience: "everyone", target: null, width: 1, ...over,
});
const view = { last_press: {}, used: {}, goals: {}, balance: 100 };
const now = Date.parse("2026-10-22T12:00:00Z");

test("a free control with nothing in the way can be pressed", () => {
  assert.equal(blocker(button(), view, now), null);
});

test("cooldown counts down in whole seconds, then clears", () => {
  const pressed = { ...view, last_press: { a: "2026-10-22T11:59:58Z" } };
  assert.equal(blocker(button({ cooldown_seconds: 5 }), pressed, now), "3s");
  assert.equal(blocker(button({ cooldown_seconds: 2 }), pressed, now), null);
});

test("game-disabled, per-stream limit, full goal and too little Valor each block", () => {
  assert.equal(blocker(button(), { ...view, state: { a: { disabled: true } } }, now), "Unavailable");
  assert.equal(blocker(button({ per_stream_limit: 2 }), { ...view, used: { a: 2 } }, now), "Limit reached");
  assert.equal(blocker(button({ kind: "goal", target: 10 }), { ...view, goals: { a: 10 } }, now), "Complete");
  assert.equal(blocker(button({ cost: 150 }), view, now), "Not enough");
  assert.equal(blocker(button({ cost: 100 }), view, now), null);
});

test("counters read like the website's", () => {
  const counter = { id: "c", label: "C", value: 4, extra: 2, odds: null };
  assert.equal(counterText({ ...counter, kind: "tally" }), "4–2");
  assert.equal(counterText({ ...counter, kind: "deaths" }), "4");
  assert.match(counterText({ ...counter, kind: "shiny", odds: 4096 }), /^4 encounters · phase 3 · 0\.1% by now \(1 in 4,096\)$/);
});
