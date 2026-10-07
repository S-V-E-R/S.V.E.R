/** The three factions (docs/FACTIONS.md, "The factions"; lore from docs/LORE.md). One place for names, copy, colors and crests. */
export const factions = [
  {
    slug: "myria", rejects: "Shortcuts, coasting on talent, and taking credit for work you didn't do.", name: "Myria", title: "The Vanguard", color: "#FF9A1F",
    creed: "Earn everything. Accept nothing.",
    belief: "We believe nothing is given. Everything is earned through discipline and conviction.",
    values: "Discipline · Conviction · Endurance",
    people: "Competitors, speedrunners, challenge hunters and makers who keep working at their craft, even when nobody is watching.",
    turf: ["FPS & battle royale", "Fighting", "Sports & racing", "Speedrunning", "Crafting & making"],
    welcome: "The Vanguard. Nothing is given here; your first results start today.",
    lore: "When the Beacon fell, everyone ran from the crater except the people who would become Myria. The fire took them, and they came out of the ash still holding the one shard that wouldn't go out. They built the Kiln on the site of the fall and forged a people who earn everything they have. A Myrian's word is their bond, and their progress is their proof.",
    line: "The ones who stayed in the fire, and rose from its ashes.",
    battleCry: "From ashes, we rise.",
    homeland: "the Kiln", homelandNote: "the crater where the Beacon fell, rebuilt as a forge-city",
    relic: "the Phoenix Flame", relicGives: "Endurance. Its bearers get back up when they should have stayed down.",
    relicCost: "It feeds on rest. A Myrian who stops for too long feels the Flame gutter, so many burn themselves out.",
    shadow: "Contempt for anyone who stops, and the pride that won't admit exhaustion.",
    telling: "We stood under the Beacon when it fell. The others ran, and we stayed, because someone had to. The fire took us, and we rose. Nobody handed us anything after that. Not then, not since.",
    founders: [
      { name: "Ondra the Unbowed", story: "The first Flamekeeper. She carried the Flame out of the crater on her back for nine days without sleeping. Myrians still say \"nine days\" for any task finished the hard way." },
      { name: "Hale Corrow", story: "The Quartermaster. He built the Kiln's forges and taught that every tool is earned by learning to make it. Makers and crafters claim him." },
      { name: "Ryn Fastmarch", story: "The Courier. She ran the war's messages between the shards faster than anyone alive, and set records nobody broke for a century. Speedrunners claim her." },
    ],
  },
  {
    slug: "aetheron", rejects: "Willful ignorance, empty confidence, and chaos for its own sake.", name: "Aetheron", title: "The Arcane", color: "#A68BFF",
    creed: "Always learning. Never finished.",
    belief: "We believe mastery comes from curiosity. Every answer is the start of a better question.",
    values: "Curiosity · Mastery · Discovery",
    people: "Strategists, artists, educators, developers and theorycrafters who learn by testing, asking better questions and sharing what they find.",
    turf: ["RTS & MOBA", "Strategy & 4X", "Card & board", "Puzzle & simulation", "Art", "Education & coding"],
    welcome: "The Arcane. Learn it, then teach it. Your side is glad you came.",
    lore: "When the Beacon broke, one shard rose instead of falling, and the people who would become Aetheron were the only ones watching the sky. They built Selenne on the cliffs to read the Pale Moon and found it showed patterns: in battles, in markets, in people. Aetheron learns first, then teaches what it learns, because a pattern only one person can see dies with them. There is always more to learn.",
    line: "The ones who looked up, and learned to read the second moon.",
    battleCry: "Knowledge ascends. Power follows.",
    homeland: "Selenne", homelandNote: "an observatory city on the cliffs, built to watch the second moon",
    relic: "the Pale Moon", relicGives: "Moonsight: patterns, odds and the shape of what comes next.",
    relicCost: "The more you see, the less you feel. Readers who stare too long become the Moonstruck, lost in patterns and cold to the people in front of them.",
    shadow: "Cold pride, and the belief that anything unmeasured doesn't matter.",
    telling: "The Beacon didn't fall to fire. It fell to a flaw nobody bothered to study: it fed the bright and starved the rest. While everyone else ran, we looked up and saw one shard rising. We've been reading it ever since, so the next light isn't built with the same flaw.",
    founders: [
      { name: "Ilen Marrow", story: "The First Reader. She charted the Pale Moon's phases and wrote the Codex of Turns, which Aetheron still adds to each season. Theorycrafters and educators claim her." },
      { name: "Sefa Lumen", story: "The Painter. Most people can't read the Moon, so she painted what it showed, and her murals taught a generation. Artists claim her." },
      { name: "Tavik the Gamesman", story: "He built war tables with carved pieces so commanders could test a battle before fighting it. Strategy players and card and board gamers claim him." },
    ],
  },
  {
    slug: "glint", rejects: "Gatekeeping, scarcity thinking, and building walls instead of doors.", name: "Glint", title: "The Sovereign", color: "#E9C35A",
    creed: "All are welcome. None are forgotten.",
    belief: "We believe the strongest force on any platform is a room where everyone belongs.",
    values: "Belonging · Trust · Momentum",
    people: "Musicians, co-op teams, cozy gamers and community builders who remember the newcomer and leave room for one more.",
    turf: ["Community events", "MMOs & RPGs", "Co-op & party", "Cozy & sandbox", "Music"],
    welcome: "The Sovereign. Everyone belongs here. Leave room for one more.",
    lore: "After the Ashfall, people were scattered and alone in the dark. The people who would become Glint found the Beacon's capstone, the Crown, and learned it shone only when people gathered around it. So they lit fires at the crossroads and called in anyone who would come. Aurel grew from those fires. A Glint sovereign is chosen by the hall, not born to it, and their fortune is the fortune of everyone at the table.",
    line: "The ones who lit fires in the dark and called everyone in.",
    battleCry: "Every hall starts with one open seat.",
    homeland: "Aurel", homelandNote: "a hall-city at the crossroads, where every road ends at an open door",
    relic: "the Crown", relicGives: "Gathering. Around the Crown, strangers become a company and a company becomes a people.",
    relicCost: "The Crown answers to the crowd. A sovereign who stops listening loses it, and one who only listens follows the crowd wherever it goes, even when the crowd is wrong.",
    shadow: "Comfort, and the crowd's will mistaken for what's right.",
    telling: "The Beacon was never the light. The people standing around it were. When it went dark, we didn't go looking for fire or for omens. We went looking for each other.",
    founders: [
      { name: "Maren of the Open Table", story: "The first sovereign. The crowd at the crossroads chose her because she gave her last coin to a stranger so he could sit at the fire." },
      { name: "Dallo Reed", story: "The Minstrel. His songs carried news between the scattered camps, and a camp that knew his songs knew it wasn't alone. Musicians claim him." },
      { name: "The Hearthway Twins", story: "Two innkeepers who never turned a traveller away, even in the Dim Years. Co-op, MMO and cozy players claim them." },
    ],
  },
] as const;
/** The shared world behind the three factions (docs/LORE.md, "The world"), at two lengths. */
export const WORLD = {
  line: "When the Beacon burned out, three shards survived, and three peoples rose to carry them.",
  paragraph: "Once, a great light called the Beacon let every voice in the world be seen. Over generations its light narrowed onto a famous few until it cracked, and the Ashfall left everyone else in the dark. Three shards survived: the Phoenix Flame, the Pale Moon and the Crown. Myria, Aetheron and Glint formed around them, and they have fought ever since over who should relight the Beacon. The Grey Wolf, who belongs to none of them, keeps the Accord that limits their war.",
  accord: ["Ground may be taken. A voice may never be silenced.", "Every light gets its turn.", "No coin buys the light.", "Only the living are counted.", "When the season ends, the map is redrawn."],
} as const;
export type Faction = typeof factions[number]["slug"];
/** Alias used by the sign-up and home components. */
export type FactionSlug = Faction;
export type FactionDetails = typeof factions[number];
export const factionInfo = (faction: Faction) => factions.find(f => f.slug === faction)!;
export const isFaction = (value: unknown): value is Faction => factions.some(f => f.slug === value);

/** The same list in the shape the onboarding wizard, home and cards use (value and turf lists). */
export type FactionCard = Omit<FactionDetails, "values" | "turf"> & { values: string[]; turf: string[] };
export const FACTIONS: readonly FactionCard[] = factions.map(f => ({ ...f, values: f.values.split(" · "), turf: [...f.turf] }));
export function factionOf(slug: string | null | undefined): FactionCard | null {
  return FACTIONS.find(f => f.slug === slug) ?? null;
}
export const crestSrc = (slug: Faction) => `/factions/${slug}.webp`;
