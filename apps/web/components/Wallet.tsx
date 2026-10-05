"use client";
import { useCallback, useState } from "react";
import { Section } from "./Form";
import { send, useLoad } from "../lib/client-api";

type Data = { valor: number; locked: boolean; available: boolean; packs: { cents: number; valor: number }[]; minor: boolean; guardian_confirmed: boolean; month_cents: number | null; cap_cents: number | null; allow_gifts: boolean };
const dollars = (cents: number) => `$${(cents / 100).toFixed(2)}`;

/** Purchased Valor (docs/SUPPORT.md): balance and packs. Checkout is Stripe-hosted; Valor arrives by webhook. */
export function Wallet({ returned }: { returned: boolean }) {
  const [data, setData] = useState<Data | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [consent, setConsent] = useState(false);
  const load = useCallback(async () => {
    const result = await send<Data>("GET", "/api/me/wallet");
    if (result.ok) setData(result.data); else setError(result.error);
  }, []);
  useLoad(load);
  async function allowGifts(allow: boolean) {
    const result = await send<{ allow_gifts: boolean }>("PUT", "/api/me/gifts", { allow });
    if (result.ok) setData(d => d && { ...d, allow_gifts: result.data.allow_gifts }); else setError(result.error);
  }
  async function buy(cents: number) {
    setBusy(true); setError("");
    const result = await send<{ url: string }>("POST", "/api/me/wallet/checkout", { cents, guardian_consent: consent });
    if (result.ok) window.location.assign(result.data.url);
    else { setError(result.error); setBusy(false); }
  }
  if (!data) return error ? <p role="alert" className="form-message error">{error}</p> : <p className="loading">Loading…</p>;
  const needsConsent = data.minor && !data.guardian_confirmed;
  return <>
    {returned && <p role="status" className="form-message">Payment received. Your Valor appears here within a minute.</p>}
    <Section title="Your Valor" intro="Pay tribute in a streamer's chat: at least 10 Valor with a highlighted message. The streamer earns 0.8¢ for every Valor. Valor never expires, can't be transferred and can't be cashed out.">
      <p className="valor-balance">{data.valor.toLocaleString()} Valor</p>
      {data.locked && <p role="alert" className="form-message error">A payment was refunded or disputed after its Valor was spent, so spending is locked until the balance is back above zero.</p>}
    </Section>
    <Section title="Get Valor" intro="Paid by card through Stripe; S.V.E.R never sees your card. Bigger packs include bonus Valor.">
      {!data.available ? <p className="muted">Buying Valor isn&apos;t available yet.</p> : <>
        {data.minor && <p className="muted">Accounts under 18 can spend up to {dollars(data.cap_cents ?? 0)} a month ({dollars(data.month_cents ?? 0)} so far this month).</p>}
        {needsConsent && <label className="checkbox"><input type="checkbox" checked={consent} onChange={e => setConsent(e.target.checked)} /> I&apos;m this account holder&apos;s parent or legal guardian, I&apos;m the cardholder, and I consent to this purchase.</label>}
        <ul className="valor-packs">{data.packs.map(p => <li key={p.cents}>
          <button type="button" disabled={busy || (needsConsent && !consent)} onClick={() => buy(p.cents)}><strong>{p.valor.toLocaleString()} Valor</strong><span>{dollars(p.cents)}</span></button>
        </li>)}</ul>
        <p className="muted small">Purchases are non-refundable except where the law or the refund policy says otherwise. A parent can report an unauthorized purchase through support.</p>
      </>}
      {error && <p role="alert" className="form-message error">{error}</p>}
    </Section>
    <Section title="Gift subs" intro="Viewers can gift subscriptions to people in a channel's chat.">
      <label className="checkbox"><input type="checkbox" checked={data.allow_gifts} onChange={e => allowGifts(e.target.checked)} /> Let people gift me subscriptions</label>
    </Section>
  </>;
}
