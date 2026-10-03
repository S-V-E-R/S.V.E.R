// node --test scripts/link-templates.test.mjs: handle -> URL templates for the social-link field (lib/types.ts).
import assert from "node:assert/strict";
import test from "node:test";
const { linkUrl, linkHost } = await import(new URL("../apps/web/lib/types.ts", import.meta.url).href);
test("handles build platform URLs", () => {
  const cases = [["twitch", "@joe", "https://twitch.tv/joe"], ["youtube", "joe", "https://youtube.com/@joe"], ["youtube", "@joe", "https://youtube.com/@joe"], ["kick", "joe", "https://kick.com/joe"], ["tiktok", "joe", "https://tiktok.com/@joe"], ["instagram", "joe", "https://instagram.com/joe"], ["x", "@joe", "https://x.com/joe"], ["bluesky", "joe.bsky.social", "https://bsky.app/profile/joe.bsky.social"], ["discord", "abc123", "https://discord.gg/abc123"], ["facebook", "joe", "https://facebook.com/joe"], ["patreon", "joe", "https://patreon.com/joe"], ["kofi", "joe", "https://ko-fi.com/joe"], ["fourthwall", "joeshop", "https://joeshop.4thwall.com"], ["website", "example.com/me", "https://example.com/me"]];
  for (const [platform, handle, url] of cases) assert.equal(linkUrl(platform, handle), url, platform);
});
test("typed addresses are kept for the server to validate", () => {
  assert.equal(linkUrl("twitch", " https://www.twitch.tv/joe "), "https://www.twitch.tv/joe");
  assert.equal(linkUrl("twitch", "http://twitch.tv/joe"), "http://twitch.tv/joe");
  assert.equal(linkUrl("twitch", "twitch.tv/joe"), "https://twitch.tv/joe");
  assert.equal(linkUrl("x", ""), "");
  assert.equal(linkUrl("x", "a b/c"), "https://x.com/a%20b%2Fc");
});
test("host shown without www", () => {
  assert.equal(linkHost("https://www.twitch.tv/joe"), "twitch.tv");
  assert.equal(linkHost("not a url"), "");
});
