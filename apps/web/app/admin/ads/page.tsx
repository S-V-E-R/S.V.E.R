"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { UsernameInput } from "../../../components/UsernameInput";

type Segment = { id: string; channel: string; created_at: string; viewers: number; revenue: number | null; split: { streamer_percent: number; per_viewer: number; platform: number } | null; settled_at: string | null };
type Balance = { account: "adv" | "earnings"; user: string; balance: number };
type View = { segments: Segment[]; balances: Balance[]; max_test_cents: number };
const tenths = (t: number) => `$${(t / 1000).toFixed(3)}`;

/** Staff → Ads (docs/ADS.md): test ad breaks and settlements in the separate test book. No real money. */
export default function AdminAds() {
  const [view, setView] = useState<View | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/admin/ads");
    if (r.ok) setView(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function start(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const r = await send<View>("POST", "/api/admin/ads/test-breaks", { username: new FormData(event.currentTarget).get("username") });
    if (r.ok) { setView(r.data); setMessage("Test break started."); } else setMessage(r.error);
  }
  async function settle(s: Segment) {
    const dollars = Number(window.prompt(`Test revenue for this break on @${s.channel}, in dollars:`));
    if (!dollars || dollars <= 0) return;
    const r = await send<View>("POST", `/api/admin/ads/test-breaks/${encodeURIComponent(s.id)}/settle`, { revenue_cents: Math.round(dollars * 100) });
    if (r.ok) { setView(r.data); setMessage("Settled in the test book."); } else setMessage(r.error);
  }
  if (!view) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <><h1>Ads (test book)</h1>
    <p><Link href="/admin/ads/network">Ad network: rate card, review and campaigns</Link></p>
    <p>Ads are designed, not live: no provider is chosen and no ad is shown. Test breaks capture who would be counted on a live channel; settling posts the tier split to the <code>adtest:</code> book, which never reaches payouts. Every action is audited.</p>
    <Section title="Start a test break"><form onSubmit={start} className="row">
      <UsernameInput label="Live channel" required />
      <button type="submit" className="small">Start</button>
    </form></Section>
    <Section title="Segments">{view.segments.length === 0 ? <p className="muted">None yet.</p> : <ul className="list">{view.segments.map(s => <li key={s.id}>
      <p><strong>@{s.channel}</strong> · {new Date(s.created_at).toLocaleString()} · {s.viewers} counted</p>
      {s.split && s.revenue ? <p className="muted small">{tenths(s.revenue)}: streamer {s.split.streamer_percent}%, {tenths(s.split.per_viewer)} per viewer, S.V.E.R {tenths(s.split.platform)}</p>
        : <button type="button" className="small" onClick={() => settle(s)}>Settle with test revenue</button>}
    </li>)}</ul>}</Section>
    <Section title="Test balances">{view.balances.length === 0 ? <p className="muted">None.</p> : <ul className="list">{view.balances.map(b => <li key={b.account + b.user}>@{b.user} · {b.account === "adv" ? "Ad Valor" : "Streamer earnings"} · {tenths(b.balance)}</li>)}</ul>}</Section>
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}
