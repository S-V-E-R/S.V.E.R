/** The three factions (docs/FACTIONS.md, "The factions"). One place for names, copy and crests. */
export type FactionSlug = "myria" | "aetheron" | "glint";

export type Faction = {
  slug: FactionSlug; name: string; title: string; creed: string; color: string;
  /** Short home-turf line for the sign-up cards. */
  turf: string;
  /** One line shown after choosing ("Welcome to …"). */
  welcome: string;
};

export const FACTIONS: readonly Faction[] = [
  { slug: "myria", name: "Myria", title: "The Vanguard", creed: "Earn everything. Accept nothing.", color: "#FF9A1F",
    turf: "FPS, battle royale, fighting, speedrunning, crafting", welcome: "The Vanguard. Nothing is given here; your first results start today." },
  { slug: "aetheron", name: "Aetheron", title: "The Arcane", creed: "Always learning. Never finished.", color: "#A68BFF",
    turf: "RTS, MOBA, strategy, art, education and coding", welcome: "The Arcane. Learn it, then teach it. Your side is glad you came." },
  { slug: "glint", name: "Glint", title: "The Sovereign", creed: "All are welcome. None are forgotten.", color: "#E9C35A",
    turf: "MMOs, co-op, cozy games, music, community events", welcome: "The Sovereign. Everyone belongs here. Leave room for one more." },
];

export function factionOf(slug: string | null | undefined): Faction | null {
  return FACTIONS.find(f => f.slug === slug) ?? null;
}

export const crestSrc = (slug: FactionSlug) => `/factions/${slug}.webp`;
