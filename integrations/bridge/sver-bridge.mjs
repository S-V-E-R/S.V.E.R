#!/usr/bin/env node
// The S.V.E.R bridge (docs/CROWDSYNC.md "Outputs"): a small app on the streamer's PC that turns
// board presses into OBS scene and source changes. It connects to S.V.E.R's integration gateway
// with a bridge token from Creator Studio and to OBS through OBS's own WebSocket server (Tools →
// WebSocket Server Settings). It never connects to S.V.E.R's database or servers directly, and it
// has no dependencies: Node 22 or newer.
//
//   node sver-bridge.mjs bridge.json
//
// bridge.json (see bridge.example.json): the token, OBS's address and password, and "actions":
// what each board control does. A key is a control id, "skill:<id>" for a Skill, or "surge" for a
// Surge level. Each action is a list of steps run in order:
//   {"scene": "BRB"}                                   switch the program scene
//   {"source": "Confetti", "scene": "Main", "visible": true | false | "toggle"}
//   {"filter": "Shake", "source": "Camera", "enabled": true | false}
//   {"wait": 3000}                                     pause (milliseconds)
// and an optional "cooldown" (milliseconds) per key: {"steps": [...], "cooldown": 10000}.
import { createHash, randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export const DEFAULT_GATEWAY = "wss://sver.tv/api/integrations/ws";
const log = (...parts) => console.log(new Date().toISOString(), ...parts);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

/** OBS WebSocket v5: authentication = base64(sha256(base64(sha256(password + salt)) + challenge)). */
export function obsAuth(password, salt, challenge) {
  const secret = createHash("sha256").update(password + salt).digest("base64");
  return createHash("sha256").update(secret + challenge).digest("base64");
}

/** A connection to OBS's WebSocket server: `request(type, data)` resolves with the response data. */
export function connectObs({ url = "ws://127.0.0.1:4455", password = "" }) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url, "obswebsocket.json");
    const waiting = new Map();
    const obs = {
      request(requestType, requestData = {}) {
        const requestId = randomUUID();
        return new Promise((ok, fail) => {
          waiting.set(requestId, { ok, fail });
          socket.send(JSON.stringify({ op: 6, d: { requestType, requestId, requestData } }));
        });
      },
      close: () => socket.close(),
    };
    socket.onmessage = event => {
      const { op, d } = JSON.parse(event.data);
      if (op === 0) {
        const identify = { rpcVersion: 1, eventSubscriptions: 0 };
        if (d.authentication) identify.authentication = obsAuth(password, d.authentication.salt, d.authentication.challenge);
        socket.send(JSON.stringify({ op: 1, d: identify }));
      } else if (op === 2) resolve(obs);
      else if (op === 7) {
        const pending = waiting.get(d.requestId);
        if (!pending) return;
        waiting.delete(d.requestId);
        if (d.requestStatus?.result) pending.ok(d.responseData ?? {});
        else pending.fail(new Error(`OBS ${d.requestType}: ${d.requestStatus?.comment ?? d.requestStatus?.code}`));
      }
    };
    socket.onclose = event => {
      for (const { fail } of waiting.values()) fail(new Error("OBS disconnected."));
      reject(new Error(event.code === 4009 ? "OBS rejected the password." : "Could not connect to OBS."));
    };
  });
}

/** Runs one action's steps against OBS. */
export async function runSteps(obs, steps) {
  for (const step of steps) {
    if (step.wait) await sleep(Math.min(step.wait, 60000));
    else if (step.filter) {
      await obs.request("SetSourceFilterEnabled", { sourceName: step.source, filterName: step.filter, filterEnabled: step.enabled !== false });
    } else if (step.source) {
      const { sceneItemId } = await obs.request("GetSceneItemId", { sceneName: step.scene, sourceName: step.source });
      let enabled = step.visible ?? true;
      if (enabled === "toggle") {
        const current = await obs.request("GetSceneItemEnabled", { sceneName: step.scene, sceneItemId });
        enabled = !current.sceneItemEnabled;
      }
      await obs.request("SetSceneItemEnabled", { sceneName: step.scene, sceneItemId, sceneItemEnabled: enabled });
    } else if (step.scene) await obs.request("SetCurrentProgramScene", { sceneName: step.scene });
  }
}

/** The action key for a gateway event: a control id, "skill:<id>" or "surge". */
export function actionKey(event) {
  if (event.type !== "board_effect") return null;
  if (event.control) return event.control;
  if (event.skill) return `skill:${event.skill}`;
  if (event.surge) return "surge";
  return null;
}

/** Runs the bridge until stopped; returns {stop, idle}. Actions run one at a time, in order. */
export async function startBridge(config, { obs: givenObs } = {}) {
  if (!config.token?.startsWith("sver_b_")) throw new Error("Use a bridge token from Creator Studio → Board → Connections.");
  const obs = givenObs ?? await connectObs(config.obs ?? {});
  log("Connected to OBS.");
  const cooldowns = new Map();
  let queue = Promise.resolve();
  let stopped = false;
  let socket;
  let retry = 1000;
  const handle = event => {
    const key = actionKey(event);
    const action = key && config.actions?.[key];
    if (!action) return;
    const steps = Array.isArray(action) ? action : action.steps ?? [];
    const cooldown = Array.isArray(action) ? 0 : action.cooldown ?? 0;
    const now = Date.now();
    if ((cooldowns.get(key) ?? 0) > now) return;
    cooldowns.set(key, now + cooldown);
    queue = queue.then(() => runSteps(obs, steps))
      .then(() => log(`Ran "${key}" for ${event.user?.username ?? "the crowd"}.`))
      .catch(error => log(`"${key}" failed: ${error.message}`));
  };
  const connect = () => new Promise(resolve => {
    socket = new WebSocket(`${config.gateway ?? DEFAULT_GATEWAY}?token=${encodeURIComponent(config.token)}`);
    socket.onmessage = message => {
      const event = JSON.parse(message.data);
      if (event.type === "hello") {
        retry = 1000;
        log(`Connected to S.V.E.R as ${event.channel}'s bridge.`);
        resolve();
      } else handle(event);
    };
    socket.onclose = event => {
      resolve();
      if (stopped) return;
      if (event.code === 4001) { log("The bridge token was revoked. Create a new one in Creator Studio."); return; }
      log(`Disconnected from S.V.E.R; retrying in ${retry / 1000}s.`);
      setTimeout(() => { if (!stopped) connect(); }, retry);
      retry = Math.min(retry * 2, 30000);
    };
  });
  await connect();
  return {
    stop() { stopped = true; socket?.close(); if (!givenObs) obs.close(); },
    idle: () => queue,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const file = process.argv[2] ?? "bridge.json";
  let config;
  try { config = JSON.parse(readFileSync(file, "utf8")); } catch (error) {
    console.error(`Could not read ${file}: ${error.message}. Copy bridge.example.json to bridge.json and fill it in.`);
    process.exit(1);
  }
  startBridge(config).catch(error => { console.error(error.message); process.exit(1); });
}
