// Component acceptance only: synthetic API responses and the existing test-only jsdom.
// node scripts/check-stream-studio.cjs C:\streaming\SVER\node_modules\jsdom
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const webRequire = createRequire(path.resolve("apps/web/package.json"));
const { JSDOM } = require(process.argv[2] || "jsdom");
const dom = new JSDOM('<div id="root"></div>', { url: "http://localhost/studio/stream", pretendToBeVisual: true });
for (const name of ["window", "document", "FormData", "HTMLElement", "HTMLInputElement", "Event"]) global[name] = dom.window[name];
Object.defineProperty(global, "navigator", { value: dom.window.navigator, configurable: true });
global.IS_REACT_ACT_ENVIRONMENT = true;
const React = webRequire("react");
const { act } = React;
const { createRoot } = webRequire("react-dom/client");
const ts = webRequire("typescript");
const nativeTimeout = global.setTimeout, nativeClear = global.clearTimeout;
let poll, expiry, hidden = false, copied = "", delayedKey, delayNextKey = false;
global.setInterval = fn => { poll = fn; return 1; };
global.clearInterval = () => {};
global.setTimeout = (fn, ms, ...args) => ms === 60000 ? (expiry = fn, -1) : nativeTimeout(fn, ms, ...args);
global.clearTimeout = id => { if (id !== -1) nativeClear(id); };
Object.defineProperty(document, "hidden", { get: () => hidden, configurable: true });
Object.defineProperty(navigator, "clipboard", { value: { writeText: async value => { copied = value; } } });
let state = { configured: true, eligible: true, disconnect_pending: false, credential: null, broadcast: null,
  settings: { title: "Original title", category_id: "coding", revision: 0 } };
