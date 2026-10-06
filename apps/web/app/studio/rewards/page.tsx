"use client";
import { FormEvent, useCallback, useEffect, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import type { Reward } from "../../../components/Rewards";
import { send, useLoad } from "../../../lib/client-api";
import type { Chip } from "../../../lib/types";

type Mine = { rewards: Reward[]; max_rewards: number; username: string };
type Redemption = { id: string; name: string; cost: number; input: string | null; status: "pending" | "done" | "refunded"; created_at: string; user: Chip };
type Draft = { name: string; cost: string; cooldown: string; limit: string; prompt: string; enabled: boolean };
const blank: Draft = { name: "", cost: "100", cooldown: "0", limit: "", prompt: "", enabled: true };
const toDraft = (r: Reward): Draft => ({ name: r.name, cost: String(r.cost), cooldown: String(Math.round(r.cooldown_seconds / 60)), limit: r.per_stream_limit ? String(r.per_stream_limit) : "", prompt: r.prompt ?? "", enabled: r.enabled });
const body = (d: Draft) => ({ name: d.name, cost: Number(d.cost), cooldown_seconds: Math.round(Number(d.cooldown || 0) * 60), per_stream_limit: d.limit ? Number(d.limit) : null, prompt: d.prompt || null, enabled: d.enabled });

function RewardForm({ draft, highlight, busy, onChange, onSubmit, submit }: { draft: Draft; highlight: boolean; busy: boolean; onChange: (d: Draft) => void; onSubmit: (e: FormEvent) => void; submit: string }) {
  return <form onSubmit={onSubmit} className="reward-form">
    {!highlight && <label className="field"><span>Name</span><input value={draft.name} maxLength={45} required onChange={e => onChange({ ...draft, name: e.target.value })} /></label>}
    <label className="field narrow"><span>Cost</span><input type="number" min={1} max={1000000} required value={draft.cost} onChange={e => onChange({ ...draft, cost: e.target.value })} /></label>
    <label className="field narrow"><span>Cooldown (minutes)</span><input type="number" min={0} max={10080} value={draft.cooldown} onChange={e => onChange({ ...draft, cooldown: e.target.value })} /></label>
    <label className="field narrow"><span>Limit per stream</span><input type="number" min={1} max={1000} placeholder="No limit" value={draft.limit} onChange={e => onChange({ ...draft, limit: e.target.value })} /></label>
    {!highlight && <label className="field"><span>Viewer must enter (optional)</span><input value={draft.prompt} maxLength={100} placeholder="For example: Which song?" onChange={e => onChange({ ...draft, prompt: e.target.value })} /></label>}
    <label className="checkbox"><input type="checkbox" checked={draft.enabled} onChange={e => onChange({ ...draft, enabled: e.target.checked })} /> Available to viewers</label>
    <button disabled={busy}>{submit}</button>
  </form>;
}

/** Creator Studio → Rewards: Engagement Valor rewards and the redemption queue (docs/SUPPORT.md). */
export default function StudioRewards() {
  const [mine, setMine] = useState<Mine | null>(null);
  const [queue, setQueue] = useState<Redemption[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(blank);
  const [created, setCreated] = useState<Draft>(blank);
  const [busy, setBusy] = useState(false);
  const [state, setState] = useState<SaveState>({});
  const loadQueue = useCallback(async (username: string) => {
    const r = await send<{ items: Redemption[] }>("GET", `/api/channels/${encodeURIComponent(username)}/redemptions`);
    if (r.ok) setQueue(r.data.items);
  }, []);
  const load = useCallback(async () => {
    const r = await send<Mine>("GET", "/api/me/rewards");
    if (r.ok) { setMine(r.data); await loadQueue(r.data.username); } else setState(r);
  }, [loadQueue]);
  useLoad(load);
  const username = mine?.username;
  useEffect(() => {
    if (!username) return;
    const timer = setInterval(() => { if (!document.hidden) void loadQueue(username); }, 15000);
    return () => clearInterval(timer);
  }, [username, loadQueue]);

  async function save(method: string, path: string, payload?: unknown, saved = "Saved.") {
    setBusy(true);
    const r = await send<{ rewards: Reward[] }>(method, path, payload);
    setBusy(false); setState(r.ok ? { saved } : r);
    if (r.ok) { setMine(m => m && { ...m, rewards: r.data.rewards }); setEditing(null); }
    return r.ok;
  }
  async function resolve(item: Redemption, action: "done" | "refund") {
    if (!username) return;
    const r = await send("POST", `/api/channels/${encodeURIComponent(username)}/redemptions/${encodeURIComponent(item.id)}`, { action });
    setState(r.ok ? { saved: action === "done" ? "Marked done." : "Refunded." } : r);
    await loadQueue(username);
  }
  if (!mine) return state.error ? <Status state={state} /> : <p className="loading">Loading…</p>;
  const pending = queue.filter(q => q.status === "pending");
  const custom = mine.rewards.filter(r => r.kind === "custom").length;

  return <><h1>Rewards</h1>
    <p>Viewers earn Engagement Valor in your channel by watching, chatting and following, and spend it on your rewards. It has no cash value.</p>
    <Section title={`Queue · ${pending.length} pending`} intro="Mark each redemption done, or refund it to give the points back. Highlighted messages post immediately.">
      {queue.length === 0 ? <p className="muted">No redemptions in the last week.</p> : <ul className="list">{queue.map(item => <li key={item.id} className="row between wrap">
        <span><strong>{item.user.display_name}</strong> · {item.name} · {item.cost.toLocaleString()}{item.input && <> · “{item.input}”</>} <span className="muted">· {new Date(item.created_at).toLocaleString()}</span></span>
        {item.status === "pending" ? <span className="row"><button type="button" className="small" onClick={() => resolve(item, "done")}>Done</button><button type="button" className="small quiet" onClick={() => resolve(item, "refund")}>Refund</button></span> : <span className="muted">{item.status === "done" ? "Done" : "Refunded"}</span>}
      </li>)}</ul>}
    </Section>
    <Section title={`Your rewards · ${custom}/${mine.max_rewards - 1}`}>
      <ul className="list">{mine.rewards.map(reward => <li key={reward.id}>
        {editing === reward.id
          ? <RewardForm draft={draft} highlight={reward.kind === "highlight"} busy={busy} onChange={setDraft} submit="Save reward" onSubmit={e => { e.preventDefault(); void save("PUT", `/api/me/rewards/${encodeURIComponent(reward.id)}`, body(draft)); }} />
          : <div className="row between wrap">
            <span><strong>{reward.name}</strong> · {reward.cost.toLocaleString()}{reward.cooldown_seconds > 0 && ` · ${Math.round(reward.cooldown_seconds / 60)} min cooldown`}{reward.per_stream_limit && ` · ${reward.per_stream_limit} per stream`}{reward.prompt && ` · asks “${reward.prompt}”`}{!reward.enabled && <span className="muted"> · off</span>}{reward.kind === "highlight" && <span className="muted"> · built in</span>}</span>
            <span className="row"><button type="button" className="small quiet" onClick={() => { setEditing(reward.id); setDraft(toDraft(reward)); }}>Edit</button>
              {reward.kind === "custom" && <button type="button" className="small quiet danger-text" disabled={busy} onClick={() => { if (window.confirm(`Delete ${reward.name}?`)) void save("DELETE", `/api/me/rewards/${encodeURIComponent(reward.id)}`, undefined, "Reward deleted."); }}>Delete</button>}</span>
          </div>}
      </li>)}</ul>
    </Section>
    <Section title="New reward">
      <RewardForm draft={created} highlight={false} busy={busy} onChange={setCreated} submit="Add reward" onSubmit={async e => { e.preventDefault(); if (await save("POST", "/api/me/rewards", body(created), "Reward added.")) setCreated(blank); }} />
    </Section>
    <Status state={state} />
  </>;
}
