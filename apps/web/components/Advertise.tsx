"use client";
import { FormEvent, useCallback, useEffect, useState } from "react";
import { Section, Status, type SaveState } from "./Form";
import { send, useLoad } from "../lib/client-api";

type Creative = { id: string; format: string; title: string; link: string; status: string; teen_safe: boolean; review_note: string; picture: string; video: string | null };
type Campaign = { id: string; name: string; placement: string; impressions: number; delivered: number; amount_cents: number; refund_cents: number; starts_on: string; ends_on: string; status: string; ending: string | null; creative_id: string | null; can_cancel: boolean; can_withdraw: boolean };
type Rate = { placement: string; cpm_cents: number | null; min_spend_cents: number; formats: string[] };
type View = { blocked: string | null; account: { name: string; website: string; country: string } | null; rate_card: Rate[]; creatives: Creative[]; campaigns: Campaign[]; sales_open: boolean };
type Quote = { amount_cents: number; cpm_cents: number; available: number; fits: boolean; soonest_start: string | null; below_minimum: boolean; min_spend_cents: number };
type Genre = { id: string; name: string };

const dollars = (cents: number) => `$${(cents / 100).toFixed(2)}`;
const placements: Record<string, string> = { break: "Streamer ad breaks", feed: "Beacons feed cards", site: "Site placement (Home and Browse)" };
const formats: Record<string, string> = { video_landscape: "Video, 16:9 (ad breaks)", video_portrait: "Video, 9:16 (feed)", image_portrait: "Picture, 9:16 (feed)", image_landscape: "Picture, 16:9 (site)" };
const statuses: Record<string, string> = { checkout: "Waiting for payment", paid: "Paid, waiting to start", running: "Running", delivered: "Delivered", refund_due: "Refunding", cancelled: "Cancelled", withdrawn: "Withdrawn" };
const day = (offset: number) => new Date(Date.now() + offset * 86400000).toISOString().slice(0, 10);

function Account({ view, reload }: { view: View; reload: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const r = await send("PUT", "/api/advertise/account", { name: form.get("name"), website: form.get("website"), country: form.get("country") });
    setState(r.ok ? { saved: "Saved." } : r);
    if (r.ok) reload();
  }
  return <Section title="Your business" intro="Shown with your ads and on receipts.">
    <form onSubmit={save}>
      <label className="field"><span>Business or person name</span><input name="name" required maxLength={80} defaultValue={view.account?.name} /></label>
      <label className="field"><span>Website</span><input name="website" type="url" required placeholder="https://" defaultValue={view.account?.website} /></label>
      <label className="field narrow"><span>Billing country (two letters, like US)</span><input name="country" required minLength={2} maxLength={2} defaultValue={view.account?.country} /></label>
      <button className="small">Save</button>
      <Status state={state} />
    </form>
  </Section>;
}

