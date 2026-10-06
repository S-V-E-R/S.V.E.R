"use client";
import { useCallback, useState } from "react";
import { counterText, type Counter } from "../../../components/Crowd";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Mine = { counters: Counter[]; username: string; max: number };
const KINDS: [Counter["kind"], string, string][] = [
  ["deaths", "Death counter", "deaths"],
  ["shiny", "Shiny counter (encounters, phase and odds)", "shiny"],
  ["tally", "Win/loss tally", "record"],
  ["custom", "Custom counter", "count"],
];

/** Creator Studio → Counters (docs/CROWDSYNC.md "Counter widgets"). */
export default function StudioCounters() {
  const [mine, setMine] = useState<Mine | null>(null);
  const [state, setState] = useState<SaveState>({});
  const [draft, setDraft] = useState({ kind: "deaths" as Counter["kind"], id: "deaths", label: "Deaths", odds: "8192" });
  const load = useCallback(async () => {
    const r = await send<Mine>("GET", "/api/me/counters");
    if (r.ok) setMine(r.data); else setState({ error: r.error });
  }, []);
  useLoad(load);
  async function act(method: string, path: string, body?: unknown, saved?: string) {
    setState({});
    const r = await send<Mine>(method, path, body);
    if (r.ok) { setMine(r.data); if (saved) setState({ saved }); } else setState({ error: r.error, field: r.field });
  }
  if (!mine) return state.error ? <p role="alert" className="form-message error">{state.error}</p> : <p className="loading">Loading…</p>;
  return <><h1>Counters</h1>
    <Section title="Your counters" intro="Viewers see them under your player and in your OBS overlay. You and your moderators update them there or in chat: !id adds one, !id - takes one away, !id +5 or !id =10 changes the number, !id win / !id loss counts a tally, and !id phase starts a new shiny phase.">
      {mine.counters.length === 0 ? <p className="muted">No counters yet.</p> : <ul className="list">{mine.counters.map(c => <li key={c.id}>
        <span><strong>{c.label}</strong> <code>!{c.id}</code> · {counterText(c)}</span>
        <button type="button" className="link-button" onClick={() => { if (window.confirm(`Delete ${c.label}?`)) void act("DELETE", `/api/me/counters/${c.id}`, undefined, "Deleted."); }}>Delete</button>
      </li>)}</ul>}
    </Section>
    {mine.counters.length < mine.max && <Section title="Add a counter">
      <form className="reward-form" onSubmit={e => { e.preventDefault(); void act("POST", "/api/me/counters", { kind: draft.kind, id: draft.id, label: draft.label, odds: draft.kind === "shiny" ? Number(draft.odds) : null }, "Added."); }}>
        <label className="field narrow"><span>Type</span><select value={draft.kind} onChange={e => { const k = KINDS.find(x => x[0] === e.target.value)!; setDraft({ ...draft, kind: k[0], id: k[2], label: k[1].split(" (")[0] }); }}>{KINDS.map(([k, name]) => <option key={k} value={k}>{name}</option>)}</select></label>
        <label className="field narrow"><span>Chat name</span><input value={draft.id} maxLength={16} pattern="[A-Za-z0-9]+" required onChange={e => setDraft({ ...draft, id: e.target.value })} /></label>
        <label className="field"><span>Label</span><input value={draft.label} maxLength={40} required onChange={e => setDraft({ ...draft, label: e.target.value })} /></label>
        {draft.kind === "shiny" && <label className="field narrow"><span>Odds (1 in …)</span><input type="number" min={2} max={1000000} value={draft.odds} onChange={e => setDraft({ ...draft, odds: e.target.value })} /></label>}
        <button>Add</button>
      </form>
    </Section>}
    <Status state={state} />
  </>;
}
