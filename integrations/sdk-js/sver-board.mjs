// S.V.E.R board SDK for JavaScript and TypeScript (docs/CROWDSYNC.md "Game SDK"). No
// dependencies: it uses the standard WebSocket, so it runs in browsers and Node 22+.
//
//   import { SverBoard } from "./sver-board.mjs";
//   const board = new SverBoard({ token: "sver_g_..." });
//   board.on("press", p => { if (p.control === "jump") player.jump(); });
//   board.on("input", m => player.steer(m.x, m.y));            // joystick moves
//   board.on("hello", () => board.ready());                     // after every (re)connect
//   await board.connect();
//   await board.setState({ jump: { disabled: true, label: "Jump (cooling)" }, coins: { progress: 3 } });
//   await board.define("My game", { screens: [...] });          // the game's own board, if the streamer allows it
//
// "Connect to S.V.E.R" instead of a pasted token: register the game once in Settings → Developer
// (no client secret) and give its client ID. connect() then shows the streamer a code to enter at
// sver.tv/go, keeps the sign-in (in localStorage, or where `store` says) and renews it on its own:
//
//   const board = new SverBoard({ clientId: "…", onCode: ({ code, url }) => show(`Enter ${code} at ${url}`) });
//   await board.connect();                                      // a new code only when the sign-in is gone
//
// Events: "hello" (connected; the published board), "press" (a viewer pressed a control),
// "input" (joystick move), "effect" (a Skill, emote combo or Surge celebration), "board" (a new
// version was published or the board was paused), "state" (labels, availability or goal progress
// changed), "result" (a "Game confirms" press was captured or released), "error", "close". For a
// press with `confirm: true`, call capture(press.id) once the effect happened, or release(press.id). It reconnects on its own unless the token was revoked or the app removed.

export const DEFAULT_GATEWAY = "wss://sver.tv/api/integrations/ws";
export const DEFAULT_API = "https://sver.tv";
const DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code";

/** Keeps the sign-in in a file (Node): `new SverBoard({ clientId, store: fileStore("sver.json") })`. */
export function fileStore(path) {
  const fs = () => import("node:fs/promises");
  return {
    async get() { try { return JSON.parse(await (await fs()).readFile(path, "utf8")); } catch { return null; } },
    async set(grant) {
      const { writeFile, rm } = await fs();
      if (grant) await writeFile(path, JSON.stringify(grant), { mode: 0o600 }); else await rm(path, { force: true });
    },
  };
}
// The browser's localStorage under the client ID; elsewhere (or when it's blocked) only memory.
function defaultStore(clientId) {
  const key = `sver-board:${clientId}`;
  let memory = null, local;
  try { local = globalThis.window?.localStorage; } catch { /* blocked: memory only */ }
  return {
    get() { try { return JSON.parse(local.getItem(key)) ?? memory; } catch { return memory; } },
    set(grant) {
      memory = grant;
      try { if (grant) local.setItem(key, JSON.stringify(grant)); else local.removeItem(key); } catch { /* memory only */ }
    },
  };
}
// An error that trying again won't fix (the streamer refused, the app is unknown, close() was called).
function final(message) { return Object.assign(new Error(message), { final: true }); }

export class SverBoard {
  /**
   * Give `token` (a game token from Creator Studio), or `clientId` to sign in at sver.tv/go.
   * @param {{token?: string, clientId?: string, onCode?: (shown: {code: string, url: string, link: string, expiresIn: number}) => void,
   *   store?: {get(): any, set(grant: any): any}, api?: string, url?: string, reconnect?: boolean, WebSocket?: typeof WebSocket}} options
   */
  constructor({ token, clientId, onCode, store, api = DEFAULT_API, url = DEFAULT_GATEWAY, reconnect = true, WebSocket: Socket = globalThis.WebSocket }) {
    if (!token && !clientId) throw new Error("A connection token from Creator Studio, or your app's client ID, is required.");
    this.token = token;
    this.clientId = token ? undefined : clientId;
    this.onCode = onCode ?? (({ code, url: page }) => console.log(`Connect to S.V.E.R: enter ${code} at ${page}`));
    this.store = store ?? defaultStore(clientId);
    this.api = api.replace(/\/$/, "");
    this.url = url;
    this.reconnect = reconnect;
    this.Socket = Socket;
    this.listeners = new Map();
    this.pending = new Map();
    this.next = 1;
    this.retry = 1000;
    this.stopped = false;
    /** The latest snapshot: {board, version, disabled, state, goals}. */
    this.current = null;
  }

