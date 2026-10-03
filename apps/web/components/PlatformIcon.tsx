const marks: Record<string, string> = { twitch: "Tw", youtube: "▶", kick: "K", tiktok: "♪", instagram: "Ig", x: "X", bluesky: "Bs", discord: "Dc", facebook: "f", patreon: "P", kofi: "☕", fourthwall: "4W", website: "↗" };

/** Small monogram tile for a social platform, drawn in the site's own style (no third-party logo files). */
export function PlatformIcon({ platform }: { platform: string }) {
  return <span className={`platform-icon platform-${platform}`} aria-hidden="true">{marks[platform] || "↗"}</span>;
}
