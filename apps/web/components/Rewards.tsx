"use client";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";

export type Reward = { id: string; kind: "custom" | "highlight"; name: string; cost: number; cooldown_seconds: number; per_stream_limit: number | null; prompt: string | null; enabled: boolean; ready_at: string | null };
type Data = { rewards: Reward[]; balance: number | null };

/**
 * The channel's Engagement Valor and rewards under its chat (docs/SUPPORT.md "Engagement Valor").
 * "Highlight my message" arms the chat box; other rewards go to the owner's queue. `version`
 * changes after a highlight is paid, so the balance reloads.
 */
export function Rewards({ username, account, version, onHighlight }: { username: string; account: string | null; version: number; onHighlight: (cost: number) => void }) {
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [data, setData] = useState<Data | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const load = useCallback(async () => {
    const r = await send<Data>("GET", `${path}/rewards?v=${version}`);
    if (r.ok) setData(r.data);
  }, [path, version]);
  useLoad(load);
  if (!data || data.rewards.length === 0) return null;
  const own = !!account && account.toLowerCase() === username.toLowerCase();

  async function redeem(reward: Reward) {
    setBusy(true); setNote("");
    const r = await send<{ balance: number }>("POST", `${path}/rewards/${encodeURIComponent(reward.id)}/redeem`, { id: crypto.randomUUID(), input: reward.prompt ? input : undefined });
    setBusy(false);
    if (r.ok) { setNote(`Redeemed ${reward.name}. The streamer will see it in their queue.`); setActive(null); setInput(""); }
    else setNote(r.error);
    await load();
  }
  return <details className="chat-rewards">
    <summary>Channel rewards{data.balance !== null && <> · <strong>{data.balance.toLocaleString()}</strong> Engagement Valor</>}</summary>
    <p className="muted small">Earned in this channel by watching, chatting and following. It has no cash value.</p>
    <ul className="list">{data.rewards.map(reward => {
      const affordable = data.balance !== null && data.balance >= reward.cost;
      return <li key={reward.id}>
        <span><strong>{reward.name}</strong> · {reward.cost.toLocaleString()}</span>
        {account && !own && (reward.kind === "highlight"
          ? <button type="button" className="small quiet" disabled={!affordable} onClick={() => onHighlight(reward.cost)}>Highlight my next message</button>
          : active === reward.id
            ? <span className="row wrap">{reward.prompt && <input aria-label={reward.prompt} placeholder={reward.prompt} value={input} maxLength={200} onChange={e => setInput(e.target.value)} />}<button type="button" className="small" disabled={busy} onClick={() => redeem(reward)}>Redeem</button><button type="button" className="small quiet" onClick={() => setActive(null)}>Cancel</button></span>
            : <button type="button" className="small quiet" disabled={busy || !affordable} onClick={() => { setActive(reward.id); setInput(""); }}>Redeem</button>)}
      </li>;
    })}</ul>
    {note && <p role="status" className="form-message">{note}</p>}
  </details>;
}
