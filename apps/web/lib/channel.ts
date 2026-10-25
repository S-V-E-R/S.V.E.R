import type { Metadata } from "next";
import { notFound, permanentRedirect, redirect } from "next/navigation";
import { cache } from "react";
import { apiGet } from "./server-api";
import type { Channel } from "./types";

/** One channel read per request, shared by metadata and the page; every miss is the same 404. */
export const loadChannel = cache(async (username: string): Promise<Channel> => {
  const { status, data } = await apiGet<Channel>(`/api/channels/${encodeURIComponent(username)}`);
  if (status !== 200 || !data) notFound();
  if (data.redirect_to) redirect(`/${data.redirect_to}`);
  if (data.channel.username !== username) permanentRedirect(`/${data.channel.username}`);
  return data;
});

export async function channelMetadata(username: string, section?: string): Promise<Metadata> {
  const { status, data } = await apiGet<Channel>(`/api/channels/${encodeURIComponent(username)}`);
  if (status !== 200 || !data?.channel) return { title: "This channel doesn't exist. | S.V.E.R", robots: { index: false, follow: false } };
  const c = data.channel;
  const title = `${c.display_name} (@${c.username})${section ? ` · ${section}` : ""} | S.V.E.R`;
  const description = c.bio?.trim() ? c.bio.slice(0, 200) : "Channel on S.V.E.R";
  // The share card (app/[username]/opengraph-image.tsx), named here so every channel tab carries it.
  const images = [{ url: `https://sver.tv/${c.username}/opengraph-image`, width: 1200, height: 630, alt: `${c.display_name} on S.V.E.R` }];
  return {
    title,
    description,
    alternates: { canonical: `https://sver.tv/${c.username}${section ? `/${section.toLowerCase().replace(" ", "-")}` : ""}` },
    openGraph: { title, description, url: `https://sver.tv/${c.username}`, siteName: "S.V.E.R", type: "profile", images },
    twitter: { card: "summary_large_image", title, description, images },
    robots: { index: false, follow: false },
  };
}

export type ChannelParams = Promise<{ username: string }>;
