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
  // Signed out, Main mockup order: front-line banner, MAGNet rotation (spotlights first, with their
  // reason), live now, just went live; no personal shelves.
  const order = ["front-line-title", "rot-h", "live-h", "new-h"].map(id => position(html, id));
  assert(order.every(p => p > 0) && order.every((p, i) => i === 0 || p > order[i - 1]), "shelf order");
  assert.equal(position(html, "fol-h"), -1);
  assert.equal(position(html, "fac-h"), -1);
  assert.match(html, /href="\/First\/live"/, "live cards open the watch page");
  assert.match(html, /&lt;First&gt; playing/, "titles are text");
  assert.match(html, /New creator/);
  assert.match(html, /Community pick/);
  assert.match(html, /First stream on S.V.E.R/, "spotlight reason in the rotation");
  assert.match(html, /href="\/signup"[^>]*>Enlist/, "front line offers Enlist to guests");
  assert.match(html, /src="https:\/\/media.example\/thumbs\/b-second\/1.webp\?v=0"/, "live still on the card");
  assert.doesNotMatch(html, /<video/, "previews never start playback");
  // Signed in with a faction: the own-faction shelf after Live now; Following · live is in the sidebar.
  account = { username: "Viewer", faction: "glint" };
  home = { ...home, following: [second], faction: [first] };
  html = renderToStaticMarkup(await Home());
  assert.equal(position(html, "fol-h"), -1);
  assert(position(html, "fac-h") > position(html, "live-h"));
  assert.match(html, /From Glint/);
  // Nothing live: recently live channels and the war map, never an empty page.
  home = { ...home, live: [], following: [], faction: [], fresh: [], spotlights: [], recent: [{ user: { username: "Gone", display_name: "Gone", avatar: null }, ended_at: "2026-10-04T12:00:00Z" }] };
  html = renderToStaticMarkup(await Home());
  assert.match(html, /Nothing live right now/);
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

  // Still refreshes are visible-only, failures fall back, and the next minute retries.
  const { LiveThumbnail } = compile("apps/web/components/LiveThumbnail.tsx");
  const realInterval = global.setInterval, realClear = global.clearInterval, realNow = Date.now;
  let now = realNow(), tick, intersection, disconnected = false;
  Date.now = () => now;
  global.setInterval = fn => { tick = fn; return 1; };
  global.clearInterval = () => {};
  global.IntersectionObserver = class {
    constructor(fn) { intersection = fn; }
    observe() {}
    disconnect() { disconnected = true; }
  };
  Object.defineProperty(document, "hidden", { value: false, configurable: true });
  await act(async () => root.render(React.createElement(LiveThumbnail, { src: "/api/discovery/thumbnails/first", label: "Art" })));
  const img = () => document.querySelector(".live-thumbnail img");
  const initial = img().src;
  await act(async () => img().dispatchEvent(new window.Event("error")));
  assert(img().hidden, "failed image leaves the category fallback");
  assert.match(document.querySelector(".live-thumbnail").textContent, /Art/);
  now += 61000;
  await act(async () => tick());
  assert.equal(img().src, initial, "offscreen cards do not refresh");
  await act(async () => intersection([{ isIntersecting: true }]));
  assert.notEqual(img().src, initial, "entering the viewport refreshes an old card");
  assert(!img().hidden, "a new minute retries a failed image");
  const visible = img().src;
  Object.defineProperty(document, "hidden", { value: true, configurable: true });
  now += 61000;
  await act(async () => tick());
  assert.equal(img().src, visible, "hidden tabs do not refresh");
  Object.defineProperty(document, "hidden", { value: false, configurable: true });
  await act(async () => document.dispatchEvent(new window.Event("visibilitychange")));
  assert.notEqual(img().src, visible, "returning to the tab refreshes");
  assert.equal(document.querySelector("video"), null, "no playback for thumbnails");
  await act(async () => root.unmount());
  assert(disconnected, "observer is cleaned up");
  global.setInterval = realInterval; global.clearInterval = realClear; Date.now = realNow;

  // MAGNet co-streams: a merged squad gets member tabs on one player; a separate one links out.
  let lane = { id: "global", name: "Global", enabled: true, next: null, others: [], featured: { stream: first, kind: "fair", reason: "Fair turn", since: "2026-10-06T12:00:00Z", moves_on_by: null, holding: false, squad: { mode: "MERGED", members: [first, second] } } };
  const { MagnetHype } = compile("apps/web/components/MagnetHype.tsx", {
    "../lib/client-api": { send: async (_method, url) => ({ ok: true, data: url === "/api/magnet" ? { lanes: [] } : lane }), useLoad: load => React.useEffect(() => { void load(); }, [load]) },
    "./LivePlayer": { LivePlayer: ({ username }) => React.createElement("div", { className: "player" }, username) },
    "./HypeChat": { HypeChat: () => null },
  });
  const hype = createRoot(document.getElementById("root"));
  await act(async () => hype.render(React.createElement(MagnetHype, { lane: "global", account: null, viewerFaction: null })));
  const tabs = () => [...document.querySelectorAll('[role="tab"]')];
  assert.deepEqual(tabs().map(t => t.textContent), ["First", "Second"]);
  assert.equal(document.querySelectorAll(".player").length, 1, "one player for the squad");
  assert.equal(document.querySelector(".player").textContent, "First");
  await act(async () => tabs()[1].dispatchEvent(new window.MouseEvent("click", { bubbles: true })));
  assert.equal(document.querySelector(".player").textContent, "Second", "a tab switches the player");
  assert.equal(tabs()[1].getAttribute("aria-selected"), "true");
  assert.match(document.querySelector(".magnet-why").innerHTML, /href="\/Second\/live"/, "the selected member opens the watch page");
  await act(async () => hype.unmount());
  lane = { ...lane, featured: { ...lane.featured, squad: { mode: "SEPARATE", members: [second] } } };
  const separate = createRoot(document.getElementById("root"));
  await act(async () => separate.render(React.createElement(MagnetHype, { lane: "global", account: null, viewerFaction: null })));
  assert.equal(tabs().length, 0);
  assert.match(document.querySelector(".magnet-why").textContent, /Co-streaming with Second/);
  assert.match(document.querySelector(".magnet-why").innerHTML, /href="\/Second\/live"/);
  await act(async () => separate.unmount());
  // "You just watched": after a switch, only a viewer who watched the previous stream gets the card.
  const polls = [];
  global.setInterval = fn => polls.push(fn); global.clearInterval = () => {};
  const calls = [];
  const watched = { reason: "Fair turn", ended_at: "2026-10-07T12:00:00Z", members: [{ username: "First", display_name: "First", avatar: null, live: true, following: false, own: false }] };
  const solo = stream => ({ stream, kind: "fair", reason: "Fair turn", since: "2026-10-07T12:00:00Z", moves_on_by: null, holding: false, squad: null });
  let view = { id: "global", name: "Global", enabled: true, next: null, previous: null, others: [], featured: solo(first) };
  const hypeClient = { send: async (method, url) => { calls.push(`${method} ${url}`); return { ok: true, data: url === "/api/magnet" ? { lanes: [] } : url.startsWith("/api/follows/") ? { following: method === "PUT" } : view }; }, useLoad: load => React.useEffect(() => { void load(); }, [load]) };
  const { MagnetHype: Hype } = compile("apps/web/components/MagnetHype.tsx", {
    "../lib/client-api": hypeClient,
    "./JustWatched": compile("apps/web/components/JustWatched.tsx", { "../lib/client-api": hypeClient }),
    "./LivePlayer": { LivePlayer: ({ username }) => React.createElement("div", { className: "player" }, username) },
    "./HypeChat": { HypeChat: () => null },
  });
  const card = () => document.querySelector(".magnet-previous");
  const switchTo = async next => { view = next; await act(async () => { for (const poll of polls) poll(); }); };
  const watcher = createRoot(document.getElementById("root"));
  await act(async () => watcher.render(React.createElement(Hype, { lane: "global", account: "viewer", viewerFaction: null })));
  assert.equal(card(), null, "no card before a switch");
  await switchTo({ ...view, featured: solo(second), previous: watched });
  assert.match(card().textContent, /You just watched First/);
  const follow = () => [...card().querySelectorAll("button")].find(b => /Follow/.test(b.textContent));
  await act(async () => follow().dispatchEvent(new window.MouseEvent("click", { bubbles: true })));
  assert(calls.includes("PUT /api/follows/First"), "one tap follows");
  assert.equal(follow().textContent, "Following");
  assert.equal(follow().getAttribute("aria-pressed"), "true");
  assert.match(card().innerHTML, /href="\/First\/live"/, "a link back to the stream");
  await act(async () => card().querySelector('[aria-label="Dismiss"]').dispatchEvent(new window.MouseEvent("click", { bubbles: true })));
  assert.equal(card(), null, "dismissed");
  await act(async () => watcher.unmount());
  // A visitor who arrives after the switch never watched First; a held viewer never saw it either.
  const late = createRoot(document.getElementById("root"));
  await act(async () => late.render(React.createElement(Hype, { lane: "global", account: null, viewerFaction: null })));
  assert.equal(card(), null, "no card for a stream this viewer didn't watch");
  await act(async () => late.unmount());
  polls.length = 0;
  view = { ...view, featured: { ...solo(first), holding: true }, previous: null };
  const held = createRoot(document.getElementById("root"));
  await act(async () => held.render(React.createElement(Hype, { lane: "global", account: "viewer", viewerFaction: null })));
  await switchTo({ ...view, featured: solo(second), previous: watched });
  assert.equal(card(), null, "no card after a holding card");
  await act(async () => held.unmount());
  global.setInterval = realInterval; global.clearInterval = realClear;
  console.log("Discovery UI passed: shelves, stills, fallback/retry, visible-only refresh, spotlights, empty/outage states, stream-end countdown and cancel, MAGNet co-stream tabs and links, You just watched.");
})().catch(error => { console.error(error); process.exitCode = 1; }).finally(() => dom.window.close());
