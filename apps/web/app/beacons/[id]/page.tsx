import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { BeaconFeed } from "../../../components/BeaconFeed";
import type { BeaconItem } from "../../../lib/beacons";
import { apiGet } from "../../../lib/server-api";
import { currentAccount } from "../../session";

type Share = { title: string; author_name: string; url: string; mp4: string; thumbnail: string | null };

/** Preview tags so a link shows the video on Discord, X and Reddit (public, non-18+ Beacons). */
export async function generateMetadata({ params }: { params: Promise<{ id: string }> }): Promise<Metadata> {
  const { id } = await params;
  const { data } = await apiGet<Share>(`/api/beacons/${encodeURIComponent(id)}/share`);
  if (!data) return { title: "Beacon | S.V.E.R" };
  const images = data.thumbnail ? [{ url: data.thumbnail, width: 540, height: 960 }] : [];
  return {
    title: `${data.title} | S.V.E.R`, description: `A Beacon from ${data.author_name}`, alternates: { canonical: data.url },
    openGraph: { type: "video.other", title: data.title, description: `A Beacon from ${data.author_name}`, url: data.url, images, videos: [{ url: data.mp4, secureUrl: data.mp4, type: "video/mp4", width: 720, height: 1280 }] },
    twitter: { card: "player", title: data.title, images: data.thumbnail ? [data.thumbnail] : [], players: [{ playerUrl: data.url, streamUrl: data.mp4, width: 720, height: 1280 }] },
  };
}

/** One Beacon's page: it plays first, then the feed continues into the viewer's rotation. */
export default async function BeaconPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const [account, result] = await Promise.all([currentAccount(), apiGet<BeaconItem>(`/api/beacons/${encodeURIComponent(id)}`)]);
  if (result.status === 404 || result.status === 400) notFound();
  const gated = result.status === 403 && !result.data;
  return <>
    <h1 className="sr-only">{result.data?.beacon.title ?? "Beacon"}</h1>
    <BeaconFeed key={id} initial={null} start={result.data} signedIn={!!account} gateId={gated ? id : null} />
  </>;
}
