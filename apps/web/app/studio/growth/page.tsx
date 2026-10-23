import Link from "next/link";
import { Section } from "../../../components/Form";
import { StatusMark } from "../../../components/shell/Icons";
import CreatorTierTable, { TIER_METRICS } from "../../../components/CreatorTiers";
import { AchievementGroups, type Achievement } from "../../../components/Achievements";
import { apiGet } from "../../../lib/server-api";

type Metrics = { streams: number; hours: number; avg_viewers: number; followers: number; subscribers: number; unique_viewers: number; days_active: number };
type Tier = { tier: number; name: string; split: number; metrics: Metrics; next: { name: string; split: number; requirements: Metrics } | null; held: boolean };
type Week = { streams: number; hours: number; avg_viewers: number; peak_viewers: number; followers: number; subscribers: number };
type Growth = { this_week: Week; last_week: Week; unique_viewers: number; achievements: Achievement[] };
const WEEK: [keyof Week, string][] = [["streams", "Streams"], ["hours", "Hours streamed"], ["avg_viewers", "Average viewers"], ["peak_viewers", "Peak viewers"], ["followers", "New verified followers"], ["subscribers", "New subscribers"]];

function change(now: number, before: number) {
  const d = Math.round((now - before) * 10) / 10;
  if (d === 0) return <span className="muted">no change</span>;
  return <span className={d > 0 ? "growth-up" : "growth-down"}>{d > 0 ? "▲" : "▼"} {Math.abs(d).toLocaleString()}</span>;
}

/** Creator Studio → Growth (docs/ACHIEVEMENTS.md "Streamer growth"): tier progress, this week
 *  against last week, and streamer achievements with progress. */
export default async function StudioGrowth() {
  const [{ data: tier }, { data: growth }] = await Promise.all([apiGet<Tier>("/api/me/tier"), apiGet<Growth>("/api/me/growth")]);
  if (!tier || !growth) return <><h1>Growth</h1><p role="alert" className="form-message error">Growth couldn&apos;t load. Try again in a moment.</p></>;
  const next = tier.next;
  const earned = growth.achievements.filter(a => a.earned_at);
  const locked = growth.achievements.filter(a => !a.earned_at);
  return <><h1>Growth</h1>
    <Section title={`Creator tier · ${tier.name}`} intro={`You keep ${tier.split}% of subscriptions and gift subs. Tiers are checked every Monday against the last 90 days and never go down; viewer numbers count Trusted viewers only.`}>
      {tier.held && <p className="form-message">Promotion is paused while a staff review of your streams is open.</p>}
      {next ? <table className="table"><thead><tr><th>Last 90 days</th><th>You</th><th>{next.name} ({next.split}%)</th></tr></thead>
        <tbody>{TIER_METRICS.map(([key, label]) => <tr key={key}><td>{label}</td><td>{tier.metrics[key].toLocaleString()}</td><td>{tier.metrics[key] >= next.requirements[key] && <StatusMark ok />}{next.requirements[key].toLocaleString()}</td></tr>)}</tbody></table>
        : <p>You&apos;re at the top tier.</p>}
      <CreatorTierTable current={tier.tier} />
      <p><Link href="/help/creator-tiers">See every tier</Link> and how they work.</p>
    </Section>
    <Section title="This week" intro="The last 7 days against the 7 before them.">
      <table className="table"><thead><tr><th></th><th>Last 7 days</th><th>7 days before</th><th>Change</th></tr></thead>
        <tbody>{WEEK.map(([key, label]) => <tr key={key}><td>{label}</td><td>{growth.this_week[key].toLocaleString()}</td><td>{growth.last_week[key].toLocaleString()}</td><td>{change(growth.this_week[key], growth.last_week[key])}</td></tr>)}</tbody></table>
      <p className="muted">{growth.unique_viewers.toLocaleString()} unique Trusted viewers in the last 7 days.</p>
    </Section>
    <Section title="Streamer achievements" intro={`${earned.length} of ${growth.achievements.length} earned. Each creator tier has at least 3 in every category; the ones still to earn are below, by tier.`}>
      <AchievementGroups list={locked} />
      {earned.length > 0 && <details className="achievements-locked"><summary>Earned ({earned.length})</summary>
        <AchievementGroups list={earned} /></details>}
    </Section>
  </>;
}
