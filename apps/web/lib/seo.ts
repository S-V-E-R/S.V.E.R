import type { Metadata } from "next";

export const ORIGIN = "https://sver.tv";
export const SITE_NAME = "S.V.E.R";
export const SITE_DESCRIPTION = "A live streaming platform built on community. Choose your faction, grow with your people, and get discovered: every live stream gets a fair turn.";
export const DEFAULT_IMAGE = `${ORIGIN}/icons/icon-512.png`;

/** Paths crawlers should not index. A trailing slash is a prefix. A trailing $ is exact. */
export const ROBOTS_DISALLOW = [
  "/api/",
  "/login",
  "/signup",
  "/oauth-signup",
  "/oauth/",
  "/forgot",
  "/reset",
  "/verify",
  "/mfa",
  "/account",
  "/settings",
  "/studio",
  "/admin",
  "/following",
  "/notifications",
  "/messages",
  "/wallet",
  "/welcome",
  "/overlay/",
  "/embed/",
  "/unsubscribe",
  "/advertise",
  "/choose-faction",
  "/choose-side",
  "/squads/",
  "/go$",
];

/** Google-style prefix match, with $ for an exact path. */
export function blockedByRobots(path: string): boolean {
  return ROBOTS_DISALLOW.some(rule => {
    if (rule.endsWith("$")) return path === rule.slice(0, -1) || path.startsWith(`${rule.slice(0, -1)}?`);
    return path === rule || path.startsWith(rule);
  });
}

export const PRIVATE: Metadata = { robots: { index: false, follow: false } };

export function publicMeta(title: string, description: string, path: string, image?: string): Metadata {
  const url = path === "/" ? ORIGIN : `${ORIGIN}${path}`;
  const picture = image ?? DEFAULT_IMAGE;
  return {
    title,
    description,
    alternates: { canonical: url },
    openGraph: { title, description, url, siteName: SITE_NAME, type: "website", images: [{ url: picture, alt: title }] },
    twitter: { card: image ? "summary_large_image" : "summary", title, description, images: [picture] },
  };
}

type Ld = Record<string, unknown>;

export function websiteJsonLd(): Ld {
  return {
    "@context": "https://schema.org",
    "@type": "WebSite",
    name: SITE_NAME,
    url: ORIGIN,
    description: SITE_DESCRIPTION,
    potentialAction: {
      "@type": "SearchAction",
      target: { "@type": "EntryPoint", urlTemplate: `${ORIGIN}/search?q={search_term_string}` },
      "query-input": "required name=search_term_string",
    },
  };
}

export function profileJsonLd(input: { username: string; name: string; description: string; image?: string; followers: number }): Ld {
  const url = `${ORIGIN}/${input.username}`;
  return {
    "@context": "https://schema.org",
    "@type": "ProfilePage",
    url,
    mainEntity: {
      "@type": "Person",
      name: input.name,
      alternateName: `@${input.username}`,
      description: input.description,
      url,
      image: input.image,
      interactionStatistic: {
        "@type": "InteractionCounter",
        interactionType: "https://schema.org/FollowAction",
        userInteractionCount: input.followers,
      },
    },
  };
}

export function broadcastJsonLd(input: { username: string; name: string; description: string; image?: string; started?: string }): Ld {
  const url = `${ORIGIN}/${input.username}/live`;
  return {
    "@context": "https://schema.org",
    "@type": "BroadcastEvent",
    name: `${input.name} is live`,
    description: input.description,
    isLiveBroadcast: true,
    startDate: input.started,
    url,
    image: input.image,
    broadcastOfEvent: { "@type": "VideoObject", name: `${input.name} is live`, description: input.description, thumbnailUrl: input.image, url, uploadDate: input.started, publication: { "@type": "BroadcastEvent", isLiveBroadcast: true, startDate: input.started } },
  };
}

export function videoJsonLd(input: { title: string; description: string; url: string; thumbnail?: string | null; embed?: string; authorName: string; authorUrl: string }): Ld {
  return {
    "@context": "https://schema.org",
    "@type": "VideoObject",
    name: input.title,
    description: input.description,
    thumbnailUrl: input.thumbnail ?? undefined,
    url: input.url,
    embedUrl: input.embed,
    author: { "@type": "Person", name: input.authorName, url: input.authorUrl },
  };
}

export type SitemapLists = { channels?: string[]; videos?: { id: string; kind: string }[]; guilds?: string[] };

/** Static public pages plus whatever the API listed. Caps live on the API. */
export function sitemapEntries(lists: SitemapLists): { url: string; changeFrequency: "daily" | "weekly" | "monthly"; priority: number }[] {
  const pages: [string, "daily" | "weekly" | "monthly", number][] = [
    ["/", "daily", 1],
    ["/browse", "daily", 0.8],
    ["/magnet", "daily", 0.8],
    ["/beacons", "daily", 0.7],
    ["/factions", "weekly", 0.6],
    ["/factions/myria", "weekly", 0.5],
    ["/factions/aetheron", "weekly", 0.5],
    ["/factions/glint", "weekly", 0.5],
    ["/guilds", "daily", 0.5],
    ["/war-map", "daily", 0.5],
    ["/wall", "weekly", 0.4],
    ["/about", "monthly", 0.4],
    ["/help", "monthly", 0.3],
    ["/roadmap", "weekly", 0.3],
    ["/supporters", "monthly", 0.3],
    ["/credits", "monthly", 0.2],
    ["/open-data", "weekly", 0.4],
    ["/terms", "monthly", 0.2],
    ["/privacy", "monthly", 0.2],
    ["/guidelines", "monthly", 0.2],
    ["/dmca", "monthly", 0.2],
    ["/contact", "monthly", 0.2],
    ["/take-it-down", "monthly", 0.2],
  ];
  const out = pages.map(([path, changeFrequency, priority]) => ({ url: `${ORIGIN}${path === "/" ? "" : path}`, changeFrequency, priority }));
  for (const name of lists.channels ?? []) {
    if (USERNAME_OK.test(name)) out.push({ url: `${ORIGIN}/${name}`, changeFrequency: "daily", priority: 0.6 });
  }
  for (const video of lists.videos ?? []) {
    if (!/^[0-9a-f-]{36}$/i.test(video.id)) continue;
    const path = video.kind === "CLIP" ? "clips" : "videos";
    out.push({ url: `${ORIGIN}/${path}/${video.id}`, changeFrequency: "weekly", priority: 0.5 });
  }
  for (const slug of lists.guilds ?? []) {
    if (USERNAME_OK.test(slug)) out.push({ url: `${ORIGIN}/g/${slug}`, changeFrequency: "weekly", priority: 0.4 });
  }
  return out;
}

const USERNAME_OK = /^[A-Za-z0-9_]{1,40}$/;
