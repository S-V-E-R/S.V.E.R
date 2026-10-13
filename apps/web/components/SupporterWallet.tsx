"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { founderNumber, monthly, tierName, type SupporterMine, type SupportersData } from "../lib/supporters";
import { Section, Status, type SaveState } from "./Form";
import { SupporterBadge, type SupporterTier } from "./Supporter";

const date = (iso: string) => new Date(iso).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" });

/** /wallet: the platform supporter plan, its next charge, change tier, cancel, and the Founder name setting. */
export function SupporterWallet() {
  const [data, setData] = useState<SupportersData | null>(null);
  const [state, setState] = useState<SaveState>({});
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const r = await send<SupportersData>("GET", "/api/supporters");
    if (r.ok) setData(r.data);
  }, []);
  useLoad(load);
  if (!data) return null;
  const mine = data.mine;
  const plan = mine?.plan ?? null;

  async function act(method: string, path: string, body: unknown, saved: string) {
    setBusy(true); setState({});
    const r = await send<SupporterMine>(method, path, body);
    setBusy(false);
    if (!r.ok) { setState(r); return; }
    setData(d => d && { ...d, mine: r.data });
    setState({ saved });
  }
  const change = (tier: SupporterTier) => {
    const up = tier === "greycloak";
    const ask = up ? "Move up to Greycloak now? Stripe charges the difference for the rest of this month today." : "Move down to Wolfsworn? You keep Greycloak until this month ends, then renew at $5 a month.";
    if (window.confirm(ask)) void act("POST", "/api/supporters/change", { tier }, up ? "You're a Greycloak now." : "You'll move to Wolfsworn when this month ends.");
  };

  if (!plan && !mine?.founder) return <Section title="Support S.V.E.R" intro="A monthly subscription to S.V.E.R itself. The perks are only how you look.">
    <p><Link href="/supporters" className="button quiet">See the tiers</Link></p>
  </Section>;

  const other: SupporterTier = plan?.tier === "greycloak" ? "wolfsworn" : "greycloak";
  return <Section title="Support S.V.E.R" intro="Your subscription to S.V.E.R itself. Cancelling keeps your look to the end of the paid month.">
    {plan ? <div className="supporter-plan">
      <p><SupporterBadge tier={plan.tier} monthsSupported={plan.months} /> <strong>{tierName[plan.tier]}</strong> · {plan.months} {plan.months === 1 ? "month" : "months"} supported</p>
      <p>{plan.auto_renew ? <>Next charge: {monthly(plan.next_cents).replace(" a month", "")} on {date(plan.paid_through)}{plan.pending_tier && ` (moving to ${tierName[plan.pending_tier]})`}.</> : <>Renewal is off. Your supporter look ends {date(plan.paid_through)}.</>}</p>
      {plan.auto_renew && <div className="row wrap">
        {plan.pending_tier ? <button type="button" className="small quiet" disabled={busy} onClick={() => act("POST", "/api/supporters/change", { tier: plan.tier }, `You'll stay ${tierName[plan.tier]}.`)}>Stay {tierName[plan.tier]}</button>
          : <button type="button" className="small" disabled={busy} onClick={() => change(other)}>{other === "greycloak" ? "Move up to Greycloak ($25 a month)" : "Move down to Wolfsworn ($5 a month)"}</button>}
        <button type="button" className="small quiet" disabled={busy} onClick={() => { if (window.confirm("Stop renewing? Your supporter look lasts to the end of the paid month.")) void act("POST", "/api/supporters/cancel", undefined, "Renewal is off."); }}>Cancel renewal</button>
      </div>}
      {!plan.auto_renew && <p><Link href="/supporters" className="button quiet">Subscribe again</Link></p>}
    </div> : <p><Link href="/supporters" className="button quiet">Support S.V.E.R again</Link></p>}
    {mine?.founder && <label className="checkbox"><input type="checkbox" checked={mine.founder.hidden} disabled={busy}
      onChange={e => act("PUT", "/api/supporters/founder", { hidden: e.target.checked }, e.target.checked ? `The Founders list now shows "Founder ${founderNumber(mine.founder!.number)}".` : "Your name is back on the Founders list.")} />
      {" "}Hide my name on the Founders list (you&apos;re Founder {founderNumber(mine.founder.number)})</label>}
    <Status state={state} />
  </Section>;
}
