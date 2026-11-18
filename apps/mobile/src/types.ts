/** Shapes returned by the API today. Field names follow discovery.rs, profiles.rs, playback.rs, chat.rs and auth.rs. */

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
  user: ChannelChip;
  ended_at: string;
};

/** profiles::chip: a channel as lists show it. `username` is null for a deleted account. */
export type ChannelChip = { username: string | null; display_name: string; avatar?: Avatar; faction?: string | null; deleted?: boolean };

/** A home spotlight: a staff pick (its `stream` is null while the channel is offline) or a first or returning stream. */
export type Spotlight = { kind: "staff" | "first_stream" | "returning"; reason: string; stream: StreamCard | null; user?: ChannelChip; ends_at?: string };

/** GET /api/discovery/home. Every list is in rotation order; `faction` is null for viewers without one. */
export type HomeFeed = {
  as_of: string;
  rotates_at: string | null;
  live: StreamCard[];
  following: StreamCard[];
  faction: StreamCard[] | null;
  fresh: StreamCard[];
  spotlights: Spotlight[];
  recent: RecentChannel[];
};

/** Width → URL maps, as `avatar_json`, `art_json` and `banner_json` return them. */
export type Sizes = Record<string, string> | null;

export type BrowseCategory = { id: string; name: string; live: number; image: Sizes };
export type BrowseGenre = { id: string; name: string; home: string | null; live: number; categories: BrowseCategory[] };
/** GET /api/discovery/browse. `live` is the total live now. */
export type Browse = { genres: BrowseGenre[]; live: number };

/** GET /api/channels/{username}: the fields this app reads (the response carries many more). */
export type ChannelHeader = {
  channel: {
    username: string;
    display_name: string;
    bio: string;
    avatar: Avatar;
    banner: Sizes;
    faction: string | null;
    follower_count: number;
    live: boolean;
    plays: boolean;
    board: boolean;
    live_started_at?: string;
  };
  viewer: { signed_in: boolean; is_owner: boolean; following: boolean; blocked: boolean; interaction_blocked: boolean };
};
/** A renamed channel answers with its current username. */
export type ChannelRedirect = { redirect_to: string };

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

export type SearchChannel = ChannelChip;

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

/**
 * GET /api/channels/{username}/chat, and the socket's `snapshot`. A `pin` socket event carries `pinned` again.
 * `followers_only_until` is set only while followers-only chat is on.
 */
export type ChatRoom = { messages: ChatMessage[]; pinned?: ChatMessage | null; followers_only_until?: string | null; subs_only?: boolean };

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

const isRecord = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object";

/** Keeps only the well-formed cards (lists, suggestions, spotlight streams). */
export function streamCards(value: unknown): StreamCard[] {
  return Array.isArray(value) ? value.filter(isStreamCard) : [];
}

export function isHomeFeed(value: unknown): value is HomeFeed {
  return isRecord(value) && Array.isArray(value.live) && Array.isArray(value.following) && Array.isArray(value.spotlights) && Array.isArray(value.recent);
}

export function isBrowse(value: unknown): value is Browse {
  return isRecord(value) && typeof value.live === "number" && Array.isArray(value.genres) && value.genres.every(g => isRecord(g) && typeof g.name === "string" && Array.isArray(g.categories));
}

export function isChannelRedirect(value: unknown): value is ChannelRedirect {
  return isRecord(value) && typeof value.redirect_to === "string";
}

export function isChannelHeader(value: unknown): value is ChannelHeader {
  return isRecord(value) && isRecord(value.channel) && typeof value.channel.username === "string" && typeof value.channel.display_name === "string" && isRecord(value.viewer);
}
