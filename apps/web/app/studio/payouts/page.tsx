"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Payouts = {
  available: boolean;
  requirements: { email_verified: boolean; mfa_enabled: boolean; age_ok: boolean };
  guardian: boolean;
  account: { guardian: boolean; details_submitted: boolean; payouts_enabled: boolean; requirements: string[] } | null;
  can_earn: boolean;
  earnings_tenths: number;
};
// Earnings accrue in tenths of a cent and round down only at payout.
const dollars = (tenths: number) => `$${(Math.floor(tenths / 10) / 100).toFixed(2)}`;

/** Creator Studio → Payouts (docs/SUPPORT.md "Who can earn" and "Payouts"). */
export default function StudioPayouts() {
  const [data, setData] = useState<Payouts | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const result = await send<Payouts>("GET", "/api/me/payouts");
    if (result.ok) setData(result.data); else setError(result.error);
  }, []);
  useLoad(load);
  async function setup() {
    setBusy(true); setError("");
    const result = await send<{ url: string }>("POST", "/api/me/payouts/setup");
    if (result.ok) window.location.assign(result.data.url);
    else { setError(result.error); setBusy(false); }
  }
  if (!data) return error ? <p role="alert" className="form-message error">{error}</p> : <p className="loading">Loading…</p>;
  const r = data.requirements;
  const ready = r.email_verified && r.mfa_enabled && r.age_ok;
  const a = data.account;
  return <><h1>Payouts</h1>
    <Section title="Earnings" intro="Tributes pay you 0.8¢ per Valor at every creator tier. Paydays and Early Pay arrive in a later update.">
      <p className="valor-balance">{dollars(data.earnings_tenths)}</p>
      <p className="muted">{data.can_earn ? "Your channel can receive tributes." : "Finish payout setup to start receiving tributes."}</p>
    </Section>
    <Section title="Payout setup" intro="Payouts go through Stripe Connect. Stripe collects your identity, bank details and tax form; S.V.E.R never sees them.">
      <ul className="list">
        <li>{r.email_verified ? "✓" : "✗"} Verified email</li>
        <li>{r.mfa_enabled ? "✓" : "✗"} Authenticator two-factor authentication {!r.mfa_enabled && <Link href="/account">Turn it on</Link>}</li>
        <li>{r.age_ok ? "✓" : "✗"} Age 13 or older</li>
        <li>{a?.details_submitted ? "✓" : "✗"} Stripe onboarding, including the tax form</li>
      </ul>
      {data.guardian && <p className="form-message">You&apos;re under 18, so a parent or legal guardian must complete Stripe onboarding, accept Stripe&apos;s Connected Account Agreement and receive the payouts.</p>}
      {a && !a.details_submitted && a.requirements.length > 0 && <p className="muted">Stripe still needs {a.requirements.length} item{a.requirements.length === 1 ? "" : "s"}.</p>}
      {a?.details_submitted && !a.payouts_enabled && <p className="muted">Stripe is reviewing your details. Payouts turn on when it&apos;s done.</p>}
      {!data.available ? <p className="muted">Payouts aren&apos;t available yet.</p>
        : <button type="button" disabled={busy || !ready} onClick={setup}>{a?.details_submitted ? "Open Stripe dashboard" : a ? "Continue setup" : "Set up payouts"}</button>}
      {error && <p role="alert" className="form-message error">{error}</p>}
    </Section>
  </>;
}
