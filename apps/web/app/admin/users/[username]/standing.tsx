"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";
import { BanForm, UsernameResetForm } from "../../../../components/Bans";
import { StrikeFields, strikeFrom } from "../../reports/queue";

type Strike = { id: string; reason: string; severity: string; penalty: string; level: number; issued_at: string; issued_by: string | null; status: string; penalty_until: string | null; staff_note: string };
type Data = { username: string; internal: boolean; deleted: boolean; level: number; restriction: null | { until: string | null; indefinite: boolean }; strikes: Strike[]; interim_restrictions: { id: string; until: string; resolution: string }[]; history: { action: string; note: string; created_at: string; actor: string | null }[] };
const when = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

export default function UserStanding({ username }: { username: string }) {
  const [data, setData] = useState<Data | null>(null);
  const [error, setError] = useState("");
  const [state, setState] = useState<SaveState>({});
  const base = `/api/admin/users/${encodeURIComponent(username)}`;
  const load = useCallback(async () => { const r = await send<Data>("GET", `${base}/standing`); if (r.ok) setData(r.data); else setError(r.status === 404 ? "No such user." : r.error); }, [base]);
  useLoad(load);
  const done = (r: { ok: boolean; status?: number; error?: string }, ok: string) => { setState(r.ok ? { saved: ok } : { error: r.status === 403 ? "Confirm your sign-in method again on the Account page, then retry." : r.error }); load(); };
  async function strike(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const s = strikeFrom(f);
    if (!s) return setState({ error: "Choose a strike reason." });
    done(await send("POST", `${base}/strikes`, { ...s, note: f.get("note") }), "Strike issued.");
  }
  async function interim(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    done(await send("POST", `${base}/interim-restriction`, { hours: Number(f.get("hours")), note: f.get("note") }), "Interim restriction set.");
  }
  async function lift() {
    const note = window.prompt("Note for lifting the restriction");
    if (note) done(await send("POST", `${base}/restriction/lift`, { note }), "Restriction lifted.");
  }
  if (!data) return <p className="loading">{error || "Loading…"}</p>;
  return <><h1>@{data.username}</h1>
    <Section title="Standing">
      <p>Active strikes: {data.level}. {data.restriction ? `Restricted ${data.restriction.indefinite ? "indefinitely" : `until ${when(data.restriction.until!)}`}.` : "Not restricted."}{data.internal && " Internal account: strikes and restrictions don't apply."}{data.deleted && " Deletion pending."}</p>
      {data.restriction && <button type="button" className="small quiet" onClick={lift}>Lift restriction</button>}
      <Status state={state} />
    </Section>
    {!data.internal && <>
      <BanForm username={data.username} onDone={load} />
      <UsernameResetForm username={data.username} />
      <Section title="Issue a strike"><form onSubmit={strike}><StrikeFields /><label className="field"><span>Moderator note (required)</span><textarea name="note" required maxLength={500} rows={2} /></label><button type="submit" className="small">Issue strike</button></form></Section>
      <Section title="Interim restriction" intro="Hides the channel for up to 24 hours while a report is reviewed. It needs no strike and never stacks with the strike it becomes."><form onSubmit={interim} className="row wrap"><label className="field narrow"><span>Hours</span><input name="hours" type="number" min={1} max={24} defaultValue={24} required /></label><label className="field grow"><span>Note (required)</span><input name="note" required maxLength={500} /></label><button type="submit" className="small">Restrict</button></form></Section>
    </>}
    <Section title="Strikes">{data.strikes.length === 0 ? <p className="muted">No strikes.</p> : <ul className="list">{data.strikes.map(s => <li key={s.id}>{s.penalty} · {s.reason} · {s.severity} · {s.status} · {when(s.issued_at)}{s.issued_by && ` · @${s.issued_by}`}{s.staff_note && <p className="muted">{s.staff_note}</p>}</li>)}</ul>}</Section>
    <Section title="Moderation history">{data.history.length === 0 ? <p className="muted">None.</p> : <ul className="list">{data.history.map((h, i) => <li key={i}>{h.action} · {h.actor ? `@${h.actor}` : "system"} · {when(h.created_at)}{h.note && ` — ${h.note}`}</li>)}</ul>}</Section>
  </>;
}
