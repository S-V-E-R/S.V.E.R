"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Window = { at: string; raw: number; counted: number; trusted: number; excluded: number; pending: number };
type Case = {
  id: string; username: string; opened_at: string; status: string; note: string; hold_payouts: boolean; pause_tier: boolean;
  decided_at: string | null; decided_by: string | null;
  evidence: { windows: Window[]; flags: Record<string, number>; networks_excluded: number; guests: number };
};
const when = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

/** Viewer integrity cases: aggregate evidence only. Bots aimed at a channel never punish it without staff review. */
export default function Integrity() {
  const [cases, setCases] = useState<Case[] | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<{ cases: Case[] }>("GET", "/api/admin/integrity"); if (r.ok) setCases(r.data.cases); }, []);
  useLoad(load);
  async function decide(event: FormEvent<HTMLFormElement>, id: string) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const r = await send("POST", `/api/admin/integrity/${id}/decision`, { outcome: f.get("outcome"), note: f.get("note"), hold_payouts: f.get("hold") === "on", pause_tier: f.get("pause") === "on" });
    setState(r.ok ? { saved: "Decision saved." } : r);
    load();
  }
  async function hold(c: Case) {
    const note = window.prompt(c.hold_payouts ? "Why lift the payout hold?" : "Why hold payouts?");
    if (!note) return;
    const r = await send<{ payouts_held: boolean }>("POST", `/api/admin/integrity/${c.id}/hold`, { hold: !c.hold_payouts, note });
    setState(!r.ok ? r : { saved: !c.hold_payouts ? "Payouts held." : r.data.payouts_held ? `Lifted on this case; another case still holds @${c.username}'s payouts.` : "Payout hold lifted." });
    load();
  }
  if (!cases) return <p className="loading">Loading…</p>;
  return <><h1>Viewer integrity</h1><Status state={state} />
    <p className="muted">A case opens when a broadcast&apos;s share of uncounted viewers stays high. Uncounted viewers are already left out of every count; act against a streamer only with evidence that they caused the traffic. Issue strikes from the user&apos;s page so the normal appeal applies.</p>
    {cases.length === 0 ? <p className="muted">No cases.</p> : cases.map(c => <Section key={c.id} title={`@${c.username} · ${when(c.opened_at)} · ${c.status.toLowerCase()}`}>
      <ul className="list">{c.evidence.windows.map(w => <li key={w.at}>{when(w.at)}: {w.raw} sessions, {w.counted} counted ({w.trusted} trusted), {w.excluded} excluded, {w.pending} pending</li>)}</ul>
      <p className="muted">Signals: {Object.entries(c.evidence.flags).map(([k, n]) => `${k.replaceAll("_", " ")} ${n}`).join(", ") || "none"} · {c.evidence.networks_excluded} networks among excluded sessions · {c.evidence.guests} guests</p>
      <p className="row wrap"><Link href={`/admin/users/${c.username}`}>Open @{c.username}&apos;s standing</Link>
        {c.status !== "DISMISSED" && <button type="button" className="small" onClick={() => hold(c)}>{c.hold_payouts ? "Lift payout hold" : "Hold payouts"}</button>}</p>
      {c.status === "OPEN" ? <form onSubmit={e => decide(e, c.id)}>
        <fieldset className="row"><legend className="sr-only">Outcome</legend><label className="row"><input type="radio" name="outcome" value="dismiss" required /> Dismiss</label><label className="row"><input type="radio" name="outcome" value="action" /> Take action</label></fieldset>
        <label className="row"><input type="checkbox" name="hold" defaultChecked={c.hold_payouts} /> Hold payouts</label>
        <label className="row"><input type="checkbox" name="pause" /> Pause tier promotion</label>
        <label className="field"><span>Staff note (required)</span><textarea name="note" required maxLength={500} rows={2} /></label>
        <button type="submit" className="small">Decide</button>
      </form> : <p className="muted">{c.decided_by ? `@${c.decided_by}` : "Staff"} · {c.decided_at && when(c.decided_at)}{c.hold_payouts && " · payouts held"}{c.pause_tier && " · tier paused"} · {c.note}</p>}
    </Section>)}
  </>;
}
