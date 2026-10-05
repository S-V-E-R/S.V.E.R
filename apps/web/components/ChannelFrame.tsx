import Link from "next/link";
import { joined, linkHost, platformNames, type Channel } from "../lib/types";
import { PlatformIcon } from "./PlatformIcon";
import { Avatar } from "./Avatar";
import { ChannelActions } from "./ChannelActions";
import { ChannelTabs } from "./ChannelTabs";
import { LivePlayer } from "./LivePlayer";
import { SongPlayer } from "./SongPlayer";
import { Crest } from "./Crest";
import { Crest as FactionCrest } from "./FactionIdentity";
import { factionInfo, factionOf } from "../lib/factions";
import "../styles/profiles.css";

/** Header frame shared by every channel tab (docs/PROFILES.md, "Header frame"). */
export function ChannelFrame({ data, path, children }: { data: Channel; path: string; children: React.ReactNode }) {
  const c = data.channel;
  const banner = c.banner ? Object.entries(c.banner) : [];
  const faction = factionOf(c.faction);
  // Channel pages wear the owner's faction colors inside the channel area (decided October 3, 2026).
  return <div className="channel" data-theme={faction?.slug ?? "neutral"}>
    <section className={banner.length ? "player-slot" : "player-slot no-banner"} aria-label="Stream">
      <LivePlayer username={c.username}>
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {banner.length ? <img className="banner" src={banner[0][1]} srcSet={banner.map(([w, u]) => `${u} ${w}w`).join(", ")} sizes="(max-width: 900px) 100vw, 1100px" alt="" /> : <div className="banner default-banner" aria-hidden="true">{faction && <Crest faction={faction.slug} initial="" size={88} />}</div>}
      <span className="offline badge">Offline</span>
      </LivePlayer>
    </section>
    <section className="identity panel frame">
      <Avatar sizes={c.avatar} name={c.display_name} size={112} />
      <div className="identity-text">
        {data.header?.label && <span className="page-label">{data.header.label}</span>}
        <h1>{c.display_name}</h1>
        <p className="handle">@{c.username}</p>
        {data.header?.welcome && <p className="welcome-line">{data.header.welcome}</p>}
        {(c.mood_emoji || c.status_text) && <p className="status-line">{c.mood_emoji && <span aria-label="Mood">{c.mood_emoji}</span>} {c.status_text}</p>}
        {faction && <p className="faction-line"><Link href={`/factions/${faction.slug}`}><Crest faction={faction.slug} initial="" size={22} />{faction.name} · {faction.title}</Link></p>}
        {c.bio && <p className="bio">{c.bio}</p>}
        {c.links.length > 0 && <ul className="links">{c.links.map(l => <li key={l.url}><a href={l.url} rel="nofollow noopener noreferrer ugc" target="_blank"><PlatformIcon platform={l.platform} /><span>{platformNames[l.platform] || l.platform}</span>{linkHost(l.url) && <small className="link-host">{linkHost(l.url)}</small>}</a></li>)}</ul>}
        <p className="counts"><Link href={`/${c.username}/followers`}><strong>{c.follower_count.toLocaleString()}</strong> followers</Link><Link href={`/${c.username}/following`}><strong>{c.following_count.toLocaleString()}</strong> following</Link><span className="muted">Joined {joined(c.joined_at)}</span>{data.header?.vibe && <span className="muted page-vibe">Vibe: <strong>{data.header.vibe}</strong></span>}</p>
      </div>
      <ChannelActions username={c.username} displayName={c.display_name} viewer={data.viewer} path={path} />
    </section>
    {!!c.season_rewards?.length && <div className="season-banner" data-theme={c.season_rewards[0].faction}><FactionCrest faction={c.season_rewards[0].faction} size={44} /><div><strong>Season {c.season_rewards[0].season} champion</strong><p>{factionInfo(c.season_rewards[0].faction).name} · {c.season_rewards.map(r => `Season ${r.season}`).join(" · ")}</p></div><span className="badge">Season victor</span></div>}
    {c.song && <SongPlayer song={c.song} />}
    {c.song_notice && data.viewer.is_owner && <p className="panel notice">Spotify links aren&apos;t supported. <Link href="/studio/channel/song">Add a YouTube or SoundCloud track</Link>.</p>}
    <ChannelTabs username={c.username} tabs={data.tabs} />
    <div className="channel-body">{children}</div>
  </div>;
}
