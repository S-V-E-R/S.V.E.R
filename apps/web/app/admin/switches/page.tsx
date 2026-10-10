"use client";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Switch = { name: string; label: string; off: boolean; changed_by: string | null; changed_at: string | null };
type View = { switches: Switch[]; banner: { message: string; ends_at: string | null } | null };

/** Emergency switches and the site banner (docs/ADMIN.md "Operations"). Every change needs a note. */
export default function Switches() {
  const [view, setView] = useState<View | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/admin/switches");
    if (r.ok) setView(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function flip(event: FormEvent<HTMLFormElement>, s: Switch) {
    event.preventDefault();
    const note = new FormData(event.currentTarget).get("note");
    const r = await send<View>("PUT", `/api/admin/switches/${s.name}`, { off: !s.off, note });
    if (r.ok) { setView(r.data); setMessage(`${s.label} ${s.off ? "is back on" : "is off"}.`); } else setMessage(r.error);
  }
  async function banner(event: FormEvent<HTMLFormElement>, clear: boolean) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const ends = String(f.get("ends_at") ?? "");
    const r = await send<View>("PUT", "/api/admin/banner", { message: clear ? null : f.get("message"), ends_at: !clear && ends ? new Date(ends).toISOString() : null, note: f.get("note") });
    if (r.ok) { setView(r.data); setMessage(clear ? "Banner removed." : "Banner saved."); } else setMessage(r.error);
  }
  return <section className="panel section"><h1>Emergency switches</h1>
    <p className="intro">Turn a feature off during an incident. People see a short message where it would be; nothing else changes. Every flip is audited.</p>
    {message && <p role="status" className="form-message">{message}</p>}
    {!view ? <p className="loading">Loading…</p> : <>
      <ul className="list">{view.switches.map(s => <li key={s.name} className="stack">
        <span><strong>{s.label}</strong> <span className={s.off ? "badge danger" : "badge"}>{s.off ? "Off" : "On"}</span>{s.changed_by && <span className="muted small"> · last changed by {s.changed_by}{s.changed_at && ` ${new Date(s.changed_at).toLocaleString()}`}</span>}</span>
        <form className="row wrap" onSubmit={e => void flip(e, s)}>
          <label className="field narrow"><span className="sr-only">Note for {s.label}</span><input name="note" required maxLength={500} placeholder="Note (required)" /></label>
          <button type="submit" className={s.off ? "small" : "small danger"}>{s.off ? "Turn back on" : "Turn off"}</button>
        </form>
      </li>)}</ul>
      <h2>Site banner</h2>
      <form className="stack" onSubmit={e => void banner(e, false)}>
        <label className="field"><span>Message (up to 280 characters, shown on every page)</span><input name="message" required maxLength={280} defaultValue={view.banner?.message ?? ""} /></label>
        <label className="field narrow"><span>Ends (optional)</span><input name="ends_at" type="datetime-local" /></label>
        <label className="field"><span>Note (required)</span><input name="note" required maxLength={500} /></label>
        <span className="row wrap"><button type="submit">Save banner</button></span>
      </form>
      {view.banner && <form className="row wrap" onSubmit={e => void banner(e, true)}>
        <label className="field narrow"><span className="sr-only">Note for removing the banner</span><input name="note" required maxLength={500} placeholder="Note (required)" /></label>
        <button type="submit" className="quiet small">Remove banner</button>
      </form>}
    </>}
  </section>;
}
