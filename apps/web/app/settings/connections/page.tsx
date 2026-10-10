"use client";
import { useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Connection = { id: string; app: string; owner: string; scopes: string[]; created_at: string; last_used_at: string | null };

/** Settings → Connected apps (docs/DEVELOPER_PLATFORM.md §1). */
export default function ConnectedApps() {
  const [rows, setRows] = useState<Connection[] | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ connections: Connection[] }>("GET", "/api/me/connections");
    if (r.ok) setRows(r.data.connections); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function revoke(c: Connection) {
    const r = await send<{ connections: Connection[] }>("DELETE", `/api/me/connections/${c.id}`);
    if (r.ok) { setRows(r.data.connections); setMessage(`${c.app} can no longer use your account.`); } else setMessage(r.error);
  }
  return <section className="panel section"><h1>Connected apps</h1>
    <p className="intro">Apps you&apos;ve allowed to use your account. Removing one stops it right away. Apps can never spend Valor or move money.</p>
    {message && <p role="status" className="form-message">{message}</p>}
    {rows === null ? <p className="loading">Loading…</p> : rows.length === 0 ? <p className="muted">No apps are connected.</p>
      : <ul className="list">{rows.map(c => <li key={c.id} className="row between wrap">
        <span><strong>{c.app}</strong> <span className="muted small">by @{c.owner} · {c.scopes.join(", ")} · connected {new Date(c.created_at).toLocaleDateString()}{c.last_used_at ? ` · last used ${new Date(c.last_used_at).toLocaleDateString()}` : ""}</span></span>
        <button type="button" className="small quiet danger-text" onClick={() => revoke(c)}>Remove</button>
      </li>)}</ul>}
  </section>;
}
