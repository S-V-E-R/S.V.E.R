import Link from "next/link";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { People, type PeoplePage } from "../../../components/People";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Followers");
}
export default async function FollowersList({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ cursor?: string }> }) {
  const data = await loadChannel((await params).username);
  const name = data.channel.username;
  const cursor = (await searchParams).cursor;
  const page = (await apiGet<PeoplePage>(`/api/channels/${encodeURIComponent(name)}/followers${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`)).data;
  return <ChannelFrame data={data} path={`/${name}/followers`}>
    <section className="panel section"><div className="row between"><h2>Followers</h2>{cursor && <Link href={`/${name}/followers`}>Back to start</Link>}</div><People page={page} base={`/${name}/followers`} /></section>
  </ChannelFrame>;
}
