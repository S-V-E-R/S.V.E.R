// Checks the bridge against a fake OBS WebSocket server (v5, with a password) and a fake S.V.E.R
// gateway: `node test.mjs`. The gateway itself is covered by the API's gateway test.
import assert from "node:assert/strict";
import { actionKey, connectObs, obsAuth, startBridge } from "./sver-bridge.mjs";
import { serve } from "../test/ws-server.mjs";

const requests = [];
const obsServer = await serve(socket => {
  socket.send(JSON.stringify({ op: 0, d: { obsWebSocketVersion: "5.5.0", rpcVersion: 1, authentication: { challenge: "c-123", salt: "s-456" } } }));
  socket.on("message", text => {
    const { op, d } = JSON.parse(text);
    if (op === 1) {
      if (d.authentication !== obsAuth("hunter2", "s-456", "c-123")) return socket.close(4009);
      return socket.send(JSON.stringify({ op: 2, d: { negotiatedRpcVersion: 1 } }));
    }
    if (op !== 6) return;
    requests.push([d.requestType, d.requestData]);
    const responseData = d.requestType === "GetSceneItemId" ? { sceneItemId: 7 }
      : d.requestType === "GetSceneItemEnabled" ? { sceneItemEnabled: false } : {};
    const ok = d.requestData.sceneName !== "Missing";
    socket.send(JSON.stringify({ op: 7, d: { requestType: d.requestType, requestId: d.requestId, requestStatus: { result: ok, code: ok ? 100 : 600, comment: ok ? undefined : "No source was found." }, responseData } }));
  });
});
let gatewaySocket;
const settled = [];
const gateway = await serve((socket, request) => {
  gatewaySocket = socket;
  socket.on("message", text => settled.push(JSON.parse(text)));
  assert.equal(new URL(request.url, "http://x").searchParams.get("token"), "sver_b_test");
  socket.send(JSON.stringify({ type: "hello", kind: "bridge", channel: "Tester", protocol: 1, board: null }));
});

// The OBS password handshake.
await assert.rejects(connectObs({ url: `ws://127.0.0.1:${obsServer.port}`, password: "wrong" }), /password/);
assert.equal(actionKey({ type: "board_effect", skill: "crown" }), "skill:crown");
assert.equal(actionKey({ type: "board_input", control: "stick" }), null);

const bridge = await startBridge({
  token: "sver_b_test",
  gateway: `ws://127.0.0.1:${gateway.port}/api/integrations/ws`,
  obs: { url: `ws://127.0.0.1:${obsServer.port}`, password: "hunter2" },
  actions: {
    brb: [{ scene: "BRB" }, { wait: 10 }, { scene: "Main" }],
    confetti: { steps: [{ source: "Confetti", scene: "Main", visible: "toggle" }], cooldown: 60000 },
    shake: [{ filter: "Shake", source: "Camera", enabled: true }],
    "skill:crown": [{ source: "Crown", scene: "Main", visible: true }],
    broken: [{ scene: "Missing" }],
  },
});
const press = event => gatewaySocket.send(JSON.stringify({ type: "board_effect", user: { username: "Viewer" }, ...event }));
press({ control: "brb" });
press({ control: "confetti" });
press({ control: "confetti" }); // cooling down: ignored
press({ control: "unmapped" });
press({ control: "broken" }); // OBS error: logged, the queue keeps going
press({ control: "shake" });
press({ skill: "crown" });
press({ control: "shake", confirm: true, id: "p-ok" });
press({ control: "broken", confirm: true, id: "p-bad" });
await new Promise(resolve => setTimeout(resolve, 300));
await bridge.idle();

assert.deepEqual(requests.map(r => r[0]), [
  "SetCurrentProgramScene", "SetCurrentProgramScene",
  "GetSceneItemId", "GetSceneItemEnabled", "SetSceneItemEnabled",
  "SetCurrentProgramScene",
  "SetSourceFilterEnabled",
  "GetSceneItemId", "SetSceneItemEnabled",
  "SetSourceFilterEnabled", "SetCurrentProgramScene",
]);
assert.deepEqual(settled, [{ type: "capture", press: "p-ok" }, { type: "release", press: "p-bad" }], "confirm presses are settled");
assert.deepEqual(requests[0][1], { sceneName: "BRB" });
assert.deepEqual(requests[4][1], { sceneName: "Main", sceneItemId: 7, sceneItemEnabled: true }, "toggled on");
assert.deepEqual(requests[6][1], { sourceName: "Camera", filterName: "Shake", filterEnabled: true });

bridge.stop();
console.log("Bridge checks passed: OBS password handshake, scene/source/filter steps, waits, cooldowns, Skills, errors.");
process.exit(0);
