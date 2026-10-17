"use client";
import { useCallback, useEffect, useState } from "react";
import { send, useLoad } from "../../lib/client-api";

type Order = { slot: number; label: string; rarity: string; target: number; progress: number; done: boolean; xp: number; ev?: number; ev_channel?: string | null };
type Orders = { orders: Order[]; streak?: number; week?: number; can_reroll?: boolean; verify?: boolean };

/** Sidebar daily orders (docs/PROGRESSION.md section 3): three a day, reset at midnight UTC. */
export function DailyOrders() {
  const [data, setData] = useState<Orders | null>(null);
  const load = useCallback(async () => {
    const r = await send<Orders>("GET", "/api/me/orders");
    if (r.ok) setData(r.data);
  }, []);
  useLoad(load);
  // Progress moves while you watch: refresh each minute while visible, and on coming back.
  useEffect(() => {
    const refresh = () => { if (!document.hidden) void load(); };
    const timer = setInterval(refresh, 60_000);
    document.addEventListener("visibilitychange", refresh);
    window.addEventListener("focus", refresh);
    return () => { clearInterval(timer); document.removeEventListener("visibilitychange", refresh); window.removeEventListener("focus", refresh); };
  }, [load]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function reroll(slot: number) {
    setBusy(true);
    const r = await send<Orders>("POST", `/api/me/orders/${slot}/reroll`);
    if (r.ok) { setData(r.data); setError(""); } else setError(r.error);
    setBusy(false);
  }
  if (!data || (!data.orders.length && !data.verify)) return null;
  return <section className="side-section daily-orders frame" aria-labelledby="side-orders">
    <div className="side-label"><span id="side-orders">Daily orders</span>{!!data.streak && <span title="Days in a row with an order done">{data.streak}-day streak</span>}</div>
    {data.verify ? <p className="side-empty">Verify your email to get daily orders.</p>
      : <ul className="order-list">{data.orders.map(o => <li key={o.slot} className={o.done ? "done" : undefined} data-rarity={o.rarity.toLowerCase()}>
        <span className="order-label">{o.label}</span>
        <span className="order-meta"><span title={o.done && o.ev_channel ? `${o.ev} Engagement Valor in ${o.ev_channel}` : "Engagement Valor goes to the channel you're in when it's done"}>{o.rarity} · {o.xp} XP{o.done && !o.ev ? "" : ` · ${o.ev ?? 0} Engagement Valor`}</span><span>{o.done ? "Done" : `${o.progress}/${o.target}`}</span></span>
        <span className="xp-bar" aria-hidden="true"><span style={{ width: `${Math.round(100 * o.progress / o.target)}%` }} /></span>
        {data.can_reroll && !o.done && <button type="button" className="link-button order-reroll" disabled={busy} onClick={() => void reroll(o.slot)}>Reroll<span className="sr-only"> {o.label}</span></button>}
      </li>)}</ul>}
    {error && <p role="alert" className="side-empty">{error}</p>}
    {!data.verify && <p className="side-empty">{data.week ?? 0} done this week · milestones at 7, 14, 21</p>}
  </section>;
}
