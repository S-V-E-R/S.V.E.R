import type { Chip, Sizes, Occurrence } from "./types";
export type Guild = { id: string; slug: string; name: string; tag: string; tagline: string; about: string; status: string; recruiting: boolean; verified: boolean; avatar: string | null; banner: Sizes; revision: number; role?: string | null };
export type GuildPage = {
  guild: Guild;
  members: { user: Chip; live: boolean; role: string; title: string; joined_at: string }[];
  schedule: { user: Chip; occurrence: Occurrence }[];
  viewer: { signed_in: boolean; role: string | null; can_manage: boolean; following: boolean; muted: boolean; badge: boolean; invited: boolean; application: { status: string; message: string; note: string; reapply_at: string | null } | null };
  management: null | { applications: { id: string; user: Chip; message: string; created_at: string }[]; events: { id: number; action: string; actor: string | null; subject: string | null; created_at: string }[]; blocks: string[]; verification: { status: string; note: string } | null };
};
export type MyGuilds = { items: Guild[]; can_create: boolean; badge: string | null };
