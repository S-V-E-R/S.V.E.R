"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";

type Tier = { tier: number; cents: number; valor: number };
type Mine = { tier: number; paid_through: string; months: number; auto_renew: boolean; card: boolean };
type Status = { available: boolean; can_subscribe: boolean; own: boolean; tiers: Tier[]; gift_counts: number[]; mine: Mine | null };
const dollars = (cents: number) => `$${(cents / 100).toFixed(2)}`;
const date = (iso: string) => new Date(iso).toLocaleDateString();

/**
 * Subscribe and gift subs on a channel page (docs/SUPPORT.md "Subscriptions"). Card payments open
 * Stripe Checkout. On a merged co-stream's page `squad` pools the money among its members.
 */
export function Subscribe({ username, squad }: { username: string; squad?: string }) {
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [status, setStatus] = useState<Status | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [consent, setConsent] = useState(false);
  const [needsConsent, setNeedsConsent] = useState(false);
  const [gift, setGift] = useState({ tier: 1, count: 1, recipient: "" });
  const load = useCallback(async () => {
    const r = await send<Status>("GET", `${path}/subscription`);
    if (r.ok) setStatus(r.data);
  }, [path]);
  useLoad(load);
  if (!status || !status.available || !status.can_subscribe) return null;

  async function act(route: string, body?: unknown, done?: string) {
    setBusy(true); setError(""); setMessage("");
    const r = await send<{ url?: string }>("POST", `${path}${route}`, body);
    if (r.ok && r.data.url) { window.location.assign(r.data.url); return; }
    setBusy(false);
    if (r.ok) { setMessage(done ?? "Done."); await load(); return; }
    if (r.field === "guardian_consent") setNeedsConsent(true);
    setError(r.error);
  }
  const subscribe = (tier: number, pay: "card" | "valor") =>
    act("/subscription", { tier, pay, id: crypto.randomUUID(), guardian_consent: consent, squad }, "Subscribed for a month. Thank you!");
  const sendGift = (pay: "card" | "valor") =>
    act("/gifts", { ...gift, recipient: gift.count === 1 ? gift.recipient : undefined, pay, id: crypto.randomUUID(), guardian_consent: consent, squad }, "Gift sent. Thank you!");
  const mine = status.mine;
  const price = status.tiers[gift.tier - 1];

  return <>
    <button type="button" className={mine ? "small quiet" : "small"} aria-expanded={open} onClick={() => setOpen(!open)}>{mine ? `Subscribed · Tier ${mine.tier}` : "Subscribe"}</button>
    {open && <div className="panel inline-panel subscribe-panel">
      {mine ? <section>
        <h3>Your subscription</h3>
        <p>Tier {mine.tier} · {mine.months} {mine.months === 1 ? "month" : "months"} · {mine.auto_renew ? `renews ${date(mine.paid_through)}` : `ends ${date(mine.paid_through)}`}</p>
        {mine.card && mine.auto_renew && <div className="row wrap">
          {status.tiers.filter(t => t.tier > mine.tier).map(t => <button key={t.tier} type="button" className="small" disabled={busy} onClick={() => act("/subscription/upgrade", { tier: t.tier }, `Upgraded to Tier ${t.tier}.`)}>Upgrade to Tier {t.tier} ({dollars(t.cents)}/month)</button>)}
          <button type="button" className="small quiet" disabled={busy} onClick={() => { if (window.confirm("Stop renewing? Your benefits run to the end of the paid month.")) void act("/subscription/cancel", undefined, "Auto-renewal is off."); }}>Cancel renewal</button>
        </div>}
        {!mine.card && <div className="row wrap">{status.tiers.map(t => <button key={t.tier} type="button" className="small quiet" disabled={busy} onClick={() => subscribe(t.tier, "valor")}>Add a Tier {t.tier} month for {t.valor.toLocaleString()} Valor</button>)}</div>}
      </section> : <section>
        <h3>Subscribe to {username}</h3>
        <p className="muted">A subscriber badge, subscriber emotes and subscriber-only chat. Cancel anytime.</p>
        <ul className="subscribe-tiers">{status.tiers.map(t => <li key={t.tier}>
          <strong>Tier {t.tier}</strong> <span>{dollars(t.cents)}/month</span>
          <button type="button" className="small" disabled={busy} onClick={() => subscribe(t.tier, "card")}>Subscribe by card</button>
          <button type="button" className="small quiet" disabled={busy} onClick={() => subscribe(t.tier, "valor")}>{t.valor.toLocaleString()} Valor for one month</button>
        </li>)}</ul>
      </section>}
      <section>
        <h3>Gift subs</h3>
        <p className="muted">A gifted month never renews. Random gifts go to recent chatters who allow gifts.</p>
        <div className="row wrap">
          <label className="field narrow"><span>Tier</span><select value={gift.tier} onChange={e => setGift({ ...gift, tier: Number(e.target.value) })}>{status.tiers.map(t => <option key={t.tier} value={t.tier}>Tier {t.tier}</option>)}</select></label>
          <label className="field narrow"><span>How many</span><select value={gift.count} onChange={e => setGift({ ...gift, count: Number(e.target.value) })}>{status.gift_counts.map(n => <option key={n} value={n}>{n === 1 ? "1 to a viewer" : `${n} to random chatters`}</option>)}</select></label>
          {gift.count === 1 && <label className="field narrow"><span>Username</span><input value={gift.recipient} maxLength={26} onChange={e => setGift({ ...gift, recipient: e.target.value })} /></label>}
        </div>
        <div className="row wrap">
          <button type="button" className="small" disabled={busy} onClick={() => sendGift("card")}>Gift by card ({dollars(price.cents * gift.count)})</button>
          <button type="button" className="small quiet" disabled={busy} onClick={() => sendGift("valor")}>Gift with {(price.valor * gift.count).toLocaleString()} Valor</button>
        </div>
      </section>
      {needsConsent && <label className="checkbox"><input type="checkbox" checked={consent} onChange={e => setConsent(e.target.checked)} /> I&apos;m this account holder&apos;s parent or legal guardian, I&apos;m the cardholder, and I consent to this purchase.</label>}
      {message && <p role="status" className="form-message">{message}</p>}
      {error && <p role="alert" className="form-message error">{error}{/Valor/.test(error) && <> <Link href="/wallet">Get Valor</Link></>}</p>}
    </div>}
  </>;
}
