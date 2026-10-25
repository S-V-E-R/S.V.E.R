"use client";
import { useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Money = Record<string, { count: number; valor: number; usd_cents: number }>;
type Stats = {
  signups: number; verified_signups: number; viewers: number; chatters: number; messages: number; follows: number;
  broadcasts: number; streamers: number; broadcast_hours: number; watch_hours: number; peak_viewers: number;
  faction_joins: Record<string, number>; money: Money;
  top_channels: { username: string; watch_hours: number }[]; top_categories: { category: string; broadcast_hours: number }[];
};
type Day = { day: string; stats: Stats; partial: boolean };
type CategoryRow = { category: string; genre: string | null; hours: number; streamers: number; broadcasts: number; watch_hours: number; peak_viewers: number };
type Streams = { categories: CategoryRow[]; totals: { hours: number; streamers: number; broadcasts: number; categories: number; watch_hours: number; peak_viewers: number } };
const METRICS: [keyof Stats, string, "sum" | "max"][] = [
  ["signups", "Sign-ups", "sum"], ["viewers", "Signed-in viewers", "max"], ["watch_hours", "Watch hours", "sum"],
  ["peak_viewers", "Peak viewers", "max"], ["streamers", "Streamers live", "max"], ["broadcast_hours", "Broadcast hours", "sum"],
  ["messages", "Chat messages", "sum"], ["follows", "Follows", "sum"],
];
const usd = (cents: number) => `$${(cents / 100).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
const round = (n: number) => Math.round(n * 10) / 10;

/** A small trend line; the label states the values in words for screen readers. */
function Spark({ values, label }: { values: number[]; label: string }) {
  const max = Math.max(1, ...values);
  const step = values.length > 1 ? 100 / (values.length - 1) : 100;
  const points = values.map((v, i) => `${(i * step).toFixed(1)},${(28 - 26 * v / max).toFixed(1)}`).join(" ");
  return <svg className="spark" viewBox="0 0 100 30" preserveAspectRatio="none" role="img" aria-label={`${label}: from ${values[0] ?? 0} to ${values.at(-1) ?? 0}, highest ${max}`}>
    <polyline points={points} fill="none" stroke="currentColor" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
  </svg>;
}
/** Adds up a top list across days. */
function combine<T extends Record<string, string | number>>(days: Day[], pick: (s: Stats) => T[], key: keyof T, value: keyof T) {
  const totals = new Map<string, number>();
  for (const d of days) for (const row of pick(d.stats)) totals.set(String(row[key]), (totals.get(String(row[key])) ?? 0) + Number(row[value]));
  return [...totals].sort((a, b) => b[1] - a[1]).slice(0, 10);
}

const hours = (n: number) => `${n >= 10 ? Math.round(n).toLocaleString() : round(n)} ${n === 1 ? "hour" : "hours"}`;
/** Ready-to-post lines for X from the range's streams; staff review and post them themselves. */
function postLines(s: Streams, period: string) {
  const t = s.totals;
  const lines = [`${hours(t.hours)} streamed on S.V.E.R ${period} by ${t.streamers} ${t.streamers === 1 ? "streamer" : "streamers"} across ${t.categories} ${t.categories === 1 ? "game and category" : "games and categories"}.`];
  if (t.watch_hours >= 1) lines.push(`${hours(t.watch_hours)} watched on S.V.E.R ${period}.`);
  for (const c of s.categories.filter(c => c.category !== "Uncategorized" && c.hours >= 1).slice(0, 5)) lines.push(`${hours(c.hours)} of ${c.category} ${c.hours === 1 ? "has" : "have"} been streamed on S.V.E.R ${period}!`);
  return lines.map(line => `${line} sver.tv`);
}
function PostLine({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return <li className="row wrap"><span>{text}</span>
    <button type="button" className="small quiet" onClick={async () => { try { await navigator.clipboard.writeText(text); setCopied(true); setTimeout(() => setCopied(false), 2000); } catch { /* the text stays selectable */ } }}>{copied ? "Copied" : "Copy"}</button>
    <a className="button small quiet" href={`https://x.com/intent/post?text=${encodeURIComponent(text)}`} target="_blank" rel="noopener noreferrer">Post on X</a>
  </li>;
}

