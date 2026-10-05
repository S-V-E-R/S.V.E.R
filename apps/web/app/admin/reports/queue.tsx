"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { reasons } from "../../../lib/types";

type Report = { id: string; reason: string; note: string; field: string | null; snapshot: { field: string | null; value: unknown }; created_at: string };
type Group = { target_type: string; target_id: string; username: string; count: number; reasons: string[]; reports: Report[]; oldest: string; current: unknown; history: { action: string; note: string; created_at: string; actor: string | null }[] };
type Interim = { id: string; username: string; starts_at: string; until: string; overdue: boolean; note: string };
type Queue = { groups: Group[]; next_cursor: string | null; interim_restrictions: Interim[]; pending_appeals: number };
const when = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
const STEP_UP = "Confirm your sign-in method again on the Account page, then retry.";

export function StrikeFields() {
  return <fieldset className="editor"><legend>Strike</legend>
    <label className="field narrow"><span>Reason</span><select name="strike_reason" defaultValue="">{[<option key="" value="">No strike</option>, ...reasons.map(([v, l]) => <option key={v} value={v}>{l}</option>)]}</select></label>
    <label className="field narrow"><span>Severity</span><select name="strike_severity" defaultValue="STANDARD"><option value="STANDARD">Standard (counts 90 days)</option><option value="SEVERE">Severe (level 3, counts 365 days)</option></select></label>
    <label className="field"><span>Message to the user (optional)</span><textarea name="strike_message" maxLength={500} rows={2} /></label>
  </fieldset>;
}
export function strikeFrom(form: FormData) {
  const reason = String(form.get("strike_reason") || "");
  return reason ? { reason, severity: form.get("strike_severity"), message_to_user: form.get("strike_message") || "" } : undefined;
}

function Action({ group, onDone }: { group: Group; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const action = String(f.get("action"));
    const body = { action, note: f.get("note"), field: f.get("field") || undefined, interim_hours: action === "restrict" ? Number(f.get("interim_hours")) : undefined, strike: action === "dismiss" ? undefined : strikeFrom(f) };
    const result = await send<{ closed_reports: number }>("POST", `/api/admin/reports/${group.target_type}/${encodeURIComponent(group.target_id)}/actions`, body);
    setState(result.ok ? { saved: `Closed ${result.data.closed_reports} report(s).` } : result.status === 403 ? { error: STEP_UP } : result);
    if (result.ok) onDone();
  }
  return <form onSubmit={submit} className="admin-action">
    <div className="row wrap"><label className="field narrow"><span>Action</span><select name="action" defaultValue="dismiss"><option value="dismiss">Dismiss</option><option value="remove_content">Remove content</option>{group.target_type === "profile" && <option value="reset_field">Reset field</option>}<option value="restrict">Interim restriction / strike only</option></select></label>
      {group.target_type === "profile" && <label className="field narrow"><span>Field to reset</span><input name="field" defaultValue={group.reports[0]?.field || ""} /></label>}
      <label className="field narrow"><span>Interim hours (1-24)</span><input name="interim_hours" type="number" min={1} max={24} defaultValue={24} /></label></div>
    <StrikeFields />
    <label className="field"><span>Moderator note (required)</span><textarea name="note" required maxLength={500} rows={2} /></label>
    <button type="submit" className="small">Apply</button>
    <Status state={state} />
  </form>;
}

export default function QueuePage() {
  const [queue, setQueue] = useState<Queue | null>(null);
  const [filter, setFilter] = useState({ type: "", reason: "" });
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const q = new URLSearchParams(Object.entries(filter).filter(([, v]) => v));
    const r = await send<Queue>("GET", `/api/admin/reports?${q}`);
    if (r.ok) setQueue(r.data); else setError(r.error);
  }, [filter]);
  useLoad(load);
  async function lift(name: string) {
    const note = window.prompt("Note for lifting this interim restriction");
    if (!note) return;
    const r = await send("DELETE", `/api/admin/users/${encodeURIComponent(name)}/interim-restriction`, { note });
    if (!r.ok) setError(r.status === 403 ? STEP_UP : r.error);
    load();
  }
  if (!queue) return <p className="loading">{error || "Loading…"}</p>;
  return <><h1>Reports</h1>
    {error && <p role="alert" className="form-message error">{error}</p>}
    <p className="muted"><Link href="/admin/appeals">{queue.pending_appeals} strike appeal(s) waiting</Link> · <Link href="/admin/bans">Account bans and ban appeals</Link></p>
    {queue.interim_restrictions.length > 0 && <Section title="Interim restrictions">
      <ul className="list">{queue.interim_restrictions.map(i => <li key={i.id} className="row between"><span><Link href={`/admin/users/${i.username}`}>@{i.username}</Link> until {when(i.until)} {i.overdue && <span className="badge danger-text">Overdue: record a strike or lift</span>}</span><button type="button" className="small quiet" onClick={() => lift(i.username)}>Lift</button></li>)}</ul>
    </Section>}
    <div className="row"><select aria-label="Type" value={filter.type} onChange={e => setFilter({ ...filter, type: e.target.value })}><option value="">All types</option><option value="profile">Channels</option><option value="wall_post">Wall posts</option><option value="wall_reply">Wall replies</option><option value="fan_art">Fan art</option><option value="setup_photo">Setup photos</option><option value="chat_message">Chat messages</option><option value="live_stream">Live streams</option><option value="emote">Emotes</option><option value="faction_post">Faction posts</option><option value="guild">Guilds</option><option value="guild_emblem">Guild emblems</option></select>
      <select aria-label="Reason" value={filter.reason} onChange={e => setFilter({ ...filter, reason: e.target.value })}>{[<option key="" value="">All reasons</option>, ...reasons.map(([v, l]) => <option key={v} value={v}>{l}</option>)]}</select></div>
    {queue.groups.length === 0 ? <p className="panel section muted">The queue is empty.</p> : queue.groups.map(g => <Section key={`${g.target_type}:${g.target_id}`} title={`${g.target_type.replace("_", " ")} · @${g.username}`}>
      <p className="muted">{g.count} report(s) since {when(g.oldest)} · {g.reasons.join(", ")} · <Link href={`/admin/users/${g.username}`}>Standing</Link></p>
      <details open><summary>Reports and snapshots</summary><ul className="list">{g.reports.map(r => <li key={r.id}><strong>{r.reason}</strong>{r.field && ` · ${r.field}`} · {when(r.created_at)}{r.note && <p>{r.note}</p>}<pre className="snapshot">{JSON.stringify(r.snapshot.value, null, 2)}</pre></li>)}</ul></details>
      <details><summary>Current content</summary><pre className="snapshot">{JSON.stringify(g.current, null, 2)}</pre></details>
      {g.history.length > 0 && <details><summary>History</summary><ul className="list">{g.history.map((h, i) => <li key={i}>{h.action} · {h.actor ? `@${h.actor}` : "system"} · {when(h.created_at)}{h.note && ` — ${h.note}`}</li>)}</ul></details>}
      <Action group={g} onDone={load} />
    </Section>)}
  </>;
}
