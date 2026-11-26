import Link from "next/link";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { People, type PeoplePage } from "../../../components/People";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Followers");
}
export default async function FollowersList({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ cursor?: string }> }) {
  const { username } = await params;
  const cursor = (await searchParams).cursor;
  // The list read runs alongside the channel read; loadChannel redirects unless the name is canonical.
  const [data, page] = await Promise.all([
    loadChannel(username),
    apiGet<PeoplePage>(`/api/channels/${encodeURIComponent(username)}/followers${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`).then(r => r.data),
  ]);
  const name = data.channel.username;
  return <ChannelFrame data={data} path={`/${name}/followers`}>
    <section className="panel section"><div className="row between"><h2>Followers</h2>{cursor && <Link href={`/${name}/followers`}>Back to start</Link>}</div><People page={page} base={`/${name}/followers`} /></section>
  </ChannelFrame>;
}
