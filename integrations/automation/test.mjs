// Tests the automation bridge against a fake S.V.E.R and fake Streamer.bot/SAMMI: node automation/test.mjs
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { forwardings, flatten, run } from "./sver-automation.mjs";
import { serve } from "../test/ws-server.mjs";

assert.deepEqual(flatten({ user: "Ann", tier: 2, gift: { from: "Bo" }, list: [1] }), { user: "Ann", tier: "2", gift_from: "Bo", list: "[1]" });
const both = { streamerbot: {}, sammi: { password: "pw" } };
const [sb, sammi] = forwardings({ topic: "channel:streamer:subs", data: { user: "Ann", tier: 2 }, at: "t" }, both);
assert.deepEqual(sb, { url: "http://127.0.0.1:7474/DoAction", headers: {}, body: { action: { name: "S.V.E.R Subscription" }, args: { user: "Ann", tier: "2", sver_event: "Subscription", sver_at: "t" } } });
assert.deepEqual(sammi.headers, { authorization: "pw" });
assert.equal(sammi.body.trigger, "sver_subscription");
assert.equal(forwardings({ topic: "channel:streamer:raid:incoming", data: {} }, both)[0].body.action.name, "S.V.E.R Raid");
assert.deepEqual(forwardings({ topic: "channel:streamer:board", data: {} }, both), [], "only the mapped events");

// End to end: the bridge subscribes to the private topics with its token and forwards an event,
// then resubscribes from the last event ID after the socket drops.
const subscribes = [];
let socket;
const server = await serve((s, request) => {
  socket = s;
  assert.match(request.url, /^\/api\/events\?client_id=client&access_token=access-1$/);
  s.on("message", text => {
    const m = JSON.parse(text);
    if (m.method === "subscribe") subscribes.push(m.params);
  });
});
const posted = [];
const fetch = async (url, init) => {
  if (new URL(url).pathname === "/api/v1/me") return { ok: true, status: 200, json: async () => ({ username: "Streamer" }) };
  posted.push({ url, body: JSON.parse(init.body), headers: init.headers });
  return { ok: true, status: 204, json: async () => ({}) };
};
const tokensPath = join(mkdtempSync(join(tmpdir(), "sver-automation-")), "automation.tokens.json");
writeFileSync(tokensPath, JSON.stringify({ access: "access-1", refresh: "refresh-1", expires: Date.now() + 3_600_000 }));
const until = async test => { for (let i = 0; i < 100 && !test(); i++) await new Promise(r => setTimeout(r, 50)); assert.ok(test()); };
const bridge = await run({ client_id: "client", origin: `http://127.0.0.1:${server.port}`, ...both }, { fetch, tokensPath });
try {
  await until(() => subscribes.length === 1);
  assert.ok(subscribes[0].topics.includes("channel:Streamer:tributes") && subscribes[0].since === undefined);
  socket.send(JSON.stringify({ type: "event", id: 41, topic: "channel:Streamer:tributes", data: { user: "Ann", valor: 500, message: "gg" }, at: "now" }));
  await until(() => posted.length === 2);
  assert.deepEqual(posted[0].body.args, { user: "Ann", valor: "500", message: "gg", sver_event: "Tribute", sver_at: "now" });
  assert.equal(posted[1].body.trigger, "sver_tribute");
  socket.close();
  await until(() => subscribes.length === 2);
  assert.equal(subscribes[1].since, 41, "replays what it missed");
} finally {
  bridge.stop();
  server.close();
}
console.log("automation: ok");
