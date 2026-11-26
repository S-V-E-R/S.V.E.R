/** Public site by default. Override for a local API (`docs` in apps/mobile/README.md). */
export const apiUrl = (process.env.EXPO_PUBLIC_API_URL ?? "https://sver.tv").replace(/\/$/, "");
/** Value the API compares on mutations and chat sockets (`APP_ORIGIN`). */
export const appOrigin = (process.env.EXPO_PUBLIC_APP_ORIGIN ?? "https://sver.tv").replace(/\/$/, "");
/** Where faction crests are served (the website's /public folder). */
export const webOrigin = (process.env.EXPO_PUBLIC_WEB_ORIGIN ?? appOrigin).replace(/\/$/, "");

export function absolute(path: string): string {
  if (/^https?:\/\//i.test(path)) return path;
  return `${apiUrl}${path.startsWith("/") ? path : `/${path}`}`;
}

/**
 * The Take It Down request form (docs/TAKE_IT_DOWN.md). For a reported item it fills in the same
 * location link as the website's report form (apps/web/components/Report.tsx TakeDownLink), which
 * the API reads to hide the item while staff review the request.
 */
export function takeItDownUrl(item?: { type: string; id: string }): string {
  if (!item) return `${webOrigin}/take-it-down`;
  const location = `https://sver.tv/take-it-down?report=${encodeURIComponent(item.type)}&id=${encodeURIComponent(item.id)}`;
  return `${webOrigin}/take-it-down?location=${encodeURIComponent(location)}`;
}

export function crestUrl(slug: string): string {
  return `${webOrigin}/factions/${slug}.webp`;
}

/** Bunny HLS URLs rotate a path token. The stream identity is the rest of the URL. */
export function playbackKey(url: string): string {
  return url.replace(/\/bcdn_token=[^/]*/, "");
}

/** A channel's chat socket, or a merged co-stream's shared chat when `squad` is set. */
export function chatSocketUrl(username: string, squad?: string | null): string {
  const base = apiUrl.replace(/^http/i, "ws");
  return `${base}/api/chat/ws?${squad ? `squad=${encodeURIComponent(squad)}` : `channel=${encodeURIComponent(username)}`}`;
}
