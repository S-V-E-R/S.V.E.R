import { FounderMark, SupporterBadge, SupportingSince } from "./Supporter";
import Link from "next/link";
import { CREATOR_TIERS, joined, linkHost, platformNames, type Channel } from "../lib/types";
import { PlatformIcon } from "./PlatformIcon";
import { Avatar } from "./Avatar";
import { ChannelActions } from "./ChannelActions";
import { ChannelTabs } from "./ChannelTabs";
import { Board } from "./Board";
import { Crowd } from "./Crowd";
import { LivePlayer } from "./LivePlayer";
import { SongPlayer } from "./SongPlayer";
import { Crest } from "./Crest";
import { factionInfo, factionOf } from "../lib/factions";
import { DonateLink } from "./DonateLink";
import { JsonLd } from "./JsonLd";
import { profileJsonLd } from "../lib/seo";
import { currentAccount } from "../app/session";
import "../styles/profiles.css";

/** The next scheduled stream, in the channel's own time zone when it has one. */
function nextStream(data: Channel): string | null {
  const next = data.schedule_next?.items.find(o => !o.live && Date.parse(o.start_at) > Date.now());
  if (!next) return null;
  const when = new Date(next.start_at).toLocaleString("en-US", { weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", timeZone: data.schedule_next.timezone ?? "UTC", timeZoneName: "short" });
  return `Next stream ${when}${next.label ? ` · ${next.label}` : ""}`;
}
const day = (iso: string | null) => iso ? new Date(iso).toLocaleDateString("en-US", { month: "short", year: "numeric", timeZone: "UTC" }) : "";

/**
 * Header frame shared by every channel tab (docs/PROFILES.md "Header frame" and "Persona pass"):
 * the banner, then who this person is at a glance (framed avatar, name with level and title,
 * live mark, mood and Currently, bio, links, song, showcase), then the live area while live, then
 * the tabs. The channel area wears the owner's faction colors in their chosen variant.
 */
export async function ChannelFrame({ data, path, children }: { data: Channel; path: string; children: React.ReactNode }) {
  const account = await currentAccount();
  const c = data.channel;
  const description = c.bio?.trim() ? c.bio.slice(0, 200) : "Channel on S.V.E.R";
  const banner = c.banner ? Object.entries(c.banner) : [];
  const faction = factionOf(c.faction);
  const next = c.live ? null : nextStream(data);
  const owner = data.viewer.is_owner;
  const showcase = c.showcase ?? [];
  const seasons = c.season_rewards ?? [];
  const currently = c.live && c.streaming_category ? { label: "Streaming", text: c.streaming_category } : c.currently && (c.currently.text || c.currently.label) ? c.currently : null;
  // Persona first: while live, a compact card under the header instead of the player.
  const personaFirst = c.layout === "persona";
  const image = c.live_preview ? `https://sver.tv${c.live_preview}` : c.avatar?.["400"];
  return <div className="channel" data-theme={faction?.slug ?? "neutral"} data-variant={c.theme_variant ?? undefined}>
    <JsonLd data={profileJsonLd({ username: c.username, name: c.display_name, description, image, followers: c.follower_count })} />
    <section className="channel-header frame" aria-label={`${c.display_name}'s channel`}>
      <div className={banner.length ? "channel-banner" : "channel-banner none"}>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        {banner.length > 0 && <img className="banner" src={banner[0][1]} srcSet={banner.map(([w, u]) => `${u} ${w}w`).join(", ")} sizes="(max-width: 900px) 100vw, 1100px" alt="" />}
        {/* No banner yet: the owner's crest and home city instead of an empty strip (UI review, October 2026). */}
        {!banner.length && faction && <div className="banner-default" aria-hidden="true"><Crest faction={faction.slug} initial="" size={56} /><span><strong>{factionInfo(faction.slug).homeland.replace(/^the /, "The ")}</strong><small>{faction.name} · {faction.title}</small></span></div>}
      </div>
      <div className="identity">
        <span className="avatar-frame" data-frame={c.frame ?? 0}><Avatar sizes={c.avatar} name={c.display_name} size={96} /></span>
        <div className="identity-text">
          {data.header?.label && <span className="page-label">{data.header.label}</span>}
          <h1>{c.display_name}{c.live && <Link className="live-mark" href={`/${c.username}/live`}>Live</Link>}</h1>
          <p className="handle">@{c.username}{!!c.level && <span className="level-hex player-level" data-frame={c.frame ?? 0} title={`Level ${c.level}`}>Lv {c.level}{c.title && <> · {c.title}</>}</span>}{!!c.creator_tier && <Link href="/help/creator-tiers" className="badge tier-badge" title={`${CREATOR_TIERS[c.creator_tier]} creator tier: see every tier`}>{CREATOR_TIERS[c.creator_tier]}</Link>}{c.supporter?.tier && <SupporterBadge tier={c.supporter.tier} monthsSupported={c.supporter.months ?? undefined} />}{!!c.supporter?.founder && <FounderMark number={c.supporter.founder} />}{c.discord_rank && <span className="badge discord-rank" title={`Discord rank on the S.V.E.R server · Level ${c.discord_rank.level}`}>Discord · {c.discord_rank.name}</span>}</p>
          {faction && <p className="faction-line"><Link href={`/factions/${faction.slug}`}><Crest faction={faction.slug} initial="" size={22} />{faction.name} · {faction.title}</Link></p>}
          {!c.live && next && <p className="next-line">{next}</p>}
          {data.header?.welcome && <p className="welcome-line">{data.header.welcome}</p>}
          {(c.mood_emoji || c.status_text || currently) && <p className="status-line">{c.mood_emoji && <span aria-label="Mood">{c.mood_emoji}</span>}{currently && <span className="currently"><strong>{currently.label}</strong> {currently.text}</span>}{c.status_text && <span>{c.status_text}</span>}</p>}
          {c.supporter?.since && <p className="supporter-line"><SupportingSince since={c.supporter.since} /></p>}
          {c.shine?.live && <p className="shine-banner" role="note">Charity stream for <strong>{c.shine.live.name}</strong> <DonateLink className="button small" url={c.shine.live.url}>Donate to {c.shine.live.name}</DonateLink></p>}
          {c.shine && c.shine.badges.length > 0 && <p className="good-works">{c.shine.badges.map(b => <span key={b.charity + b.verified_at} className="badge good-works-badge" title="Good Works, verified by S.V.E.R staff">Good Works · {b.charity} · ${(b.raised_cents / 100).toLocaleString()}</span>)}</p>}
          {c.bio && <p className="bio">{c.bio}</p>}
          {c.links.length > 0 && <ul className="links">{c.links.map(l => <li key={l.url}><a href={l.url} rel="nofollow noopener noreferrer ugc" target="_blank"><PlatformIcon platform={l.platform} /><span>{platformNames[l.platform] || l.platform}</span>{linkHost(l.url) && <small className="link-host">{linkHost(l.url)}</small>}</a></li>)}</ul>}
          <p className="counts"><Link href={`/${c.username}/followers`}><strong>{c.follower_count.toLocaleString()}</strong> followers</Link><Link href={`/${c.username}/following`}><strong>{c.following_count.toLocaleString()}</strong> following</Link><span className="muted">Joined {joined(c.joined_at)}</span>{data.header?.vibe && <span className="muted page-vibe">Vibe: <strong>{data.header.vibe}</strong></span>}</p>
        </div>
        <div className="identity-side">
          <ChannelActions username={c.username} displayName={c.display_name} viewer={data.viewer} path={path} credit={account?.username ?? null} />
          {c.song && <SongPlayer song={c.song} why={c.song_why} />}
        </div>
      </div>
      {(showcase.length > 0 || seasons.length > 0) ? <ul className="showcase-strip" aria-label="Showcase">
        {seasons.map(r => <li key={`season-${r.season}`} className="showcase-tile season" data-theme={r.faction}><Crest faction={r.faction} initial="" size={28} /><strong>Season {r.season} champion</strong><small>{factionInfo(r.faction).name}</small></li>)}
        {showcase.map(a => <li key={a.key} className="showcase-tile" title={a.description}><svg aria-hidden="true" width="28" height="28" viewBox="0 0 28 28"><path d="M14 2l11 6v12l-11 6-11-6V8z" fill="none" stroke="currentColor" strokeWidth="1.6" /><path d="M14 9l2 4 4.5.6-3.3 3 .9 4.4-4.1-2.2-4.1 2.2.9-4.4-3.3-3L12 13z" fill="currentColor" /></svg><strong>{a.name}</strong>{a.earned_at && <small>{day(a.earned_at)}</small>}</li>)}
      </ul> : owner && <p className="showcase-empty muted small"><Link href="/settings/profile#look">Choose up to 3 achievements to showcase</Link></p>}
    </section>
    {c.live && (personaFirst
      ? <section className="live-now panel" aria-label="Live now"><span className="live badge">Live</span><span>{c.streaming_category ? <>Streaming <strong>{c.streaming_category}</strong></> : "Streaming now"}</span><Link className="button small" href={`/${c.username}/live`}>Watch</Link></section>
      : <>
        <section className="channel-stream" aria-label="Stream"><LivePlayer username={c.username} /></section>
        <Crowd username={c.username} signedIn={!!data.viewer.signed_in} />
        {c.board && <Board username={c.username} />}
      </>)}
    {c.song_notice && owner && <p className="panel notice">Spotify links aren&apos;t supported. <Link href="/settings/profile#sound">Add a YouTube or SoundCloud track</Link>.</p>}
    <ChannelTabs username={c.username} tabs={data.tabs} />
    <div className="channel-body">{children}</div>
  </div>;
}
