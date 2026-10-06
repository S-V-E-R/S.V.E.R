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
type Metrics = { streams: number; hours: number; avg_viewers: number; followers: number; subscribers: number; unique_viewers: number; days_active: number };
type Tier = { tier: number; name: string; split: number; vod_hours: number; metrics: Metrics; next: { name: string; split: number; requirements: Metrics } | null; held: boolean };
type Run = { id: string; kind: "payday" | "early_standard" | "early_instant"; amount_cents: number; fee_cents: number; status: string; error: string | null; created_at: string };
type Summary = { available_cents: number; pending_cents: number; next_payday: string; early: { limit_cents: number; used_today: boolean; percent: number; instant_fee_percent: number }; enabled: boolean; held: boolean; history: Run[] };
const money = (cents: number) => `$${(cents / 100).toFixed(2)}`;
// Earnings accrue in tenths of a cent and round down only at payout.
const tenths = (t: number) => money(Math.floor(t / 10));
const LABELS: [keyof Metrics, string][] = [["streams", "Streams"], ["hours", "Stream hours"], ["avg_viewers", "Average viewers"], ["followers", "Followers"], ["subscribers", "Subscribers"], ["unique_viewers", "Unique viewers"], ["days_active", "Days active"]];
const KIND = { payday: "Payday", early_standard: "Early Pay (standard)", early_instant: "Early Pay (instant)" };

/** Creator Studio → Payouts (docs/SUPPORT.md "Creator tiers", "Who can earn" and "Payouts"). */
export default function StudioPayouts() {
  const [data, setData] = useState<Payouts | null>(null);
  const [tier, setTier] = useState<Tier | null>(null);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const [p, t, s] = await Promise.all([send<Payouts>("GET", "/api/me/payouts"), send<Tier>("GET", "/api/me/tier"), send<Summary>("GET", "/api/me/payouts/summary")]);
    if (p.ok) setData(p.data); else setError(p.error);
    if (t.ok) setTier(t.data);
    if (s.ok) setSummary(s.data);
  }, []);
  useLoad(load);
  async function setup() {
    setBusy(true); setError("");
    const result = await send<{ url: string }>("POST", "/api/me/payouts/setup");
    if (result.ok) window.location.assign(result.data.url);
    else { setError(result.error); setBusy(false); }
  }
  async function early(method: "standard" | "instant") {
    if (!summary) return;
    const fee = method === "instant" ? Math.ceil(summary.early.limit_cents * summary.early.instant_fee_percent / 100) : 0;
    if (!window.confirm(method === "instant" ? `Withdraw ${money(summary.early.limit_cents)} instantly? Stripe's ${summary.early.instant_fee_percent}% fee (${money(fee)}) comes out of it, so ${money(summary.early.limit_cents - fee)} arrives in minutes.` : `Withdraw ${money(summary.early.limit_cents)}? It arrives in about 2 business days.`)) return;
    setBusy(true); setError("");
    const result = await send<Summary>("POST", "/api/me/payouts/early", { method });
    setBusy(false);
    if (result.ok) setSummary(result.data); else setError(result.error);
  }
  if (!data) return error ? <p role="alert" className="form-message error">{error}</p> : <p className="loading">Loading…</p>;
  const r = data.requirements;
  const ready = r.email_verified && r.mfa_enabled && r.age_ok;
  const a = data.account;
  const next = tier?.next;
  return <><h1>Payouts</h1>
    {tier && <Section title={`Creator tier · ${tier.name}`} intro={`You keep ${tier.split}% of subscriptions and gift subs, and 0.8¢ per Valor at every tier. Tiers are checked every Monday against the last 90 days and never go down.`}>
      {tier.held && <p className="form-message">Promotion is paused while a staff review of your streams is open.</p>}
      {next ? <table className="table"><thead><tr><th>Last 90 days</th><th>You</th><th>{next.name} ({next.split}%)</th></tr></thead>
        <tbody>{LABELS.map(([key, label]) => <tr key={key}><td>{label}</td><td>{tier.metrics[key].toLocaleString()}</td><td>{tier.metrics[key] >= next.requirements[key] ? "✓ " : ""}{next.requirements[key].toLocaleString()}</td></tr>)}</tbody></table>
        : <p>You&apos;re at the top tier.</p>}
    </Section>}
    <Section title="Earnings" intro="Payday every two weeks pays your whole balance. Between paydays, Early Pay withdraws up to 75% of what you've earned since the last payday, once a day. Taxes aren't withheld; Stripe handles your tax forms.">
      <p className="valor-balance">{tenths(data.earnings_tenths)}</p>
      {summary && <>
        <p className="muted">Next payday {new Date(summary.next_payday).toLocaleDateString()}{summary.pending_cents > 0 && ` · ${money(summary.pending_cents)} on its way`}</p>
        {summary.held && <p className="form-message">Payouts are on hold while a staff review is open.</p>}
        {summary.enabled && !summary.held && <div className="row wrap">
          <span>Early Pay available: <strong>{money(summary.early.limit_cents)}</strong>{summary.early.used_today && " · used today"}</span>
          <button type="button" className="small" disabled={busy || summary.early.used_today || summary.early.limit_cents < 1} onClick={() => early("standard")}>Standard (free, about 2 business days)</button>
          <button type="button" className="small quiet" disabled={busy || summary.early.used_today || summary.early.limit_cents < 2} onClick={() => early("instant")}>Instant ({summary.early.instant_fee_percent}% fee)</button>
        </div>}
        {summary.history.length > 0 && <table className="table"><thead><tr><th>Date</th><th>Type</th><th>Amount</th><th>Status</th></tr></thead>
          <tbody>{summary.history.map(run => <tr key={run.id}><td>{new Date(run.created_at).toLocaleDateString()}</td><td>{KIND[run.kind]}</td><td>{money(run.amount_cents)}{run.fee_cents > 0 && <span className="muted"> (fee {money(run.fee_cents)})</span>}</td><td>{run.status}{run.error && <span className="muted"> · {run.error}</span>}</td></tr>)}</tbody></table>}
      </>}
      <p className="muted">{data.can_earn ? "Your channel can receive tributes and subscriptions." : "Finish payout setup to start earning."}</p>
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
