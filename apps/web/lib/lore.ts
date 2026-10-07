import type { Faction } from "./factions";

/**
 * World lore used on the war map, from the S.V.E.R Faction Bible (approved October 6, 2026).
 * Keep wording in step with the bible; names, creeds and home turf stay in lib/factions.ts.
 */
export const homelands: Record<Faction, { city: string; about: string; relic: string; cry: string }> = {
  myria: { city: "The Kiln", about: "The crater where the Beacon fell, rebuilt as a forge-city.", relic: "The Phoenix Flame", cry: "From ashes, we rise." },
  aetheron: { city: "Selenne", about: "An observatory city on the cliffs, built to watch the second moon.", relic: "The Pale Moon", cry: "Knowledge ascends. Power follows." },
  glint: { city: "Aurel", about: "A hall-city at the crossroads, where every road ends at an open door.", relic: "The Crown", cry: "Every hall starts with one open seat." },
};

/** The five terms every faction swears to, written by the Grey Wolf in 41 AF. */
export const accord = [
  "Ground may be taken. A voice may never be silenced.",
  "Every light gets its turn.",
  "No coin buys the light.",
  "Only the living are counted.",
  "When the season ends, the map is redrawn.",
];

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
