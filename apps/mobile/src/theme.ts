/** Design tokens from docs/DESIGN.md. Faction accent hex values also match apps/web/lib/factions.ts. */
export const palette = {
  neutral: { accent: "#C9CED6", accentLight: "#ECEEF2", accentDark: "#5A606C", accentDeep: "#12151C" },
  myria: { accent: "#FF9A1F", accentLight: "#FFC56E", accentDark: "#8A4A06", accentDeep: "#2A1604" },
  aetheron: { accent: "#A68BFF", accentLight: "#D3C6FF", accentDark: "#4B2A9A", accentDeep: "#1C1038" },
  glint: { accent: "#E9C35A", accentLight: "#F8E6A8", accentDark: "#7E5F14", accentDeep: "#0A1A3C" },
} as const;

export const ink = "#F2F0EA";
export const inkMuted = "#A3A6AE";
export const inkDim = "#8E929B";
export const bg = "#07080B";
export const live = "#E5262E";
export const surface = "rgba(8, 9, 13, 0.85)";

export type ThemeName = keyof typeof palette;
export type Colors = (typeof palette)[ThemeName];

/** Same rule as apps/web/lib/theme.ts. */
export function themeFor(account: { faction?: unknown } | null): ThemeName {
  const faction = account?.faction;
  return faction === "myria" || faction === "aetheron" || faction === "glint" ? faction : "neutral";
}
