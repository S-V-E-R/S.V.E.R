import type { MetadataRoute } from "next";
import { ORIGIN, ROBOTS_DISALLOW } from "../lib/seo";

export default function robots(): MetadataRoute.Robots {
  return {
    rules: { userAgent: "*", allow: "/", disallow: ROBOTS_DISALLOW },
    sitemap: `${ORIGIN}/sitemap.xml`,
    host: ORIGIN,
  };
}
