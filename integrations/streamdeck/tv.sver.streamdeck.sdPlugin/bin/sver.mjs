// S.V.E.R for Stream Deck: sign-in and the key actions, with no Stream Deck code in here so it can
// be tested with plain node (../../test.mjs). It talks only to S.V.E.R's public API as the person
// who signed in (docs/DEVELOPER_PLATFORM.md, "App actions" and "Device sign-in").

export const ORIGIN = "https://sver.tv";
/// The official S.V.E.R Stream Deck app's public client ID (Settings → Developer). Client IDs are
/// public; there is no secret in a desktop app.
export const CLIENT_ID = "";
export const SCOPES = "user:read channel:edit channel:run";

/** A short error people can read on the key's title or in the property inspector. */
export class SverError extends Error {}

/**
 * One signed-in person. `tokens` is { access, refresh, expires } (expires in ms since epoch);
 * `save(tokens)` stores new ones (Stream Deck global settings), and `fetch` is injectable for tests.
 */
export class Sver {
  constructor({ tokens = null, save = () => {}, fetch = globalThis.fetch, origin = ORIGIN, clientId = CLIENT_ID, now = Date.now } = {}) {
    Object.assign(this, { tokens, save, fetch, origin, clientId, now });
    this.username = null;
  }

  async form(path, fields) {
    const response = await this.fetch(`${this.origin}${path}`, { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body: new URLSearchParams(fields).toString() });
    return { status: response.status, body: await response.json().catch(() => ({})) };
  }

  /** Starts device sign-in. Returns { code, url } to show, and a promise that resolves when approved. */
  async signIn({ wait = ms => new Promise(r => setTimeout(r, ms)) } = {}) {
    if (!this.clientId) throw new SverError("This build has no S.V.E.R client ID.");
    const start = await this.form("/api/oauth/device", { client_id: this.clientId, scope: SCOPES });
    if (start.status !== 200) throw new SverError(start.body.error || "Sign-in isn't available right now.");
    const { device_code, user_code, verification_uri_complete, interval = 5, expires_in = 600 } = start.body;
    const done = (async () => {
      let every = interval * 1000;
      const until = this.now() + expires_in * 1000;
      while (this.now() < until) {
        await wait(every);
        const poll = await this.form("/api/oauth/token", { grant_type: "urn:ietf:params:oauth:grant-type:device_code", device_code, client_id: this.clientId });
        if (poll.status === 200) return this.keep(poll.body);
        if (poll.body.error === "slow_down") every += 5000;
        else if (poll.body.error !== "authorization_pending") throw new SverError(poll.body.error === "access_denied" ? "Sign-in was cancelled." : "The code expired. Try again.");
      }
      throw new SverError("The code expired. Try again.");
    })();
    return { code: user_code, url: verification_uri_complete, done };
  }

  keep(body) {
    this.tokens = { access: body.access_token, refresh: body.refresh_token, expires: this.now() + (body.expires_in - 60) * 1000 };
    this.save(this.tokens);
    return this.tokens;
  }

  signOut() {
    this.tokens = null;
    this.username = null;
    this.save(null);
  }

  async refresh() {
    const r = await this.form("/api/oauth/token", { grant_type: "refresh_token", refresh_token: this.tokens.refresh, client_id: this.clientId });
    if (r.status !== 200) { this.signOut(); throw new SverError("Signed out. Sign in again."); }
    return this.keep(r.body);
  }

  /** An API call as the person, refreshing the access token when it's about to expire or was refused once. */
  async call(method, path, body) {
    if (!this.tokens) throw new SverError("Sign in to S.V.E.R first.");
    if (this.now() >= this.tokens.expires) await this.refresh();
    const send = () => this.fetch(`${this.origin}${path}`, { method, headers: { authorization: `Bearer ${this.tokens.access}`, "sver-client-id": this.clientId, ...(body === undefined ? {} : { "content-type": "application/json" }) }, body: body === undefined ? undefined : JSON.stringify(body) });
    let response = await send();
    if (response.status === 401) { await this.refresh(); response = await send(); }
    const data = await response.json().catch(() => ({}));
    if (!response.ok) throw new SverError(data.error || `S.V.E.R said ${response.status}.`);
    return data;
  }

  async me() {
    this.username ??= (await this.call("GET", "/api/v1/me")).username;
    return this.username;
  }
  async channelPath(rest) {
    return `/api/channels/${encodeURIComponent(await this.me())}${rest}`;
  }

  // ---- Key actions ----

  /** Changes the stream title, keeping the category. */
  async setTitle(title) {
    const clean = String(title ?? "").trim();
    if (!clean) throw new SverError("Set a title in the key's settings.");
    const channel = await this.call("GET", `/api/v1/channels/${encodeURIComponent(await this.me())}`);
    if (!channel.category_id) throw new SverError("Pick a category in Creator Studio first.");
    return this.call("PATCH", "/api/me/stream", { title: clean, category_id: channel.category_id, revision: channel.revision });
  }
  raid(username) {
    const target = String(username ?? "").trim().replace(/^@/, "");
    if (!target) throw new SverError("Set who to raid in the key's settings.");
    return this.call("POST", "/api/me/raids", { username: target });
  }
  async poll({ kind = "poll", question, options, seconds = 120 }) {
    const list = (Array.isArray(options) ? options : String(options ?? "").split(",")).map(o => o.trim()).filter(Boolean);
    if (!String(question ?? "").trim() || list.length < 2) throw new SverError("Set a question and at least two options.");
    return this.call("POST", await this.channelPath("/polls"), { kind, question: String(question).trim(), options: list, seconds: Number(seconds) });
  }
  async marker(label) {
    return this.call("POST", await this.channelPath("/marker"), { label: String(label ?? "").trim() || null });
  }
  async boardPaused(disabled) {
    return this.call("PUT", await this.channelPath("/board/disabled"), { disabled: !!disabled });
  }
}
