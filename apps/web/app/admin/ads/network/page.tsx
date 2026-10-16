"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";

type Rate = { placement: string; cpm_cents: number | null; min_spend_cents: number; daily_estimate: number };
type Day = { day: string; forecast: number; promised: number; sellable: number };
type Creative = { id: string; format: string; title: string; link: string; picture: string; video: string | null; advertiser: { name: string; website: string; username: string } | null };
type Campaign = { id: string; name: string; advertiser: string | null; placement: string; impressions: number; delivered: number; amount_cents: number; refund_cents: number; starts_on: string; ends_on: string; status: string; refund_error: string | null };
type Revenue = { placements: { placement: string; impressions: number; gross: number; streamers: number; viewers: number }[]; rejected: { reason: string; count: number }[] };
type View = { revenue: Revenue; rate_card: Rate[]; inventory: { placement: string; days: Day[] }[]; review: Creative[]; campaigns: Campaign[]; sales_open: boolean };

const tenths = (t: number) => `$${(t / 1000).toFixed(3)}`;
const dollars = (cents: number) => `$${(cents / 100).toFixed(2)}`;
const placements: Record<string, string> = { break: "Ad breaks", feed: "Beacons feed", site: "Site placement" };

/** Staff → Ad network (docs/ADVERTISERS.md): rate card, inventory, ad review and campaigns. */
export default function AdNetwork() {
  const [view, setView] = useState<View | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/admin/adnet");
    if (r.ok) setView(r.data); else setState(r);
  }, []);
  useLoad(load);
  async function saveRates(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const placementsInput = Object.fromEntries(view!.rate_card.map(r => {
      const cpm = String(form.get(`cpm-${r.placement}`) ?? "").trim();
      return [r.placement, { cpm_cents: cpm === "" ? null : Math.round(Number(cpm) * 100), min_spend_cents: Math.round(Number(form.get(`min-${r.placement}`)) * 100), daily_estimate: Number(form.get(`daily-${r.placement}`)) }];
    }));
    const r = await send("PUT", "/api/admin/adnet/rate-card", { placements: placementsInput, note: form.get("note") });
    setState(r.ok ? { saved: "Rate card saved." } : r);
    if (r.ok) load();
  }
  async function review(c: Creative, action: "approve" | "approve_teen" | "reject") {
    const note = action === "reject" ? window.prompt("Why isn't this ad approved? The advertiser sees this.") : "";
    if (action === "reject" && !note) return;
    const r = await send("POST", `/api/admin/adnet/creatives/${c.id}/review`, { action, note });
    setState(r.ok ? { saved: "Reviewed." } : r);
    load();
  }
  async function refund(c: Campaign) {
    const note = window.prompt(`Refund what "${c.name}" hasn't delivered? Reason (audited):`);
    if (!note) return;
    const r = await send<{ refund_cents: number }>("POST", `/api/admin/adnet/campaigns/${c.id}/refund`, { note });
    setState(r.ok ? { saved: `Refunding ${dollars(r.data.refund_cents)}.` } : r);
    load();
  }
  if (!view) return state.error ? <p role="alert" className="form-message error">{state.error}</p> : <p className="loading">Loading…</p>;
  return <><h1>Ad network</h1>
    <p>S.V.E.R&apos;s own approved ads (docs/ADVERTISERS.md). Ad sales are <strong>{view.sales_open ? "open" : "off"}</strong> (the switch stops new sales; campaigns already paid keep running); turn them on in <Link href="/admin/switches">Switches</Link> only after the advertiser terms, privacy policy, legal review and tax decision. <Link href="/admin/ads">Test book</Link></p>
    <Section title="Rate card" intro="Price per 1,000 verified impressions; leave it empty to keep a placement off sale. The daily estimate is the starting forecast of verified impressions a day, until delivery history exists. Only 70% of the forecast is sold. Existing campaigns keep their price.">
      <form onSubmit={saveRates}>
        <div className="table-scroll"><table className="table"><thead><tr><th scope="col">Placement</th><th scope="col">CPM ($)</th><th scope="col">Minimum spend ($)</th><th scope="col">Daily estimate</th></tr></thead>
          <tbody>{view.rate_card.map(r => <tr key={r.placement}><th scope="row">{placements[r.placement]}</th>
            <td><input aria-label={`${placements[r.placement]} CPM in dollars`} name={`cpm-${r.placement}`} type="number" min={0.5} step="0.01" defaultValue={r.cpm_cents == null ? "" : r.cpm_cents / 100} /></td>
            <td><input aria-label={`${placements[r.placement]} minimum spend in dollars`} name={`min-${r.placement}`} type="number" min={1} step="0.01" required defaultValue={r.min_spend_cents / 100} /></td>
            <td><input aria-label={`${placements[r.placement]} verified impressions a day`} name={`daily-${r.placement}`} type="number" min={0} step={1} required defaultValue={r.daily_estimate} /></td></tr>)}</tbody></table></div>
        <label className="field"><span>Note (required, audited)</span><input name="note" required maxLength={500} /></label>
        <button className="small">Save rate card</button>
      </form>
    </Section>
    <Section title="Inventory, next 14 days" intro="Forecast is verified impressions a day; promised is what paid and held campaigns are owed that day; sellable is what a new campaign could still buy for that one day.">
      {view.inventory.map(p => <div key={p.placement} className="table-scroll"><table className="table"><caption>{placements[p.placement]}</caption>
        <thead><tr><th scope="col">Day</th><th scope="col">Forecast</th><th scope="col">Promised</th><th scope="col">Sellable</th></tr></thead>
        <tbody>{p.days.slice(0, 14).map(d => <tr key={d.day}><th scope="row">{d.day}</th><td>{d.forecast.toLocaleString()}</td><td>{d.promised.toLocaleString()}</td><td>{d.sellable.toLocaleString()}</td></tr>)}</tbody></table></div>)}
    </Section>
    <Section title="Ads waiting for review" intro="Approve for 18+ only, approve as suitable for teens (needed for all-ages campaigns), or reject with a reason.">
      {view.review.length === 0 ? <p className="muted">None.</p> : <ul className="list">{view.review.map(c => <li key={c.id} className="row wrap">
        {c.video ? <video src={c.video} poster={c.picture} controls muted preload="none" width={240} /> : <img src={c.picture} alt={c.title} width={240} />}
        <span><strong>{c.title}</strong> · {c.format}<br /><a href={c.link} rel="noopener noreferrer nofollow" target="_blank">{c.link}</a><br />
          <span className="muted small">{c.advertiser ? `${c.advertiser.name} (@${c.advertiser.username}, ${c.advertiser.website})` : ""}</span></span>
        <span className="row wrap">
          <button type="button" className="small" onClick={() => review(c, "approve_teen")}>Approve, all ages</button>
          <button type="button" className="small" onClick={() => review(c, "approve")}>Approve, 18+</button>
          <button type="button" className="small danger" onClick={() => review(c, "reject")}>Reject</button>
        </span>
      </li>)}</ul>}
    </Section>
    <Section title="Ad revenue" intro="Settled verified impressions: gross, what streamers and viewers earned, and what S.V.E.R keeps. Rejections are from the last 30 days.">
      {view.revenue.placements.length === 0 ? <p className="muted">No verified impressions yet.</p> : <div className="table-scroll"><table className="table"><thead><tr><th scope="col">Placement</th><th scope="col">Impressions</th><th scope="col">Gross</th><th scope="col">Streamers</th><th scope="col">Viewers</th><th scope="col">S.V.E.R</th></tr></thead>
        <tbody>{view.revenue.placements.map(p => <tr key={p.placement}><th scope="row">{placements[p.placement] ?? p.placement}</th><td>{p.impressions.toLocaleString()}</td><td>{tenths(p.gross)}</td><td>{tenths(p.streamers)}</td><td>{tenths(p.viewers)}</td><td>{tenths(p.gross - p.streamers - p.viewers)}</td></tr>)}</tbody></table></div>}
      {view.revenue.rejected.length > 0 && <p className="small">Not counted: {view.revenue.rejected.map(r => `${r.count.toLocaleString()} ${r.reason.replace(/_/g, " ")}`).join(" · ")}</p>}
    </Section>
    <Section title="Campaigns">
      {view.campaigns.length === 0 ? <p className="muted">None yet.</p> : <div className="table-scroll"><table className="table"><thead><tr><th scope="col">Campaign</th><th scope="col">Advertiser</th><th scope="col">Placement</th><th scope="col">Delivered</th><th scope="col">Dates</th><th scope="col">Paid</th><th scope="col">Status</th><th scope="col"><span className="sr-only">Actions</span></th></tr></thead>
        <tbody>{view.campaigns.map(c => <tr key={c.id}><th scope="row">{c.name}</th><td>{c.advertiser ?? "(deleted)"}</td><td>{placements[c.placement]}</td><td>{c.delivered.toLocaleString()} / {c.impressions.toLocaleString()}</td><td>{c.starts_on} to {c.ends_on}</td><td>{dollars(c.amount_cents)}{c.refund_cents > 0 ? ` (refunded ${dollars(c.refund_cents)})` : ""}</td><td>{c.status}{c.refund_error ? <><br /><span className="small" role="alert">Stripe declined the refund ({c.refund_error}). Fix the cause, then retry.</span></> : null}</td>
          <td>{(c.status === "paid" || c.status === "running") && <button type="button" className="small quiet" onClick={() => refund(c)}>Refund undelivered</button>}{c.refund_error && <button type="button" className="small quiet" onClick={() => refund(c)}>Retry refund</button>}</td></tr>)}</tbody></table></div>}
    </Section>
    <Status state={state} />
  </>;
}
