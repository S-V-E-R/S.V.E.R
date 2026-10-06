import type { Chip } from "./types";
export type Video = { id: string; kind: "VOD" | "HIGHLIGHT" | "CLIP"; status: string; approval: string; visibility: string; recording: boolean; mature: boolean; title: string; category: string | null; faction: string | null; created_at: string; started_at: string; duration_ms: number; views: number; expires_at: string | null };
export type Chapter = { id: string; offset_ms: number; label: string; source: string };
export type VideoPage = { video: Video; channel: Chip; live: boolean; chapters: Chapter[]; signed_in: boolean; is_owner: boolean; can_manage: boolean; can_download: boolean; can_highlight: boolean; can_delete: boolean; chat_replay: boolean; clip_permission: string; playback: string; thumbnail: string | null };
export type VideoCard = { video: Video; thumbnail: string | null; channel?: Chip };
export function duration(ms: number) { const s = Math.max(0, Math.floor(ms / 1000)); return s >= 3600 ? `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}` : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; }
export const videoPath = (v: Video) => `/${v.kind === "CLIP" ? "clips" : "videos"}/${v.id}`;
