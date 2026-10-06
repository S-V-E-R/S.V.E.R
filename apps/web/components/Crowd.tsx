"use client";
import { useCallback, useEffect, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { playerDelay } from "./BoardEffects";
import "../styles/boards.css";

export type PollState = {
  id: string; kind: "poll" | "prediction"; question: string; options: string[]; ends_at: string; grace_seconds: number;
  status: "open" | "ended" | "locked" | "resolved" | "cancelled"; winner: number | null; counts: number[]; pools: number[] | null;
  mine?: { option: number; stake: number; payout: number | null } | null;
};
export type Counter = { id: string; kind: "shiny" | "deaths" | "tally" | "custom"; label: string; value: number; extra: number; odds: number | null };
type State = { poll: PollState | null; prediction: PollState | null; counters: Counter[]; balance: number | null; can_run: boolean; can_resolve: boolean; max_stake: number };

/** How a counter reads: "12", "4–2", or a shiny hunt's encounters, phase and chance so far. */
export function counterText(c: Counter) {
  if (c.kind === "tally") return `${c.value}–${c.extra}`;
  if (c.kind === "shiny") {
    const chance = c.odds ? ` · ${((1 - Math.pow(1 - 1 / c.odds, c.value)) * 100).toFixed(1)}% by now (1 in ${c.odds.toLocaleString()})` : "";
    return `${c.value.toLocaleString()} encounters · phase ${c.extra + 1}${chance}`;
  }
  return c.value.toLocaleString();
}

export function Results({ poll }: { poll: PollState }) {
  const total = poll.counts.reduce((a, b) => a + b, 0);
  return <ul className="list crowd-results">{poll.options.map((option, i) => {
    const share = total ? Math.round((poll.counts[i] / total) * 100) : 0;
    return <li key={option}>
      <span>{poll.winner === i && "✓ "}{option}{poll.mine?.option === i && <strong> (you)</strong>}</span>
      <span className="muted">{poll.pools ? `${poll.pools[i].toLocaleString()} EV · ` : ""}{poll.counts[i]} ({share}%)</span>
      <progress max={100} value={share} aria-label={`${option}: ${share}%`} />
    </li>;
  })}</ul>;
}

function PollCard({ poll, path, state, now, onChange }: { poll: PollState; path: string; state: State; now: number; onChange: () => void }) {
  const [stake, setStake] = useState("100");
  const [winner, setWinner] = useState(0);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  // The window closes by stream time: when this viewer's own video reaches the end.
  const left = Math.ceil((Date.parse(poll.ends_at) + Math.min(playerDelay(), (poll.grace_seconds - 1) * 1000) - now) / 1000);
  const open = poll.status === "open" && left > 0;
  const prediction = poll.kind === "prediction";
  async function act(url: string, body: unknown) {
    setBusy(true); setNote("");
    const r = await send("POST", url, body);
    setBusy(false);
    if (!r.ok) setNote(r.error);
    onChange();
  }
  const status = poll.status === "resolved" ? `Result: ${poll.options[poll.winner ?? 0]}` : poll.status === "cancelled" ? "Cancelled; stakes were refunded." : poll.status === "locked" ? "Locked. Waiting for the result." : open ? `${left}s left` : "Voting has closed.";
  return <section className="crowd-card frame" aria-label={prediction ? "Prediction" : "Poll"}>
    <span className="eyebrow">{prediction ? "Prediction · Engagement Valor" : "Poll"}</span>
    <h3>{poll.question}</h3>
    <p className="muted small" role="status">{status}{poll.mine?.payout != null && ` · You got back ${poll.mine.payout.toLocaleString()} EV`}</p>
    {open && !poll.mine && state.balance !== null ? <div className="crowd-options">
      {prediction && <label className="field narrow"><span>Stake (you have {state.balance.toLocaleString()})</span><input type="number" min={1} max={Math.min(state.max_stake, state.balance)} value={stake} onChange={e => setStake(e.target.value)} /></label>}
      {poll.options.map((option, i) => <button key={option} type="button" className="small" disabled={busy} onClick={() => act(`${path}/polls/${poll.id}/vote`, { option: i, stake: prediction ? Number(stake) : undefined })}>{option}</button>)}
    </div> : <Results poll={poll} />}
    {state.can_run && (poll.status === "open" || poll.status === "locked") && <div className="row wrap">
      {poll.status === "open" && <button type="button" className="small quiet" disabled={busy} onClick={() => act(`${path}/polls/${poll.id}/close`, { action: "end" })}>{prediction ? "Lock now" : "End now"}</button>}
      {prediction && state.can_resolve && <>
        <select aria-label="Outcome that happened" value={winner} onChange={e => setWinner(Number(e.target.value))}>{poll.options.map((o, i) => <option key={o} value={i}>{o}</option>)}</select>
        <button type="button" className="small" disabled={busy} onClick={() => { if (window.confirm(`Pay out "${poll.options[winner]}"?`)) void act(`${path}/polls/${poll.id}/close`, { action: "resolve", winner }); }}>Resolve</button>
      </>}
      <button type="button" className="small quiet" disabled={busy} onClick={() => { if (window.confirm(prediction ? "Cancel and refund every stake?" : "Cancel this poll?")) void act(`${path}/polls/${poll.id}/close`, { action: "cancel" }); }}>Cancel</button>
    </div>}
    {note && <p role="alert" className="form-message error">{note}</p>}
  </section>;
}

function StartForm({ path, onChange }: { path: string; onChange: () => void }) {
  const [kind, setKind] = useState<"poll" | "prediction">("poll");
  const [question, setQuestion] = useState("");
  const [options, setOptions] = useState("");
  const [seconds, setSeconds] = useState("120");
  const [note, setNote] = useState("");
  async function submit(e: React.FormEvent) {
    e.preventDefault(); setNote("");
    const r = await send("POST", `${path}/polls`, { kind, question, options: options.split("\n").map(o => o.trim()).filter(Boolean), seconds: Number(seconds) });
    if (r.ok) { setQuestion(""); setOptions(""); onChange(); } else setNote(r.error);
  }
  return <details className="crowd-start">
    <summary>Start a poll or prediction</summary>
    <form onSubmit={submit}>
      <label className="field narrow"><span>Type</span><select value={kind} onChange={e => setKind(e.target.value as "poll" | "prediction")}><option value="poll">Poll (2–5 options)</option><option value="prediction">Prediction (2–10 outcomes, Engagement Valor)</option></select></label>
      <label className="field"><span>Question</span><input value={question} maxLength={120} required onChange={e => setQuestion(e.target.value)} /></label>
      <label className="field"><span>Options, one per line</span><textarea value={options} rows={4} required onChange={e => setOptions(e.target.value)} /></label>
      <label className="field narrow"><span>{kind === "poll" ? "Voting time" : "Locks after"} (seconds)</span><input type="number" min={15} max={1800} value={seconds} onChange={e => setSeconds(e.target.value)} /></label>
      <button className="small">Start</button>
      {note && <p role="alert" className="form-message error">{note}</p>}
    </form>
  </details>;
}

/**
 * Polls, predictions and counters under the player (docs/CROWDSYNC.md). Live updates arrive over
 * the chat socket (Chat's "sver:board" events); only the owner and moderators see the controls.
 */
export function Crowd({ username }: { username: string }) {
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [state, setState] = useState<State | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const load = useCallback(async () => {
    const r = await send<State>("GET", `${path}/crowd`);
    if (r.ok) setState(r.data);
  }, [path]);
  useLoad(load);
  useEffect(() => {
    const on = (event: Event) => {
      const { channel, event: e } = (event as CustomEvent<{ channel: string; event: { type: string; poll?: PollState; counters?: Counter[] } }>).detail;
      if (channel !== username.toLowerCase()) return;
      if (e.type === "counters" && e.counters) { const counters = e.counters; setState(s => s && { ...s, counters }); }
      else if (e.type === "poll" && e.poll) {
        const poll = e.poll;
        // Live results; the viewer's own vote, stake and payout come from a reload when they change.
        setState(s => s && { ...s, [poll.kind]: { ...poll, mine: s[poll.kind]?.id === poll.id ? s[poll.kind]?.mine : null } });
        if (poll.status === "resolved" || poll.status === "cancelled") void load();
      }
    };
    window.addEventListener("sver:board", on);
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => { window.removeEventListener("sver:board", on); clearInterval(tick); };
  }, [username, load]);
  if (!state || (!state.poll && !state.prediction && !state.counters.length && !state.can_run)) return null;
  const counter = async (id: string, body: unknown) => { await send("POST", `${path}/counters/${encodeURIComponent(id)}`, body); };
  return <div className="crowd">
    {state.counters.length > 0 && <ul className="crowd-counters" aria-label="Counters">{state.counters.map(c => <li key={c.id}>
      <span>{c.label}: <strong>{counterText(c)}</strong></span>
      {state.can_run && <span className="row">
        {c.kind === "tally" ? <><button type="button" className="small quiet" onClick={() => counter(c.id, { amount: 1 })}>+W</button><button type="button" className="small quiet" onClick={() => counter(c.id, { amount: 1, extra: true })}>+L</button></>
          : <><button type="button" className="small quiet" aria-label={`${c.label} plus one`} onClick={() => counter(c.id, { amount: 1 })}>+1</button><button type="button" className="small quiet" aria-label={`${c.label} minus one`} onClick={() => counter(c.id, { amount: -1 })}>−1</button>{c.kind === "shiny" && <button type="button" className="small quiet" onClick={() => counter(c.id, { amount: 1, extra: true })}>New phase</button>}</>}
      </span>}
    </li>)}</ul>}
    {state.poll && <PollCard poll={state.poll} path={path} state={state} now={now} onChange={load} />}
    {state.prediction && <PollCard poll={state.prediction} path={path} state={state} now={now} onChange={load} />}
    {state.can_run && <StartForm path={path} onChange={load} />}
  </div>;
}
