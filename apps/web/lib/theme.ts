/** The four site themes from docs/DESIGN.md ("Themes"). The server sets one on <html data-theme>. */
export type Theme = "neutral" | "myria" | "aetheron" | "glint";

/**
 * Theme for the signed-in account (or `null` when signed out).
 * Factions aren't stored yet: Module 4 (Factions) supplies the account's faction, and this
 * returns "myria", "aetheron" or "glint" from it. Until then everyone gets the neutral steel theme.
 */
export function themeFor(account: { username: string } | null): Theme {
  void account;
  return "neutral";
}
