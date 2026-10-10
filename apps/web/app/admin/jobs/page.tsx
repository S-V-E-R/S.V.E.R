"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Queue = { name: string; label: string; waiting: number; stuck: number; gave_up: number; retry: boolean; link?: string };
type Item = { queue: string; id: string; attempts: number; created_at: string; next_try: string | null; gave_up: boolean; error: string | null };
type View = { queues: Queue[]; items: Item[] };

/** Failed and stuck background work (docs/ADMIN.md "Jobs"). A retry makes the job due now; the worker still runs it once. */
export default function Jobs() {
  const [view, setView] = useState<View | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/admin/jobs");
    if (r.ok) setView(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function retry(event: FormEvent<HTMLFormElement>, item: Item) {
    event.preventDefault();
    const r = await send<View>("POST", `/api/admin/jobs/${item.queue}/${encodeURIComponent(item.id)}/retry`, { note: new FormData(event.currentTarget).get("note") });
    if (r.ok) { setView(r.data); setMessage("Retrying."); } else setMessage(r.error);
  }
  const label = (name: string) => view?.queues.find(q => q.name === name)?.label ?? name;
  return <section className="panel section"><h1>Jobs</h1>
    {message && <p role="status" className="form-message">{message}</p>}
    {!view ? <p className="loading">Loading…</p> : <>
      <div className="table-scroll"><table className="table"><thead><tr><th scope="col">Queue</th><th scope="col">Waiting</th><th scope="col">Stuck</th><th scope="col">Gave up</th></tr></thead>
        <tbody>{view.queues.map(q => <tr key={q.name}><th scope="row">{q.link ? <Link href={q.link}>{q.label}</Link> : q.label}</th><td>{q.waiting}</td><td>{q.stuck}</td><td>{q.gave_up}</td></tr>)}</tbody></table></div>
      <h2>Stuck and failed</h2>
      {view.items.length === 0 ? <p className="muted">Nothing is stuck.</p> : <ul className="list">{view.items.map(item => <li key={`${item.queue}:${item.id}`} className="stack">
        <span><strong>{label(item.queue)}</strong> <code>{item.id}</code> <span className="muted small">{item.attempts} attempts · queued {new Date(item.created_at).toLocaleString()} · {item.gave_up ? "gave up" : `next try ${new Date(item.next_try ?? "").toLocaleString()}`}</span></span>
        {item.error && <span className="small">{item.error}</span>}
        <form className="row wrap" onSubmit={e => void retry(e, item)}>
          <label className="field narrow"><span className="sr-only">Note for retrying {item.id}</span><input name="note" required maxLength={500} placeholder="Note (required)" /></label>
          <button type="submit" className="small">Retry now</button>
        </form>
      </li>)}</ul>}
    </>}
  </section>;
}
