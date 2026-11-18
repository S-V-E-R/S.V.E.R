/** One wording for counts and times everywhere in the app. */

const minutesSince = (iso: string, now: number) => Math.max(0, Math.floor((now - new Date(iso).getTime()) / 60000));

/** "1,234 watching". */
export function watching(n: number): string {
  return `${n.toLocaleString()} watching`;
}

/** Time live: "12m", "3h 40m", or "2d 5h" for multi-day streams. */
export function uptime(startedAt: string, now = Date.now()): string {
  const m = minutesSince(startedAt, now);
  if (m < 60) return `${m}m`;
  if (m >= 1440) return `${Math.floor(m / 1440)}d ${Math.floor(m / 60) % 24}h`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** "just now", "5m ago", "2h ago", "3d ago". */
export function ago(iso: string, now = Date.now()): string {
  const m = minutesSince(iso, now);
  if (m < 1) return "just now";
  if (m < 60) return `${m}m ago`;
  if (m < 1440) return `${Math.floor(m / 60)}h ago`;
  return `${Math.floor(m / 1440)}d ago`;
}
