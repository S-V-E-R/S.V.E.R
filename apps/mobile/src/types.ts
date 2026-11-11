/** Shapes returned by the API today. Field names follow discovery.rs, playback.rs, chat.rs and auth.rs. */

export type Avatar = Record<string, string> | null;

export type StreamCard = {
  broadcast_id: string;
  username: string;
  display_name: string;
  avatar: Avatar;
  faction: string | null;
  title: string;
  category: string | null;
  category_id: string | null;
  genre: string | null;
  started_at: string;
  viewers: number;
  thumbnail: string;
  label: string | null;
  charity: string | null;
  fresh: boolean;
  mature: boolean;
  language: string | null;
};

export type RecentChannel = {
  user: { username: string | null; display_name: string; faction?: string | null; avatar?: Avatar };
  ended_at: string;
};

export type DiscoveryLive = { as_of: string; items: StreamCard[]; recent: RecentChannel[] };

export type MagnetFeatured = {
  stream: StreamCard;
  kind: string | null;
  reason: string | null;
  holding: boolean;
};

export type MagnetLane = {
  id: string;
  name: string;
  enabled: boolean;
  featured: MagnetFeatured | null;
};

export type SearchChannel = {
  username: string | null;
  display_name: string;
  faction?: string | null;
  deleted?: boolean;
};

export type SearchResult = { query: string; channels: SearchChannel[]; categories: { id: string; name: string; genre: string }[] };

export type Playback = { webrtc: string | null; hls: string | null; preferred: "webrtc" | "hls" };

export type Live =
  | { live: false; hosting?: { username: string; display_name: string } }
  | {
      live: true;
      broadcast_id: string;
      state: "LIVE" | "RECONNECTING";
      title: string;
      category: string | null;
      started_at: string;
      viewers: number;
      is_owner: boolean;
      banned?: boolean;
      mature?: boolean;
      mature_warn?: boolean;
      mature_blocked?: boolean;
      playback: Playback | null;
    };

export type ChatAuthor = {
  username: string | null;
  display_name: string;
  faction?: string | null;
};

export type ChatMessage = {
  id: string;
  seq: number;
  body: string | null;
  created_at: string;
  author: ChatAuthor;
  role?: string | null;
  /** A Skill's name when the message played one (bought on the website). */
  skill?: string | null;
};

export type ChatRoom = { messages: ChatMessage[] };

export type Account = {
  id: string;
  username: string;
  faction: string | null;
  email_verified: boolean;
  deletion_due: string | null;
};

export type LoginBody = { signed_in?: boolean; requires_mfa?: boolean; pending_deletion?: boolean };

export function isStreamCard(value: unknown): value is StreamCard {
  if (!value || typeof value !== "object") return false;
  const card = value as Record<string, unknown>;
  return typeof card.broadcast_id === "string" && typeof card.username === "string" && typeof card.title === "string" && typeof card.display_name === "string";
}

export function isChatMessage(value: unknown): value is ChatMessage {
  if (!value || typeof value !== "object") return false;
  const message = value as Record<string, unknown>;
  const author = message.author;
  return typeof message.id === "string" && typeof message.seq === "number" && author !== null && typeof author === "object" && typeof (author as { display_name?: unknown }).display_name === "string";
}
