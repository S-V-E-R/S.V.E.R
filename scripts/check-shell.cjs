// Pure component checks: no browser or server traffic.
// node scripts/check-shell.cjs C:/streaming/SVER/node_modules/jsdom
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
const React = webRequire("react");
const { act } = React;
const { createRoot } = webRequire("react-dom/client");
const ts = webRequire("typescript");
let pathname = "/";
function compile(file, dependencies = {}) {
  const source = ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const compiled = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${source}\n})`, { filename: file })(name => {
    if (name === "next/link") return ({ href, children, ...props }) => React.createElement("a", { href, ...props }, children);
    if (name === "next/navigation") return { usePathname: () => pathname };
    if (name === "next/image") return ({ unoptimized, ...props }) => React.createElement("img", props);
    if (dependencies[name]) return dependencies[name];
    if (name.startsWith(".")) { const resolved=path.resolve(path.dirname(file),name); return compile(fs.existsSync(resolved+".tsx") ? resolved+".tsx" : resolved+".ts"); }
    return webRequire(name);
  }, compiled, compiled.exports);
  return compiled.exports;
}
const icons = compile("apps/web/components/shell/Icons.tsx");
const chrome = compile("apps/web/components/shell/Chrome.tsx", { "./Icons": icons });
const { SideNav } = compile("apps/web/components/shell/SideNav.tsx", { "./Icons": icons });
const { default: SiteShell } = compile("apps/web/components/SiteShell.tsx", { "./shell/Chrome": chrome });
const root = createRoot(document.getElementById("root"));
const pages = ["/about", "/factions", "/roadmap", "/help", "/terms", "/privacy", "/guidelines", "/dmca", "/contact", "/take-it-down"];
async function render(route, signedIn = false) {
  pathname = route;
  await act(async () => root.render(React.createElement(SiteShell, {
    account: signedIn ? { username: "ExampleUser" } : null, alerts: false,
    actions: signedIn ? React.createElement("a", { href: "/ExampleUser", className: "player-chip" }, "ExampleUser") : React.createElement("a", { href: "/login" }, "Log in"),
    sidebar: React.createElement(SideNav, { signedIn, faction: null }),
    children: React.createElement("h1", null, "Page content")
  })));
  assert.equal(document.querySelectorAll("main#main").length, 1);
  assert.equal(document.querySelectorAll("footer").length, 1);
  for (const href of pages) assert.ok(document.querySelector(`footer a[href="${href}"]`), `Footer keeps ${href}`);
}
(async () => {
  try {
    for (const signedIn of [false, true]) for (const route of pages) {
      await render(route, signedIn);
      assert.equal(document.querySelector("aside"), null);
      assert.equal(document.querySelector('form[role="search"]'), null);
      assert.ok(document.querySelector(`nav a[href="${route}"][aria-current="page"]`));
    }
    for (const route of ["/login", "/welcome", "/choose-side", "/choose-faction", "/signup", "/oauth-signup", "/forgot", "/reset", "/verify", "/mfa"]) {
      await render(route);
      assert.ok(document.querySelector("header.minimal"));
      assert.equal(document.querySelector("aside"), null);
      // Choosing a side happens signed in, so those screens offer no Log in / Enlist link.
      if (route === "/welcome" || route.startsWith("/choose-")) assert.equal(document.querySelector("header .topbar-note"), null);
      else assert.ok(document.querySelector(`header a[href="${route === "/login" ? "/signup" : "/login"}"]`));
    }
    // Account tools live in the sidebar's "Your channel" group (docs/DESIGN.md "Layout"); only
    // destinations that exist are listed, so nothing is greyed out as "coming soon".
    await render("/settings/profile", true);
    assert.ok(document.querySelector('aside a[href="/settings/profile"][aria-current="page"]'));
    const navLabels = [...document.querySelectorAll('aside .nav-item')].map(item => item.querySelector('span')?.textContent);
    assert.deepEqual(navLabels, ['Home', 'Following', 'Factions', 'Browse', 'MAGNet Hype', 'War map', 'Creator Studio', 'Settings', 'Account security']);
    assert.equal(document.querySelectorAll('aside [aria-disabled="true"]').length, 0);
    await render('/studio/channel', true);
    assert.ok(document.querySelector('aside a[href="/studio/channel"][aria-current="page"]'));
    assert.equal(document.querySelectorAll('aside a[href="/browse"]').length, 1, "Browse is in the sidebar");
    const toggle = () => document.querySelector("button.menu-toggle");
    await act(async () => toggle().click());
    assert.equal(toggle().getAttribute("aria-expanded"), "true");
    assert.ok(document.querySelector("aside.open").contains(document.activeElement));
    await act(async () => document.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    assert.equal(toggle().getAttribute("aria-expanded"), "false");
    assert.equal(document.activeElement, toggle());
    await act(async () => toggle().click());
    await render("/following", true);
    assert.equal(toggle().getAttribute("aria-expanded"), "false");
    console.log("Shell checks passed: public/auth/application layouts, shared footer, active links, drawer focus/Escape and navigation closure.");
  } finally {
    await act(async () => root.unmount());
    dom.window.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
