import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { cache } from "react";
import { JsonLd } from "../../../components/JsonLd";
import { VideoWatch } from "../../../components/VideoWatch";
import { apiGet } from "../../../lib/server-api";
import { videoJsonLd } from "../../../lib/seo";
import type { VideoPage } from "../../../lib/videos";
import { currentAccount } from "../../session";

type Share = { kind: string; title: string; author_name: string; author_url: string; url: string; embed: string; mp4: string | null; thumbnail: string | null };

const loadShare = cache(async (id: string) => apiGet<Share>(`/api/videos/${encodeURIComponent(id)}/share`));

export async function generateMetadata({ params }: { params: Promise<{ id: string }> }): Promise<Metadata> {
  const { data } = await loadShare((await params).id);
  if (!data) return { title: "Recording | S.V.E.R", robots: { index: false, follow: false } };
  const kind = data.kind === "CLIP" ? "clip" : data.kind === "HIGHLIGHT" ? "highlight" : "recording";
  const description = `A ${kind} from ${data.author_name} on S.V.E.R`;
  const images = data.thumbnail ? [{ url: data.thumbnail }] : [];
  const videos = data.mp4 ? [{ url: data.mp4, secureUrl: data.mp4, type: "video/mp4", width: 1280, height: 720 }] : undefined;
  return {
    title: `${data.title} | S.V.E.R`,
    description,
    alternates: { canonical: data.url, ...(data.kind === "CLIP" ? { types: { "application/json+oembed": `/api/oembed?url=${encodeURIComponent(data.url)}` } } : {}) },
    openGraph: { type: "video.other", title: data.title, description, url: data.url, images, videos },
    twitter: data.mp4
      ? { card: "player", title: data.title, description, images: data.thumbnail ? [data.thumbnail] : [], players: [{ playerUrl: data.embed, streamUrl: data.mp4, width: 1280, height: 720 }] }
      : { card: "summary_large_image", title: data.title, description, images: data.thumbnail ? [data.thumbnail] : [] },
  };
}

export default async function Recording({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const [result, share, account] = await Promise.all([apiGet<VideoPage>(`/api/videos/${encodeURIComponent(id)}`), loadShare(id), currentAccount()]);
  if (result.status === 404) notFound();
  const preview = share.data;
  const kind = preview?.kind === "CLIP" ? "clip" : preview?.kind === "HIGHLIGHT" ? "highlight" : "recording";
  return <>
    {preview && <JsonLd data={videoJsonLd({ title: preview.title, description: `A ${kind} from ${preview.author_name} on S.V.E.R`, url: preview.url, thumbnail: preview.thumbnail, embed: preview.embed, authorName: preview.author_name, authorUrl: preview.author_url })} />}
    <VideoWatch key={id} id={id} initial={result.data} credit={account?.username ?? null} />
  </>;
}
