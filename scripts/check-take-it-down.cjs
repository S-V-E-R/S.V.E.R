// Public removal form checks with synthetic data and mocked HTTP/Turnstile; no provider traffic.
// node scripts/check-take-it-down.cjs <path-to-jsdom>
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const webRequire = createRequire(path.resolve("apps/web/package.json"));
const { JSDOM } = require(process.argv[2] || "jsdom");
const dom = new JSDOM('<div id="root"></div>', { url: "http://localhost/take-it-down" });
for (const name of ["window", "document", "FormData", "HTMLElement", "Event"]) global[name] = dom.window[name];
global.IS_REACT_ACT_ENVIRONMENT = true;
const React = webRequire("react");
const { act } = React;
const { createRoot } = webRequire("react-dom/client");
const ts = webRequire("typescript");
const widgets = new Map();
let widgetId = 0;
window.turnstile = {
  render: (_, options) => { const id = String(++widgetId); widgets.set(id, options); return id; },
  remove: id => widgets.delete(id),
};
function compile(file, deps = {}) {
  const source = ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${source}\n})`, { filename: file })(name => deps[name] || webRequire(name), module, module.exports);
  return module.exports;
}
const turnstile = compile("apps/web/components/Turnstile.tsx", {
  "next/script": function Script({ onReady }) { React.useEffect(onReady, []); return null; },
});
const client = compile("apps/web/lib/client-api.ts");
const { default: Forms } = compile("apps/web/app/take-it-down/request-forms.tsx", { "../../lib/client-api": client, "../../components/Turnstile": turnstile });
const calls = [];
let fail = true;
global.fetch = async (url, options) => {
  calls.push({ url, body: JSON.parse(options.body) });
  assert.equal(options.credentials, "same-origin");
  assert.equal(options.cache, "no-store");
  return { ok: !fail, status: fail ? 400 : 200, json: async () => fail ? { error: "Synthetic validation failure" } : { number: "TID-2026-000123", status: url.endsWith("/status") ? "not_removed" : "received", reason: url.endsWith("/status") ? "Synthetic mistaken request" : undefined } };
};
const root = createRoot(document.getElementById("root"));
const form = label => document.querySelector(`form[aria-label="${label}"]`);
async function solve(action) {
  const widget = [...widgets.values()].find(w => w.action === action);
  assert.ok(widget);
  await act(async () => widget.callback(`synthetic-${action}`));
}
async function submit(node) { await act(async () => node.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }))); }
(async () => {
  try {
    await act(async () => root.render(React.createElement(Forms, { sitekey: "test", location: "https://sver.tv/Example?report=profile&id=Example&field=avatar" })));
    const request = form("Request removal");
    assert.match(request.elements.locations.value, /field=avatar/);
    assert.equal(document.querySelector('input[type="file"]'), null, "Never request a copy of the image");
    assert.equal(request.querySelector("button").disabled, true);
    for (const name of ["name", "email", "locations", "good_faith", "signature", "signed_on"]) assert.equal(request.elements[name].required, true);
    await act(async () => {
      request.elements.capacity.value = "authorized";
      request.elements.capacity.dispatchEvent(new Event("change", { bubbles: true }));
    });
    assert.equal(request.elements.authority.required, true);
    Object.assign(request.elements.name, { value: "Synthetic Requester" });
    request.elements.email.value = "synthetic@example.invalid";
    request.elements.authority.value = "Synthetic authorization";
    request.elements.signature.value = "Synthetic Requester";
    request.elements.signed_on.value = "2026-10-04";
    request.elements.good_faith.checked = true;
    await solve("take_down");
    assert.equal(request.querySelector("button").disabled, false);
    assert.equal(form("Check removal request").querySelector("button").disabled, true, "The status form needs its own challenge");
    await submit(request);
    assert.match(document.querySelector('[role="alert"]').textContent, /Synthetic validation/);
    assert.equal(request.querySelector("button").disabled, true, "Used challenges are reset after errors");
    assert.equal(request.elements.name.value, "Synthetic Requester", "Keep the entered request after an error");
    fail = false;
    await solve("take_down");
    await submit(request);
    assert.equal(form("Request removal"), null);
    assert.match(document.querySelector('[role="status"]').textContent, /TID-2026-000123/);
    assert.equal(calls.at(-1).body.capacity, "authorized");
    assert.equal(calls.at(-1).body.good_faith, true);
    assert.deepEqual(calls.at(-1).body.locations, ["https://sver.tv/Example?report=profile&id=Example&field=avatar"]);
    const lookup = form("Check removal request");
    lookup.elements.number.value = "TID-2026-000123";
    lookup.elements.email.value = "synthetic@example.invalid";
    await solve("take_down_status");
    await submit(lookup);
    assert.match(lookup.querySelector('[role="status"]').textContent, /Not removed.*Synthetic mistaken request/s);
    assert.equal(lookup.querySelector("button").disabled, true);
    assert.deepEqual(Object.keys(calls.at(-1).body).sort(), ["email", "number", "turnstile_token"]);
    // Staff review preserves an existing legal reference and distinguishes service acceptance
    // from confirmed receipt. The audit detail remains text, never executable markup.
    const { default: Queue } = compile("apps/web/app/admin/take-it-down/page.tsx", {
      "../../../lib/client-api": client,
      "../../../components/StaffRemovalAlerts": { EnableStaffPush: () => null },
    });
    const now = new Date().toISOString();
    const item = { number: "TID-2026-000123", status: "under_review", received_at: now, deadline: now, resolved_at: null, reason: "", media_pending: 0, target_count: 1 };
    let detail = { number: item.number, minor: true, preservation_reference: "Synthetic preservation reference", details: { name: "Synthetic Requester", email: "synthetic@example.invalid", capacity: "shown", authority: "", locations: [], description: "", extra: "", signature: "Synthetic Requester", signed_on: "2026-10-04", good_faith: true }, events: [{ at: now, action: "under_review", actor: "synthetic-staff", detail: "<script>not executable</script>" }], targets: [{kind:"profile",id:"synthetic"}], evidence: [], notices: [{ audience: "requester", channel: "email", state: "expired", queued_at: now, last_attempt_at: null, attempts: 0, accepted_at: null }] };
    const staffCalls = [];
    global.fetch = async (url, options) => {
      staffCalls.push({url,method:options.method,body:options.body && JSON.parse(options.body)});
      return { ok: true, status: 200, json: async () => options.method === "POST" ? {saved:true} : url.endsWith(item.number) ? detail : {requests:[item],monthly:{received:1,removed:0,overdue:0,median_hours:null,longest_hours:null}} };
    };
    await act(async () => root.render(React.createElement(Queue)));
    const button = text => [...document.querySelectorAll("button")].find(node => node.textContent === text);
    await act(async () => button(`Review ${item.number}`).click());
    assert.equal(document.querySelector('input[name="minor"]').checked,true);
    assert.equal(document.querySelector('input[name="minor"]').disabled,true);
    assert.equal(document.querySelector('input[name="preservation_reference"]').value,"Synthetic preservation reference");
    assert.match(document.body.textContent,/Contact the recipient through an available channel/);
    assert.equal(document.querySelector("script"),null);
    assert.match(document.body.textContent,/<script>not executable<\/script>/);
    const reviewForm = document.querySelector('select[name="action"]').form;
    reviewForm.elements.reason.value = "Synthetic continued review";
    await submit(reviewForm);
    assert.equal(staffCalls.find(call=>call.method==="POST").body.preservation_reference,"Synthetic preservation reference");
    detail = {...detail, notices: [{...detail.notices[0],state:"accepted",attempts:1,accepted_at:now,last_attempt_at:now}]};
    await act(async () => button("Refresh case record").click());
    assert.match(document.body.textContent,/Accepted by email service/);
    assert.doesNotMatch(document.body.textContent,/Contact the recipient through an available channel/);
    // An action refused because the staff window lapsed opens the shared prompt (mounted once by the
    // admin layout); confirming with a code reruns the refused action once.
    const { StaffStepUp } = compile("apps/web/components/StepUp.tsx", { "../lib/client-api": client });
    const promptRoot = createRoot(document.body.appendChild(document.createElement("div")));
    await act(async () => promptRoot.render(React.createElement(StaffStepUp)));
    let stale = true;
    global.fetch = async (url, options) => {
      staffCalls.push({url,method:options.method,body:options.body && JSON.parse(options.body)});
      if (url === "/api/admin/confirm") { stale = false; return { ok: true, status: 200, json: async () => ({ confirmed: true }) }; }
      if (options.method === "POST" && stale) return { ok: false, status: 403, json: async () => ({ error: "Confirm your sign-in method again before using admin tools." }) };
      return { ok: true, status: 200, json: async () => options.method === "POST" ? {saved:true} : url.endsWith(item.number) ? detail : {requests:[item],monthly:{received:1,removed:0,overdue:0,median_hours:null,longest_hours:null}} };
    };
    reviewForm.elements.reason.value = "Synthetic dismissal";
    await submit(reviewForm);
    const secret = document.querySelector('input[name="secret"]');
    assert.ok(secret, "step-up prompt shown");
    secret.value = "123456";
    const before = staffCalls.length;
    await act(async () => { await submit(secret.form); await new Promise(resolve => setTimeout(resolve, 0)); });
    const rerun = staffCalls.slice(before).filter(call => call.method === "POST" && call.url.endsWith(item.number));
    assert.equal(rerun.length, 1, "action reruns once after confirming");
    assert.equal(rerun[0].body.reason, "Synthetic dismissal");
    assert.equal(document.querySelector('input[name="secret"]'), null, "step-up prompt closes");
    await act(async () => promptRoot.unmount());
    console.log("Take It Down form checks passed: anonymous required fields, location prefill, independent challenges, error recovery, receipt and private status lookup.");
    console.log("Staff removal checks passed: saved minor flag and preservation reference, safe audit text, notice failures and refreshed provider acceptance.");
  } finally {
    await act(async () => root.unmount());
    dom.window.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
