export const factions = [
  {
    slug: "myria", name: "Myria", title: "The Vanguard",
    creed: "Earn everything. Accept nothing.",
    belief: "We believe nothing is given. Everything is earned through discipline and conviction.",
    values: "Discipline · Conviction · Endurance",
    people: "Competitors, speedrunners, challenge hunters and makers who keep working at their craft, even when nobody is watching.",
    turf: ["FPS & battle royale", "Fighting", "Sports & racing", "Speedrunning", "Crafting & making"],
    lore: "Myria was not founded. It was forged by people who refused to quit. Where you started matters less than whether you show up when it gets hard. Your word is your bond; your progress is your proof.",
  },
  {
    slug: "aetheron", name: "Aetheron", title: "The Arcane",
    creed: "Always learning. Never finished.",
    belief: "We believe mastery comes from curiosity. Every answer is the start of a better question.",
    values: "Curiosity · Mastery · Discovery",
    people: "Strategists, artists, educators, developers and theorycrafters who learn by testing, asking better questions and sharing what they find.",
    turf: ["RTS & MOBA", "Strategy & 4X", "Card & board", "Puzzle & simulation", "Art", "Education & coding"],
    lore: "Aetheron moves by knowledge. Its people study how systems work and how creators improve, then pass that understanding on. Mastery is the goal, discovery is the fuel, and there is always more to learn.",
  },
  {
    slug: "glint", name: "Glint", title: "The Sovereign",
    creed: "All are welcome. None are forgotten.",
    belief: "We believe the strongest force on any platform is a room where everyone belongs.",
    values: "Belonging · Trust · Momentum",
    people: "Musicians, co-op teams, cozy gamers and community builders who remember the newcomer and leave room for one more.",
    turf: ["Community events", "MMOs & RPGs", "Co-op & party", "Cozy & sandbox", "Music"],
    lore: "Glint builds its strength wherever people gather. A room where everyone feels welcome can become a community that lasts. Trust connects its people, and lifting someone else helps the whole side move forward.",
  },
] as const;
export type Faction = typeof factions[number]["slug"];
export const factionInfo = (faction: Faction) => factions.find(f => f.slug === faction)!;
export const isFaction = (value: unknown): value is Faction => factions.some(f => f.slug === value);
