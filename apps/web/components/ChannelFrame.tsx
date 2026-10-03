import Link from "next/link";
import { joined, platformNames, type Channel } from "../lib/types";
import { Avatar } from "./Avatar";
import { ChannelActions } from "./ChannelActions";
import { ChannelTabs } from "./ChannelTabs";
import { LivePlayer } from "./LivePlayer";
import { SongPlayer } from "./SongPlayer";
import "../styles/profiles.css";

/** Header frame shared by every channel tab (docs/PROFILES.md, "Header frame"). */
export function ChannelFrame({ data, path, children }: { data: Channel; path: string; children: React.ReactNode }) {
  const c = data.channel;
  const banner = c.banner ? Object.entries(c.banner) : [];
  return <div className="channel">
    <section className="player-slot" aria-label="Stream">
      <LivePlayer username={c.username}>
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {banner.length ? <img className="banner" src={banner[0][1]} srcSet={banner.map(([w, u]) => `${u} ${w}w`).join(", ")} sizes="(max-width: 900px) 100vw, 1100px" alt="" /> : <div className="banner default-banner" aria-hidden="true" />}
      <span className="offline badge">Offline</span>
      </LivePlayer>
    </section>
    <section className="identity panel">
      <Avatar sizes={c.avatar} name={c.display_name} size={112} />
      <div className="identity-text">
        <h1>{c.display_name}</h1>
        <p className="handle">@{c.username}</p>
        {(c.mood_emoji || c.status_text) && <p className="status-line">{c.mood_emoji && <span aria-label="Mood">{c.mood_emoji}</span>} {c.status_text}</p>}
        <span className="faction-slot" aria-hidden="true" />
        {c.bio && <p className="bio">{c.bio}</p>}
        {c.links.length > 0 && <ul className="links">{c.links.map(l => <li key={l.url}><a href={l.url} rel="nofollow noopener noreferrer ugc" target="_blank">{platformNames[l.platform] || l.platform}</a></li>)}</ul>}
        <p className="counts"><Link href={`/${c.username}/followers`}><strong>{c.follower_count.toLocaleString()}</strong> followers</Link><Link href={`/${c.username}/following`}><strong>{c.following_count.toLocaleString()}</strong> following</Link><span className="muted">Joined {joined(c.joined_at)}</span></p>
      </div>
      <ChannelActions username={c.username} viewer={data.viewer} path={path} />
    </section>
    {c.song && <SongPlayer song={c.song} />}
    {c.song_notice && data.viewer.is_owner && <p className="panel notice">Spotify links aren&apos;t supported. <Link href="/studio/channel/song">Add a YouTube or SoundCloud track</Link>.</p>}
    <ChannelTabs username={c.username} tabs={data.tabs} />
    <div className="channel-body">{children}</div>
  </div>;
}
