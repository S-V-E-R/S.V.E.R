// Synthetic component acceptance; no live accounts or network requests.
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
  vm.runInThisContext(`(function(require,module,exports){${source}\n})`, { filename: file })(name => {
    if (name === "next/link") return ({ children, ...props }) => React.createElement("a", props, children);
    if (name === "next/image") return props => React.createElement("img", props);
    return deps[name] || webRequire(name);
  }, mod, mod.exports);
  return mod.exports;
}
const avatar = { Avatar: ({ name }) => React.createElement("span", null, name) };
const shelf = compile("apps/web/components/StreamShelf.tsx", { "./Avatar": avatar });
const spotlight = compile("apps/web/components/LiveSpotlight.tsx", { "./StreamShelf": shelf });
const streams = ["First", "Second"].map(username => ({ user: { username, display_name: username, avatar: null, linked: true }, banner: null, live: true, title: `<${username}> playing`, category: "Art", viewers: 3, started_at: "2026-10-04T12:00:00Z" }));
let directory = { live: streams, recent: [], has_more: true, as_of: "2026-10-04T12:30:00Z" };
let account = null;
const { default: Home } = compile("apps/web/app/page.tsx", {
  "../lib/server-api": { apiGet: async url => ({ data: url.startsWith("/api/streams") ? directory : url === "/api/me/following" ? { items: [{ user: streams[0].user }] } : { categories: [{ id: "art", name: "Art", genre: "art" }] } }) },
  "../components/StreamShelf": shelf, "../components/LiveSpotlight": spotlight,
  "../components/shell/Icons": compile("apps/web/components/shell/Icons.tsx"),
  "./session": { currentAccount: async () => account },
});
const root = createRoot(document.getElementById("root"));
(async () => {
  try {
    let html = renderToStaticMarkup(await Home({ searchParams: Promise.resolve({}) }));
    const ids = ["front-line-title", "spotlight-title", "live-now", "beacons-title", "just-live", "territories-title", "clips-title"];
    for (let i = 1; i < ids.length; i++) assert(html.indexOf(`id="${ids[i - 1]}"`) < html.indexOf(`id="${ids[i]}"`), "Planned shelf order");
    assert.match(html, /href="\/First\/live"/);
    assert.match(html, /&lt;First&gt; playing/);
    assert.match(html, /More live streams/);
    assert.match(html, /<h3>Art<\/h3>/);
    account = { username: "Viewer" };
    html = renderToStaticMarkup(await Home({ searchParams: Promise.resolve({}) }));
    assert.match(html, /Following · live/);
    directory = { ...directory, live: [], recent: [{ ...streams[0], live: false }], has_more: false };
    html = renderToStaticMarkup(await Home({ searchParams: Promise.resolve({}) }));
    assert.match(html, /Recently live/);
    assert.match(html, /href="\/First"/);
    assert.doesNotMatch(html, /href="\/First\/live"/);
    directory = null;
    html = renderToStaticMarkup(await Home({ searchParams: Promise.resolve({ page: "invalid" }) }));
    assert.match(html, /Streams couldn’t be loaded/);
    assert.doesNotMatch(html, /Nothing live right now/);
    await act(async () => root.render(React.createElement(spotlight.LiveSpotlight, { streams })));
    const watch = () => document.querySelector('a.button').getAttribute("href");
    assert.equal(watch(), "/First/live");
    await act(async () => document.querySelector('[aria-label="Next stream"]').click());
    assert.equal(watch(), "/Second/live");
    await act(async () => document.querySelector('[aria-label="Next stream"]').click());
    assert.equal(watch(), "/First/live");
    await act(async () => document.querySelector('[aria-label="Previous stream"]').click());
    assert.equal(watch(), "/Second/live");
    assert.equal(document.querySelectorAll("video").length, 0, "Previews do not start playback");
    console.log("Discovery UI passed: shelf order, watch links, following, recent fallback, outage state, safe text, and carousel wraparound.");
  } finally { await act(async () => root.unmount()); dom.window.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
