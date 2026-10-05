"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { Crest, EnrollmentSteps } from "../../components/FactionIdentity";
import { factions, factionInfo, isFaction, type Faction } from "../../lib/factions";
import { send } from "../../lib/client-api";

export type Membership = { faction: Faction | null; can_choose: boolean; free_switch_available: boolean; free_switch_until: string | null; between_seasons: boolean; next_switch_at: string | null };
export default function ChooseFaction({ membership, verified, preferred }: { membership: Membership; verified: boolean; preferred: Faction | null }) {
  const [selected, setSelected] = useState<Faction | null>(preferred ?? membership.faction);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  useEffect(() => {
    const remembered = sessionStorage.getItem("preferred-faction");
    // eslint-disable-next-line react-hooks/set-state-in-effect -- restore a preference across provider redirects
    if (!preferred && !membership.faction && isFaction(remembered)) setSelected(remembered);
  }, [preferred, membership.faction]);
  useEffect(() => {
    const previous = document.documentElement.dataset.theme;
    if (selected) document.documentElement.dataset.theme = selected;
    return () => { document.documentElement.dataset.theme = previous; };
  }, [selected]);
  async function choose() {
    if (!selected) return;
    setBusy(true);
    const result = await send("PUT", "/api/me/faction", { faction: selected });
    setBusy(false);
    if (result.ok) { sessionStorage.removeItem("preferred-faction"); setSaved(true); } else setMessage(result.error);
  }
  return <section className="enlist-page frame">
    {!membership.faction && <EnrollmentSteps step={saved ? 3 : 2} />}
    {saved && selected ? <div className="enlist-welcome"><Crest faction={selected} size={56} /><h1>Welcome to {factionInfo(selected).name}</h1><p>{verified ? "Your email is confirmed. You're ready to join your faction." : "Check your inbox to confirm your email. You can browse and watch now; chat and going live unlock after confirmation."}</p>{!verified && <button className="quiet" disabled={busy} onClick={async () => { setBusy(true); const r = await send("POST", "/api/auth/email/resend", {}); setBusy(false); setMessage(r.ok ? "Verification email queued. Check your inbox." : r.error); }}>Resend confirmation email</button>}<p><a className="button" href={`/factions/${selected}`}>Enter your faction hub</a></p></div> : <>
      <p className="eyebrow">{membership.faction ? "Your allegiance" : "Step 2 · Enlist"}</p><h1>Choose your side</h1><p>Choose by what matters to you. Every faction can stream and watch every allowed category.</p>
      <div className="enlist-grid" role="radiogroup" aria-label="Faction">{factions.map(f => <label key={f.slug} className="enlist-card frame" data-theme={f.slug}><input type="radio" name="faction" value={f.slug} checked={selected === f.slug} onChange={() => setSelected(f.slug)} disabled={!membership.can_choose} /><Crest faction={f.slug} size={56} /><h2>{f.name}</h2><span className="eyebrow">{f.title}</span><p className="faction-creed">{f.creed}</p><p>{f.belief}</p><h3>Home turf</h3><ul>{f.turf.map(g => <li key={g}>{g}</li>)}</ul></label>)}</div>
      <p>One free switch within seven days of your original choice. After that, switch during the seven-day break between seasons. Earlier influence stays with your old faction.</p>
      {membership.free_switch_available && membership.free_switch_until && <p>Your free switch is available until {new Date(membership.free_switch_until).toLocaleString()}.</p>}
      {!membership.can_choose ? <p className="notice">Your faction is locked{membership.next_switch_at && ` until ${new Date(membership.next_switch_at).toLocaleString()}`}. <Link href={`/factions/${membership.faction}`}>Visit your hub</Link></p> : <button disabled={!selected || busy || selected === membership.faction} onClick={choose}>{busy ? "Saving…" : selected ? `${membership.faction ? "Switch to" : "Enlist in"} ${factionInfo(selected).name}` : "Choose a faction"}</button>}
    </>}{message && <p className="notice" role="status">{message}</p>}
  </section>;
}
