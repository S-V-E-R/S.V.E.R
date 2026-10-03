"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Strike = { id: string; reason: string; severity: string; penalty: string; level: number; issued_at: string; issued_by: string | null; staff_note: string; content: { field: string | null; value: unknown }; status: string };
type Appeal = { id: string; body: string; created_at: string; username: string; strike: Strike; reports: { reason: string; note: string }[]; history: Strike[] };
const when = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

function Decide({ appeal, onDone }: { appeal: Appeal; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const r = await send("POST", `/api/admin/appeals/${appeal.id}/decision`, { outcome: f.get("outcome"), staff_note: f.get("staff_note"), message_to_user: f.get("message_to_user") || "" });
    setState(r.ok ? { saved: "Decision recorded." } : r);
    if (r.ok) onDone();
  }
  return <form onSubmit={submit}>
    <div className="row"><label className="checkbox"><input type="radio" name="outcome" value="upheld" required /> Uphold</label><label className="checkbox"><input type="radio" name="outcome" value="overturned" /> Overturn (removes the strike and restores removed content)</label></div>
    <label className="field"><span>Staff note (required)</span><textarea name="staff_note" required maxLength={500} rows={2} /></label>
    <label className="field"><span>Message to the user (optional, signed “S.V.E.R moderators”)</span><textarea name="message_to_user" maxLength={500} rows={2} /></label>
    <button type="submit" className="small">Record decision</button>
    <Status state={state} />
  </form>;
}

export default function Appeals() {
  const [items, setItems] = useState<Appeal[] | null>(null);
  const load = useCallback(async () => { const r = await send<{ appeals: Appeal[] }>("GET", "/api/admin/appeals"); if (r.ok) setItems(r.data.appeals); }, []);
  useLoad(load);
  if (!items) return <p className="loading">Loading…</p>;
  return <><h1>Appeals</h1>
    {items.length === 0 ? <p className="panel section muted">No appeals waiting.</p> : items.map(a => <Section key={a.id} title={`@${a.username} · ${a.strike.penalty} · ${a.strike.reason}`}>
      <p className="muted">Appealed {when(a.created_at)} · Strike issued {when(a.strike.issued_at)} by {a.strike.issued_by ? `@${a.strike.issued_by}` : "staff"} · <Link href={`/admin/users/${a.username}`}>Standing</Link></p>
      <blockquote><p>{a.body}</p></blockquote>
      <details><summary>Strike content and note</summary>{a.strike.staff_note && <p>{a.strike.staff_note}</p>}<pre className="snapshot">{JSON.stringify(a.strike.content?.value, null, 2)}</pre>{a.reports.map((r, i) => <p key={i} className="muted">Report: {r.reason}{r.note && ` — ${r.note}`}</p>)}</details>
      <details><summary>Strike history ({a.history.length})</summary><ul className="list">{a.history.map(s => <li key={s.id}>{s.penalty} · {s.reason} · {s.status} · {when(s.issued_at)}</li>)}</ul></details>
      <Decide appeal={a} onDone={load} />
    </Section>)}
  </>;
}
