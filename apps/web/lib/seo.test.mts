import assert from "node:assert/strict";
import test from "node:test";
import { blockedByRobots, broadcastJsonLd, profileJsonLd, sitemapEntries, videoJsonLd, websiteJsonLd } from "./seo.ts";

test("robots keeps private routes out and leaves public ones in", () => {
  for (const path of ["/api/auth/me", "/login", "/settings/profile", "/studio/stream", "/admin/users", "/messages", "/wallet", "/overlay/abc", "/embed/abc", "/go", "/squads/abc"]) {
    assert.equal(blockedByRobots(path), true, path);
  }
  for (const path of ["/", "/browse", "/magnet", "/Ada", "/Ada/live", "/clips/abc", "/videos/abc", "/g/wolves", "/gold", "/beacons"]) {
    assert.equal(blockedByRobots(path), false, path);
  }
});

test("sitemap lists public pages, channels, clips and recordings", () => {
  const urls = sitemapEntries({
    channels: ["Ada", "bad name"],
    videos: [{ id: "11111111-1111-1111-1111-111111111111", kind: "CLIP" }, { id: "22222222-2222-2222-2222-222222222222", kind: "VOD" }, { id: "nope", kind: "CLIP" }],
    guilds: ["wolves"],
  }).map(entry => entry.url);
  assert.ok(urls.includes("https://sver.tv"));
  assert.ok(urls.includes("https://sver.tv/magnet"));
  assert.ok(urls.includes("https://sver.tv/Ada"));
  assert.ok(!urls.includes("https://sver.tv/bad name"));
  assert.ok(urls.includes("https://sver.tv/clips/11111111-1111-1111-1111-111111111111"));
  assert.ok(urls.includes("https://sver.tv/videos/22222222-2222-2222-2222-222222222222"));
  assert.ok(urls.includes("https://sver.tv/g/wolves"));
  assert.equal(urls.some(url => url.includes("/login") || url.includes("/api/")), false);
});

test("structured data names a person, a live broadcast and a video", () => {
  const person = profileJsonLd({ username: "Ada", name: "Ada", description: "Builds things.", image: "https://media.example/a.webp", followers: 3 });
  assert.equal(person["@type"], "ProfilePage");
  assert.equal((person.mainEntity as { name: string }).name, "Ada");
  const live = broadcastJsonLd({ username: "Ada", name: "Ada", description: "Live now", started: "2026-10-22T00:00:00Z" });
  assert.equal(live["@type"], "BroadcastEvent");
  assert.equal(live.isLiveBroadcast, true);
  const video = videoJsonLd({ title: "Boss fight", description: "A clip from Ada", url: "https://sver.tv/clips/1", thumbnail: "https://sver.tv/t.webp", authorName: "Ada", authorUrl: "https://sver.tv/Ada" });
  assert.equal(video["@type"], "VideoObject");
  const site = websiteJsonLd();
  assert.equal(site["@type"], "WebSite");
  assert.match(JSON.stringify(site), /search\?q=/);
});
