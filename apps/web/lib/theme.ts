import { isFaction, type Faction } from "./factions";
export type Theme = "neutral" | Faction;
export function themeFor(account: { faction?: unknown } | null): Theme { return isFaction(account?.faction) ? account.faction : "neutral"; }
