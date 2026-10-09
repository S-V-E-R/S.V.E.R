import Link from "next/link";
import { Board } from "../../../components/Board";
import { Chat } from "../../../components/Chat";
import { Crowd } from "../../../components/Crowd";
import { LivePlayer } from "../../../components/LivePlayer";
import { PlaysControls } from "../../../components/PlaysControls";
import { EditStreamInfo } from "../../../components/EditStreamInfo";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { currentAccount } from "../../session";
import { Crest } from "../../../components/FactionIdentity";
import { factionInfo } from "../../../lib/factions";
import { Avatar } from "../../../components/Avatar";
import { ChannelActions } from "../../../components/ChannelActions";
import { SectionHead, StreamGrid } from "../../../components/home/Shelves";
import type { LiveCard } from "../../../components/home/types";
import { apiGet } from "../../../lib/server-api";
import { CoStreamPlayers, type Squad } from "../../../components/Squads";
import "../../../styles/profiles.css";
import "../../../styles/home.css";
import "../../../styles/teams.css";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Live");
}

/**
 * Focused watch layout (docs/DESIGN.md "Watch page"): 16:9 player, streamer bar, the stream's
 * interactive panels (Plays, CrowdSync, board), Up next, and chat at 340 px. Chat stays open while offline.
 */
export default async function Live({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const c = data.channel;
  const [account, suggestions, costream] = await Promise.all([currentAccount(), apiGet<{ items: LiveCard[] }>(`/api/channels/${encodeURIComponent(c.username)}/suggestions`), apiGet<{ squad: Squad | null }>(`/api/channels/${encodeURIComponent(c.username)}/squad`)]);
  const squad = costream.data?.squad ?? null;
  const squadHost = squad?.members.find(m => m.host)?.username;
  const next = suggestions.data?.items.slice(0, 4) ?? [];
  return <div className="channel watch" data-theme={c.faction ?? "neutral"}>
    <div className="watch-main">
    {squad ? <CoStreamPlayers initial={squad} focus={c.username} account={account?.username ?? null} /> : <div className="watch-player">
    <LivePlayer username={c.username} focused signedIn={!!account}>
      <section className="watch-offline" aria-label="Stream">
        <h2>{c.display_name} is offline</h2>
        <p><Link className="button quiet" href={`/${c.username}`}>Go to the channel</Link></p>
      </section>
    </LivePlayer>
    </div>}
    <section className="streamer-bar" aria-label="Streamer">
      <Link href={`/${c.username}`} aria-label={`${c.display_name}'s channel`}>{c.faction ? <Crest faction={c.faction} size={48} /> : <Avatar sizes={c.avatar} name={c.display_name} size={48} />}</Link>
      <div className="streamer-name"><h1><Link href={`/${c.username}`}>{c.display_name}</Link></h1><p className="handle">@{c.username}</p><p className="streamer-tags">{c.faction && <Link href={`/factions/${c.faction}`} className="badge faction-tag">{factionInfo(c.faction).name}</Link>}<span className="muted">{c.follower_count.toLocaleString()} followers</span></p></div>
      <ChannelActions username={c.username} displayName={c.display_name} viewer={data.viewer} path={`/${c.username}/live`} />
      {account && <EditStreamInfo username={c.username} />}
    </section>
    <div className="watch-interact">
      {c.plays && <PlaysControls username={c.username} />}
      <Crowd username={c.username} />
      {c.board && <Board username={c.username} />}
    </div>
    <section className="watch-next" aria-labelledby="up-next"><SectionHead id="up-next" title="Up next" note="Same genre first, then same faction" href="/browse" link="Browse" />{next.length ? <StreamGrid streams={next} viewerFaction={account?.faction ?? null} /> : <p className="shelf-empty panel">{suggestions.data ? "No other streams are live right now." : "Live channels couldn’t be loaded."} <Link href="/">Explore the homepage</Link></p>}</section>
    </div>
    {squad?.mode === "MERGED" && squadHost ? <Chat key={squad.id} username={squadHost} squad={squad.id} account={account?.username ?? null} /> : <Chat username={c.username} account={account?.username ?? null} />}
  </div>;
}
