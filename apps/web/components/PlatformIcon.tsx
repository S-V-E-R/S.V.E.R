import { ExternalIcon } from "./shell/Icons";
const marks: Record<string, string> = { twitch: "Tw", youtube: "YT", kick: "K", tiktok: "Tk", instagram: "Ig", x: "X", bluesky: "Bs", discord: "Dc", facebook: "f", patreon: "P", kofi: "Ko", fourthwall: "4W" };

/** Small monogram tile for a social platform, drawn in the site's own style (no third-party logo files). */
export function PlatformIcon({ platform }: { platform: string }) {
  return <span className={`platform-icon platform-${platform}`} aria-hidden="true">{marks[platform] ?? <ExternalIcon size={12} />}</span>;
}
