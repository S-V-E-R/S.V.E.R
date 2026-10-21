import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { rotationOrder } from "./streams.ts";
import { palette, themeFor } from "./theme.ts";
import { isStreamCard, type StreamCard } from "./types.ts";
import { absolute, playbackKey } from "./urls.ts";

function card(username: string, viewers: number): StreamCard {
  return {
    broadcast_id: username, username, display_name: username, avatar: null, faction: null,
    title: username, category: null, category_id: null, genre: null, started_at: "2026-10-21T00:00:00Z",
    viewers, thumbnail: "/api/discovery/thumbnails/x", label: null, charity: null, fresh: false, mature: false, language: null,
  };
}

test("discovery order stays put when a later stream has more viewers", () => {
  const items = [card("small", 3), card("large", 3000)];
  assert.deepEqual(rotationOrder(items).map(item => item.username), ["small", "large"]);
});

test("faction accents match the web faction colors", () => {
  const source = readFileSync(new URL("../../web/lib/factions.ts", import.meta.url), "utf8");
  for (const slug of ["myria", "aetheron", "glint"] as const) {
    const block = source.slice(source.indexOf(`slug: "${slug}"`));
    const color = /color: "(#[0-9A-F]+)"/.exec(block)?.[1];
    assert.equal(palette[slug].accent, color);
  }
});

test("theme follows the signed-in faction and is neutral otherwise", () => {
  assert.equal(themeFor(null), "neutral");
  assert.equal(themeFor({ faction: "glint" }), "glint");
  assert.equal(themeFor({ faction: "other" }), "neutral");
});

test("playback identity ignores the rotating CDN token", () => {
  assert.equal(playbackKey("https://cdn.example/live/abc/bcdn_token=secret/index.m3u8"), "https://cdn.example/live/abc/index.m3u8");
  assert.equal(absolute("/api/discovery/live").endsWith("/api/discovery/live"), true);
});

test("a discovery card is recognized and a partial object is not", () => {
  assert.equal(isStreamCard(card("a", 1)), true);
  assert.equal(isStreamCard({ username: "a" }), false);
});

test("live discovery, playback and chat match the readers", { skip: !process.env.SVER_MOBILE_LIVE }, async () => {
  const origin = process.env.EXPO_PUBLIC_API_URL ?? "https://sver.tv";
  const live = await fetch(`${origin}/api/discovery/live`).then(r => r.json()) as { items: unknown[] };
  assert.ok(Array.isArray(live.items));
  for (const item of live.items) assert.equal(isStreamCard(item), true);
  assert.deepEqual(rotationOrder(live.items.filter(isStreamCard)).map(item => item.broadcast_id), live.items.filter(isStreamCard).map(item => item.broadcast_id));
  const first = live.items.find(isStreamCard);
  if (!first) return;
  const channel = await fetch(`${origin}/api/channels/${encodeURIComponent(first.username)}/live?transport=hls`).then(r => r.json()) as { live: boolean; playback?: { hls?: unknown } };
  assert.equal(channel.live, true);
  assert.equal(typeof channel.playback?.hls === "string" || channel.playback?.hls === null, true);
  const chat = await fetch(`${origin}/api/channels/${encodeURIComponent(first.username)}/chat`).then(r => r.json()) as { messages?: unknown };
  assert.ok(Array.isArray(chat.messages));
});