  /** Subscribes to an event; returns a function that unsubscribes. */
  on(type, fn) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type).add(fn);
    return () => this.listeners.get(type).delete(fn);
  }

  emit(type, value) {
    for (const fn of this.listeners.get(type) ?? []) {
      try { fn(value); } catch (error) { console.error(error); }
    }
  }

  /**
   * Connects and resolves with the hello message (the channel and its published board). With a
   * client ID it signs in first: silently while the stored sign-in works, otherwise `onCode` gets
   * a code for the streamer to enter at sver.tv/go and this waits until they approve it.
   */
  connect() {
    this.stopped = false;
    if (!this.clientId) return this.open(this.token);
    return this.accessToken().then(token => this.open(token), error => {
      // S.V.E.R couldn't be reached: keep trying, like a dropped socket. A refusal is final.
      if (!error.final) this.again();
      throw error;
    });
  }

  again() {
    if (!this.reconnect || this.stopped) return;
    setTimeout(() => { if (!this.stopped) this.connect().catch(() => {}); }, this.retry);
    this.retry = Math.min(this.retry * 2, 30000);
  }

  /** A working access token. One sign-in at a time: a refresh token works once, and using it twice ends the grant. */
  accessToken() {
    // ponytail: two tabs of one game share localStorage and can still race a refresh (the loser
    // shows a new code); use the Web Locks API here if games really run in several tabs.
    return this.signing ??= this.signIn().finally(() => { this.signing = null; });
  }

  async signIn() {
    const grant = await this.store.get();
    if (grant && !this.stale && grant.expires_at > Date.now() + 60000) return grant.access_token;
    let tokens = grant?.refresh_token && await this.oauth("token", { grant_type: "refresh_token", refresh_token: grant.refresh_token });
    if (!tokens || tokens.error) {
      // No sign-in, or it's gone (removed in Settings → Connected apps, unused for 60 days): a new code.
      await this.store.set(null);
      tokens = await this.device();
    }
    this.stale = false;
    await this.store.set({ access_token: tokens.access_token, refresh_token: tokens.refresh_token, expires_at: Date.now() + tokens.expires_in * 1000 });
    return tokens.access_token;
  }

  /** Device sign-in (RFC 8628): shows a code, then asks no faster than S.V.E.R's interval until it is approved. */
  async device() {
    for (;;) {
      const start = await this.oauth("device", { scope: "board:control" });
      if (start.error) throw final(`S.V.E.R doesn't know this app (${start.error}). Check the client ID.`);
      this.onCode({ code: start.user_code, url: start.verification_uri, link: start.verification_uri_complete, expiresIn: start.expires_in });
      let wait = start.interval ?? 5;
      for (;;) {
        // A little over the interval, so a request that travels faster than the last isn't "too soon".
        await new Promise(resolve => setTimeout(resolve, wait * 1000 + 250));
        if (this.stopped) throw final("Closed.");
        const tokens = await this.oauth("token", { grant_type: DEVICE_GRANT, device_code: start.device_code });
        if (!tokens.error) return tokens;
        if (tokens.error === "slow_down") wait += 5;
        else if (tokens.error === "expired_token") break; // nobody entered it in time: show a new code
        else if (tokens.error !== "authorization_pending") throw final(tokens.error === "access_denied" ? "The sign-in was cancelled at sver.tv/go." : `S.V.E.R refused the sign-in (${tokens.error}).`);
      }
    }
  }

  // The OAuth endpoints take a form and no secret: the client ID is public. A 400 carries {error}.
  async oauth(path, fields) {
    const response = await fetch(`${this.api}/api/oauth/${path}`, { method: "POST", body: new URLSearchParams({ client_id: this.clientId, ...fields }) });
    if (!response.ok && response.status !== 400) throw new Error(`S.V.E.R sign-in isn't available right now (${response.status}).`);
    return response.json();
  }

  /** Disconnects and forgets the sign-in, here and on S.V.E.R; the next connect() shows a new code. */
  async signOut() {
    this.close();
    const grant = await this.store.get();
    await this.store.set(null);
    if (grant?.refresh_token) await this.oauth("revoke", { token: grant.refresh_token }).catch(() => {});
  }

  open(token) {
    return new Promise((resolve, reject) => {
      // The token goes in the URL because browsers can't set WebSocket headers; use wss://.
      const app = this.clientId ? `&client_id=${encodeURIComponent(this.clientId)}` : "";
      const socket = new this.Socket(`${this.url}?token=${encodeURIComponent(token)}${app}`);
      this.socket = socket;
      let greeted = false;
      socket.onmessage = event => {
        let message;
        try { message = JSON.parse(event.data); } catch { return; }
        if (message.type === "hello") {
          greeted = true;
          this.retry = 1000;
          this.current = snapshot(message);
          this.emit("hello", message);
          resolve(message);
        } else this.dispatch(message);
      };
      socket.onclose = event => {
        for (const { reject: fail } of this.pending.values()) fail(new Error("Disconnected."));
        this.pending.clear();
        this.emit("close", { code: event.code, reason: event.reason });
        if (!greeted) {
          // Refused before hello: a signed-in game renews its access before the next try.
          this.stale = true;
          reject(new Error("Could not connect. Check the token and gateway URL."));
        }
        // 4001: the token was revoked in Creator Studio, or the app was removed in Settings →
        // Connected apps. Don't reconnect; a signed-in game's next connect() shows a new code.
        if (event.code !== 4001) this.again();
        else if (this.clientId) Promise.resolve(this.store.set(null)).catch(() => {});
      };
    });
  }

  dispatch(message) {
    switch (message.type) {
      case "board_effect":
        this.emit(message.control ? "press" : "effect", message);
        break;
      case "board_input":
        this.emit("input", message);
        break;
      case "board":
        this.current = snapshot(message);
        this.emit("board", this.current);
        break;
      case "board_result":
        this.emit("result", message);
        break;
      case "board_state":
        if (this.current) Object.assign(this.current, { state: message.state, goals: message.goals });
        this.emit("state", message);
        break;
      case "ack":
      case "error":
      case "pong": {
        const waiting = this.pending.get(message.id);
        if (waiting) {
          this.pending.delete(message.id);
          if (message.type === "error") waiting.reject(new Error(message.message));
          else waiting.resolve();
        } else if (message.type === "error") this.emit("error", new Error(message.message));
        break;
      }
    }
  }

  request(body) {
    if (!this.socket || this.socket.readyState !== 1) return Promise.reject(new Error("Not connected."));
    const id = this.next++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ ...body, id }));
    });
  }

  /**
   * Changes the published board (game connections only): per control id, `label` (text, or null to
   * restore), `disabled` (true/false) and, for goals, `progress`. At most 10 messages a second.
   */
  setState(controls) { return this.request({ type: "state", controls }); }
  disable(control, disabled = true) { return this.setState({ [control]: { disabled } }); }
  label(control, label) { return this.setState({ [control]: { label } }); }
  progress(control, progress) { return this.setState({ [control]: { progress } }); }
  ping() { return this.request({ type: "ping" }); }
  /**
   * The game is listening (game connections only). Viewers see "Starting…" and can't press from
   * each connect until this, so call it on every "hello", once the game can take input.
   */
  ready() { return this.request({ type: "ready" }); }
  /** A "Game confirms" press happened: the viewer is charged. Games and bridges both may confirm. */
  capture(press) { return this.request({ type: "capture", press }); }
  /** It didn't happen: the viewer is refunded. Unconfirmed presses are released after 60 seconds. */
  release(press) { return this.request({ type: "release", press }); }
  /**
   * Shows each group of viewers one screen (game connections only): by "faction" with
   * {myria|aetheron|glint: screen name}, "random" with [2–4 screen names], or "users" with
   * {username: screen name} (up to 500). setGroups(null) shows everyone every screen again.
   */
  setGroups(by, screens) { return this.request({ type: "groups", by, screens }); }
  /** The most presses and joystick moves a second sent to the game (1–100, or null for none). */
  setInputCap(perSecond) { return this.request({ type: "cap", per_second: perSecond }); }
  /**
   * Makes this game's own board the live board while the game is connected (game connections
   * only). The streamer must have switched on "May set my board" for this connection; otherwise it
   * rejects with the reason. The board is checked like one built in Creator Studio, can't use the
   * channel's own Skills as effects, and its costs can't exceed what a typical viewer of the
   * channel has. The streamer's board comes back when the game disconnects, so call this again
   * after every "hello". At most once every 10 seconds. A "board" event follows with the new version.
   */
  define(name, board) { return this.request({ type: "define", name, board }); }

  close() {
    this.stopped = true;
    this.socket?.close();
  }
}

function snapshot({ board, version, disabled, state, goals, starting, input_cap }) {
  return { board, version, disabled, state, goals, starting, input_cap };
}
