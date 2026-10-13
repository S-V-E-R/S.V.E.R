"use client";
import Link from "next/link";
import { useState } from "react";
import { send } from "../lib/client-api";
import { monthly, tierName, type SupportersData } from "../lib/supporters";
import { SupporterBadge, type SupporterTier } from "./Supporter";

const PERKS: Record<SupporterTier, string[]> = {
  wolfsworn: ["The Wolf badge in chat and on your card", "A silver player-card frame", "“Supporting since” on your profile"],
  greycloak: ["The Wolf badge, whose eyes catch the light", "Your name in the Wolf’s silver", "A silver line that walks your player card", "“Supporting since” on your profile", "A numbered Founder mark while the first 500 last"],
};

/** The two tier cards on /supporters, with card checkout (docs/SUPPORTERS.md "Paying"). */
export function SupporterTiers({ initial }: { initial: SupportersData }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [consent, setConsent] = useState(false);
  const [needsConsent, setNeedsConsent] = useState(false);
  const plan = initial.mine?.plan ?? null;
  const foundersLeft = initial.founders_total - initial.founders_claimed;

  async function subscribe(tier: SupporterTier) {
    setBusy(true); setError("");
    const r = await send<{ url: string }>("POST", "/api/supporters/subscribe", { tier, guardian_consent: consent });
    if (r.ok) { window.location.assign(r.data.url); return; }
    setBusy(false);
    if (r.field === "guardian_consent") setNeedsConsent(true);
    setError(r.error);
  }

  return <section className="supporters-tiers" aria-label="Tiers">
    <div className="supporters-cards">{initial.tiers.map(t => {
      const current = plan?.tier === t.tier;
      return <article key={t.tier} className="supporter-tier frame" data-tier={t.tier}>
        <h2><SupporterBadge tier={t.tier} /> {tierName[t.tier]}</h2>
        <p className="supporter-price">{monthly(t.cents)}</p>
        <ul>{PERKS[t.tier].map(p => <li key={p}>{p}{p.startsWith("A numbered") && <span className="muted"> ({foundersLeft > 0 ? `${foundersLeft} left` : "all claimed"})</span>}</li>)}</ul>
        {current ? <p className="supporter-current">Your plan. <Link href="/wallet">Change or cancel it in your wallet</Link>.</p>
          : plan ? <p><Link href="/wallet" className="button quiet">Switch in your wallet</Link></p>
          : !initial.signed_in ? <p><Link href={`/login?next=${encodeURIComponent("/supporters")}`} className="button">Sign in to subscribe</Link></p>
          : !initial.available ? <p className="muted">Card payments aren&apos;t set up yet.</p>
          : !initial.sales_open ? <p className="muted">New subscriptions are paused for now. Please check back soon.</p>
          : <button type="button" disabled={busy} onClick={() => subscribe(t.tier)}>Subscribe for {monthly(t.cents)}</button>}
        <p className="supporter-terms muted">{t.terms}</p>
      </article>;
    })}</div>
    {needsConsent && <label className="checkbox"><input type="checkbox" checked={consent} onChange={e => setConsent(e.target.checked)} /> I&apos;m this account holder&apos;s parent or legal guardian, I&apos;m the cardholder, and I consent to this purchase.</label>}
    {error && <p role="alert" className="form-message error">{error}</p>}
    <p className="muted small">Card only, through Stripe. Valor can&apos;t pay for it, because Valor is creators&apos; money. Accounts under 18 need a parent or guardian to confirm, and stay within $50 a month across all purchases.</p>
  </section>;
}
