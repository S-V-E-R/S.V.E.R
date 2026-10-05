import type { Sizes } from "../../lib/types";

export type LiveCard = {
  username: string; display_name: string; avatar: Sizes; faction: string | null;
  title: string; category: string | null; genre: string | null; started_at: string; viewers: number;
  broadcast_id?: string; category_id?: string | null; thumbnail?: string | null;
  /** Discovery label: "New creator" or "Returning creator". */
  label?: string | null; fresh?: boolean;
};
export type Recent = { user: { username: string | null; display_name: string; avatar: Sizes }; ended_at: string };

/** Uptime like "1h 05m", or "12 min" under an hour. */
export function uptime(startedAt: string, now = Date.now()): string {
  const minutes = Math.max(0, Math.floor((now - new Date(startedAt).getTime()) / 60000));
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}

// Dark scene gradients for cards until stream thumbnails exist (docs/DESIGN.md, "Home").
const SCENES = [["#24384A", "#0E141C"], ["#3A2A1E", "#120D0A"], ["#1F3A2E", "#0B1410"], ["#38243F", "#120C16"], ["#2E3442", "#0E1016"], ["#41301A", "#140F08"], ["#1E2F45", "#0A1018"], ["#3B1F26", "#130A0D"]];
export function scene(key: string, angle = 150): string {
  let h = 0;
  for (const ch of key) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  const [a, b] = SCENES[h % SCENES.length];
  return `linear-gradient(${angle}deg, ${a}, ${b})`;
}
