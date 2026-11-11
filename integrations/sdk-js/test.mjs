// Checks the SDK against a fake gateway that speaks the real protocol: `node test.mjs`.
// (The gateway itself is covered by apps/api/crates/sver/tests/streams/gateway.rs.)
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { SverBoard, fileStore } from "./sver-board.mjs";
import { serve } from "../test/ws-server.mjs";

const board = { screens: [{ name: "Game", controls: [{ id: "jump", kind: "button", label: "Jump" }] }] };
let client, allowed = false;
const seen = [];
const gateway = await serve((socket, request) => {
  client = socket;
  assert.equal(new URL(request.url, "http://x").searchParams.get("token"), "sver_g_test");
  socket.on("message", text => {
    const message = JSON.parse(text);
    seen.push(message);
    if (["ready", "cap", "capture", "release", "groups"].includes(message.type)) socket.send(JSON.stringify({ type: "ack", id: message.id }));
    if (message.type === "define") {
      // Refused until the streamer allows it; then the new board follows the ack, like a publish.
      if (!allowed) socket.send(JSON.stringify({ type: "error", id: message.id, message: "The streamer hasn't let this connection set their board." }));
      else {
        socket.send(JSON.stringify({ type: "ack", id: message.id, version: 5 }));
        socket.send(JSON.stringify({ type: "board", board: message.board, version: 5, disabled: false, state: {}, goals: {} }));
      }
    }
    if (message.type === "state") {
      if (message.controls.jump?.label === "") socket.send(JSON.stringify({ type: "error", id: message.id, message: "Labels are 1–40 characters." }));
      else {
        socket.send(JSON.stringify({ type: "board_state", state: { jump: message.controls.jump }, goals: {} }));
        socket.send(JSON.stringify({ type: "ack", id: message.id }));
      }
    }
  });
  socket.send(JSON.stringify({ type: "hello", kind: "game", channel: "Tester", protocol: 1, board, version: 3, disabled: false, state: {}, goals: {} }));
});

const sdk = new SverBoard({ token: "sver_g_test", url: `ws://127.0.0.1:${gateway.port}/api/integrations/ws`, reconnect: false });
const events = [];
for (const type of ["press", "input", "effect", "board", "state", "result"]) sdk.on(type, value => events.push([type, value]));

const hello = await sdk.connect();
assert.equal(hello.channel, "Tester");
assert.equal(sdk.current.version, 3);

// Events are sorted by kind.
client.send(JSON.stringify({ type: "board_effect", control: "jump", label: "Jump", user: { username: "Viewer" } }));
client.send(JSON.stringify({ type: "board_input", control: "stick", x: 0.5, y: 0 }));
client.send(JSON.stringify({ type: "board_effect", skill: "crown", caption: "Viewer played Crown" }));
client.send(JSON.stringify({ type: "board", board, version: 4, disabled: true, state: {}, goals: {} }));
await new Promise(resolve => setTimeout(resolve, 100));
assert.deepEqual(events.map(e => e[0]), ["press", "input", "effect", "board"]);
assert.equal(events[0][1].user.username, "Viewer");
assert.equal(sdk.current.version, 4);

// State changes are acknowledged, or rejected with the gateway's message.
await sdk.setState({ jump: { disabled: true, label: "Jump (cooling)" } });
assert.equal(sdk.current.state.jump.disabled, true);
await assert.rejects(sdk.label("jump", ""), /1–40/);
const sent = seen.filter(m => m.type === "state");
assert.equal(sent.length, 2);
assert.notEqual(sent[0].id, sent[1].id, "each request has its own id");

// Ready and the input cap.
await sdk.ready();
await sdk.setInputCap(20);
assert.deepEqual(seen.filter(m => m.type === "ready" || m.type === "cap").map(m => m.per_second ?? m.type), ["ready", 20]);

// Groups.
await sdk.setGroups("random", ["Red", "Blue"]);
assert.deepEqual(seen.at(-1).screens, ["Red", "Blue"]);

