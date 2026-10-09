import SitePage from "../../components/SitePage";
import { apiGet } from "../../lib/server-api";

export const metadata = { title: "Open data | S.V.E.R", description: "How S.V.E.R is doing and where the money goes, week by week." };

type Row = { period: string; from?: string; [metric: string]: number | string | null | undefined };
type Data = { weekly: Row[]; monthly: Row[]; notes: Record<string, string> };

const WEEKLY: [string, string, string][] = [
  ["accounts_total", "Accounts", ""], ["accounts_new", "New accounts", ""], ["channels_streamed", "Channels that streamed", ""],
  ["hours_streamed", "Hours streamed", ""], ["rotation_reach", "Streams the fair rotation put first", "%"],
  ["faction_myria", "Myria members", ""], ["faction_aetheron", "Aetheron members", ""], ["faction_glint", "Glint members", ""],
];
const MONTHLY: [string, string][] = [["creators_cents", "Creators' share"], ["platform_cents", "S.V.E.R's share"], ["payouts_cents", "Payouts sent"]];
const dollars = (cents: number) => `$${(cents / 100).toLocaleString(undefined, { maximumFractionDigits: 0 })}`;

/** One figure over time: a bar per period, "fewer than 10" where the value is withheld. */
function Bars({ rows, metric, unit = "", money = false }: { rows: Row[]; metric: string; unit?: string; money?: boolean }) {
  const values = rows.map(r => (typeof r[metric] === "number" ? r[metric] as number : null));
  const max = Math.max(1, ...values.map(v => v ?? 0));
  const label = (v: number) => money ? dollars(v) : `${v.toLocaleString()}${unit}`;
  return <svg className="open-chart" viewBox={`0 0 ${rows.length * 28} 120`} role="img" aria-label={rows.map((r, i) => `${r.period}: ${values[i] === null ? "fewer than 10" : label(values[i]!)}`).join("; ")}>
    {rows.map((r, i) => {
      const v = values[i];
      const h = v === null ? 0 : Math.max(2, (v / max) * 96);
      return <g key={r.period}>
        {v === null ? <rect x={i * 28 + 4} y={100} width={20} height={4} className="open-bar-withheld" /> : <rect x={i * 28 + 4} y={104 - h} width={20} height={h} className="open-bar"><title>{`${r.period}: ${label(v)}`}</title></rect>}
        <text x={i * 28 + 14} y={118} textAnchor="middle" className="open-tick">{r.period.slice(-2)}</text>
      </g>;
    })}
  </svg>;
}

/** /open-data (docs/CHANNEL_ADDITIONS.md "Open data"): figures written nightly; no per-person numbers. */
export default async function OpenData() {
  const data = (await apiGet<Data>("/api/open-data")).data;
  return <SitePage path="/open-data" title="Open data" intro="How S.V.E.R is doing and where the money goes. Numbers update nightly. There are no per-person or per-channel figures: a weekly figure under 10 shows as fewer than 10, and a month in which fewer than 10 creators were paid is combined with the next." wide>
    {!data ? <p role="alert">The figures couldn&apos;t be loaded. Please try again later.</p> : <>
      <h2>Weekly <a className="small" href="/api/open-data/weekly.csv">CSV</a></h2>
      <div className="open-grid">{WEEKLY.map(([metric, title, unit]) => <section key={metric} className="panel">
        <h3>{title}</h3>
        <Bars rows={data.weekly} metric={metric} unit={unit} />
        <p className="muted small">{data.notes[metric]}. Bars are ISO weeks; a flat stub means fewer than 10.</p>
      </section>)}</div>
      <h2>Monthly <a className="small" href="/api/open-data/monthly.csv">CSV</a></h2>
      {data.monthly.length === 0 ? <p className="muted">Monthly money figures appear once 10 creators have been paid in a month; until then each month is combined with the next.</p>
        : <div className="open-grid">{MONTHLY.map(([metric, title]) => <section key={metric} className="panel">
          <h3>{title}</h3>
          <Bars rows={data.monthly} metric={metric} money />
          <p className="muted small">{data.notes[metric]}.</p>
        </section>)}</div>}
    </>}
  </SitePage>;
}
