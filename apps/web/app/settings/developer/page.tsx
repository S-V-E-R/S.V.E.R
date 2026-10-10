"use client";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type DevApp = { client_id: string; name: string; redirect_uris: string[]; confidential: boolean; suspended: boolean; users: number };
type Apps = { apps: DevApp[]; max: number; created?: { client_id: string; client_secret: string | null } };

/** Settings → Developer: register apps that use S.V.E.R sign-in (docs/DEVELOPER_PLATFORM.md §1). */
export default function Developer() {
  const [data, setData] = useState<Apps | null>(null);
  const [secret, setSecret] = useState<{ client_id: string; client_secret: string } | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<Apps>("GET", "/api/me/apps");
    if (r.ok) setData(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const v = new FormData(form);
    const r = await send<Apps>("POST", "/api/me/apps", { name: v.get("name"), redirect_uris: String(v.get("uris") ?? "").split(/\s+/).filter(Boolean), confidential: v.get("confidential") === "on" });
    if (!r.ok) { setMessage(r.error); return; }
    setData(r.data); form.reset(); setMessage("App registered.");
    if (r.data.created?.client_secret) setSecret({ client_id: r.data.created.client_id, client_secret: r.data.created.client_secret });
  }
  async function rotate(id: string) {
    const r = await send<{ client_id: string; client_secret: string }>("POST", `/api/me/apps/${id}/secret`);
    if (r.ok) setSecret(r.data); else setMessage(r.error);
  }
  async function remove(app: DevApp) {
    const r = await send<Apps>("DELETE", `/api/me/apps/${app.client_id}`);
    if (r.ok) { setData(r.data); setMessage(`${app.name} deleted; its access to everyone's accounts ended.`); } else setMessage(r.error);
  }
  return <section className="panel section"><h1>Developer</h1>
    <p className="intro">Register apps that sign people in with S.V.E.R (OAuth 2.1 with PKCE). People approve each app on a consent screen and can remove it in Connected apps. You need a verified email and two-factor sign-in.</p>
    {message && <p role="status" className="form-message">{message}</p>}
    {secret && <div className="panel" role="alert"><p><strong>Client secret for {secret.client_id}</strong>, shown once. Store it on your server; never put it in a browser or desktop app.</p><code className="secret">{secret.client_secret}</code> <button type="button" className="small quiet" onClick={() => setSecret(null)}>I&apos;ve saved it</button></div>}
    {data === null ? <p className="loading">Loading…</p> : <>
      {data.apps.length > 0 && <ul className="list">{data.apps.map(a => <li key={a.client_id} className="stack">
        <span><strong>{a.name}</strong> {a.suspended && <span className="badge">Suspended by staff</span>} <span className="muted small">{a.users} connected</span></span>
        <span className="small">Client ID <code>{a.client_id}</code> · {a.confidential ? "server app (has a secret)" : "public app (PKCE only)"}</span>
        <span className="muted small">Redirects: {a.redirect_uris.join(", ")}</span>
        <span className="row wrap">{a.confidential && <button type="button" className="small quiet" onClick={() => rotate(a.client_id)}>New secret</button>}<button type="button" className="small quiet danger-text" onClick={() => remove(a)}>Delete</button></span>
      </li>)}</ul>}
      {data.apps.length < data.max && <form className="stack" onSubmit={create}>
        <h2>Register an app</h2>
        <label className="field"><span>Name</span><input name="name" required maxLength={40} /></label>
        <label className="field"><span>Redirect URIs (one per line; https://, or http://127.0.0.1 for desktop apps)</span><textarea name="uris" required rows={2} /></label>
        <label className="row"><input type="checkbox" name="confidential" /> Server app (gets a client secret)</label>
        <button type="submit">Register</button>
      </form>}
      <details><summary>How to use it</summary><ol className="small">
        <li>Send people to <code>https://sver.tv/oauth/authorize?client_id=…&amp;redirect_uri=…&amp;response_type=code&amp;scope=user:read&amp;state=…&amp;code_challenge=…&amp;code_challenge_method=S256</code>.</li>
        <li>Exchange the returned <code>code</code> at <code>POST /api/oauth/token</code> (form-encoded) with <code>grant_type=authorization_code</code>, your <code>client_id</code>, the <code>code_verifier</code> and the same <code>redirect_uri</code>.</li>
        <li>Call the API with <code>Authorization: Bearer …</code> and <code>SVER-Client-Id</code>. Access tokens last an hour; refresh tokens 60 days and work once each.</li>
        <li>Scopes: user:read, channel:read, chat:read, chat:write, channel:moderate, channel:edit, events:private, board:control, whispers:read, channel:run.</li>
      </ol></details>
    </>}
    <Webhooks />
  </section>;
}

type Hook = { id: string; app: string | null; topics: string[]; url: string; failures: number; disabled: boolean };
type Hooks = { hooks: Hook[]; max: number; created?: { id: string; secret: string } };

/** Webhooks for live events (docs/DEVELOPER_PLATFORM.md §2): the same events as /api/events, posted to a URL. */
function Webhooks() {
  const [data, setData] = useState<Hooks | null>(null);
  const [secret, setSecret] = useState("");
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<Hooks>("GET", "/api/hooks");
    if (r.ok) setData(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const v = new FormData(form);
    const r = await send<Hooks>("POST", "/api/hooks", { url: v.get("url"), topics: String(v.get("topics") ?? "").split(/\s+/).filter(Boolean) });
    if (!r.ok) { setMessage(r.error); return; }
    setData(r.data); form.reset(); setMessage("Webhook added.");
    if (r.data.created) setSecret(r.data.created.secret);
  }
  async function remove(id: string) {
    const r = await send<Hooks>("DELETE", `/api/hooks/${id}`);
    if (r.ok) { setData(r.data); setMessage("Webhook removed."); } else setMessage(r.error);
  }
  return <section className="stack">
    <h2>Webhooks</h2>
    <p className="muted small">S.V.E.R posts each event as JSON to your HTTPS URL, signed in the <code>SVER-Signature</code> header with the webhook&apos;s secret. Failed deliveries are retried 8 times; after 50 failures in a row the webhook turns off and you&apos;re notified.</p>
    {message && <p role="status" className="form-message">{message}</p>}
    {secret && <div className="panel" role="alert"><p><strong>Signing secret</strong>, shown once.</p><code className="secret">{secret}</code> <button type="button" className="small quiet" onClick={() => setSecret("")}>I&apos;ve saved it</button></div>}
    {data === null ? <p className="loading">Loading…</p> : <>
      {data.hooks.length > 0 && <ul className="list">{data.hooks.map(h => <li key={h.id} className="stack">
        <span><code>{h.url}</code> {h.disabled && <span className="badge">Turned off</span>} {h.app && <span className="muted small">added by {h.app}</span>}</span>
        <span className="muted small">{h.topics.join(", ")}{h.failures > 0 && !h.disabled && ` · ${h.failures} failed in a row`}</span>
        <span className="row wrap"><button type="button" className="small quiet danger-text" onClick={() => remove(h.id)}>Remove</button></span>
      </li>)}</ul>}
      {data.hooks.length < data.max && <form className="stack" onSubmit={create}>
        <label className="field"><span>URL</span><input name="url" type="url" required maxLength={500} placeholder="https://" /></label>
        <label className="field"><span>Topics (one per line, e.g. channel:yourname:follows or channel:yourname:subs)</span><textarea name="topics" required rows={3} /></label>
        <button type="submit">Add webhook</button>
      </form>}
    </>}
  </section>;
}