// Confirming a held press, and its result event.
await sdk.capture("p1");
await sdk.release("p2");
assert.deepEqual(seen.filter(m => m.type === "capture" || m.type === "release").map(m => [m.type, m.press]), [["capture", "p1"], ["release", "p2"]]);
client.send(JSON.stringify({ type: "board_result", id: "p2", control: "boost", outcome: "released" }));
await new Promise(resolve => setTimeout(resolve, 100));
assert.equal(events.at(-1)[0], "result");

// Defining the game's own board: refused without the streamer's permission, live with it.
const own = { screens: [{ name: "Runner", controls: [{ id: "hop", kind: "button", label: "Hop" }] }] };
await assert.rejects(sdk.define("Crowd Runner", own), /set their board/);
allowed = true;
await sdk.define("Crowd Runner", own);
await new Promise(resolve => setTimeout(resolve, 100));
assert.deepEqual(seen.at(-1), { type: "define", name: "Crowd Runner", board: own, id: seen.at(-1).id });
assert.equal(events.at(-1)[0], "board");
assert.deepEqual([sdk.current.version, sdk.current.board.screens[0].name], [5, "Runner"]);

sdk.close();
gateway.close();

// ---- "Connect to S.V.E.R": device sign-in at sver.tv/go instead of a pasted token ----
// The fake speaks /api/oauth/device, /api/oauth/token and /api/oauth/revoke like the API does
// (apps/api/crates/sver/tests/streams/devapps.rs covers the real ones).
const INTERVAL = 0.2; // seconds; the real one is 5
const fake = { codes: 0, issued: 0, approved: false, refresh: null, polls: [], forms: [], revoked: [], sockets: [] };
function issue() {
  fake.issued++;
  fake.refresh = `svr_${fake.issued}`;
  return { access_token: `sva_${fake.issued}`, token_type: "Bearer", expires_in: 3600, refresh_token: fake.refresh, scope: "board:control" };
}
const server = await serve((socket, request) => {
  const query = new URL(request.url, "http://x").searchParams;
  fake.sockets.push({ socket, token: query.get("token"), client: query.get("client_id") });
  socket.send(JSON.stringify({ type: "hello", kind: "game", channel: "Tester", protocol: 1, board, version: 1, disabled: false, state: {}, goals: {} }));
}, (request, response) => {
  let body = "";
  request.on("data", chunk => { body += chunk; });
  request.on("end", () => {
    assert.equal(request.headers["content-type"], "application/x-www-form-urlencoded;charset=UTF-8");
    const form = Object.fromEntries(new URLSearchParams(body));
    fake.forms.push(form);
    const reply = (status, value) => response.writeHead(status, { "content-type": "application/json" }).end(JSON.stringify(value));
    if (form.client_id !== "game-app") return reply(400, { error: "invalid_client" });
    if (request.url === "/api/oauth/device") {
      fake.codes++;
      fake.approved = false;
      return reply(200, { device_code: `svd_${fake.codes}`, user_code: "BCD-FGH", verification_uri: "https://sver.tv/go", verification_uri_complete: "https://sver.tv/go?code=BCD-FGH", expires_in: 600, interval: INTERVAL });
    }
    if (request.url === "/api/oauth/revoke") { fake.revoked.push(form.token); fake.refresh = null; return reply(200, {}); }
    if (form.grant_type === "refresh_token") return form.refresh_token === fake.refresh ? reply(200, issue()) : reply(400, { error: "invalid_grant" });
    fake.polls.push(Date.now());
    if (form.device_code !== `svd_${fake.codes}`) return reply(400, { error: "invalid_grant" });
    return fake.approved ? reply(200, issue()) : reply(400, { error: "authorization_pending" });
  });
});
const memory = () => { let grant = null; return { get: () => grant, set: value => { grant = value; } }; };
const store = memory();
const shown = [];
const signedIn = () => new SverBoard({
  clientId: "game-app", store, reconnect: false, api: `http://127.0.0.1:${server.port}/`, url: `ws://127.0.0.1:${server.port}/api/integrations/ws`,
  // The streamer enters the code at sver.tv/go a little later: after the game has asked twice.
  onCode: code => { shown.push(code); setTimeout(() => { fake.approved = true; }, INTERVAL * 2000 + 100); },
});

