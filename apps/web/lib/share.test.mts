import assert from "node:assert/strict";
import test from "node:test";
import { creditName, parseStart, redditIntent, shareQuery, shareUrl, timestampParam, xIntent } from "./share.ts";

test("a channel copy stays the canonical URL", () => {
  const url = shareUrl("https://sver.tv", "/Ada", shareQuery("Ada", {}), true, true);
  assert.equal(url, "https://sver.tv/Ada");
});

test("X and Reddit links carry the sharer's username", () => {
  const url = shareUrl("https://sver.tv", "/Ada", shareQuery("Bea", { t: "90" }), false, true);
  assert.equal(url, "https://sver.tv/Ada?t=90&ref=Bea");
  assert.equal(xIntent(url, "Ada is live"), "https://x.com/intent/post?text=Ada%20is%20live&url=https%3A%2F%2Fsver.tv%2FAda%3Ft%3D90%26ref%3DBea");
  assert.match(redditIntent(url, "Ada"), /reddit\.com\/submit\?url=/);
});

test("a clip or recording copy includes the timestamp and credit", () => {
  const url = shareUrl("https://sver.tv", "/clips/11111111-1111-1111-1111-111111111111", shareQuery("Bea", { t: timestampParam(12500) }), true, false);
  assert.equal(url, "https://sver.tv/clips/11111111-1111-1111-1111-111111111111?t=12&ref=Bea");
});

test("credit is only a public username", () => {
  assert.equal(creditName("Bea"), "Bea");
  assert.equal(creditName("no"), undefined);
  assert.equal(creditName("has space"), undefined);
  assert.equal(creditName("../admin"), undefined);
  const url = shareUrl("https://sver.tv", "/clips/x", shareQuery("no", {}), true, false);
  assert.equal(url, "https://sver.tv/clips/x");
});

test("timestamps parse the forms people paste", () => {
  assert.equal(parseStart("90"), 90);
  assert.equal(parseStart("1m30s"), 90);
  assert.equal(parseStart("1h2m3s"), 3723);
  assert.equal(parseStart("0"), null);
  assert.equal(parseStart("nope"), null);
  assert.equal(parseStart(String(49 * 3600)), null);
});
