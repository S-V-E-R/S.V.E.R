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

export function crestUrl(slug: string): string {
  return `${webOrigin}/factions/${slug}.webp`;
}

/** Bunny HLS URLs rotate a path token. The stream identity is the rest of the URL. */
export function playbackKey(url: string): string {
  return url.replace(/\/bcdn_token=[^/]*/, "");
}

export function chatSocketUrl(username: string): string {
  const base = apiUrl.replace(/^http/i, "ws");
  return `${base}/api/chat/ws?channel=${encodeURIComponent(username)}`;
}
