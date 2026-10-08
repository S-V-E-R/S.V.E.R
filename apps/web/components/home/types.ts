import type { Sizes } from "../../lib/types";

export type LiveCard = {
  username: string; display_name: string; avatar: Sizes; faction: string | null;
  title: string; category: string | null; genre: string | null; started_at: string; viewers: number;
  broadcast_id?: string; category_id?: string | null; thumbnail?: string | null;
  /** Discovery label: "New creator" or "Returning creator". */
  label?: string | null; fresh?: boolean;
  /** Labeled mature (docs/CHANNEL_ADDITIONS.md). */
  mature?: boolean;
};
export type Recent = { user: { username: string | null; display_name: string; avatar: Sizes }; ended_at: string };

/** Uptime like "1h 05m", or "12 min" under an hour. */
export function uptime(startedAt: string, now = Date.now()): string {
  const minutes = Math.max(0, Math.floor((now - new Date(startedAt).getTime()) / 60000));
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}
