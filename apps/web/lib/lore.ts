import { factions, WORLD, type Faction } from "./factions";

/**
 * World lore used on the war map, from the S.V.E.R Faction Bible (approved October 6, 2026).
 * Homelands, relics, battle cries and the Accord come from lib/factions.ts (docs/LORE.md); only the genre-to-founder map lives here.
 */
const cap = (text: string) => text[0].toUpperCase() + text.slice(1);
export const homelands = Object.fromEntries(factions.map(f => [f.slug, { city: cap(f.homeland), about: `${cap(f.homelandNote)}.`, relic: cap(f.relic), cry: f.battleCry }])) as Record<Faction, { city: string; about: string; relic: string; cry: string }>;

/** The five terms every faction swears to, written by the Grey Wolf in 41 AF. */
export const accord: readonly string[] = WORLD.accord;

/** Founders the bible says a genre's people claim, keyed by genre id. */
export const founders: Record<string, { name: string; deed: string }> = {
  crafting_making: { name: "Hale Corrow, the Quartermaster", deed: "He built the Kiln’s forges and taught that every tool is earned by learning to make it." },
  speedrunning: { name: "Ryn Fastmarch, the Courier", deed: "She ran the war’s messages between the shards faster than anyone alive, and set records nobody broke for a century." },
  education_coding: { name: "Ilen Marrow, the First Reader", deed: "She charted the Pale Moon’s phases and wrote the Codex of Turns, which Aetheron still adds to each season." },
  art: { name: "Sefa Lumen, the Painter", deed: "Most people can’t read the Moon, so she painted what it showed, and her murals taught a generation." },
  strategy_4x: { name: "Tavik the Gamesman", deed: "He built war tables with carved pieces so commanders could test a battle before fighting it." },
  card_board: { name: "Tavik the Gamesman", deed: "He built war tables with carved pieces so commanders could test a battle before fighting it." },
  music: { name: "Dallo Reed, the Minstrel", deed: "His songs carried news between the scattered camps, and a camp that knew his songs knew it wasn’t alone." },
  coop_party: { name: "The Hearthway Twins", deed: "Two innkeepers who never turned a traveller away, even in the Dim Years." },
  mmos_rpgs: { name: "The Hearthway Twins", deed: "Two innkeepers who never turned a traveller away, even in the Dim Years." },
  cozy_sandbox: { name: "The Hearthway Twins", deed: "Two innkeepers who never turned a traveller away, even in the Dim Years." },
};

/** Each season is one year After the Fall; Season 1 is 301 AF. */
export const yearAF = (season: number | null | undefined) => 300 + (season ?? 0);