const requests = [];
const reply = (data, status = 200) => ({ ok: status === 200, status, json: async () => structuredClone(data) });
global.fetch = async (url, options = {}) => {
  const body = options.body ? JSON.parse(options.body) : null;
  requests.push({ url, method: options.method, body });
  if (url.startsWith("/api/categories?")) return reply({ categories: [{ id: "coding", name: "Coding" }] });
  if (url === "/api/auth/me") return reply({ has_password: true, reauthenticated: false });
  if (url === "/api/auth/reauth") { assert.equal(body.password, "synthetic-password"); return reply({ confirmed: true }); }
  if (url === "/api/me/stream" && options.method === "GET") return reply(state);
  if (url === "/api/me/stream" && options.method === "PATCH") {
    if (body.revision !== state.settings.revision) return reply({ error: "This changed in another tab. Reload to see the latest." }, 409);
    state.settings = { ...body, revision: body.revision + 1 };
    return reply({ revision: state.settings.revision });
  }
  if (url === "/api/me/stream/stop") {
    state.credential.revoked = true; state.disconnect_pending = true;
    return reply({ disconnect_pending: true });
  }
  assert.match(url, /^\/api\/me\/stream\/key(\/reveal|\/rotate)?$/);
  assert.equal(body.code, "synthetic-proof");
  state.credential = { created_at: new Date().toISOString(), revoked: false };
  const result = { server: "rtmp://localhost/rebuild", key: "synthetic-secret-key", disconnect_pending: false };
  if (delayNextKey) { delayNextKey = false; return new Promise(resolve => { delayedKey = () => resolve(reply(result)); }); }
  return reply(result);
};
const cache = new Map();
function component(file) {
  file = path.resolve(file);
  if (cache.has(file)) return cache.get(file).exports;
  const compiled = { exports: {} }; cache.set(file, compiled);
  const source = ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: {
    jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true,
  } }).outputText;
  vm.runInThisContext("(function(require,module,exports){" + source + "\n})", { filename: file })(name => {
    if (name === "next/link") return ({ href, children, ...props }) => React.createElement("a", { href, ...props }, children);
    if (name.startsWith(".")) {
      const base = path.resolve(path.dirname(file), name);
      return component(fs.existsSync(base + ".tsx") ? base + ".tsx" : base + ".ts");
    }
    return webRequire(name);
  }, compiled, compiled.exports);
  return compiled.exports;
}
const Page = component("apps/web/app/studio/stream/page.tsx").default;
const root = createRoot(document.getElementById("root"));
const button = label => [...document.querySelectorAll("button")].find(item => item.textContent.trim() === label);
const field = label => [...document.querySelectorAll("label")].find(item => item.querySelector("span")?.textContent.startsWith(label))?.querySelector("input");
async function click(label) {
  await act(async () => { const item = button(label); assert(item && !item.disabled, label); item.click(); });
}
async function type(label, value) {
  await act(async () => {
    const input = field(label); assert(input, label);
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function submit() { await act(async () => { document.querySelector("form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); }); }
async function visibility(value) {
  await act(async () => { hidden = value; document.dispatchEvent(new Event("visibilitychange")); });
}
(async () => {
  try {
    await act(async () => { root.render(React.createElement(Page)); });
    assert.equal(field("Title").value, "Original title");
    await type("Title", "Unsaved edit");
    state.settings = { title: "Another tab's title", category_id: "coding", revision: 4 };
    await act(async () => { await poll(); });
    assert.equal(field("Title").value, "Unsaved edit", "Polling preserves edits");
    await submit();
    assert.equal(requests.findLast(r => r.method === "PATCH").body.revision, 0, "Polling must not hide a stale-edit conflict");
    assert.match(document.body.textContent, /changed in another tab/);
    await click("Reload saved details");
    assert.equal(field("Title").value, "Another tab's title");
    await type("Title", "Updated title");
    await submit();
    assert.equal(state.settings.title, "Updated title");
    await type("Current password", "synthetic-password");
    await type("Authenticator or recovery code", "synthetic-proof");
    await click("Create stream key");
    assert.equal(field("Stream key").value, "synthetic-secret-key");
    assert.equal(field("Current password").value, "");
    assert.equal(field("Authenticator or recovery code").value, "");
    const writes = requests.filter(r => r.method === "POST");
    assert.equal(writes[0].url, "/api/auth/reauth");
    assert.equal(writes[1].url, "/api/me/stream/key");
    await click("Copy key");
    assert.equal(copied, "synthetic-secret-key");
    await visibility(true);
    assert.equal(field("Stream key"), undefined);
    await visibility(false);
    delayNextKey = true;
    await type("Authenticator or recovery code", "synthetic-proof");
    await click("Show stream key");
    assert(delayedKey);
    await visibility(true); await visibility(false);
    await act(async () => { delayedKey(); });
    assert.equal(field("Stream key"), undefined, "Late response must not undo a visibility hide");
    await type("Authenticator or recovery code", "synthetic-proof");
    await click("Show stream key");
    await act(async () => { expiry(); });
    assert.equal(field("Stream key"), undefined, "Key auto-hides after one minute");
    await click("Replace key…");
    assert.match(document.body.textContent, /Replacing this key disconnects/);
    await type("Authenticator or recovery code", "synthetic-proof");
    await click("Replace key and disconnect OBS");
    assert.equal(requests.filter(r => r.method === "POST").at(-1).url, "/api/me/stream/key/rotate");
    await click("Stop stream and revoke key");
    assert.equal(field("Stream key"), undefined);
    assert.match(document.body.textContent, /disconnection is pending/);
    assert.match(document.body.textContent, /Disconnecting/);
    state.eligible = false;
    await act(async () => { await poll(); });
    assert(button("Create stream key").disabled);
    assert.match(document.body.textContent, /Not measured/);
    state.broadcast = { state: "LIVE", observed_at: "2000-01-01T00:00:00Z", health: {} };
    state.disconnect_pending = false;
    await act(async () => { await poll(); });
    assert.match(document.body.textContent, /Signal not confirmed/);
    process.stdout.write("Stream Studio checks passed: edit conflicts, step-up order, key hiding/races, rotation, pending stop and eligibility.\n");
  } finally {
    await act(async () => { root.unmount(); });
    dom.window.close();
    global.setTimeout = nativeTimeout; global.clearTimeout = nativeClear;
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
