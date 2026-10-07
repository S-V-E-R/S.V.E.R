import { NextResponse, type NextRequest } from "next/server";

// Channel routing (docs/PROFILES.md, "URLs and routing"): legacy aliases, ?tab= mapping,
// canonical casing (308) and rename-hold redirects (302, no-store). Static routes are excluded
// by the matcher; reserved names guarantee no user shadows them.
const STATIC = new Set(["api", "_next", "login", "signup", "oauth-signup", "forgot", "reset", "verify", "mfa", "account", "settings", "studio", "admin", "following", "about", "factions", "roadmap", "help", "terms", "privacy", "guidelines", "dmca", "contact", "favicon.ico", "robots.txt"]);
const TABS: Record<string, string> = { wall: "/wall", schedule: "/schedule", about: "/about", videos: "/videos" };
const NAME = /^[A-Za-z0-9_]{1,40}$/;
const api = () => process.env.API_INTERNAL_ORIGIN || "http://127.0.0.1:8080";

function to(request: NextRequest, path: string, status: 302 | 308, query = request.nextUrl.search) {
  const url = request.nextUrl.clone();
  url.pathname = path;
  url.search = query;
  const response = NextResponse.redirect(url, status);
  if (status === 302) response.headers.set("Cache-Control", "no-store");
  return response;
}

export async function proxy(request: NextRequest) {
  const segments = request.nextUrl.pathname.split("/").filter(Boolean);
  if (["videos", "clips", "embed", "beacons"].includes(segments[0])) return NextResponse.next();
  if (segments.length === 0 || STATIC.has(segments[0])) return NextResponse.next();
  const [first, ...rest] = segments;
  // Aliases: /s/{name}, /u/{name} and /@{name} are permanent; /watch/{name} goes to the live view.
  if ((first === "s" || first === "u") && rest.length > 0) return to(request, `/${rest.join("/")}`, 308);
  if (first.startsWith("@") && first.length > 1) return to(request, `/${[first.slice(1), ...rest].join("/")}`, 308);
  if (first === "watch" && rest.length > 0) return to(request, `/${rest[0]}/live`, 302);
  if (!NAME.test(first)) return NextResponse.next();
  const tab = request.nextUrl.searchParams.get("tab");
  if (tab !== null && rest.length === 0) {
    const query = new URLSearchParams(request.nextUrl.searchParams);
    query.delete("tab");
    const search = query.size ? `?${query}` : "";
    return to(request, `/${first}${TABS[tab] ?? ""}`, 308, search);
  }
  let resolved: { username?: string; redirect_to?: string } | null = null;
  try {
    const response = await fetch(`${api()}/api/channels/${encodeURIComponent(first)}/resolve`, { cache: "no-store" });
    if (response.ok) resolved = await response.json();
  } catch {
    resolved = null;
  }
  const sub = rest.length ? `/${rest.join("/")}` : "";
  if (resolved?.redirect_to) return to(request, `/${resolved.redirect_to}${sub}`, 302);
  if (resolved?.username && resolved.username !== first) return to(request, `/${resolved.username}${sub}`, 308);
  return NextResponse.next();
}

export const config = { matcher: ["/((?!api/|_next/|favicon\\.ico).*)"] };
