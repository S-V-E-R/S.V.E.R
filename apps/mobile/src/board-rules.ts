/** CrowdSync board and counter rules shared by src/crowd.tsx and its test (no React Native imports). */

export type Control = { id: string; kind: "button" | "label" | "text" | "goal" | "joystick" | "rally"; label: string; cost: number; cooldown_seconds: number; per_stream_limit: number | null; audience: "everyone" | "followers" | "subscribers" | "moderators"; target: number | null; width: number; confirm?: boolean };
export type BoardView = { board: { screens: { name: string; controls: Control[] }[] } | null; version: number; disabled: boolean; starting?: boolean; screen?: number | null; live: boolean; goals: Record<string, number>; used: Record<string, number>; last_press: Record<string, string>; balance: number | null; signed_in: boolean; state?: Record<string, { label?: string; disabled?: boolean }> };
export type Counter = { id: string; kind: "shiny" | "deaths" | "tally" | "custom"; label: string; value: number; extra: number; odds: number | null };

/** Why a control can't be pressed right now, or null when it can (the website's rules, apps/web/components/Board.tsx). */
export function blocker(c: Control, view: Pick<BoardView, "last_press" | "state" | "used" | "goals" | "balance">, now: number): string | null {
  const last = view.last_press[c.id];
  const ready = last ? Date.parse(last) + c.cooldown_seconds * 1000 : 0;
  if (c.cooldown_seconds && ready > now) return `${Math.ceil((ready - now) / 1000)}s`;
  if (view.state?.[c.id]?.disabled) return "Unavailable";
  if (c.per_stream_limit && (view.used[c.id] ?? 0) >= c.per_stream_limit) return "Limit reached";
  if (c.target && (view.goals[c.id] ?? 0) >= c.target) return "Complete";
  if (c.cost && (view.balance ?? 0) < c.cost) return "Not enough";
  return null;
}

/** How a counter reads: "12", "4–2", or a shiny hunt's encounters, phase and chance so far (apps/web/components/Crowd.tsx). */
export function counterText(c: Counter) {
  if (c.kind === "tally") return `${c.value}–${c.extra}`;
  if (c.kind === "shiny") {
    const chance = c.odds ? ` · ${((1 - Math.pow(1 - 1 / c.odds, c.value)) * 100).toFixed(1)}% by now (1 in ${c.odds.toLocaleString()})` : "";
    return `${c.value.toLocaleString()} encounters · phase ${c.extra + 1}${chance}`;
  }
  return c.value.toLocaleString();
}
