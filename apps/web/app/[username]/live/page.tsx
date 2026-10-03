import Link from "next/link";
import { Chat } from "../../../components/Chat";
import { LivePlayer } from "../../../components/LivePlayer";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { currentAccount } from "../../session";
import "../../../styles/profiles.css";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Live");
}

/** Focused watch layout: the player and chat, with the channel one link away. Chat stays open while offline. */
export default async function Live({ params }: { params: ChannelParams }) {
  const c = (await loadChannel((await params).username)).channel;
  const account = await currentAccount();
  return <div className="channel watch">
    <div>
    <LivePlayer username={c.username} focused>
      <section className="panel" aria-label="Stream">
        <h1>{c.display_name} is offline</h1>
        <p><Link href={`/${c.username}`}>Go to the channel</Link></p>
      </section>
    </LivePlayer>
    <p><Link href={`/${c.username}`}>{c.display_name}</Link> <span className="muted">@{c.username}</span></p>
    </div>
    <Chat username={c.username} account={account?.username ?? null} />
  </div>;
}
