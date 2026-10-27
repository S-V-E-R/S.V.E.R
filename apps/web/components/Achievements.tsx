import { apiGet } from "../lib/server-api";
import { CREATOR_TIERS } from "../lib/types";

export type Achievement = { key: string; name: string; description: string; category: string; band: number; streamer: boolean; earned_at: string | null; rarity: number; progress?: number; need?: number };

// Mirrors CATEGORIES in apps/api/crates/sver/src/achievements.rs.
const CATEGORIES: [string, string][] = [["community", "Community"], ["scouting", "Scouting"], ["orders", "Orders and loyalty"], ["levels", "Levels"], ["support", "Support given"],
  ["streaming", "Streaming"], ["audience", "Audience"], ["raids", "Raids"], ["subscribers", "Subscribers"], ["dedication", "Tiers and dedication"]];
const LEVEL_BANDS = ["Level 1+", "Level 25+", "Level 50+", "Level 100+"];
/** Streamer bands follow the creator tiers; viewer bands follow account level. */
const bandName = (a: Achievement) => (a.streamer ? CREATOR_TIERS : LEVEL_BANDS)[a.band];

/** One achievement: earned with its date, or locked with progress toward it; both show band and rarity. */
export function AchievementItem({ a, showcased }: { a: Achievement; showcased?: boolean }) {
  const meta = <span className="muted small"> · {bandName(a)} · {a.rarity}% of accounts</span>;
  if (a.earned_at) return <li className={showcased ? "showcased" : undefined} title={a.description}>
    <strong>{a.name}</strong> <span className="muted small">{a.description} · {new Date(a.earned_at).toLocaleDateString()}</span>{meta}
  </li>;
  return <li className="locked">
    <span><strong>{a.name}</strong> <span className="muted small">{a.description}</span>{meta}</span>
    <span className="achievement-progress"><progress max={a.need} value={a.progress} aria-label={`${a.name} progress`} /><span className="small">{(a.progress ?? 0).toLocaleString()} / {(a.need ?? 0).toLocaleString()}</span></span>
  </li>;
}

/** Achievements under their category headings, in band order (closest to done first within a band). */
export function AchievementGroups({ list, showcased }: { list: Achievement[]; showcased?: Set<string> }) {
  const ratio = (a: Achievement) => (a.progress ?? 0) / (a.need ?? 1);
  return <>{CATEGORIES.map(([key, label]) => {
    const items = list.filter(a => a.category === key).sort((a, b) => a.band - b.band || Number(showcased?.has(b.key)) - Number(showcased?.has(a.key)) || ratio(b) - ratio(a));
    return items.length > 0 && <div key={key} className="achievement-group"><h3>{label}</h3>
      <ul className="achievement-list">{items.map(a => <AchievementItem key={a.key} a={a} showcased={showcased?.has(a.key)} />)}</ul></div>;
  })}</>;
}

/** A channel's achievements (docs/ACHIEVEMENTS.md): earned from records, grouped by category;
 *  the owner also sees the locked ones with progress. */
export async function Achievements({ username }: { username: string }) {
  const { data } = await apiGet<{ achievements: Achievement[]; locked: Achievement[] | null; showcase: { key: string }[]; total: number }>(`/api/users/${encodeURIComponent(username)}/achievements`);
  if (!data || (!data.achievements.length && !data.locked)) return null;
  const shown = new Set(data.showcase.map(s => s.key));
  const locked = data.locked ?? [];
  return <section className="panel section achievements" aria-labelledby="achievements-title">
    <div className="row between"><h2 id="achievements-title">Achievements</h2><span className="muted small">{data.achievements.length} of {data.total}</span></div>
    <AchievementGroups list={data.achievements} showcased={shown} />
    {locked.length > 0 && <details className="achievements-locked">
      <summary>Still to earn ({locked.length}) <span className="muted small">only you see this</span></summary>
      <AchievementGroups list={locked} />
    </details>}
  </section>;
}
