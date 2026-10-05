// Synthetic component acceptance for MAGNet discovery (docs/MAGNET.md); no accounts or network.
// node scripts/check-discovery.cjs <path-to-jsdom>
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const webRequire = createRequire(path.resolve("apps/web/package.json"));
const { JSDOM } = require(process.argv[2] || "jsdom");
const dom = new JSDOM('<div id="root"></div>', { url: "http://localhost/" });
global.window = dom.window;
global.document = dom.window.document;
global.IS_REACT_ACT_ENVIRONMENT = true;
const React = webRequire("react"), { act } = React;
const { createRoot } = webRequire("react-dom/client");
const { renderToStaticMarkup } = webRequire("react-dom/server");
const ts = webRequire("typescript");
function compile(file, deps = {}) {
  const source = ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const mod = { exports: {} };
  // A module may get its own `window` (deps.window), e.g. to record navigation.
  vm.runInThisContext(`(function(require,module,exports,window){${source}\n})`, { filename: file })(name => {
    if (name === "next/link") return ({ children, ...props }) => React.createElement("a", props, children);
    if (name === "next/image") return ({ unoptimized, ...props }) => React.createElement("img", props);
    if (deps[name]) return deps[name];
    if (name.endsWith(".css")) return {};
    if (name.startsWith(".")) { const resolved = path.resolve(path.dirname(file), name); return compile(fs.existsSync(resolved + ".tsx") ? resolved + ".tsx" : resolved + ".ts", deps); }
    return webRequire(name);
  }, mod, mod.exports, deps.window ?? global.window);
  return mod.exports;
}
const card = (username, extra = {}) => ({ username, display_name: username, avatar: null, faction: "glint", title: `<${username}> playing`, category: "Art", genre: "art", started_at: "2026-10-05T12:00:00Z", viewers: 3, broadcast_id: `b-${username}`, thumbnail: null, label: null, fresh: false, ...extra });
const first = card("First", { label: "New creator", fresh: true }), second = card("Second", { faction: "myria", thumbnail: "https://media.example/thumbs/b-second/1.webp" });
let home = { live: [first, second], following: [], faction: null, fresh: [first], spotlights: [{ kind: "first_stream", reason: "First stream on S.V.E.R", stream: first }, { kind: "staff", reason: "Community pick", stream: null, user: { username: "Quiet", display_name: "Quiet" } }], recent: [] };
let account = null;
const deps = {
  "../lib/server-api": { apiGet: async url => ({ data: url === "/api/discovery/home" ? home : null }) },
  "./session": { currentAccount: async () => account },
  "../Avatar": { Avatar: ({ name }) => React.createElement("span", null, name) },
};
const { default: Home } = compile("apps/web/app/page.tsx", deps);
const position = (html, id) => html.indexOf(`id="${id}"`);
(async () => {
  let html = renderToStaticMarkup(await Home());
  // Signed out: spotlight, rotation, live now, just went live, in that order; no personal shelves.
  const order = ["spot-h", "rot-h", "live-h", "new-h"].map(id => position(html, id));
  assert(order.every(p => p > 0) && order.every((p, i) => i === 0 || p > order[i - 1]), "shelf order");
  assert.equal(position(html, "fol-h"), -1);
  assert.equal(position(html, "fac-h"), -1);
  assert.match(html, /href="\/First"/);
  assert.match(html, /&lt;First&gt; playing/, "titles are text");
  assert.match(html, /New creator/);
  assert.match(html, /Community pick/);
  assert.match(html, /src="https:\/\/media.example\/thumbs\/b-second\/1.webp"/, "live still on the card");
  assert.doesNotMatch(html, /<video/, "previews never start playback");
  // Signed in with a faction: following and own-faction shelves.
  account = { username: "Viewer", faction: "glint" };
  home = { ...home, following: [second], faction: [first] };
  html = renderToStaticMarkup(await Home());
  assert(position(html, "fol-h") > 0 && position(html, "fac-h") > position(html, "live-h"));
  assert.match(html, /From Glint/);
  // Nothing live: recently live channels and the war map, never an empty page.
  home = { ...home, live: [], following: [], faction: [], fresh: [], spotlights: [], recent: [{ user: { username: "Gone", display_name: "Gone", avatar: null }, ended_at: "2026-10-04T12:00:00Z" }] };
  html = renderToStaticMarkup(await Home());
  assert.match(html, /Nobody is live right now/);
  assert.match(html, /href="\/Gone"/);
  assert.match(html, /href="\/war-map"/);
  home = null;
  html = renderToStaticMarkup(await Home());
  assert.match(html, /couldn&#x27;t be loaded|couldn't be loaded/);

  // Stream end: the next stream after a 10-second countdown; Cancel stops it.
  let moved = null;
  const client = { send: async () => ({ ok: true, data: { items: [second] } }) };
  const { UpNext } = compile("apps/web/components/UpNext.tsx", { "../lib/client-api": client, window: { location: { assign: url => { moved = url; } } } });
  const root = createRoot(document.getElementById("root"));
  const realTimeout = global.setTimeout;
  global.setTimeout = (fn) => realTimeout(fn, 0);
  await act(async () => root.render(React.createElement(UpNext, { username: "First", focused: true })));
  for (let i = 0; i < 12 && !moved; i++) await act(async () => new Promise(r => realTimeout(r, 5)));
  assert.equal(moved, "/Second/live");
  moved = null;
  await act(async () => root.render(React.createElement(UpNext, { key: "again", username: "First", focused: false })));
  await act(async () => document.querySelector(".up-next button").click());
  for (let i = 0; i < 12; i++) await act(async () => new Promise(r => realTimeout(r, 5)));
  assert.equal(moved, null, "Cancel keeps the viewer here");
  assert.match(document.querySelector(".up-next").textContent, /Up next:/);
  global.setTimeout = realTimeout;
  await act(async () => root.unmount());
  console.log("Discovery UI passed: shelf order, personal shelves, safe text, stills, spotlights, empty and outage states, stream-end countdown and cancel.");
})().catch(error => { console.error(error); process.exitCode = 1; }).finally(() => dom.window.close());