/** Streams in the range by the category actually streamed, genre totals, and lines to post on X. */
function StreamsByCategory({ streams, range, plays, setPlays }: { streams: Streams; range: number; plays: boolean; setPlays: (on: boolean) => void }) {
  const period = range === 7 ? "this week" : range === 30 ? "this month" : `in the last ${range} days`;
  const genres = new Map<string, number>();
  for (const c of streams.categories) genres.set(c.genre ?? "none", (genres.get(c.genre ?? "none") ?? 0) + c.hours);
  const t = streams.totals;
  return <>
    <h2>Streams by category</h2>
    <p className="muted">The last {range} days and today so far, by the category each broadcast was in at the time. Streamers count once across the range.</p>
    <label className="row"><input type="checkbox" checked={plays} onChange={e => setPlays(e.currentTarget.checked)} /> Include SVER Plays</label>
    <p>{hours(t.hours)} streamed · {t.streamers} streamers · {t.broadcasts} broadcasts · {t.categories} categories · {hours(t.watch_hours)} watched · peak {t.peak_viewers} viewers</p>
    <div className="table-scroll"><table className="table"><thead><tr><th scope="col">Category</th><th scope="col">Genre</th><th scope="col">Hours streamed</th><th scope="col">Streamers</th><th scope="col">Broadcasts</th><th scope="col">Watch hours</th><th scope="col">Peak viewers</th></tr></thead>
      <tbody>{streams.categories.map(c => <tr key={c.category}><th scope="row">{c.category}</th><td>{c.genre ?? "—"}</td><td>{round(c.hours)}</td><td>{c.streamers}</td><td>{c.broadcasts}</td><td>{round(c.watch_hours)}</td><td>{c.peak_viewers}</td></tr>)}</tbody></table></div>
    <h2>Hours by genre</h2>
    <ul>{[...genres].sort((a, b) => b[1] - a[1]).map(([g, h]) => <li key={g}>{g} · {round(h)} h</li>)}</ul>
    <h2>Post on X</h2>
    <p className="muted">Check each line before posting. &quot;Post on X&quot; opens X with the text filled in.</p>
    <ul className="stack">{postLines(streams, period).map(line => <PostLine key={line} text={line} />)}</ul>
  </>;
}

/** Admin → Raven's Eye: platform analytics from daily rollups, and streams by the category actually streamed (docs/ADMIN.md). Staff only. */
export default function RavensEye() {
  const [range, setRange] = useState(30);
  const [plays, setPlays] = useState(false);
  const [days, setDays] = useState<Day[] | null>(null);
  const [streams, setStreams] = useState<Streams | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ days: Day[]; streams: Streams }>("GET", `/api/admin/ravens-eye?days=${range}&plays=${plays}`);
    if (r.ok) { setDays(r.data.days); setStreams(r.data.streams); setMessage(""); } else setMessage(r.error);
  }, [range, plays]);
  useLoad(load);
  const money: Money = {};
  for (const d of days ?? []) for (const [kind, m] of Object.entries(d.stats.money)) {
    const t = money[kind] ?? { count: 0, valor: 0, usd_cents: 0 };
    money[kind] = { count: t.count + m.count, valor: t.valor + m.valor, usd_cents: t.usd_cents + m.usd_cents };
  }
  const factions: Record<string, number> = {};
  for (const d of days ?? []) for (const [f, n] of Object.entries(d.stats.faction_joins)) factions[f] = (factions[f] ?? 0) + n;
  return <section className="panel section ravens-eye"><h1>Raven&apos;s Eye</h1>
    <p className="muted">Platform numbers by UTC day. Finished days are kept once; today is so far. Watch hours and peaks count real (Counted and Trusted) viewers only. Chat counts exist from the day this started, since chat expires after 7 days.</p>
    <div className="row wrap" role="group" aria-label="Range">{[7, 30, 90].map(n => <button key={n} type="button" className={n === range ? "small" : "small quiet"} aria-pressed={n === range} onClick={() => setRange(n)}>{n} days</button>)}</div>
    {message && <p role="alert" className="form-message error">{message}</p>}
    {!days ? <p className="loading">Loading…</p> : <>
      <div className="stat-grid">{METRICS.map(([key, label, how]) => {
        const values = days.map(d => Number(d.stats[key]) || 0);
        const total = how === "sum" ? values.reduce((a, b) => a + b, 0) : Math.max(0, ...values);
        return <div key={key} className="stat-tile">
          <span className="muted small">{label}{how === "max" ? " (best day)" : ""}</span>
          <strong>{round(total).toLocaleString()}</strong>
          <Spark values={values} label={label} />
        </div>;
      })}</div>
      <h2>Money by kind</h2>
      <div className="table-scroll"><table className="table"><thead><tr><th scope="col">Kind</th><th scope="col">Transactions</th><th scope="col">Valor moved</th><th scope="col">USD moved</th></tr></thead>
        <tbody>{Object.entries(money).sort((a, b) => b[1].usd_cents - a[1].usd_cents).map(([kind, m]) => <tr key={kind}><th scope="row">{kind.replaceAll("_", " ")}</th><td>{m.count}</td><td>{m.valor.toLocaleString()}</td><td>{usd(m.usd_cents)}</td></tr>)}</tbody></table></div>
      <div className="row wrap top-lists">
        <section><h2>Top channels by watch hours</h2><ol>{combine(days, s => s.top_channels, "username", "watch_hours").map(([name, h]) => <li key={name}>{name} · {round(h)} h</li>)}</ol></section>
        <section><h2>Top categories by broadcast hours</h2><ol>{combine(days, s => s.top_categories, "category", "broadcast_hours").map(([name, h]) => <li key={name}>{name} · {round(h)} h</li>)}</ol></section>
        <section><h2>Faction joins</h2><ul>{Object.entries(factions).map(([f, n]) => <li key={f}>{f[0].toUpperCase() + f.slice(1)} · {n}</li>)}</ul></section>
      </div>
      {streams && <StreamsByCategory streams={streams} range={range} plays={plays} setPlays={setPlays} />}
    </>}
  </section>;
}