function Ads({ view, reload }: { view: View; reload: () => void }) {
  const [state, setState] = useState<SaveState>({});
  const [busy, setBusy] = useState(false);
  async function upload(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    setBusy(true);
    const r = await send("POST", "/api/advertise/creatives", new FormData(form));
    setBusy(false);
    setState(r.ok ? { saved: "Uploaded. Staff review every ad before it runs." } : r);
    if (r.ok) { form.reset(); reload(); }
  }
  return <Section title="Your ads" intro="Pictures are 16:9 (at least 1280×720) or 9:16 (at least 720×1280), up to 5 MB. Videos are 5 to 30 seconds in the same shapes, up to 11 MB. Ads follow the ad rules: no gambling, crypto, adult content, politics, weapons, tobacco or vaping, or misleading claims.">
    <form onSubmit={upload}>
      <label className="field"><span>Kind of ad</span><select name="format" required>{Object.entries(formats).map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select></label>
      <label className="field"><span>Title</span><input name="title" required maxLength={60} /></label>
      <label className="field"><span>Link (where a click goes)</span><input name="link" type="url" required placeholder="https://" /></label>
      <label className="field"><span>File</span><input name="file" type="file" required accept="image/png,image/jpeg,image/webp,video/mp4,video/webm,video/quicktime" /></label>
      <button className="small" disabled={busy}>{busy ? "Uploading…" : "Upload ad"}</button>
      <Status state={state} />
    </form>
    {view.creatives.length > 0 && <ul className="list">{view.creatives.map(c => <li key={c.id} className="row wrap">
      {c.video ? <video src={c.video} poster={c.picture} controls muted preload="none" width={160} /> : <img src={c.picture} alt="" width={160} />}
      <span><strong>{c.title}</strong> · {formats[c.format]}<br />
        <span className="badge">{c.status === "approved" ? (c.teen_safe ? "Approved, all ages" : "Approved, 18+ only") : c.status === "review" ? "In review" : "Not approved"}</span>
        {c.status === "rejected" && c.review_note && <span className="muted small"> {c.review_note}</span>}</span>
    </li>)}</ul>}
  </Section>;
}

function NewCampaign({ view, genres, reload }: { view: View; genres: Genre[]; reload: () => void }) {
  const [placement, setPlacement] = useState("site");
  const [impressions, setImpressions] = useState(1000);
  const [from, setFrom] = useState(day(1));
  const [to, setTo] = useState(day(14));
  const [chosen, setChosen] = useState<string[]>([]);
  const [audience, setAudience] = useState("all");
  const [quote, setQuote] = useState<Quote | null>(null);
  const [state, setState] = useState<SaveState>({});
  const rate = view.rate_card.find(r => r.placement === placement);
  const fits = view.creatives.filter(c => rate?.formats.includes(c.format));
  const forSale = !!rate?.cpm_cents;
  useEffect(() => {
    if (!forSale) return;
    const timer = setTimeout(async () => {
      const r = await send<Quote>("POST", "/api/advertise/quote", { placement, impressions, starts_on: from, ends_on: to, genres: chosen, audience });
      if (r.ok) { setQuote(r.data); setState({}); } else { setQuote(null); setState(r); }
    }, 300);
    return () => clearTimeout(timer);
  }, [placement, impressions, from, to, chosen, audience, forSale]);
  async function buy(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const r = await send<{ url: string }>("POST", "/api/advertise/campaigns", { placement, impressions, starts_on: from, ends_on: to, genres: chosen, audience, name: form.get("name"), creative_id: form.get("creative_id") });
    if (r.ok) window.location.assign(r.data.url); else { setState(r); reload(); }
  }
  return <Section title="New campaign" intro="You pay for verified impressions: real people with the ad on screen. S.V.E.R only sells what it can deliver in your dates. If your campaign isn't finished by its end date it keeps running at no extra cost, or you can withdraw it then and get the undelivered part back.">
    <form onSubmit={buy}>
      <label className="field"><span>Campaign name</span><input name="name" required maxLength={60} /></label>
      <label className="field"><span>Where it runs</span><select value={placement} onChange={e => setPlacement(e.target.value)}>{view.rate_card.map(r => <option key={r.placement} value={r.placement} disabled={!r.cpm_cents}>{placements[r.placement]}{r.cpm_cents ? ` · ${dollars(r.cpm_cents)} per 1,000` : " · not for sale yet"}</option>)}</select></label>
      <label className="field"><span>Ad</span><select name="creative_id" required>{fits.length === 0 ? <option value="">Upload an ad in this shape first</option> : fits.map(c => <option key={c.id} value={c.id}>{c.title}{c.status !== "approved" ? " (in review)" : ""}</option>)}</select></label>
      <div className="row wrap">
        <label className="field narrow"><span>Verified impressions</span><input type="number" min={1000} step={500} value={impressions} onChange={e => setImpressions(Number(e.target.value))} /></label>
        <label className="field narrow"><span>Start</span><input type="date" value={from} min={day(0)} onChange={e => setFrom(e.target.value)} /></label>
        <label className="field narrow"><span>End</span><input type="date" value={to} min={from} onChange={e => setTo(e.target.value)} /></label>
      </div>
      <fieldset className="field"><legend>Audience</legend>
        <label className="check"><input type="radio" name="audience" checked={audience === "all"} onChange={() => setAudience("all")} /> Everyone (the ad must be approved as suitable for teens)</label>
        <label className="check"><input type="radio" name="audience" checked={audience === "adult"} onChange={() => setAudience("adult")} /> Adults only (18+)</label>
      </fieldset>
      <fieldset className="field"><legend>Genres (none chosen means all)</legend><div className="row wrap">
        {genres.map(g => <label key={g.id} className="check"><input type="checkbox" checked={chosen.includes(g.id)} onChange={e => setChosen(e.target.checked ? [...chosen, g.id] : chosen.filter(x => x !== g.id))} /> {g.name}</label>)}
      </div></fieldset>
      {forSale && quote && <p role="status">{quote.fits
        ? <><strong>{dollars(quote.amount_cents)}</strong> for {impressions.toLocaleString()} verified impressions{quote.below_minimum ? ` (the minimum for this placement is ${dollars(quote.min_spend_cents)})` : ""}. {quote.available.toLocaleString()} available for these dates.</>
        : <>Only {quote.available.toLocaleString()} impressions are available for these dates.{quote.soonest_start ? ` The full amount fits starting ${quote.soonest_start}.` : " Choose fewer impressions or more days."}</>}</p>}
      <button disabled={!view.sales_open || !forSale || !quote?.fits || fits.length === 0}>{view.sales_open ? "Pay and reserve" : "Ad sales open soon"}</button>
      <Status state={state} />
    </form>
  </Section>;
}

function Campaigns({ view, reload }: { view: View; reload: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function act(c: Campaign, action: "cancel" | "withdraw") {
    const question = action === "cancel" ? `Cancel "${c.name}" and get a full refund?` : `Stop "${c.name}" and get the undelivered part back?`;
    if (!window.confirm(question)) return;
    const r = await send<{ refund_cents?: number }>("POST", `/api/advertise/campaigns/${c.id}/${action}`);
    setState(r.ok ? { saved: action === "cancel" ? "Cancelled. Any payment is refunded to your card." : `Withdrawn. ${dollars(r.data.refund_cents ?? 0)} goes back to your card.` } : r);
    reload();
  }
  async function swap(c: Campaign, creative: string) {
    const r = await send("POST", `/api/advertise/campaigns/${c.id}/creative`, { creative_id: creative });
    setState(r.ok ? { saved: "Ad changed." } : r);
    reload();
  }
  if (view.campaigns.length === 0) return null;
  return <Section title="Your campaigns">
    <ul className="list">{view.campaigns.map(c => {
      const ad = view.creatives.find(x => x.id === c.creative_id);
      const options = view.creatives.filter(x => view.rate_card.find(r => r.placement === c.placement)?.formats.includes(x.format) && x.status !== "rejected");
      return <li key={c.id}>
        <p><strong>{c.name}</strong> · {placements[c.placement]} · <span className="badge">{statuses[c.status] ?? c.status}</span></p>
        <p className="muted small">{c.delivered.toLocaleString()} of {c.impressions.toLocaleString()} verified impressions · {c.starts_on} to {c.ends_on} · paid {dollars(c.amount_cents)}{c.refund_cents > 0 ? ` · refunded ${dollars(c.refund_cents)}` : ""}</p>
        {c.status === "paid" && ad?.status !== "approved" && <p className="small">Waiting for the ad to be approved{ad?.status === "rejected" ? `: it wasn't approved (${ad.review_note}). Choose another ad or cancel.` : "."}</p>}
        <div className="row wrap">
          {(c.status === "paid" || c.status === "checkout") && options.length > 1 && <label className="field narrow"><span>Ad</span><select defaultValue={c.creative_id ?? ""} onChange={e => swap(c, e.target.value)}>{options.map(o => <option key={o.id} value={o.id}>{o.title}</option>)}</select></label>}
          {c.can_cancel && <button type="button" className="small quiet" onClick={() => act(c, "cancel")}>Cancel</button>}
          {c.can_withdraw && <button type="button" className="small quiet" onClick={() => act(c, "withdraw")}>Withdraw and refund the rest</button>}
        </div>
      </li>;
    })}</ul>
    <Status state={state} />
  </Section>;
}

export function Advertise({ paid }: { paid: string | null }) {
  const [view, setView] = useState<View | null>(null);
  const [genres, setGenres] = useState<Genre[]>([]);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/advertise");
    if (r.ok) setView(r.data); else setError(r.error);
    const g = await send<{ genres: Genre[] }>("GET", "/api/discovery/browse");
    if (g.ok) setGenres(g.data.genres.map(x => ({ id: x.id, name: x.name })));
  }, []);
  useLoad(load);
  if (!view) return error ? <p role="alert" className="form-message error">{error}</p> : <p className="loading">Loading…</p>;
  return <>
    <p>Reach people who play, build and make. Every ad is reviewed by S.V.E.R staff, labelled as an ad, and kept out of discovery. You only pay for verified impressions, and every campaign gets all of them.</p>
    {paid && <p role="status" className="form-message">Thanks. Your payment is being confirmed; the campaign starts once its ad is approved and its start date arrives.</p>}
    {view.blocked ? <p role="alert" className="form-message error">{view.blocked}</p> : <>
      <Account view={view} reload={load} />
      {view.account && <>
        <Ads view={view} reload={load} />
        <NewCampaign view={view} genres={genres} reload={load} />
        <Campaigns view={view} reload={load} />
      </>}
    </>}
  </>;
}
