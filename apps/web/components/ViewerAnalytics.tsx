"use client";
import { useCallback, useState } from "react";
import Link from "next/link";
import { Section } from "./Form";
import { send, useLoad } from "../lib/client-api";

type Totals = { visits: number; viewers: number; watch_hours: number; avg_minutes: number; median_minutes: number; new_viewers: number; returning_viewers: number;
  follower_watch_share: number; signed_in_share: number; new_follows: number; chatters: number; live_hours: number; peak_viewers: number; avg_viewers: number };
type Source = { source: string; visits: number; viewers: number; watch_hours: number; avg_minutes: number; stayed_5_share: number; follows: number };
type Rate = { hours: number; arrivals: number; per_hour: number | null } | null;
type Report = {
  totals: Totals; sources: Source[]; lengths: { bucket: number; visits: number }[];
  exposure: { surface: string; shown: number; visits: number; conversion: number | null }[];
  magnet: { viewers: number; watch_hours: number; opened_channel: number };
  lift: { live_now: Rate; magnet: Rate; otherwise: Rate };
  referrers: { host: string; visits: number }[];
  daily: { day: string; watch_hours: number; viewers: number; discovery_hours: number }[];
  channels: { username: string; watch_hours: number; viewers: number; avg_minutes: number; discovery_share: number | null }[];
  broadcasts: { id: string; started_at: string; ended_at: string | null; category: string | null; viewers: number; watch_hours: number; avg_minutes: number; peak_viewers: number; new_follows: number; top_source: string | null }[];
};

const SOURCES: Record<string, string> = {
  home: "Home page", magnet_channel: "Watched inside MAGNet", magnet: "MAGNet → your channel", search: "Search", browse: "Browse",
  following: "Following list", alert: "Go-live alerts", suggestion: "Suggested (Up next, stream end)", raid: "Raids", host: "Hosts",
  beacon: "Beacons", clip: "Clips", video: "Past broadcasts", faction: "Faction pages", channel: "Your channel page",
  external: "Other websites", direct: "Direct or typed link", other: "Other",
};
const SURFACES: Record<string, string> = { home: "Home page", browse: "Browse", search: "Search", following: "Following list", faction: "Faction pages", magnet: "MAGNet pages", suggestion: "Up next" };
const LENGTHS = ["Under 1 minute", "1–5 minutes", "5–15 minutes", "15–60 minutes", "Over an hour"];
const pct = (n: number | null | undefined) => n == null ? "—" : `${(n * 100).toFixed(n < 0.1 ? 1 : 0)}%`;
const num = (n: number) => n.toLocaleString();

/** Viewer analytics (docs/ANALYTICS.md): a streamer's own in Creator Studio, or the whole
 *  platform's in Raven's Eye (`platform`). Totals only, never a viewer. */
