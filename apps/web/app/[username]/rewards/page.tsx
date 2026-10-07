import Link from "next/link";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";

// docs/PROFILES.md P8: a channel sub-path with no tab. Engagement Valor rewards (docs/SUPPORT.md)
// are redeemed from chat, so this page points there.
export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Rewards");
}
export default async function Rewards({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const name = data.channel.username;
  return <ChannelFrame data={data} path={`/${name}/rewards`}>
    <section className="panel section" data-page="rewards">
      <h2>Channel rewards</h2>
      <p className="muted">Spend the Valor you earn watching on this channel&apos;s rewards. They&apos;re in chat, under Channel rewards.</p>
      <p><Link className="button quiet" href={`/${name}/live`}>Open chat</Link></p>
    </section>
  </ChannelFrame>;
}
