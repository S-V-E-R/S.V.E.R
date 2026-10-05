/** The three factions (docs/FACTIONS.md, "The factions"). One place for names, copy and crests. */
export type FactionSlug = "myria" | "aetheron" | "glint";

export type Faction = {
  slug: FactionSlug; name: string; title: string; creed: string; color: string;
  values: string[];
  /** One-line belief shown on cards. */
  belief: string;
  /** Who the faction speaks to. */
  people: string;
  /** What the faction stands against (carried over from legacy onboarding). */
  rejects: string;
  lore: string;
  /** Season 1 home turf (docs/FACTIONS.md, "Genres and the map"). */
  turf: string[];
  /** One line shown after choosing ("Welcome to …"). */
  welcome: string;
};

export const FACTIONS: readonly Faction[] = [
  {
    slug: "myria", name: "Myria", title: "The Vanguard", creed: "Earn everything. Accept nothing.", color: "#FF9A1F",
    values: ["Discipline", "Conviction", "Endurance"],
    belief: "We believe nothing is given. Everything is earned through discipline and conviction.",
    people: "Competitors, speedrunners, challenge hunters and makers who keep working at their craft, even when nobody is watching.",
    rejects: "Shortcuts, coasting on talent, and taking credit for work you didn't do.",
    lore: "Myria was not founded. It was forged by people who refused to quit. Where you started matters less than whether you show up when it gets hard. Your word is your bond; your progress is your proof.",
    turf: ["FPS & battle royale", "Fighting", "Sports & racing", "Speedrunning", "Crafting & making"],
    welcome: "The Vanguard. Nothing is given here; your first results start today.",
  },
  {
    slug: "aetheron", name: "Aetheron", title: "The Arcane", creed: "Always learning. Never finished.", color: "#A68BFF",
    values: ["Curiosity", "Mastery", "Discovery"],
    belief: "We believe mastery comes from curiosity. Every answer is the start of a better question.",
    people: "Strategists, artists, educators, developers and theorycrafters who learn by testing, asking better questions and sharing what they find.",
    rejects: "Willful ignorance, empty confidence, and chaos for its own sake.",
    lore: "Aetheron moves by knowledge. Its people study how systems work and how creators improve, then pass that understanding on. Mastery is the goal, discovery is the fuel, and there is always more to learn.",
    turf: ["RTS & MOBA", "Strategy & 4X", "Card & board", "Puzzle & simulation", "Art", "Education & coding"],
    welcome: "The Arcane. Learn it, then teach it. Your side is glad you came.",
  },
  {
    slug: "glint", name: "Glint", title: "The Sovereign", creed: "All are welcome. None are forgotten.", color: "#E9C35A",
    values: ["Belonging", "Trust", "Momentum"],
    belief: "We believe the strongest force on any platform is a room where everyone belongs.",
    people: "Musicians, co-op teams, cozy gamers and community builders who remember the newcomer and leave room for one more.",
    rejects: "Gatekeeping, scarcity thinking, and building walls instead of doors.",
    lore: "Glint builds its strength wherever people gather. A room where everyone feels welcome can become a community that lasts. Trust connects its people, and lifting someone else helps the whole side move forward.",
    turf: ["Community events", "MMOs & RPGs", "Co-op & party", "Cozy & sandbox", "Music"],
    welcome: "The Sovereign. Everyone belongs here. Leave room for one more.",
  },
];

export function factionOf(slug: string | null | undefined): Faction | null {
  return FACTIONS.find(f => f.slug === slug) ?? null;
}

export const crestSrc = (slug: FactionSlug) => `/factions/${slug}.webp`;
