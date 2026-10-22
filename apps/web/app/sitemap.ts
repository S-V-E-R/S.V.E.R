import type { MetadataRoute } from "next";
import { sitemapEntries, type SitemapLists } from "../lib/seo";
import { apiOrigin } from "../lib/server-api";

/** Public pages, channels and videos. The API is optional: a failed read still lists the static pages. */
export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  let lists: SitemapLists = {};
  try {
    const response = await fetch(`${apiOrigin()}/api/discovery/sitemap`, { signal: AbortSignal.timeout(2000), cache: "no-store" });
    if (response.ok) lists = await response.json() as SitemapLists;
  } catch { /* static pages still ship */ }
  return sitemapEntries(lists);
}
