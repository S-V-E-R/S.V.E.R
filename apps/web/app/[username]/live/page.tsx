import Link from "next/link";
import { Chat } from "../../../components/Chat";
import { LivePlayer } from "../../../components/LivePlayer";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { currentAccount } from "../../session";
import { Crest } from "../../../components/FactionIdentity";
import { factionInfo } from "../../../lib/factions";
import { Avatar } from "../../../components/Avatar";
import { ChannelActions } from "../../../components/ChannelActions";
import { ShelfHeading, StreamShelf, type StreamDirectory } from "../../../components/StreamShelf";
import { apiGet } from "../../../lib/server-api";
import "../../../styles/profiles.css";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Live");
}

/** Focused watch layout: the player and chat, with the channel one link away. Chat stays open while offline. */
export default async function Live({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const c = data.channel;
  const [account, directory] = await Promise.all([currentAccount(), apiGet<StreamDirectory>("/api/streams")]);
  const next = directory.data?.live.filter(stream => stream.user.username !== c.username).slice(0, 4) ?? [];
  return <div className="channel watch" data-theme={c.faction ?? "neutral"}>
    <div className="watch-main">
    <div className="watch-player frame">
    <LivePlayer username={c.username} focused signedIn={!!account}>
      <section className="watch-offline" aria-label="Stream">
        <h2>{c.display_name} is offline</h2>
        <p><Link className="button quiet" href={`/${c.username}`}>Go to the channel</Link></p>
      </section>
    </LivePlayer>
    </div>
    <section className="streamer-bar" aria-label="Streamer">
      <Link href={`/${c.username}`} aria-label={`${c.display_name}'s channel`}>{c.faction ? <Crest faction={c.faction} size={48} /> : <Avatar sizes={c.avatar} name={c.display_name} size={48} />}</Link>
      <div className="streamer-name"><h1><Link href={`/${c.username}`}>{c.display_name}</Link></h1><p className="handle">@{c.username}</p>{c.faction && <Link href={`/factions/${c.faction}`}>{factionInfo(c.faction).name}</Link>}<p className="muted">{c.follower_count.toLocaleString()} followers</p></div>
      <ChannelActions username={c.username} displayName={c.display_name} viewer={data.viewer} path={`/${c.username}/live`} />
    </section>
    <section className="watch-next" aria-labelledby="up-next"><ShelfHeading id="up-next" title="Up next" href="/" link="All live streams" />{next.length ? <StreamShelf streams={next} /> : <p className="shelf-empty frame">{directory.data ? "No other streams are live right now." : "Live channels couldn’t be loaded."} <Link href="/">Explore the homepage</Link></p>}</section>
    </div>
    <Chat username={c.username} account={account?.username ?? null} />
  </div>;
}
