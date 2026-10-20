import { CREATOR_TIERS } from "../lib/types";

// Mirrors apps/api/crates/sver/src/tiers.rs (SPLITS, VOD_HOURS, REQUIRED) and videos/mod.rs
// (HIGHLIGHT_HOURS); change them together.
const SHARES = [65, 70, 75, 80];
const VODS = ["24 hours", "48 hours", "72 hours", "7 days"];
const HIGHLIGHTS = [10, 25, 50, 100];
export const TIER_METRICS = [["streams", "Streams"], ["hours", "Stream hours"], ["avg_viewers", "Average viewers"], ["followers", "Followers"], ["subscribers", "Subscribers"], ["unique_viewers", "Unique viewers"], ["days_active", "Days active"]] as const;
const REQUIRED = [[0, 0, 0, 0, 0, 0, 0], [8, 15, 5, 150, 0, 0, 10], [30, 80, 20, 800, 8, 300, 40], [120, 300, 60, 2_500, 40, 3_000, 120]];

/** The full creator tier ladder (docs/SUPPORT.md "Creator tiers"); `current` highlights the viewer's own tier. */
export default function CreatorTierTable({ current }: { current?: number }) {
  const row = (label: string, cells: string[]) => <tr><th scope="row">{label}</th>{cells.map((cell, i) => <td key={CREATOR_TIERS[i]}>{cell}</td>)}</tr>;
  return <div className="table-scroll"><table className="open-table tier-table">
    <caption className="sr-only">Creator tiers{current !== undefined && `, your tier is ${CREATOR_TIERS[current]}`}</caption>
    <colgroup><col />{CREATOR_TIERS.map((name, i) => <col key={name} className={i === current ? "tier-current" : undefined} />)}</colgroup>
    <thead><tr><th scope="col">Tier</th>{CREATOR_TIERS.map((name, i) => <th key={name} scope="col" aria-current={i === current || undefined}>{name}{i === current && <span className="tier-you"> · You</span>}</th>)}</tr></thead>
    <tbody>
      <tr><th scope="colgroup" colSpan={5}>What you get</th></tr>
      {row("Subscriptions and gift subs", SHARES.map(s => `${s}%`))}
      {row("Valor spent on you", SHARES.map(() => "0.8¢ each"))}
      {row("VOD retention", VODS)}
      {row("Highlight time", HIGHLIGHTS.map(h => `${h} hours`))}
    </tbody>
    <tbody>
      <tr><th scope="colgroup" colSpan={5}>What it takes</th></tr>
      {TIER_METRICS.map(([key, label], r) => <tr key={key}><th scope="row">{label}</th>{REQUIRED.map((req, i) => <td key={CREATOR_TIERS[i]}>{req[r].toLocaleString("en-US")}</td>)}</tr>)}
    </tbody>
  </table></div>;
}
