/** Share links. Channel copy stays query-free (docs/PROFILES.md P1). Credit is a public username. */

export const USERNAME = /^[A-Za-z0-9_]{3,25}$/;

/** A sharer's public username, or nothing when it isn't one. */
export function creditName(value: string | null | undefined): string | undefined {
  const name = value?.trim() ?? "";
  return USERNAME.test(name) ? name : undefined;
}

/** Seconds from a share timestamp: "90", "1m30s" or "1h2m3s". Nothing past 48 hours. */
export function parseStart(raw: string | null | undefined): number | null {
  if (!raw) return null;
  const text = raw.trim().toLowerCase();
  let seconds = 0;
  if (/^\d+$/.test(text)) seconds = Number(text);
  else {
    const match = text.match(/^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?$/);
    if (!match || match[0] === "") return null;
    seconds = Number(match[1] ?? 0) * 3600 + Number(match[2] ?? 0) * 60 + Number(match[3] ?? 0);
  }
  if (!Number.isFinite(seconds) || seconds <= 0 || seconds > 48 * 3600) return null;
  return seconds;
}

export function timestampParam(ms: number): string | undefined {
  const seconds = Math.floor(ms / 1000);
  return seconds > 0 ? String(seconds) : undefined;
}

/** Absolute URL. `primary` omits credit (the channel copy-link). Network links pass primary false. */
export function shareUrl(origin: string, path: string, query: Record<string, string | undefined>, primary: boolean, canonical: boolean): string {
  const url = new URL(path, origin);
  if (primary && canonical) return url.toString();
  for (const [key, value] of Object.entries(query)) {
    if (value) url.searchParams.set(key, value);
  }
  return url.toString();
}

export function shareQuery(credit: string | null | undefined, extra: Record<string, string | undefined>): Record<string, string | undefined> {
  return { ...extra, ref: creditName(credit) };
}

export function xIntent(url: string, title: string): string {
  return `https://x.com/intent/post?text=${encodeURIComponent(title)}&url=${encodeURIComponent(url)}`;
}

export function redditIntent(url: string, title: string): string {
  return `https://www.reddit.com/submit?url=${encodeURIComponent(url)}&title=${encodeURIComponent(title)}`;
}