// No sign-in yet: the game gets a code to show, waits for the approval and connects with the
// access token and its client ID.
let game = signedIn();
assert.equal((await game.connect()).channel, "Tester");
assert.deepEqual(shown, [{ code: "BCD-FGH", url: "https://sver.tv/go", link: "https://sver.tv/go?code=BCD-FGH", expiresIn: 600 }]);
assert.deepEqual([fake.sockets[0].token, fake.sockets[0].client], ["sva_1", "game-app"]);
assert.equal(fake.forms[0].scope, "board:control");
assert.ok(fake.polls.length >= 2, "it waited for the approval");
for (let i = 1; i < fake.polls.length; i++) assert.ok(fake.polls[i] - fake.polls[i - 1] >= INTERVAL * 1000, "never faster than the interval");
assert.equal((await store.get()).refresh_token, "svr_1");
game.close();

// The stored sign-in is used as it is: no code and no call to S.V.E.R's sign-in.
const calls = fake.forms.length;
game = signedIn();
await game.connect();
assert.deepEqual([fake.sockets[1].token, fake.forms.length, shown.length], ["sva_1", calls, 1]);
game.close();

// An access token past its hour is renewed with the refresh token, still without a code.
await store.set({ ...await store.get(), expires_at: Date.now() - 1 });
game = signedIn();
await game.connect();
assert.deepEqual([fake.sockets[2].token, (await store.get()).refresh_token, shown.length], ["sva_2", "svr_2", 1]);

// The streamer removed the app: the gateway closes with 4001, the sign-in is forgotten, and the
// next connect() shows a new code.
fake.refresh = null;
fake.sockets[2].socket.close(4001);
await new Promise(resolve => game.on("close", resolve));
await new Promise(resolve => setTimeout(resolve, 20));
assert.equal(await store.get(), null);
await game.connect();
assert.deepEqual([shown.length, fake.sockets[3].token], [2, "sva_3"]);
game.close();

// A sign-in S.V.E.R no longer accepts (removed while the game was closed) also falls back to a code.
fake.refresh = null;
await store.set({ ...await store.get(), expires_at: Date.now() - 1 });
game = signedIn();
await game.connect();
assert.deepEqual([shown.length, fake.sockets[4].token], [3, "sva_4"]);

// Signing out revokes the grant and forgets it.
await game.signOut();
assert.deepEqual([fake.revoked, await store.get()], [["svr_4"], null]);

// Cancel at sver.tv/go, or an unknown app, is an error the game can show; it doesn't retry.
await assert.rejects(new SverBoard({ clientId: "nobody", store: memory(), api: `http://127.0.0.1:${server.port}`, onCode() {} }).connect(), /Check the client ID/);

// A public client: no secret is ever sent.
assert.ok(fake.forms.every(form => !("client_secret" in form)));

// The file store for Node games keeps the sign-in between runs.
const folder = mkdtempSync(join(tmpdir(), "sver-board-"));
const file = fileStore(join(folder, "sign-in.json"));
assert.equal(await file.get(), null);
await file.set({ refresh_token: "svr_x" });
assert.deepEqual(await fileStore(join(folder, "sign-in.json")).get(), { refresh_token: "svr_x" });
await file.set(null);
assert.equal(await file.get(), null);
rmSync(folder, { recursive: true });
server.close();

console.log("SDK checks passed: connect, event routing, state acks and errors, ready and cap, groups, confirms, define, sign-in at sver.tv/go (code, stored sign-in, renewal, new code when it's gone, sign-out).");
process.exit(0);
