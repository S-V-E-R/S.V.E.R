"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad, type Result } from "../../../lib/client-api";

type Lane = { id: string; name: string; enabled: boolean; featuring: string | null; reason: string | null; since: string | null; forced: string | null; next: string | null; stalled: boolean; failing_since: string | null };
type Decision = { lane: string; at: string; kind: string; chosen: string | null; reason: string; candidates: { username: string; score: number; moment: string | null; signals: Record<string, unknown> }[] };
type Admin = { lanes: Lane[]; decisions: Decision[] };

/** Staff → MAGNet: enable lanes, force and release, emergency stop, the 7-day decision log. */
export default function AdminMagnet() {
  const [data, setData] = useState<Admin | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const result = await send<Admin>("GET", "/api/admin/magnet");
    if (result.ok) setData(result.data); else setMessage(result.error);
  }, []);
  useLoad(load);
  const apply = (result: Result<Admin>, done: string) => { if (result.ok) { setData(result.data); setMessage(done); } else setMessage(result.error); };
  async function force(event: FormEvent<HTMLFormElement>, lane: string) {
    event.preventDefault();
    const username = String(new FormData(event.currentTarget).get("username") ?? "").trim();
    apply(await send<Admin>("PUT", `/api/admin/magnet/${lane}/force`, { username: username || null }), username ? `@${username} forced onto the lane.` : "Released.");
  }
  async function stop() {
    if (!window.confirm("Stop every MAGNet lane now? Each can be enabled again afterwards.")) return;
    apply(await send<Admin>("POST", "/api/admin/magnet/stop"), "Every lane is stopped.");
  }
  if (!data) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <><h1>MAGNet</h1>
    {message && <p role="status" className="form-message">{message}</p>}
    <Section title="Lanes" intro="Every action is audited. Forcing holds a live stream on a lane until released.">
      <button type="button" className="small danger-text" onClick={stop}>Emergency stop</button>
      <ul className="list">{data.lanes.map(l => <li key={l.id} className="panel inline-panel">
        <p><strong>{l.name}</strong> · {l.enabled ? (l.featuring ? <>@{l.featuring} <span className="muted">· {l.reason}</span></> : <span className="muted">nothing live</span>) : <span className="muted">stopped</span>}{l.next && <span className="muted"> · next @{l.next}</span>}{l.forced && <strong> · forced @{l.forced}</strong>}{l.stalled && <strong className="danger-text"> · stalled{l.failing_since && <> since {new Date(l.failing_since).toLocaleTimeString()}</>}, holding its stream</strong>}</p>
        <div className="row">
          <button type="button" className="small quiet" onClick={async () => apply(await send<Admin>("PUT", `/api/admin/magnet/${l.id}`, { enabled: !l.enabled }), l.enabled ? "Lane stopped." : "Lane enabled.")}>{l.enabled ? "Stop lane" : "Enable lane"}</button>
          <form className="row" onSubmit={e => force(e, l.id)}><label className="sr-only" htmlFor={`force-${l.id}`}>Force a channel</label><input id={`force-${l.id}`} name="username" placeholder={l.forced ? "Leave empty to release" : "username"} maxLength={26} /><button type="submit" className="small">{l.forced ? "Force / release" : "Force"}</button></form>
        </div>
      </li>)}</ul>
    </Section>
    <Section title="Decision log" intro="The last 100 decisions with their candidates and signals. Kept 7 days.">
      {data.decisions.length === 0 ? <p className="muted">No decisions yet.</p> : <ul className="list">{data.decisions.map((d, i) => <li key={i}><details><summary>{new Date(d.at).toLocaleString()} · {d.lane} · {d.kind}{d.chosen && <> → @{d.chosen}</>} · {d.reason}</summary>
        <ul>{d.candidates.map(c => <li key={c.username}>@{c.username} · score {c.score.toFixed(2)}{c.moment && <> · {c.moment}</>} <code>{JSON.stringify(c.signals)}</code></li>)}</ul></details></li>)}</ul>}
    </Section>
  </>;
}
