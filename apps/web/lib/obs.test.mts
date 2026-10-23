import assert from "node:assert/strict";
import test from "node:test";
import { activityText, onAir, osOf, sceneCollection } from "./obs.ts";

test("Activity lines read plainly for every alert kind", () => {
  const at = "2026-10-23T12:00:00Z";
  assert.equal(activityText({ id: 1, kind: "follow", at, data: { user: "Ana" } }).text, "Ana followed");
  assert.equal(activityText({ id: null, kind: "follow", at, pending: true, data: { user: "Bo" } }).text, "Bo followed (counts after 15 minutes of watching)");
  assert.equal(activityText({ id: 2, kind: "sub", at, data: { user: "Cy", tier: 2, months: 1 } }).text, "Cy subscribed at Tier 2");
  assert.equal(activityText({ id: 3, kind: "sub", at, data: { user: "Cy", tier: 1, months: 14 } }).text, "Cy subscribed at Tier 1 (14 months)");
  assert.deepEqual(activityText({ id: 4, kind: "tribute", at, data: { user: "Di", valor: 1500, message: "gg" } }), { text: "Di paid 1,500 Valor", message: "gg" });
  assert.equal(activityText({ id: 5, kind: "tribute", at, data: { user: "Di", valor: 10 } }).message, undefined, "the message is gone after 7 days");
  assert.equal(activityText({ id: 6, kind: "skill", at, data: { user: "Ed", skill: "Confetti", valor: 300 } }).text, "Ed played Confetti (300 Valor)");
  assert.equal(activityText({ id: 7, kind: "raid", at, data: { from: "Fay", seconds: 10 } }).text, "Fay raided");
});

type Source = { name: string; uuid: string; id: string; settings: { url?: string; items?: { name: string; source_uuid: string }[] } };

test("every scene item points at a source in the collection", () => {
  for (const os of ["windows", "mac", "linux"] as const) {
    const c = sceneCollection({ alerts: "https://sver.tv/overlay/alerts/T", chat: "https://sver.tv/joe/chat?overlay=1", os });
    const sources = c.sources as Source[];
    const byUuid = new Map(sources.map(s => [s.uuid, s.name]));
    for (const scene of sources.filter(s => s.id === "scene"))
      for (const item of scene.settings.items!) assert.equal(byUuid.get(item.source_uuid), item.name, `${os}: ${scene.name} → ${item.name}`);
    for (const { name } of c.scene_order) assert.ok(sources.some(s => s.name === name && s.id === "scene"), name);
  }
});

test("the overlay links carry the private link and widget, and only Windows gets a capture", () => {
  const c = sceneCollection({ alerts: "https://sver.tv/overlay/alerts/T", chat: "https://sver.tv/joe/chat?overlay=1", os: "linux" });
  const sources = c.sources as Source[];
  assert.deepEqual(sources.flatMap(s => s.settings.url ? [s.settings.url] : []), ["https://sver.tv/overlay/alerts/T", "https://sver.tv/overlay/alerts/T?widget=goal", "https://sver.tv/overlay/alerts/T?widget=events", "https://sver.tv/joe/chat?overlay=1"]);
  assert.ok(!sources.some(s => s.id === "game_capture"));
  assert.equal(osOf("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"), "windows");
});

test("a live broadcast without fresh media is not on air", () => {
  const now = Date.parse("2026-10-22T12:00:30Z");
  assert.equal(onAir({ state: "LIVE", observed_at: "2026-10-22T12:00:25Z" }, false, true, now).live, true);
  assert.equal(onAir({ state: "LIVE", observed_at: "2026-10-22T12:00:00Z" }, false, true, now).state, "Signal not confirmed");
  assert.equal(onAir(null, false, false, now).plain, "OBS isn't connected yet");
});
