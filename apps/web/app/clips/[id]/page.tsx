import type { Metadata } from "next";
import Recording from "../../videos/[id]/page";
import { apiGet } from "../../../lib/server-api";
type Share = { title: string; author_name: string; url: string; embed: string; mp4: string; thumbnail: string | null };
export async function generateMetadata({ params }: { params: Promise<{ id: string }> }): Promise<Metadata> {
  const { id } = await params;
  const { data } = await apiGet<Share>(`/api/videos/${encodeURIComponent(id)}/share`);
  if (!data) return { title: "Clip | S.V.E.R" };
  return { title: `${data.title} | S.V.E.R`, description: `A clip from ${data.author_name}`, alternates: { canonical: data.url, types: { "application/json+oembed": `/api/oembed?url=${encodeURIComponent(data.url)}` } }, openGraph: { type: "video.other", title: data.title, url: data.url, images: data.thumbnail ? [data.thumbnail] : [], videos: [{ url: data.mp4, secureUrl: data.mp4, type: "video/mp4", width: 1280, height: 720 }] }, twitter: { card: "player", title: data.title, images: data.thumbnail ? [data.thumbnail] : [], players: [{ playerUrl: data.embed, streamUrl: data.mp4, width: 1280, height: 720 }] } };
}
export default Recording;
