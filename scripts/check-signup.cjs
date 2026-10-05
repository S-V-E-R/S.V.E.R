// Component checks using the existing legacy jsdom installation; no browser/provider traffic.
// node scripts/check-signup.cjs C:\streaming\SVER\node_modules\jsdom
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const webRequire = createRequire(path.resolve("apps/web/package.json"));
const { JSDOM } = require(process.argv[2] || "jsdom");
const dom = new JSDOM('<div id="root"></div>', { url: "http://localhost/signup" });
for (const name of ["window", "document", "history", "FormData", "HTMLElement", "HTMLInputElement", "Event", "MouseEvent"]) global[name] = dom.window[name];
Object.defineProperty(global, "navigator", { value: dom.window.navigator, configurable: true });
global.IS_REACT_ACT_ENVIRONMENT = true;
const React = webRequire("react");
const { act } = React;
const { createRoot } = webRequire("react-dom/client");
const ts = webRequire("typescript");
let bot;
window.turnstile = { render: (_, options) => { bot = options.callback; return "test-widget"; }, remove: () => {} };
const requests = [];
let slowReply;
const reply = result => ({ ok: true, json: async () => result });
global.fetch = async (url, options = {}) => {
  if (url === "/api/auth/config") return reply({ providers: ["google", "twitch", "discord"], turnstile_site_key: "test", development: true });
  if (url === "/api/auth/oauth/signup") return reply({ provider: "google", username: "Suggested_User" });
  if (url.startsWith("/api/auth/username-availability?")) {
    const name = new URL(url, "http://localhost").searchParams.get("username");
    if (name === "Slow_Taken") return new Promise(resolve => { slowReply = () => resolve(reply({ available: false, message: "That username is already taken." })); });
    return reply({ available: name !== "Taken_User", message: name === "Taken_User" ? "That username is already taken." : "Username is available." });
  }
  assert.match(url, /^\/api\/auth\/oauth\/(google|twitch|discord)\/start$/);
  requests.push({ url, body: JSON.parse(options.body) });
  return reply({}); // Do not navigate away in this component check.
};
const compile = file => ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
const source = compile("apps/web/app/screens.tsx");
const compiled = { exports: {} };
vm.runInThisContext(`(function(require, module, exports) {${source}\n})`, { filename: "screens.test.cjs" })(name => {
  if (name === "next/link") return ({ href, children, ...props }) => React.createElement("a", { href, ...props }, children);
  if (name === "next/script") return function Script({ onReady }) { React.useEffect(onReady, []); return null; };
  // Account-only media is outside these signup checks.
  if (name === "../components/Avatar") return { Avatar: () => null };
  if (name === "../lib/factions") return { isFaction: value => ["myria", "aetheron", "glint"].includes(value) };
  if (name === "../components/Turnstile") {
    const component = { exports: {} };
    vm.runInThisContext(`(function(require,module,exports){${compile("apps/web/components/Turnstile.tsx")}\n})`)(dependency => dependency === "next/script" ? function Script({onReady}) { React.useEffect(onReady,[]); return null; } : webRequire(dependency),component,component.exports);
    return component.exports;
  }
  return webRequire(name);
}, compiled, compiled.exports);
const root = createRoot(document.getElementById("root"));
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const button = text => [...document.querySelectorAll("button")].find(item => item.textContent.trim() === text || item.querySelector("span:last-child")?.textContent === text);
async function click(text) { await act(async () => { const item = button(text); assert.ok(item && !item.disabled, `${text} must be selectable`); item.click(); }); }
async function type(name, value) {
  await act(async () => {
    const input = document.querySelector(`[name="${name}"]`);
    assert.ok(input, `Missing ${name}`);
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set.call(input, value);
    input.dispatchEvent(new window.Event("input", { bubbles: true }));
  });
}
async function settle() { await act(async () => { await pause(400); }); }
(async () => {
  try {
    await act(async () => { root.render(React.createElement(compiled.exports.default, { screen: "signup" })); });
    assert.ok(document.querySelector('[name="email"]') && document.querySelector('[name="password"]'));
    await type("email", "invalid-email");
    await type("password", "short");
    for (const provider of ["Google", "Twitch", "Discord"]) {
      await click(`Continue with ${provider}`);
      const sent = requests.at(-1);
      assert.equal(sent.url, `/api/auth/oauth/${provider.toLowerCase()}/start`);
      assert.deepEqual(sent.body, { intent: "signup", code: "" }, "Provider signup starts with no form details or bot token");
    }
    await act(async () => { root.render(React.createElement(compiled.exports.default, { screen: "oauth-signup", key: "completion" })); });
    assert.equal(document.querySelector('[name="email"]'), null);
    assert.equal(document.querySelector('[name="password"]'), null);
    assert.equal(document.querySelector('[name="username"]').value, "Suggested_User");
    assert.ok(document.querySelector('[name="date_of_birth"]'));
    await settle();
    assert.equal(document.querySelector("#username-status").textContent, "Username is available.");
    await type("username", "Taken_User");
    await settle();
    assert.equal(document.querySelector('[name="username"]').checkValidity(), false);
    assert.equal(document.querySelector("#username-status").textContent, "That username is already taken.");
    await type("username", "Slow_Taken");
    await settle();
    assert.ok(slowReply);
    await type("username", "Free_User");
    await settle();
    await act(async () => { slowReply(); });
    assert.equal(document.querySelector("#username-status").textContent, "Username is available.", "Stale response must not replace current feedback");
    assert.equal(document.querySelector('[name="username"]').checkValidity(), true);
    await type("username", "ab");
    assert.match(document.querySelector("#username-status").textContent, /3–25/);
    await type("username", "");
    assert.equal(document.querySelector("#username-status").textContent, "");
    assert.equal(requests.length, 3);
    process.stdout.write("Signup component checks passed: direct provider starts with blank/invalid forms, suggested username after OAuth, no email/password at completion, availability and stale responses.\n");
  } finally {
    await act(async () => { root.unmount(); });
    dom.window.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
