// Checks the SDK against a fake gateway that speaks the real protocol: `node test.mjs`.
// (The gateway itself is covered by apps/api/crates/sver/tests/streams/gateway.rs.)
import assert from "node:assert/strict";
import { SverBoard } from "./sver-board.mjs";
import { serve } from "../test/ws-server.mjs";

const board = { screens: [{ name: "Game", controls: [{ id: "jump", kind: "button", label: "Jump" }] }] };
let client;
const seen = [];
const gateway = await serve((socket, request) => {
  client = socket;
  assert.equal(new URL(request.url, "http://x").searchParams.get("token"), "sver_g_test");
  socket.on("message", text => {
    const message = JSON.parse(text);
    seen.push(message);
    if (["ready", "cap", "capture", "release"].includes(message.type)) socket.send(JSON.stringify({ type: "ack", id: message.id }));
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

// Confirming a held press, and its result event.
await sdk.capture("p1");
await sdk.release("p2");
assert.deepEqual(seen.filter(m => m.type === "capture" || m.type === "release").map(m => [m.type, m.press]), [["capture", "p1"], ["release", "p2"]]);
client.send(JSON.stringify({ type: "board_result", id: "p2", control: "boost", outcome: "released" }));
await new Promise(resolve => setTimeout(resolve, 100));
assert.equal(events.at(-1)[0], "result");

sdk.close();
gateway.close();
console.log("SDK checks passed: connect, event routing, state acks and errors, ready and cap.");
process.exit(0);
