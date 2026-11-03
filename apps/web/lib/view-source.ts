// Where a view came from, for the streamer's analytics (docs/ANALYTICS.md "Sources"). A click on
// an internal link remembers the page (or the nearest `data-src` section) it came from; the live
// player reads it once when it starts. Never used for discovery, MAGNet or money.
const KEY = "sver:view-source";

/** The source a click on a page at `path` carries. */
export function sourceOf(path: string): string {
  if (path === "/") return "home";
  const [first, second] = path.split("/").filter(Boolean);
  const pages: Record<string, string> = { magnet: "magnet", search: "search", browse: "browse", following: "following", notifications: "alert", beacons: "beacon", clips: "clip", videos: "video", factions: "faction", "war-map": "faction" };
  if (pages[first]) return pages[first];
  if (second === "clips") return "clip";
  if (second === "videos") return "video";
  // Another channel's watch page (Up next, the stream-end handoff) suggests the next stream.
  if (second === "live") return "suggestion";
  return second === undefined || second === "about" ? "channel" : "other";
}

/** Installed once by the site shell: remembers where each internal link click came from. */
export function trackClicks(): () => void {
  const onClick = (event: MouseEvent) => {
    const link = (event.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
    if (!link || link.origin !== window.location.origin) return;
    const tagged = link.closest<HTMLElement>("[data-src]")?.dataset.src;
    try {
      sessionStorage.setItem(KEY, JSON.stringify({ source: tagged ?? sourceOf(window.location.pathname), to: link.pathname.toLowerCase(), at: Date.now() }));
    } catch { /* storage blocked: the view counts as direct */ }
  };
  document.addEventListener("click", onClick, true);
  return () => document.removeEventListener("click", onClick, true);
}

/** How this player's viewer arrived at `/{username}…`. Read once per player. */
export function takeSource(username: string): { source: string; referrer?: string } {
  const param = new URLSearchParams(window.location.search).get("src");
  if (param === "alert") return { source: "alert" };
  try {
    const saved = JSON.parse(sessionStorage.getItem(KEY) ?? "null") as { source: string; to: string; at: number } | null;
    if (saved && Date.now() - saved.at < 60_000 && saved.to.startsWith(`/${username.toLowerCase()}`)) {
      sessionStorage.removeItem(KEY);
      return { source: saved.source };
    }
  } catch { /* fall through to the referrer */ }
  if (document.referrer) {
    const from = new URL(document.referrer);
    if (from.origin === window.location.origin) return { source: sourceOf(from.pathname) };
    return { source: "external", referrer: from.hostname };
  }
  return { source: "direct" };
}
