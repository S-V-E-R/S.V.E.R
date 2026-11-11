export type Jar = Record<string, string>;

type CookieHeaders = {
  get(name: string): string | null;
  getSetCookie?: () => string[];
};

/** Lines from a fetch response. Combined Set-Cookie headers split on the comma before the next name. */
export function setCookieLines(headers: CookieHeaders): string[] {
  if (typeof headers.getSetCookie === "function") {
    const lines = headers.getSetCookie();
    if (lines.length) return lines;
  }
  const combined = headers.get("set-cookie");
  if (!combined) return [];
  return combined.split(/,(?=\s*[^;,=\s]+=)/);
}

function cleared(line: string, value: string): boolean {
  return value.length === 0 || /(?:^|;)\s*max-age=0(?:;|$)/i.test(line);
}

/** Keep the session and challenge cookies the API sets. Empty or Max-Age=0 removes one. */
export function applySetCookie(jar: Jar, headers: CookieHeaders): Jar {
  const next: Jar = { ...jar };
  for (const line of setCookieLines(headers)) {
    const pair = line.split(";", 1)[0] ?? "";
    const eq = pair.indexOf("=");
    if (eq <= 0) continue;
    const name = pair.slice(0, eq).trim();
    const value = pair.slice(eq + 1).trim();
    if (!name) continue;
    if (cleared(line, value)) delete next[name];
    else next[name] = value;
  }
  return next;
}

export function cookieHeader(jar: Jar): string {
  return Object.entries(jar)
    .map(([name, value]) => `${name}=${value}`)
    .join("; ");
}
