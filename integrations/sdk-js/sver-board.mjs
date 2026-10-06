// S.V.E.R board SDK for JavaScript and TypeScript (docs/CROWDSYNC.md "Game SDK"). No
// dependencies: it uses the standard WebSocket, so it runs in browsers and Node 22+.
//
//   import { SverBoard } from "./sver-board.mjs";
//   const board = new SverBoard({ token: "sver_g_..." });
//   board.on("press", p => { if (p.control === "jump") player.jump(); });
//   board.on("input", m => player.steer(m.x, m.y));            // joystick moves
//   await board.connect();
//   await board.setState({ jump: { disabled: true, label: "Jump (cooling)" }, coins: { progress: 3 } });
//
// Events: "hello" (connected; the published board), "press" (a viewer pressed a control),
// "input" (joystick move), "effect" (a Skill, emote combo or Surge celebration), "board" (a new
// version was published or the board was paused), "state" (labels, availability or goal progress
// changed), "error", "close". It reconnects on its own unless the token was revoked.

export const DEFAULT_GATEWAY = "wss://sver.tv/api/integrations/ws";

export class SverBoard {
  /** @param {{token: string, url?: string, reconnect?: boolean, WebSocket?: typeof WebSocket}} options */
  constructor({ token, url = DEFAULT_GATEWAY, reconnect = true, WebSocket: Socket = globalThis.WebSocket }) {
    if (!token) throw new Error("A connection token from Creator Studio is required.");
    this.token = token;
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

  /** Connects and resolves with the hello message (the channel and its published board). */
  connect() {
    this.stopped = false;
    return new Promise((resolve, reject) => {
      // The token goes in the URL because browsers can't set WebSocket headers; use wss://.
      const socket = new this.Socket(`${this.url}?token=${encodeURIComponent(this.token)}`);
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
        if (!greeted) reject(new Error("Could not connect. Check the token and gateway URL."));
        // 4001: the token was revoked in Creator Studio.
        if (this.reconnect && !this.stopped && event.code !== 4001) {
          setTimeout(() => { if (!this.stopped) this.connect().catch(() => {}); }, this.retry);
          this.retry = Math.min(this.retry * 2, 30000);
        }
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

  close() {
    this.stopped = true;
    this.socket?.close();
  }
}

function snapshot({ board, version, disabled, state, goals }) {
  return { board, version, disabled, state, goals };
}
