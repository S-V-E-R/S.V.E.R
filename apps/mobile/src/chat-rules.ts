/** Chat rules shared by src/chat.tsx and its test (no React Native imports). Copied from apps/web/components/Chat.tsx. */
import type { ChatMessage, OutsideSender } from "./types";

/** The website keeps the last 100 lines; so does the app. */
export function merge(current: ChatMessage[], incoming: ChatMessage[]): ChatMessage[] {
  const byId = new Map(current.map(message => [message.id, message]));
  for (const message of incoming) byId.set(message.id, message);
  return [...byId.values()].sort((a, b) => a.seq - b.seq).slice(-100);
}

/** Subscriber badge milestones: 1, 3, 6, 9 and 12 months, then each further year (docs/SUPPORT.md). */
export function subBadge(months: number) {
  if (months >= 24) return `${Math.floor(months / 12)} years`;
  if (months >= 12) return "1 year";
  const step = [9, 6, 3].find(m => months >= m) ?? 1;
  return `${step} ${step === 1 ? "month" : "months"}`;
}

export const CREATOR_TIERS = ["Scout", "Trailblazer", "Pioneer", "Pathfinder"];
export const SUPPORTER_TIERS: Record<string, string> = { wolfsworn: "Wolfsworn", greycloak: "Greycloak" };
export const PLATFORM_NAMES: Record<string, string> = { twitch: "Twitch", youtube: "YouTube", kick: "Kick" };

/** "Twitch mod", "YouTube member", "Kick sub" or "Twitch broadcaster"; null without a role. */
export function outsideRole(outside: OutsideSender): string | null {
  if (!outside.role) return null;
  const platform = PLATFORM_NAMES[outside.platform] ?? outside.platform;
  const role = outside.role === "moderator" ? "mod" : outside.role === "subscriber" ? (outside.platform === "youtube" ? "member" : "sub") : "broadcaster";
  return `${platform} ${role}`;
}

/** Jittered exponential reconnect delay, capped at 30 s, so a deploy's reconnects spread out. */
export function retryDelay(attempt: number, random = Math.random()): number {
  return Math.min(30000, 1000 * 2 ** attempt) * (0.5 + random);
}

export function mentions(message: ChatMessage, username: string | null | undefined): boolean {
  return !!username && !!message.mentions?.some(name => name.toLowerCase() === username.toLowerCase());
}
