import Link from "next/link";
import { notFound } from "next/navigation";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { FanArtGallery, type FanArtPage } from "../../../components/FanArt";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Fan Art");
}
export default async function FanArtTab({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ cursor?: string }> }) {
  const data = await loadChannel((await params).username);
  if (!data.fan_art_enabled && !data.viewer.is_owner) notFound();
  const name = data.channel.username;
  const cursor = (await searchParams).cursor;
  const page = (await apiGet<FanArtPage>(`/api/channels/${encodeURIComponent(name)}/fan-art${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`)).data;
  if (!page) notFound();
  return <ChannelFrame data={data} path={`/${name}/fan-art`}>
    <section className="section">
      <h2>Fan Art</h2>
      {!page.enabled && <p className="panel muted">Fan art is turned off. <Link href="/studio/channel/fan-art">Turn it on in Creator Studio</Link>.</p>}
      <FanArtGallery username={name} page={page} />
      <nav className="pager">{cursor && <Link href={`/${name}/fan-art`}>Newest</Link>}{page.next_cursor && <Link href={`/${name}/fan-art?cursor=${encodeURIComponent(page.next_cursor)}`}>More</Link>}</nav>
    </section>
  </ChannelFrame>;
}
