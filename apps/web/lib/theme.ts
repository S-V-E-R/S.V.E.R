import type { FactionSlug } from "./factions";

/** The four site themes from docs/DESIGN.md ("Themes"). The server sets one on <html data-theme>. */
export type Theme = "neutral" | FactionSlug;

/** The signed-in account's faction theme; neutral when signed out or before choosing a side. */
export function themeFor(account: { faction: FactionSlug | null } | null): Theme {
  return account?.faction ?? "neutral";
}
