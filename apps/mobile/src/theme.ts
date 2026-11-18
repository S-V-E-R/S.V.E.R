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
/** Text on an accent fill (beveled buttons, Ally tags). */
export const onAccent = "#0A0A0C";
/** The video stage (Watch, Beacons). */
export const black = "#000";
/** Dark translucent backing for tags and controls over video and thumbnails. */
export const scrim = "rgba(0,0,0,0.55)";
/** Input and quiet-control fill (fields, quiet buttons, Plays keys). */
export const fill = "rgba(0,0,0,0.35)";

/** The 4/8/12/16/24/32/48 spacing scale. Hairlines of 1–3dp sit outside it. */
export const space = { xs: 4, s: 8, m: 12, l: 16, xl: 24, xxl: 32, xxxl: 48 } as const;

export const font = {
  display: "Cinzel_800ExtraBold",
  displayBold: "Cinzel_700Bold",
  body: "Barlow_400Regular",
  bodySemi: "Barlow_600SemiBold",
  label: "BarlowCondensed_700Bold",
  labelSemi: "BarlowCondensed_600SemiBold",
} as const;

export function hexAlpha(hex: string, alpha: number): string {
  const n = Math.round(alpha * 255).toString(16).padStart(2, "0");
  return `${hex}${n}`;
}
/** `--line`: accent at 32%, for borders and rules. */
export const line = (hex: string) => hexAlpha(hex, 0.32);
/** `--tint`: accent at 7%, for panel fills. */
export const tint = (hex: string) => hexAlpha(hex, 0.07);

export type ThemeName = keyof typeof palette;
export type Colors = (typeof palette)[ThemeName];

/** Same rule as apps/web/lib/theme.ts. */
export function themeFor(account: { faction?: unknown } | null): ThemeName {
  const faction = account?.faction;
  return faction === "myria" || faction === "aetheron" || faction === "glint" ? faction : "neutral";
}
