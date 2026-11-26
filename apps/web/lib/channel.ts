import type { Metadata } from "next";
import { notFound, permanentRedirect, redirect } from "next/navigation";
import { cache } from "react";
import { apiGet } from "./server-api";
import type { Channel } from "./types";

/** One channel read per request, shared by metadata and the page. Tabs other than Home skip the
 *  Home-only sections (War Council and Wall preview come back null, About blocks empty). */
const readChannel = cache((username: string, home: boolean) =>
  apiGet<Channel>(`/api/channels/${encodeURIComponent(username)}${home ? "" : "?tab=1"}`));

/** The channel for a page (`home` only on Home). Every miss is the same 404; an outage is an error, not a miss. */
export const loadChannel = cache(async (username: string, home = false): Promise<Channel> => {
  const { status, data } = await readChannel(username, home);
  if (status === 404) notFound();
  if (status !== 200 || !data) throw new Error(`Channel read failed (${status})`);
  if (data.redirect_to) redirect(`/${data.redirect_to}`);
  if (data.channel.username !== username) permanentRedirect(`/${data.channel.username}`);
  return data;
});

export async function channelMetadata(username: string, section?: string): Promise<Metadata> {
  const { status, data } = await readChannel(username, !section);
  if (status === 404) return { title: "This channel doesn't exist. | S.V.E.R", robots: { index: false, follow: false } };
  // An outage isn't a missing channel: no noindex, so crawlers keep the real page.
  if (status !== 200 || !data?.channel) return { title: "S.V.E.R" };
  const c = data.channel;
  const title = `${c.display_name} (@${c.username})${section ? ` · ${section}` : ""} | S.V.E.R`;
  const description = c.bio?.trim() ? c.bio.slice(0, 200) : c.live ? `${c.display_name} is live on S.V.E.R` : "Channel on S.V.E.R";
  // /{name} redirects to /{name}/live while live, so the live page's canonical is the channel root.
  const path = `https://sver.tv/${c.username}${section && section !== "Live" ? `/${section.toLowerCase().replaceAll(" ", "-")}` : ""}`;
  // The share card (app/[username]/opengraph-image.tsx), named here so every channel tab carries it.
  const images = [{ url: `https://sver.tv/${c.username}/opengraph-image`, width: 1200, height: 630, alt: `${c.display_name} on S.V.E.R` }];
  return {
    title,
    description,
    alternates: { canonical: path },
    openGraph: { title, description, url: path, siteName: "S.V.E.R", type: "profile", images },
    twitter: { card: "summary_large_image", title, description, images },
    robots: { index: true, follow: true },
  };
}

export type ChannelParams = Promise<{ username: string }>;
