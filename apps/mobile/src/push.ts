/** Expo push token shape. The API rejects anything else. */
const EXPO_TOKEN = /^(?:ExponentPushToken|ExpoPushToken)\[[A-Za-z0-9_-]{16,200}\]$/;

export function isExpoPushToken(token: string): boolean {
  return EXPO_TOKEN.test(token);
}

export type PushSettings = {
  site: boolean;
  push: boolean;
  email: boolean;
  expo_devices?: number;
  community: { kind: string; site: boolean; push: boolean }[];
};

/** Where a notification's `data.url` should open. Channel paths stay in the app. */
export function notificationTarget(url: string): { screen: "watch"; username: string } | { screen: "web"; path: string } | null {
  if (!url.startsWith("/") || url.startsWith("//") || url.includes("://") || url.includes("\\")) return null;
  const hashless = url.split("#")[0] ?? url;
  const [pathname, query] = hashless.split("?");
  if (query?.includes("://") || query?.includes("\\")) return null;
  const parts = (pathname ?? "").split("/").filter(Boolean);
  // Go-live and light alerts are `/{username}` with no query. A query means another page.
  if (!query && parts.length === 1 && /^[A-Za-z0-9_]{2,32}$/.test(parts[0] ?? "")) {
    return { screen: "watch", username: parts[0] ?? "" };
  }
  const path = query ? `${pathname}?${query}` : pathname;
  if (!path?.startsWith("/")) return null;
  return { screen: "web", path };
}
