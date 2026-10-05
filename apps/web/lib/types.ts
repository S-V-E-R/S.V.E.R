import type { Faction } from "./factions";
export type Sizes = Record<string, string> | null;
export type Chip = { username: string | null; display_name: string; avatar: Sizes; linked: boolean; deleted: boolean; live?: boolean; faction?: Faction | null };
export type Link = { platform: string; url: string };
export type Song = { provider: "youtube" | "soundcloud"; media_id: string; title: string | null; artist: string | null; thumbnail: string | null; volume: number } | null;
export type Viewer = { signed_in: boolean; is_owner: boolean; following: boolean; alerts?: boolean; blocked: boolean; interaction_blocked: boolean };
export type Reply = { id: string; author: Chip; body: string | null; status: string; status_label: string | null; created_at: string; can_delete: boolean; can_report: boolean };
export type Post = Reply & { pinned_position: number | null; like_count: number; liked: boolean; reply_count: number; replies: Reply[]; more_replies: boolean; can_pin: boolean };
export type WallViewer = { can_post: boolean; reason: string | null; is_owner: boolean; can_react?: boolean };
export type Occurrence = { start_at: string; end_at: string; label: string; kind: "weekly" | "event"; live: boolean };
export type Channel = {
  channel: { username: string; display_name: string; bio: string; mood_emoji: string; status_text: string; avatar: Sizes; banner: Sizes; joined_at: string; follower_count: number; following_count: number; links: Link[]; song: Song; song_notice: string | null; live: boolean; faction: Faction | null; season_rewards: { season: number; faction: Faction; awarded_at: string; valor_pending: boolean }[] };
  tabs: { wall: boolean; schedule: boolean; about: boolean; fan_art: boolean };
  fan_art_enabled: boolean;
  /** Owner-editable header copy with defaults resolved (docs/PROFILES.md, P9). */
  header: { label: string | null; welcome: string | null; intro_title: string; intro_body: string; vibe: string };
  war_council: { members: { position: number; user: Chip; crown: boolean }[]; unavailable_count: number };
  wall_preview: { pinned: Post[]; latest: Post[]; viewer: WallViewer };
  schedule_next: { timezone: string | null; items: Occurrence[] };
  viewer: Viewer;
  redirect_to?: string;
};
export const platformNames: Record<string, string> = { twitch: "Twitch", youtube: "YouTube", kick: "Kick", tiktok: "TikTok", instagram: "Instagram", x: "X", bluesky: "Bluesky", discord: "Discord", facebook: "Facebook", patreon: "Patreon", kofi: "Ko-fi", fourthwall: "Fourthwall", website: "Website" };
/** Profile URL templates for the social-link handle field, as legacy built them (docs/PROFILES.md, "Social links"). */
export const linkTemplates: Record<string, (handle: string) => string> = {
  twitch: h => `https://twitch.tv/${h}`, youtube: h => `https://youtube.com/@${h}`, kick: h => `https://kick.com/${h}`, tiktok: h => `https://tiktok.com/@${h}`,
  instagram: h => `https://instagram.com/${h}`, x: h => `https://x.com/${h}`, bluesky: h => `https://bsky.app/profile/${h}`, discord: h => `https://discord.gg/${h}`,
  facebook: h => `https://facebook.com/${h}`, patreon: h => `https://patreon.com/${h}`, kofi: h => `https://ko-fi.com/${h}`, fourthwall: h => `https://${h}.4thwall.com`, website: h => `https://${h}`,
};
/** A typed URL (any scheme) is kept for the server to validate; a handle, with or without "@", becomes the platform's profile URL. */
export function linkUrl(platform: string, value: string) {
  const typed = value.trim();
  if (!typed || /^[a-z][a-z0-9+.-]*:/i.test(typed)) return typed;
  // "twitch.tv/name" (an address typed without https://) is an address, not a handle.
  if (platform === "website" || /^[a-z0-9-]+(\.[a-z0-9-]+)+\//i.test(typed)) return linkTemplates.website(typed.replace(/^\/+/, ""));
  return (linkTemplates[platform] || linkTemplates.website)(encodeURIComponent(typed.replace(/^@/, "")));
}
/** Host shown next to a social link, without "www.". */
export const linkHost = (url: string) => { try { return new URL(url).host.replace(/^www\./, ""); } catch { return ""; } };
export const followedOn = (iso: string) => new Date(iso).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
export const reasons: [string, string][] = [["spam", "Spam"], ["harassment", "Harassment or bullying"], ["hate", "Hate speech"], ["sexual", "Sexual content"], ["violence", "Violence or threats"], ["impersonation", "Impersonation"], ["private_information", "Private information"], ["copyright", "Copyright"], ["other", "Something else"]];
export const joined = (iso: string) => new Date(iso).toLocaleDateString("en-US", { month: "long", year: "numeric", timeZone: "UTC" });
/** One channel activity event (docs/PROFILES.md, P7). Kinds unknown to this build are not rendered. */
export type ActivityItem = { id: string; kind: string; created_at: string; subject: Chip | null; data: Record<string, unknown> };
