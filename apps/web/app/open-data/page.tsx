import SitePage from "../../components/SitePage";
import { apiGet } from "../../lib/server-api";

export const metadata = { title: "Open data | S.V.E.R", description: "How S.V.E.R is doing and where the money goes, week by week.", alternates: { canonical: "https://sver.tv/open-data" } };

type Row = { period: string; from?: string; complete: boolean; [metric: string]: number | string | boolean | null | undefined };
type Data = { weekly: Row[]; monthly: Row[]; notes: Record<string, string>; updated: string | null };
/** `under` is a second figure drawn beneath this one in its own color (the 24/7 channel's hours under creators'). */
type Metric = { key: string; title: string; unit?: string; money?: boolean; withheld?: string; under?: { key: string; title: string } };

const WEEKLY: Metric[] = [
  { key: "accounts_total", title: "Accounts" }, { key: "accounts_new", title: "New accounts" }, { key: "channels_streamed", title: "Channels that streamed" },
  { key: "hours_streamed", title: "Hours streamed", unit: " hours", withheld: "hidden until 10 creators stream", under: { key: "hours_always_on", title: "24/7 channel" } }, { key: "rotation_reach", title: "Streams the fair rotation put first", unit: "%" },
  { key: "faction_myria", title: "Myria members" }, { key: "faction_aetheron", title: "Aetheron members" }, { key: "faction_glint", title: "Glint members" },
];
const MONTHLY: Metric[] = [["creators_cents", "Creators' share"], ["platform_cents", "S.V.E.R's share"], ["payouts_cents", "Payouts sent"], ["wall_cents", "Wall seats sold"], ["ads_cents", "Ad revenue"], ["supporters_cents", "Platform supporters"]].map(([key, title]) => ({ key, title, money: true }));

const dollars = (cents: number) => `$${(cents / 100).toLocaleString("en-US", { maximumFractionDigits: 0 })}`;
const format = (m: Metric, v: number) => m.money ? dollars(v) : `${v.toLocaleString("en-US")}${m.unit ?? ""}`;
const value = (r: Row, key: string) => typeof r[key] === "number" ? r[key] as number : null;
const shown = (m: Metric, v: number | null) => v === null ? m.withheld ?? "fewer than 10" : format(m, v);

/** Monday of an ISO week ("2026-W42"). */
function monday(period: string) {
  const [year, week] = period.split("-W").map(Number);
  const jan4 = new Date(Date.UTC(year, 0, 4));
  return new Date(jan4.getTime() + ((week - 1) * 7 - (jan4.getUTCDay() + 6) % 7) * 86_400_000);
}
const monthName = (period: string) => new Date(`${period}-01T00:00:00Z`).toLocaleString("en-US", { month: "short", year: "numeric", timeZone: "UTC" });
/** "Week of Oct 12", "Oct 2026", or "Jan 2026 – Mar 2026" for months combined to protect pay. */
function periodLabel(r: Row) {
  if (r.period.includes("W")) return `Week of ${monday(r.period).toLocaleString("en-US", { month: "short", day: "numeric", timeZone: "UTC" })}`;
  return r.from && r.from !== r.period ? `${monthName(r.from)} – ${monthName(r.period)}` : monthName(r.period);
}
const tick = (r: Row) => r.period.includes("W")
  ? monday(r.period).toLocaleString("en-US", { month: "numeric", day: "numeric", timeZone: "UTC" })
  : new Date(`${r.period}-01T00:00:00Z`).toLocaleString("en-US", { month: "short", timeZone: "UTC" });

/** The latest finished period's figure and its change from the one before. */
function Headline({ rows, metric, unit }: { rows: Row[]; metric: Metric; unit: string }) {
  const done = rows.filter(r => r.complete);
  const last = done.at(-1), before = done.at(-2);
  if (!last) return null;
  // A combined stretch of months isn't comparable with one month, so no change is shown against it.
  const combined = (r: Row) => !!r.from && r.from !== r.period;
  const now = value(last, metric.key), prev = before && !combined(before) && !combined(last) ? value(before, metric.key) : null;
  const change = now !== null && prev !== null ? Math.round((now - prev) * 10) / 10 : null;
  const week = periodLabel(last).replace("Week of", "week of");
  const under = metric.under ? value(last, metric.under.key) : null;
  // Creators' hours hidden: lead with the 24/7 channel's, which is S.V.E.R's own and always shown.
  if (metric.under && now === null && under !== null) return <p className="open-headline">
    <strong>{format(metric, under)}</strong>
    <span className="muted small"> on the {metric.under.title}, {week}; creators&apos; hours are {metric.withheld}</span>
  </p>;
  return <p className="open-headline">
    <strong>{now === null ? "Fewer than 10" : format(metric, now)}</strong>
    <span className="muted small"> {metric.under && "by creators, "}{week}{change ? <>, {change > 0 ? "up" : "down"} {format(metric, Math.abs(change))} on the {unit} before</> : change === 0 ? <>, same as the {unit} before</> : null}{under !== null && <>; plus {format(metric, under)} on the {metric.under!.title}</>}</span>
  </p>;
}

