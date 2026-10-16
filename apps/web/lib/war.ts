import type { Faction } from "./factions";
export type Genre = { id: string; name: string; home: Faction | null; holder: Faction | null; neighbors: string[]; position: number; map?: { q: number; r: number } | null; capital?: boolean; scores: { faction: Faction; influence: number; score: number }[] };
type Window = { starts_at: string; ends_at: string };
export type War = {
  season: { number: number; starts_at: string; ends_at: string; next_starts_at: string; finished: boolean; winners: Faction[] } | null;
  week: { id: number; ends_at: string; completed: boolean } | null;
  /** Skirmish Hour (docs/FACTIONS.md): influence counts double, under the usual caps. */
  skirmish?: { multiplier: number; active: Window | null; next: Window | null; raid_night?: { active: Window | null; next: Window | null }; spotlight?: { genre: string; name: string | null } | null; hill?: { genre: string; name: string; holder: Faction | null; bonus: number } | null };
  genres: Genre[];
  scoreboard: { faction: Faction; territories: number; genre_weeks: number }[];
  previous_winners: Faction[];
  history: { ends_at: string; genres: Genre[] }[];
};
/** A day, for season dates: seasons end on a day, and the minute they were created means nothing to a reader. */
export const utcDay = (iso: string) => new Date(iso).toLocaleDateString("en-US", { dateStyle: "medium", timeZone: "UTC" });
export const utcDate = (iso: string) => new Date(iso).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short", timeZone: "UTC" }) + " UTC";
export function contest(genre: Genre) {
  const scores = [...genre.scores].sort((a, b) => b.score - a.score);
  const top = scores[0];
  const second = scores[1];
  const lead = top && second && top.score > 0 ? (top.score - second.score) / top.score * 100 : 0;
  return { top, lead, contested: !!top?.score && (top.faction !== genre.holder || (second.score > 0 && lead < 10)) };
}
