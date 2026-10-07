import type { Chip } from "./types";

/** Module 9 (docs/BEACONS.md): short vertical videos that lead to creators and live streams. */
export type Beacon = {
  id: string; source: "CLIP" | "UPLOAD"; status: "DRAFT" | "PROCESSING" | "READY" | "PUBLISHED" | "REMOVED" | "FAILED" | "DELETING" | "DELETED";
  failure: string | null; publish: boolean; hidden: boolean; title: string; category: string | null; genre: string | null; mature: boolean;
  clip_id: string | null; duration_ms: number; views: number; likes: number; created_at: string; published_at: string | null;
};
export type BeaconItem = {
  beacon: Beacon; channel: Chip; clipper: Chip | null; liked: boolean;
  playback: { hd: string; sd: string } | null; thumbnail: string | null; charity: { name: string; url: string | null } | null;
  signed_in?: boolean; is_owner?: boolean;
  stats?: { completions: number; follows: number; live_joins: number };
};
export type BeaconFeedPage = { items: BeaconItem[]; has_more: boolean; next: number; live: Chip[]; signed_in: boolean };

export const beaconPath = (id: string) => `/beacons/${id}`;

/** Compact counts for the rail: 1.2K, 3.4M. */
export function compact(n: number) {
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

/** One browser session id shared with the live and recorded players (viewer integrity). */
export function browserId() {
  try {
    const id = localStorage.getItem("sver-browser") || crypto.randomUUID();
    localStorage.setItem("sver-browser", id);
    return id;
  } catch {
    return crypto.randomUUID();
  }
}
