// Chat component acceptance with synthetic HTTP/socket events; no provider or account traffic.
// node scripts/check-chat.cjs <path-to-jsdom>
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const webRequire = createRequire(path.resolve("apps/web/package.json"));
const { JSDOM } = require(process.argv[2] || "jsdom");
const dom = new JSDOM('<div id="root"></div>', { url: "http://localhost/Streamer/live" });
for (const name of ["window", "document", "HTMLElement", "Event", "location"]) global[name] = dom.window[name];
HTMLElement.prototype.scrollIntoView = function () {};
global.IS_REACT_ACT_ENVIRONMENT = true;
const React = webRequire("react");
const { act } = React;
const { createRoot } = webRequire("react-dom/client");
const ts = webRequire("typescript");
function compile(file, deps = {}) {
  const source = ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${source}\n})`, { filename: file })(name => deps[name] || webRequire(name), module, module.exports);
  return module.exports;
}
const sockets = [], calls = [];
class Socket {
  static OPEN = 1;
  readyState = 1;
  sent = [];
  constructor() { sockets.push(this); }
  send(text) { this.sent.push(JSON.parse(text)); }
  close() {}
  emit(value) { this.onmessage({ data: JSON.stringify(value) }); }
}
global.WebSocket = Socket;
window.prompt = () => "Synthetic reason";
const message = (id, body, extra = {}) => ({ id, seq: Number(id), author: { username: "Writer", display_name: "Writer" }, body, created_at: "2026-10-04T12:00:00Z", role: null, mentions: [], reply: null, ...extra });
const first = message("1", "<img src=x onerror=alert(1)> @Viewer @Unknown hi@Viewer @@Viewer", { role: "owner", mentions: ["Viewer"] });
const second = message("2", "reply", { role: "moderator", reply: { id: "1", username: "Writer", body: first.body } });
const third = message("3", "staff", { role: "staff" });
let snapshot = { messages: [first, second, third], pinned: second };
global.fetch = async (url, options = {}) => {
  const body = options.body ? JSON.parse(options.body) : null;
  calls.push({ url, method: options.method, body });
  let data;
  if (url.endsWith("/moderation")) data = { role: "owner" };
  else if (url.endsWith("/pin")) data = { pinned: body.message_id ? message(body.message_id, "new pin") : null };
  else if (options.method === "POST") data = { message: message(body.id, body.body) };
  else data = snapshot;
  return { ok: true, status: 200, json: async () => structuredClone(data) };
};
const client = compile("apps/web/lib/client-api.ts");
const { Chat } = compile("apps/web/components/Chat.tsx", {
  "../lib/client-api": client,
  "next/link": ({ children, ...props }) => React.createElement("a", props, children),
  "./Report": { ReportButton: () => null, TakeDownLink: () => null },
  "./Emote": compile("apps/web/components/Emote.tsx"),
  "./FactionIdentity": { Crest: () => null },
  "./Guilds": { GuildChatBadge: () => null },
  "./Rewards": { Rewards: () => null },
  "../styles/teams.css": {},
  "../lib/types": { CREATOR_TIERS: ["Scout", "Trailblazer", "Pioneer", "Pathfinder"] },
});
const root = createRoot(document.getElementById("root"));
const click = async node => { assert.ok(node); await act(async () => node.click()); };
const button = text => [...document.querySelectorAll("button")].find(b => b.textContent === text);
async function draft(value) {
  await act(async () => {
    const node = document.querySelector("textarea");
    Object.getOwnPropertyDescriptor(dom.window.HTMLTextAreaElement.prototype, "value").set.call(node, value);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
(async () => {
  try {
    await act(async () => root.render(React.createElement(Chat, { username: "Streamer", account: "Viewer" })));
    const socket = sockets.at(-1);
    await act(async () => { socket.onopen(); socket.emit({ type: "snapshot", ...snapshot }); });
    assert.equal(document.querySelector("img"), null, "Message and quote markup stays text");
    assert.equal(document.querySelectorAll("mark").length, 1, "Only the real mention highlights; emails and unknown accounts do not");
    assert.equal(document.querySelector("mark").textContent, "@Viewer");
    assert.deepEqual([...document.querySelectorAll(".badge")].map(n => n.textContent), ["Broadcaster", "Moderator", "Staff"]);
    assert.match(document.querySelector('[aria-label="Pinned message"]').textContent, /reply/);
    await click(button("Reply"));
    assert.equal(document.activeElement, document.querySelector("textarea"), "Reply focuses the labelled input");
    await draft("answer");
    await act(async () => document.querySelector("form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    assert.equal(socket.sent.at(-1).reply_to, "1");
    assert.equal(socket.sent.at(-1).body, "answer");
    assert.equal(document.querySelector(".chat-reply-draft"), null);
    await act(async () => socket.emit({ type: "delete", id: "1" }));
    assert.equal(document.querySelectorAll(".chat-quote").length, 2);
    for (const quote of document.querySelectorAll(".chat-quote")) assert.equal(quote.textContent, "Message deleted");
    assert.ok(!document.body.textContent.includes("onerror"), "Deletion redacts the pinned quote too");
    await act(async () => socket.emit({ type: "delete", id: "2" }));
    assert.equal(document.querySelector('[aria-label="Pinned message"]'), null);
    await click(button("Pin"));
    assert.equal(calls.at(-1).body.message_id, "3");
    assert.equal(calls.at(-1).body.reason, "Synthetic reason");
    await click(button("Unpin"));
    assert.equal(calls.at(-1).body.message_id, null);
    await draft("New pinned text");
    await click(button("Send and pin"));
    assert.equal(calls.at(-2).method, "POST");
    assert.equal(calls.at(-2).body.body, "New pinned text");
    assert.equal(calls.at(-1).body.message_id, calls.at(-2).body.id);

    // A reconnect snapshot includes a pin outside the 100-message history; HTTP fallback does too.
    snapshot = { messages: [], pinned: first };
    await act(async () => socket.emit({ type: "snapshot", ...snapshot }));
    assert.match(document.querySelector('[aria-label="Pinned message"]').textContent, /@Viewer/);
    await act(async () => root.render(React.createElement(Chat, { key: "guest", username: "Streamer", account: null })));
    await act(async () => sockets.at(-1).onclose());
    assert.ok(document.querySelector('[aria-label="Pinned message"]'));
    assert.equal(button("Unpin"), undefined);
    assert.equal(document.querySelector("textarea"), null);
    const emote = { id: "emote-1", code: "Wave", image: { "28": "/28.webp", "56": "/56.webp", "112": "/112.webp" } };
    snapshot = { messages: [message("4", "Wave wave Wave! :Wave: XWave @Wave\nWave\tWave <img src=x> @Viewer", { mentions: ["Viewer"] })], pinned: null, emotes: [emote] };
    await act(async () => root.render(React.createElement(Chat, { key: "emotes", username: "Streamer", account: "Viewer" })));
    const live = sockets.at(-1);
    await act(async () => { live.onopen(); live.emit({ type: "snapshot", ...snapshot }); });
    assert.equal(document.querySelectorAll(".chat-body img").length, 3, "Exact case-sensitive whitespace tokens only");
    assert.equal(document.querySelector(".chat-body img").alt, "Wave");
    assert.equal(document.querySelector("mark").textContent, "@Viewer", "Mention matching is preserved");
    assert.match(document.querySelector(".chat-body").textContent, /wave Wave! :Wave: XWave @Wave/);
    await click(document.querySelector('[aria-label="Insert Wave"]'));
    assert.equal(document.querySelector("textarea").value, "Wave ");
    assert.equal(document.activeElement, document.querySelector("textarea"));
    await act(async () => live.emit({ type: "emotes", emotes: [] }));
    assert.equal(document.querySelectorAll("img").length, 0, "Removal drops the catalog and renders original codes as text");
    assert.match(document.querySelector(".chat-body").textContent, /^Wave wave/);
    await act(async () => live.emit({ type: "snapshot", messages: snapshot.messages, pinned: null, emotes: [] }));
    assert.equal(document.querySelectorAll("img").length, 0, "A different channel catalog cannot reuse old emotes");
    console.log("Chat UI passed: mentions/replies/pins/badges, emote exact tokens/case/escaping, insertion, catalog removal, reconnect and HTTP fallback.");
  } finally {
    await act(async () => root.unmount());
    dom.window.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
