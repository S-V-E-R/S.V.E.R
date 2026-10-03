// Pure component check: no browser/server traffic. Uses the existing optional jsdom installation.
// node scripts/check-roadmap.cjs C:/streaming/SVER/node_modules/jsdom
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const webRequire = createRequire(path.resolve("apps/web/package.json"));
const { JSDOM } = require(process.argv[2] || "jsdom");
const dom = new JSDOM('<div id="root"></div>', { url: "http://localhost/roadmap", pretendToBeVisual: true });
global.window = dom.window;
global.document = dom.window.document;
global.IS_REACT_ACT_ENVIRONMENT = true;
const React = webRequire("react");
const { act } = React;
const { createRoot } = webRequire("react-dom/client");
const ts = webRequire("typescript");
let interval;
const originalInterval = global.setInterval;
const originalClear = global.clearInterval;
global.setInterval = (fn, delay) => { assert.equal(delay, 30000); interval = fn; return 1; };
global.clearInterval = id => { assert.equal(id, 1); interval = null; };
function compile(file, dependencies = {}) {
  const code = ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const compiled = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${code}\n})`, { filename: file })(name => dependencies[name] || webRequire(name), compiled, compiled.exports);
  return compiled.exports;
}
const data = compile("apps/web/app/roadmap/data.ts");
const Progress = compile("apps/web/app/roadmap/progress.tsx", { "./data": data, "next/link": ({ href, children }) => React.createElement("a", { href }, children) }).default;
const initial = { revision: "a", items: ["foundation", "login", "profiles", "live-streams", "factions", "magnet", "support", "vods-clips", "beacons"].map((id, number) => ({ id, number, name: id, detail: "Public progress", status: number < 2 ? "Done" : "Planned" })) };
let result = initial, offline = false, requests = 0;
global.fetch = async (url, options) => {
  assert.equal(url, "/api/roadmap");
  assert.equal(options.cache, "no-store");
  assert.equal(options.credentials, "omit");
  requests++;
  if (offline) throw new Error("offline");
  return { ok: true, json: async () => result };
};
const root = createRoot(document.getElementById("root"));
(async () => {
  try {
    for (const invalid of [null, {}, { ...initial, items: [] }, { ...initial, items: initial.items.map(item => ({ ...item, status: "Unknown" })) }, { ...initial, items: initial.items.map(item => ({ ...item, id: "duplicate" })) }]) assert.throws(() => data.parseRoadmap(invalid));
    await act(async () => { root.render(React.createElement(Progress, { initial })); });
    assert.match(document.body.textContent, /2 of 9 milestones complete/);
    result = { ...initial, revision: "b", items: initial.items.map(item => item.id === "profiles" ? { ...item, status: "Done" } : item) };
    await act(async () => { interval(); });
    assert.match(document.body.textContent, /3 of 9 milestones complete/);
    assert.equal(document.querySelector("#profiles .roadmap-status").textContent, "Done");
    offline = true;
    await act(async () => { interval(); });
    assert.match(document.querySelector('[role="status"]').textContent, /last received progress/);
    assert.match(document.body.textContent, /3 of 9 milestones complete/);
    const before = requests;
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    await act(async () => { interval(); });
    assert.equal(requests, before, "Hidden tabs do not poll");
    offline = false;
    Object.defineProperty(document, "hidden", { configurable: true, value: false });
    await act(async () => { document.dispatchEvent(new dom.window.Event("visibilitychange")); });
    assert.match(document.querySelector('[role="status"]').textContent, /Live roadmap/);
    offline = true;
    await act(async () => { root.render(React.createElement(Progress, { initial: null, key: "unavailable" })); });
    assert.equal(document.querySelectorAll(".roadmap-item").length, 0, "No invented statuses during initial outage");
    offline = false;
    await act(async () => { interval(); });
    assert.equal(document.querySelectorAll(".roadmap-item").length, 9);
    console.log("Roadmap checks passed: live status/count updates, stale data, recovery, hidden tabs, invalid payloads and cleanup.");
  } finally {
    await act(async () => { root.unmount(); });
    assert.equal(interval, null);
    global.setInterval = originalInterval;
    global.clearInterval = originalClear;
    dom.window.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
