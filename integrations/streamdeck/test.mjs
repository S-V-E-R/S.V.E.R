// Tests the Stream Deck plugin's S.V.E.R client against a fake API: node streamdeck/test.mjs
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { Sver, SverError } from "./tv.sver.streamdeck.sdPlugin/bin/sver.mjs";
import { serve } from "../test/ws-server.mjs";

const calls = [];
let clock = 1_000_000;
const accessOk = "access-2";
function api(method, path, body, headers) {
  calls.push({ method, path, body, auth: headers.authorization });
  const json = (status, data) => ({ status, ok: status < 300, json: async () => data });
  if (path === "/api/oauth/device") return json(200, { device_code: "dev", user_code: "BCD-FGH", verification_uri_complete: "https://sver.tv/go?code=BCD-FGH", interval: 5, expires_in: 600 });
  if (path === "/api/oauth/token") {
    const form = new URLSearchParams(body);
    if (form.get("grant_type") === "refresh_token") return form.get("refresh_token") === "refresh-1" ? json(200, { access_token: "access-2", refresh_token: "refresh-2", expires_in: 3600 }) : json(400, { error: "invalid_grant" });
    return calls.filter(c => c.path === "/api/oauth/token").length < 2 ? json(400, { error: "authorization_pending" }) : json(200, { access_token: "access-1", refresh_token: "refresh-1", expires_in: 3600 });
  }
  if (headers.authorization !== `Bearer ${accessOk}`) return json(401, { error: "Sign in again." });
  if (path === "/api/v1/me") return json(200, { username: "Streamer" });
  if (path === "/api/v1/channels/Streamer") return json(200, { username: "Streamer", category_id: "cat-1", revision: 7 });
  if (path === "/api/me/raids" && JSON.parse(body).username === "Nobody") return json(409, { error: "That channel isn't live." });
  return json(200, { ok: true });
}
const fetch = async (url, init) => api(init.method, new URL(url).pathname, init.body, init.headers);
let saved;
const sver = new Sver({ fetch, clientId: "client", now: () => clock, save: t => { saved = t; } });

// Device sign-in polls until approved and keeps the tokens.
const waits = [];
const { code, url, done } = await sver.signIn({ wait: async ms => { waits.push(ms); clock += ms; } });
assert.equal(code, "BCD-FGH");
assert.equal(url, "https://sver.tv/go?code=BCD-FGH");
await done;
assert.deepEqual(waits, [5000, 5000], "polls every 5 seconds until approved");
assert.equal(saved.access, "access-1");

// The first call is refused with the old token, refreshes once and retries.
await sver.setTitle("  New title  ");
const patch = calls.find(c => c.method === "PATCH");
assert.deepEqual(JSON.parse(patch.body), { title: "New title", category_id: "cat-1", revision: 7 });
assert.equal(saved.refresh, "refresh-2", "refreshed on 401");

await sver.poll({ kind: "prediction", question: "Win?", options: "Yes, No ,", seconds: "60" });
assert.deepEqual(JSON.parse(calls.at(-1).body), { kind: "prediction", question: "Win?", options: ["Yes", "No"], seconds: 60 });
assert.equal(calls.at(-1).path, "/api/channels/Streamer/polls");
await sver.marker("");
assert.deepEqual(JSON.parse(calls.at(-1).body), { label: null });
await sver.boardPaused(true);
assert.deepEqual([calls.at(-1).method, calls.at(-1).path, JSON.parse(calls.at(-1).body)], ["PUT", "/api/channels/Streamer/board/disabled", { disabled: true }]);
await sver.raid("@Friend");
assert.deepEqual(JSON.parse(calls.at(-1).body), { username: "Friend" });

// Errors people can read; nothing is sent for an unset key.
await assert.rejects(sver.raid("Nobody"), new SverError("That channel isn't live."));
assert.throws(() => sver.raid(""), SverError);
await assert.rejects(sver.poll({ question: "Only one", options: "A" }), SverError);

// An expired access token refreshes before the call; a refused refresh signs out.
clock += 3600 * 1000;
sver.tokens.refresh = "spent";
await assert.rejects(sver.marker("x"), new SverError("Signed out. Sign in again."));
assert.equal(saved, null);
await assert.rejects(sver.marker("x"), new SverError("Sign in to S.V.E.R first."));
await assert.rejects(new Sver({ fetch, clientId: "" }).signIn(), SverError);

// The plugin itself, against a fake Stream Deck: it registers, asks for its saved sign-in, and a
// key pressed while signed out shows the alert and tells the inspector why.
const seen = [];
let deck;
const server = await serve(socket => {
  deck = socket;
  socket.on("message", text => seen.push(JSON.parse(text)));
});
const plugin = spawn(process.execPath, ["streamdeck/tv.sver.streamdeck.sdPlugin/bin/plugin.mjs", "-port", String(server.port), "-pluginUUID", "plugin-1", "-registerEvent", "registerPlugin", "-info", "{}"], { stdio: "inherit" });
const until = async test => { for (let i = 0; i < 100 && !test(); i++) await new Promise(r => setTimeout(r, 50)); assert.ok(test()); };
try {
  await until(() => seen.some(m => m.event === "getGlobalSettings"));
  assert.deepEqual(seen[0], { event: "registerPlugin", uuid: "plugin-1" });
  deck.send(JSON.stringify({ event: "didReceiveGlobalSettings", payload: { settings: {} } }));
  deck.send(JSON.stringify({ event: "keyDown", action: "tv.sver.streamdeck.raid", context: "key-1", payload: { settings: { username: "Friend" } } }));
  await until(() => seen.some(m => m.event === "sendToPropertyInspector"));
  assert.ok(seen.some(m => m.event === "showAlert" && m.context === "key-1"));
  assert.equal(seen.find(m => m.event === "sendToPropertyInspector").payload.error, "Sign in to S.V.E.R first.");
} finally {
  plugin.kill();
  server.close();
}
console.log("streamdeck: ok");
