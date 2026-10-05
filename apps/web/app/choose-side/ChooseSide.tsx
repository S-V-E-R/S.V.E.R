"use client";
import Image from "next/image";
import Link from "next/link";
import { useState } from "react";
import { SignupSteps } from "../screens";
import { FACTIONS, crestSrc, factionOf, type FactionSlug } from "../../lib/factions";

type Reply = { faction?: FactionSlug; error?: string };

export function ChooseSide({ current }: { current: FactionSlug | null }) {
  const [pick, setPick] = useState<FactionSlug | null>(current);
  const [joined, setJoined] = useState<FactionSlug | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [resent, setResent] = useState(false);
  const chosen = factionOf(pick);

  async function enlist() {
    if (!pick) return;
    setBusy(true); setError("");
    try {
      const response = await fetch("/api/me/faction", { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ faction: pick }), credentials: "same-origin" });
      const result: Reply = await response.json().catch(() => ({ error: "The service could not be reached. Please try again." }));
      if (!response.ok) throw new Error(result.error || "Your side could not be saved. Please try again.");
      // The whole site takes on the faction's colors right away.
      document.documentElement.dataset.theme = pick;
      setJoined(pick);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Please try again.");
    } finally { setBusy(false); }
  }

  async function resend() {
    const response = await fetch("/api/auth/email/resend", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}", credentials: "same-origin" }).catch(() => null);
    setResent(!!response?.ok);
  }

  if (joined) {
    const f = factionOf(joined)!;
    return <div className="entry-page">
      <SignupSteps current={3} />
      <section className="auth-panel frame welcome" aria-labelledby="welcome-title">
        <Image src={crestSrc(f.slug)} width={96} height={96} alt={`${f.name} crest`} unoptimized />
        <h1 id="welcome-title" className="auth-title">Welcome to {f.name}</h1>
        <p className="intro">{f.welcome}</p>
        <div className="welcome-email">
          <span className="eyebrow">Confirm your email</span>
          <p>We sent you a confirmation link. It works for 24 hours.</p>
          <p className="small-text">You can browse and watch now. Chat and going live unlock once you confirm.</p>
        </div>
        <Link href="/" className="button primary">Start watching</Link>
        <button type="button" className="quiet primary" onClick={resend} disabled={resent}>{resent ? "Email sent again" : "Resend the email"}</button>
      </section>
    </div>;
  }

  return <div className="choose-side">
    <SignupSteps current={2} />
    <header>
      <h1>Choose your side</h1>
      <p>Your faction is how you show up, not what you stream. Every stream, watch and chat will take ground for your side in the war over categories.</p>
    </header>
    {error && <p className="notice error" role="alert">{error}</p>}
    <div className="faction-picks" role="radiogroup" aria-label="Faction">
      {FACTIONS.map(f => <button key={f.slug} type="button" role="radio" aria-checked={pick === f.slug} className={`faction-pick ${f.slug}`} onClick={() => setPick(f.slug)}>
        <Image src={crestSrc(f.slug)} width={132} height={132} alt="" unoptimized />
        <span className="faction-pick-name">{f.name}</span>
        <span className="eyebrow">{f.title}</span>
        <span className="faction-pick-creed">{f.creed}</span>
        <span className="small-text">Home turf: {f.turf}</span>
      </button>)}
    </div>
    <p className="small-text center">One free switch in your first 7 days, then only between seasons.</p>
    <div className="choose-actions">
      <button type="button" onClick={enlist} disabled={!pick || busy} data-faction={pick ?? undefined}>{busy ? "Please wait…" : chosen ? `Enlist in ${chosen.name}` : "Pick a faction"}</button>
    </div>
  </div>;
}