export function ViewerAnalytics({ endpoint, platform = false }: { endpoint: string; platform?: boolean }) {
  const your = platform ? "streams'" : "your";
  const [days, setDays] = useState(30);
  const [data, setData] = useState<Report | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const result = await send<Report>("GET", `${endpoint}?days=${days}`);
    if (result.ok) { setData(result.data); setMessage(""); } else setMessage(result.error);
  }, [days, endpoint]);
  useLoad(load);
  const t = data?.totals;
  const visits = data?.lengths.reduce((n, l) => n + l.visits, 0) ?? 0;
  const usual = data?.lift.otherwise?.per_hour ?? null;
  const lift = (rate: Rate) => !rate || !rate.hours ? "No time there yet." : <>{rate.per_hour ?? 0} new viewers an hour over {rate.hours} h{usual ? <> ({((rate.per_hour ?? 0) / usual).toFixed(1)}× {your} usual rate)</> : null}</>;
  return <>
    <p role="group" aria-label="Time range">{[7, 30, 90].map(d => <button key={d} type="button" className={d === days ? "small" : "small quiet"} aria-pressed={d === days} onClick={() => setDays(d)}>Last {d} days</button>)}</p>
    {message && <p role="alert" className="form-message error">{message}</p>}
    {!data || !t ? !message && <p className="loading">Loading…</p> : <>
      <Section title="Overview" intro={`Only real viewers count: sessions viewer integrity counted. Totals only; ${platform ? "no one" : "you never"} see${platform ? "s" : ""} who watched.`}>
        <div className="table-scroll"><table className="table"><tbody>
          <tr><th scope="row">Watch hours</th><td>{t.watch_hours}</td><th scope="row">Hours live</th><td>{t.live_hours}</td></tr>
          <tr><th scope="row">Unique viewers</th><td>{num(t.viewers)}</td><th scope="row">Visits</th><td>{num(t.visits)}</td></tr>
          <tr><th scope="row">Average viewers</th><td>{t.avg_viewers}</td><th scope="row">Peak viewers</th><td>{num(t.peak_viewers)}</td></tr>
          <tr><th scope="row">Average visit</th><td>{t.avg_minutes} min</td><th scope="row">Typical visit (median)</th><td>{t.median_minutes} min</td></tr>
          <tr><th scope="row">New viewers</th><td>{num(t.new_viewers)}</td><th scope="row">Returning viewers</th><td>{num(t.returning_viewers)}</td></tr>
          <tr><th scope="row">New follows</th><td>{num(t.new_follows)}</td><th scope="row">Chatters</th><td>{num(t.chatters)}</td></tr>
          <tr><th scope="row">Watch time from followers</th><td>{pct(t.follower_watch_share)}</td><th scope="row">Visits signed in</th><td>{pct(t.signed_in_share)}</td></tr>
        </tbody></table></div>
      </Section>
      <Section title="Where viewers come from" intro={`How each visit reached ${platform ? "a" : "your"} stream. Follows count a signed-in viewer who followed during the visit or within 10 minutes after.`}>
        {data.sources.length === 0 ? <p className="muted">No visits in this range yet.</p> : <div className="table-scroll"><table className="table"><thead><tr><th>Source</th><th>Watch hours</th><th>Share</th><th>Visits</th><th>Viewers</th><th>Average visit</th><th>Stayed 5+ min</th><th>Follows</th></tr></thead>
          <tbody>{data.sources.map(s => <tr key={s.source}><th scope="row">{SOURCES[s.source] ?? s.source}</th><td>{s.watch_hours}</td><td>{pct(t.watch_hours ? s.watch_hours / t.watch_hours : 0)}</td><td>{num(s.visits)}</td><td>{num(s.viewers)}</td><td>{s.avg_minutes} min</td><td>{pct(s.stayed_5_share)}</td><td>{num(s.follows)}</td></tr>)}</tbody></table></div>}
      </Section>
      <Section title="Did discovery bring viewers?" intro={platform ? "New arrivals an hour while streams were on top of Live Now or featured on MAGNet, against the rest of their live time." : "New arrivals an hour while you were on top of Live Now or featured on MAGNet, against the rest of your live time."}>
        <ul className="list">
          <li><strong>On top of Live Now:</strong> {lift(data.lift.live_now)}</li>
          <li><strong>Featured on MAGNet:</strong> {lift(data.lift.magnet)}</li>
          <li><strong>The rest of {your} live time:</strong> {data.lift.otherwise?.hours ? <>{usual ?? 0} new viewers an hour over {data.lift.otherwise.hours} h</> : "No time yet."}</li>
          <li><strong>MAGNet:</strong> {num(data.magnet.viewers)} viewers watched {platform ? "streams" : "you"} inside MAGNet channels ({data.magnet.watch_hours} h); {num(data.magnet.opened_channel)} then opened {platform ? "the" : "your"} channel{data.magnet.viewers ? ` (${pct(data.magnet.opened_channel / data.magnet.viewers)})` : ""}.</li>
        </ul>
      </Section>
      <Section title="Seen and clicked" intro={`How often ${platform ? "live cards were" : "your live card was"} on someone's screen for a second, and how many visits came from that page.`}>
        {data.exposure.length === 0 ? <p className="muted">No cards seen in this range yet.</p> : <div className="table-scroll"><table className="table"><thead><tr><th>Where</th><th>Times seen</th><th>Visits</th><th>Click-through</th></tr></thead>
          <tbody>{data.exposure.map(e => <tr key={e.surface}><th scope="row">{SURFACES[e.surface] ?? e.surface}</th><td>{num(e.shown)}</td><td>{num(e.visits)}</td><td>{pct(e.conversion)}</td></tr>)}</tbody></table></div>}
      </Section>
      <Section title="How long viewers stay">
        {visits === 0 ? <p className="muted">No visits yet.</p> : <div className="table-scroll"><table className="table"><thead><tr><th>Visit length</th><th>Visits</th><th>Share</th></tr></thead>
          <tbody>{data.lengths.map(l => <tr key={l.bucket}><th scope="row">{LENGTHS[l.bucket]}</th><td>{num(l.visits)}</td><td>{pct(l.visits / visits)}</td></tr>)}</tbody></table></div>}
      </Section>
      {data.referrers.length > 0 && <Section title="Other websites" intro="Sites whose links brought viewers (site names only).">
        <div className="table-scroll"><table className="table"><thead><tr><th>Site</th><th>Visits</th></tr></thead><tbody>{data.referrers.map(r => <tr key={r.host}><th scope="row">{r.host}</th><td>{num(r.visits)}</td></tr>)}</tbody></table></div>
      </Section>}
      <Section title="By day">
        {data.daily.length === 0 ? <p className="muted">Nothing yet.</p> : <div className="table-scroll"><table className="table"><thead><tr><th>Day</th><th>Watch hours</th><th>From discovery</th><th>Viewers</th></tr></thead>
          <tbody>{data.daily.map(d => <tr key={d.day}><th scope="row">{new Date(d.day).toLocaleDateString()}</th><td>{d.watch_hours}</td><td>{d.discovery_hours}</td><td>{num(d.viewers)}</td></tr>)}</tbody></table></div>}
      </Section>
      {platform && <Section title="Top channels" intro="By watch hours in the range.">
        {data.channels.length === 0 ? <p className="muted">No watch time yet.</p> : <div className="table-scroll"><table className="table"><thead><tr><th>Channel</th><th>Watch hours</th><th>Viewers</th><th>Average visit</th><th>From discovery</th></tr></thead>
          <tbody>{data.channels.map(c => <tr key={c.username}><th scope="row"><Link href={`/${c.username}`}>{c.username}</Link></th><td>{c.watch_hours}</td><td>{num(c.viewers)}</td><td>{c.avg_minutes} min</td><td>{pct(c.discovery_share)}</td></tr>)}</tbody></table></div>}
      </Section>}
      {!platform && <Section title="Broadcasts">
        {data.broadcasts.length === 0 ? <p className="muted">No broadcasts in this range.</p> : <div className="table-scroll"><table className="table"><thead><tr><th>Started</th><th>Category</th><th>Length</th><th>Watch hours</th><th>Viewers</th><th>Peak</th><th>Average visit</th><th>New follows</th><th>Top source</th></tr></thead>
          <tbody>{data.broadcasts.map(b => <tr key={b.id}><th scope="row">{new Date(b.started_at).toLocaleString()}</th><td>{b.category ?? "—"}</td><td>{b.ended_at ? `${Math.round((Date.parse(b.ended_at) - Date.parse(b.started_at)) / 60000)} min` : "Live"}</td><td>{b.watch_hours}</td><td>{num(b.viewers)}</td><td>{num(b.peak_viewers)}</td><td>{b.avg_minutes} min</td><td>{num(b.new_follows)}</td><td>{b.top_source ? SOURCES[b.top_source] ?? b.top_source : "—"}</td></tr>)}</tbody></table></div>}
      </Section>}
    </>}
  </>;
}
