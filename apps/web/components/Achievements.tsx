import { apiGet } from "../lib/server-api";

type Earned = { key: string; name: string; description: string; earned_at: string };

/** A channel's achievements (docs/ACHIEVEMENTS.md): earned from records, with the showcase first. */
export async function Achievements({ username }: { username: string }) {
  const { data } = await apiGet<{ achievements: Earned[]; showcase: { key: string }[]; total: number }>(`/api/users/${encodeURIComponent(username)}/achievements`);
  if (!data?.achievements.length) return null;
  const shown = new Set(data.showcase.map(s => s.key));
  const list = [...data.achievements].sort((a, b) => Number(shown.has(b.key)) - Number(shown.has(a.key)));
  return <section className="panel section achievements" aria-labelledby="achievements-title">
    <div className="row between"><h2 id="achievements-title">Achievements</h2><span className="muted small">{data.achievements.length} of {data.total}</span></div>
    <ul className="achievement-list">{list.map(a => <li key={a.key} className={shown.has(a.key) ? "showcased" : undefined} title={a.description}>
      <strong>{a.name}</strong> <span className="muted small">{a.description} · {new Date(a.earned_at).toLocaleDateString()}</span>
    </li>)}</ul>
  </section>;
}
