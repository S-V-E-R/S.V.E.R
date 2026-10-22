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
  const description = c.bio?.trim() ? c.bio.slice(0, 200) : c.live ? `${c.display_name} is live on S.V.E.R` : "Channel on S.V.E.R";
  const path = `https://sver.tv/${c.username}${section ? `/${section.toLowerCase().replaceAll(" ", "-")}` : ""}`;
  const liveStill = c.live_preview ? `https://sver.tv${c.live_preview}` : undefined;
  const avatar = c.avatar?.["400"];
  const image = liveStill ?? avatar;
  return {
    title,
    description,
    alternates: { canonical: path },
    openGraph: {
      title, description, url: path, siteName: "S.V.E.R", type: "profile",
      images: image ? [{ url: image, width: liveStill ? 1280 : 400, height: liveStill ? 720 : 400, alt: c.display_name }] : undefined,
    },
    twitter: { card: liveStill ? "summary_large_image" : "summary", title, description, images: image ? [image] : undefined },
    robots: { index: true, follow: true },
  };
}

export type ChannelParams = Promise<{ username: string }>;
