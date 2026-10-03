import Link from "next/link";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { People, type PeoplePage } from "../../../components/People";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Following");
}
export default async function FollowingList({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ cursor?: string }> }) {
  const data = await loadChannel((await params).username);
  const name = data.channel.username;
  const cursor = (await searchParams).cursor;
  const page = (await apiGet<PeoplePage>(`/api/channels/${encodeURIComponent(name)}/following${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`)).data;
  return <ChannelFrame data={data} path={`/${name}/following`}>
    <section className="panel section"><div className="row between"><h2>Following</h2>{cursor && <Link href={`/${name}/following`}>Back to start</Link>}</div><People page={page} base={`/${name}/following`} /></section>
  </ChannelFrame>;
}