/** One figure over time: a bar per period, a flat stub where it's withheld, a faded bar for the period still in progress. */
function Bars({ rows, metric }: { rows: Row[]; metric: Metric }) {
  const values = rows.map(r => value(r, metric.key));
  const unders = rows.map(r => metric.under ? value(r, metric.under.key) ?? 0 : 0);
  const max = Math.max(1, ...values.map((v, i) => (v ?? 0) + unders[i]));
  const describe = (r: Row, v: number | null, i = rows.indexOf(r)) => `${periodLabel(r)}: ${shown(metric, v)}${metric.under ? `, ${metric.under.title} ${format(metric, unders[i])}` : ""}${r.complete ? "" : " so far"}`;
  // At least 12 slots wide, so a chart with few bars keeps the same scale as the rest.
  const width = Math.max(rows.length, 12) * 28, every = Math.ceil(rows.length / 4);
  return <svg className="open-chart" viewBox={`0 0 ${width} 132`} role="img" aria-label={rows.map((r, i) => describe(r, values[i])).join("; ")}>
    <text x={2} y={9} className="open-tick">{format(metric, max)}</text>
    <line x1={0} x2={width} y1={12} y2={12} className="open-gridline" />
    {metric.under && <defs><pattern id={`open-under-${metric.key}`} width={4} height={4} patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width={4} height={4} className="open-bar-under" /><rect width={1.5} height={4} className="open-bar-under-stripe" /></pattern></defs>}
    {rows.map((r, i) => {
      const v = values[i];
      const u = unders[i] > 0 ? Math.max(2, (unders[i] / max) * 100) : 0;
      const h = v === null ? 0 : Math.max(2, (v / max) * 100);
      const partial = r.complete ? "" : " open-bar-partial";
      return <g key={r.period}>
        <title>{describe(r, v)}</title>
        {u > 0 && <rect x={i * 28 + 4} y={112 - u} width={20} height={u} fill={`url(#open-under-${metric.key})`} className={partial} />}
        {v === null ? <rect x={i * 28 + 4} y={108 - u} width={20} height={4} className="open-bar-withheld" /> : <rect x={i * 28 + 4} y={112 - u - h} width={20} height={h} className={`open-bar${partial}`} />}
        {(rows.length - 1 - i) % every === 0 && <text x={i * 28 + 14} y={126} textAnchor="middle" className="open-tick">{tick(r)}</text>}
      </g>;
    })}
  </svg>;
}

function Charts({ rows, metrics, notes, unit }: { rows: Row[]; metrics: Metric[]; notes: Record<string, string>; unit: string }) {
  return <div className="open-grid">{metrics.map(m => <section key={m.key} className="panel">
    <h3>{m.title}</h3>
    <Headline rows={rows} metric={m} unit={unit} />
    <Bars rows={rows} metric={m} />
    {m.under && <p className="open-legend small"><span className="open-swatch" /> Creators <span className="open-swatch open-swatch-under" /> {m.under.title} <span className="open-swatch open-swatch-withheld" /> Hidden</p>}
    <p className="muted small">{notes[m.key]}.{m.under && <> {notes[m.under.key]}.</>}</p>
  </section>)}</div>;
}

/** The same figures as a table, newest first. */
function Table({ rows, metrics: shownMetrics }: { rows: Row[]; metrics: Metric[] }) {
  const metrics = shownMetrics.flatMap(m => m.under ? [{ ...m, title: `${m.title} (creators)` }, { ...m, ...m.under, under: undefined, withheld: undefined }] : [m]);
  return <details className="open-table">
    <summary>Show as a table</summary>
    <div className="table-scroll"><table>
      <thead><tr><th scope="col">Period</th>{metrics.map(m => <th key={m.key} scope="col">{m.title}</th>)}</tr></thead>
      <tbody>{[...rows].reverse().map(r => <tr key={r.period}>
        <th scope="row">{periodLabel(r)}{r.complete ? "" : " (so far)"}</th>
        {metrics.map(m => { const v = value(r, m.key); return <td key={m.key}>{shown(m, v)}</td>; })}
      </tr>)}</tbody>
    </table></div>
  </details>;
}

/** /open-data (docs/CHANNEL_ADDITIONS.md "Open data"): figures written nightly; no per-person numbers. */
export default async function OpenData() {
  const data = (await apiGet<Data>("/api/open-data")).data;
  return <SitePage path="/open-data" title="Open data" intro="How S.V.E.R is doing and where the money goes. There are no per-person or per-channel figures: a weekly count under 10 shows as fewer than 10 (faction members are shown exactly, since each faction's member directory is public), and a month in which fewer than 10 creators were paid is combined with the next." wide>
    {!data ? <p role="alert">The figures couldn&apos;t be loaded. Please try again later.</p> : <>
      {data.updated && <p className="muted small">Updated {new Date(data.updated).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short", timeZone: "UTC" })} UTC, once a day. Weeks run Monday to Sunday (UTC); a faded bar is the week or month still in progress.</p>}
      <h2>Weekly <a className="small" href="/api/open-data/weekly.csv" download>Download CSV</a></h2>
      <Charts rows={data.weekly} metrics={WEEKLY} notes={data.notes} unit="week" />
      <Table rows={data.weekly} metrics={WEEKLY} />
      <h2>Monthly <a className="small" href="/api/open-data/monthly.csv" download>Download CSV</a></h2>
      {data.monthly.length === 0 ? <p className="muted">Monthly money figures appear once 10 creators have been paid in a month; until then each month is combined with the next.</p> : <>
        <Charts rows={data.monthly} metrics={MONTHLY} notes={data.notes} unit="period" />
        <Table rows={data.monthly} metrics={MONTHLY} />
      </>}
      <p className="muted small">The CSVs hold the same numbers: money in cents, withheld cells as &quot;fewer than 10&quot;, and a <code>complete</code> column that is false for the period still in progress. A monthly row covers <code>from</code> through <code>period</code>.</p>
    </>}
  </SitePage>;
}
