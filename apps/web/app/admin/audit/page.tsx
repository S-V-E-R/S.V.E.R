"use client";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Action = { id: string; action: string; target_type: string; target_id: string; note: string; detail: unknown; self_review: boolean; created_at: string; actor: string | null };
type Page = { actions: Action[]; next_cursor: string | null };

/** The audit log (docs/ADMIN.md "Audit log"): every staff action, read-only, searchable. */
export default function Audit() {
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<Action[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState("");
  const fetchPage = useCallback(async (q: string, cursor: string | null) => {
    const r = await send<Page>("GET", `/api/admin/audit?${q}${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`);
    if (!r.ok) { setError(r.error); return; }
    setError(""); setRows(old => cursor && old ? [...old, ...r.data.actions] : r.data.actions); setNext(r.data.next_cursor);
  }, []);
  const first = useCallback(() => fetchPage("", null), [fetchPage]);
  useLoad(first);
  function search(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const params = new URLSearchParams();
    for (const [k, v] of new FormData(event.currentTarget)) if (String(v).trim()) params.set(k, String(v).trim());
    const q = params.toString();
    setQuery(q); setRows(null); void fetchPage(q, null);
  }
  return <section className="panel section"><h1>Audit log</h1>
    <p className="intro">Every staff action, newest first. Nobody can edit or delete these rows.</p>
    <form className="row wrap" onSubmit={search} aria-label="Search the audit log">
      <label className="field narrow"><span>Staff member</span><input name="actor" maxLength={30} /></label>
      <label className="field narrow"><span>Action</span><input name="action" maxLength={60} placeholder="e.g. switch_off" /></label>
      <label className="field narrow"><span>Target</span><input name="target" maxLength={100} placeholder="ID or type" /></label>
      <label className="field narrow"><span>From</span><input name="from" type="date" /></label>
      <label className="field narrow"><span>To</span><input name="to" type="date" /></label>
      <button type="submit">Search</button>
    </form>
    {error && <p role="alert" className="error">{error}</p>}
    {!rows ? <p className="loading">Loading…</p> : rows.length === 0 ? <p className="muted">No actions match.</p> : <>
      <div className="table-scroll"><table className="table"><thead><tr><th scope="col">When</th><th scope="col">Staff</th><th scope="col">Action</th><th scope="col">Target</th><th scope="col">Note</th></tr></thead>
        <tbody>{rows.map(a => <tr key={a.id}><td>{new Date(a.created_at).toLocaleString()}</td><td>{a.actor ?? "System"}</td><td><code>{a.action}</code>{a.self_review && <span className="badge"> self-review</span>}</td><td>{a.target_type} <code>{a.target_id}</code></td><td>{a.note}</td></tr>)}</tbody></table></div>
      {next && <button type="button" className="quiet" onClick={() => void fetchPage(query, next)}>Older actions</button>}
    </>}
  </section>;
}
