#!/usr/bin/env node
// S.V.E.R automation bridge (docs/DEVELOPER_PLATFORM.md §7): a small app on the streamer's PC that
// signs in once with a code at sver.tv/go, listens to the channel's live events, and fires them
// into Streamer.bot (its HTTP server's DoAction) and SAMMI (its local webhook). It never connects
// to S.V.E.R's database or servers directly, and it has no dependencies: Node 22 or newer.
//
//   node sver-automation.mjs automation.json
//
// automation.json (see automation.example.json): the S.V.E.R app client ID and where Streamer.bot
// and SAMMI listen. Sign-in tokens are kept beside it in automation.tokens.json.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { Sver, ORIGIN } from "../streamdeck/tv.sver.streamdeck.sdPlugin/bin/sver.mjs";

export const SCOPES = "user:read events:private";
/** Event topics (after `channel:{name}:`) and the name each has in Streamer.bot and SAMMI. */
export const KINDS = { live: "Live", "follows:detail": "Follow", subs: "Subscription", tributes: "Tribute", skills: "Skill", "raid:incoming": "Raid" };
const log = (...parts) => console.log(new Date().toISOString(), ...parts);

/** Event data as flat string arguments: Streamer.bot args and SAMMI pull data read them by name. */
export function flatten(data, prefix = "", out = {}) {
  for (const [key, value] of Object.entries(data ?? {})) {
    const name = prefix ? `${prefix}_${key}` : key;
    if (value && typeof value === "object" && !Array.isArray(value)) flatten(value, name, out);
    else out[name] = Array.isArray(value) ? JSON.stringify(value) : String(value ?? "");
  }
  return out;
}

/** The local requests one S.V.E.R event becomes: { url, headers, body } for each configured target. */
export function forwardings(event, config) {
  const kind = String(event.topic ?? "").split(":").slice(2).join(":");
  const name = KINDS[kind];
  if (!name) return [];
  const args = { ...flatten(event.data), sver_event: name, sver_at: String(event.at ?? "") };
  const out = [];
  if (config.streamerbot) {
    const prefix = config.streamerbot.prefix ?? "S.V.E.R";
    out.push({ url: `${config.streamerbot.url ?? "http://127.0.0.1:7474"}/DoAction`, headers: {}, body: { action: { name: `${prefix} ${name}` }, args } });
  }
  if (config.sammi) {
    out.push({ url: `${config.sammi.url ?? "http://127.0.0.1:9450"}/webhook`, headers: config.sammi.password ? { authorization: config.sammi.password } : {}, body: { ...args, trigger: `sver_${name.toLowerCase()}` } });
  }
  return out;
}

/**
 * Runs until `stop()`: signs in if needed, subscribes to the channel's events, reconnects with
 * backoff (replaying what it missed, up to S.V.E.R's 5 minutes) and forwards every event.
 */
export async function run(config, { tokensPath, fetch = globalThis.fetch, WebSocketImpl = globalThis.WebSocket } = {}) {
  const tokens = tokensPath && existsSync(tokensPath) ? JSON.parse(readFileSync(tokensPath, "utf8")) : null;
  const origin = config.origin ?? ORIGIN;
  const sver = new Sver({ tokens, fetch, origin, clientId: config.client_id, scopes: SCOPES, save: t => tokensPath && writeFileSync(tokensPath, JSON.stringify(t), { mode: 0o600 }) });
  if (!sver.tokens) {
    const { code, url, done } = await sver.signIn();
    log(`Sign in: open ${url} and approve code ${code}.`);
    await done;
  }
  const me = await sver.me();
  const topics = Object.keys(KINDS).map(kind => `channel:${me}:${kind}`);
  log(`Signed in as @${me}. Forwarding ${Object.values(KINDS).join(", ")}.`);
  let stopped = false, ws = null, since = null, wait = 1000, ping;
  const forward = async event => {
    for (const f of forwardings(event, config)) {
      try {
        const r = await fetch(f.url, { method: "POST", headers: { "content-type": "application/json", ...f.headers }, body: JSON.stringify(f.body) });
        if (!r.ok) log(`${f.url} answered ${r.status}.`);
      } catch {
        log(`${f.url} isn't reachable. Is it running with its server on?`);
      }
    }
  };
  const connect = async () => {
    const access = await sver.access().catch(error => { log(error.message); return null; });
    if (stopped || !access) return;
    const url = `${origin.replace(/^http/, "ws")}/api/events?client_id=${encodeURIComponent(config.client_id)}&access_token=${encodeURIComponent(access)}`;
    ws = new WebSocketImpl(url);
    ws.onopen = () => {
      wait = 1000;
      ws.send(JSON.stringify({ type: "method", id: 1, method: "subscribe", params: since === null ? { topics } : { topics, since } }));
      ping = setInterval(() => ws.send(JSON.stringify({ type: "method", id: 2, method: "ping" })), 30_000);
    };
    ws.onmessage = ({ data }) => {
      const message = JSON.parse(data);
      if (message.type === "reply" && message.result?.refused?.length) log(`Not allowed: ${message.result.refused.join(", ")}`);
      if (message.type !== "event") return;
      if (Number.isInteger(message.id)) since = message.id;
      void forward(message);
    };
    ws.onclose = () => {
      clearInterval(ping);
      if (stopped) return;
      setTimeout(() => void connect(), wait);
      wait = Math.min(wait * 2, 30_000);
    };
  };
  await connect();
  return { me, stop() { stopped = true; clearInterval(ping); ws?.close(); } };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const path = process.argv[2] ?? "automation.json";
  const config = JSON.parse(readFileSync(path, "utf8"));
  await run(config, { tokensPath: path.replace(/\.json$/, "") + ".tokens.json" });
}
