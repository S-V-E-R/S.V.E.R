import Link from "next/link";
import { ChannelFrame } from "../../components/ChannelFrame";
import { Occurrences } from "../../components/Schedule";
import { Crest } from "../../components/FactionIdentity";
import { PageBlocks } from "../../components/PageBlocks";
import { VideoGrid } from "../../components/VideoGrid";
import type { VideoCard } from "../../lib/videos";
import { Avatar } from "../../components/Avatar";
import { WallPost } from "../../components/Wall";
import { ActivityFeed } from "../../components/ActivityFeed";
import { Achievements } from "../../components/Achievements";
import { Shine } from "../../components/Shine";
import { apiGet } from "../../lib/server-api";
import { channelMetadata, loadChannel, type ChannelParams } from "../../lib/channel";
import { HOME_SECTIONS, type ActivityItem, type Channel, type Chip, type HomeSection } from "../../lib/types";

/** A square War Council tile linking to the member's channel; position 1 carries a static crown (docs/PROFILES.md, War Council). */
function CouncilTile({ user, crown }: { user: Chip; crown: boolean }) {
  const body = <>
    {crown && <span className="crown" title="Top spot" role="img" aria-label="Top spot"><svg width="14" height="14" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeLinejoin="round" aria-hidden="true"><path d="M1.5 9.5h9L10 3.5 7.8 5.5 6 2 4.2 5.5 2 3.5z" /></svg></span>}
    <Avatar sizes={user.avatar} name={user.display_name} size={72} />
    {user.faction && <Crest faction={user.faction} size={18} />}<strong>{user.display_name}</strong>
    {user.username && <span className="handle">@{user.username}</span>}
  </>;
  return user.linked && user.username ? <Link className="council-tile" href={`/${user.username}`}>{body}</Link> : <span className="council-tile">{body}</span>;
}

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username);
}

/** "You're a Regular here · Following since March 2026 · You're on their War Council": only about the viewer. */
function RelationshipLine({ data }: { data: Channel }) {
  const r = data.viewer.relationship;
  if (!r) return null;
  const parts = [
    r.loyalty && <>You&apos;re a <strong>{r.loyalty}</strong> here</>,
    r.following_since && <>Following since {new Date(r.following_since).toLocaleDateString("en-US", { month: "long", year: "numeric" })}</>,
    r.on_their_council && <>You&apos;re on their <strong>War Council</strong></>,
    r.on_your_council && <>They&apos;re on your War Council</>,
  ].filter(Boolean);
  if (!parts.length) return null;
  return <p className="relationship-line">{parts.map((p, i) => <span key={i}>{p}</span>)}</p>;
}

/** The owner's own Home: every empty part of the page in one place, instead of a prompt in each section. */
function FinishYourPage({ data }: { data: Channel }) {
  const c = data.channel;
  const steps: [boolean, string, string][] = [
    [!!c.avatar, "Add an avatar", "/settings/profile#look"],
    [!!c.banner, "Add a banner", "/settings/profile#look"],
    [!!c.bio, "Write a bio", "/settings/profile#words"],
    [!!data.header?.intro_body, "Write an intro for Home", "/settings/profile#words"],
    [!!c.currently, "Say what you're up to (Currently)", "/settings/profile#words"],
    [!!c.song, "Pick a profile song", "/settings/profile#sound"],
    [!!data.blocks?.length, "Add About me blocks", "/studio/channel/blocks"],
    [data.war_council.members.length > 0, "Choose your War Council", "/studio/channel/war-council"],
    [!!c.showcase?.length, "Showcase achievements", "/settings/profile#look"],
    [!!data.schedule_next.items.length || !c.streamed, "Set a schedule", "/studio/channel/schedule"],
  ];
  const left = steps.filter(([done]) => !done);
  if (!left.length) return null;
  return <section className="panel section finish-page" aria-labelledby="finish-h">
    <div className="row between"><h2 id="finish-h">Finish your page</h2><span className="muted small">{steps.length - left.length} of {steps.length} done</span></div>
    <p className="muted">Only you see this. <Link href="/settings/profile/setup">Set up your page step by step</Link>, or pick one:</p>
    <ul className="finish-list">{left.map(([, label, href]) => <li key={label}><Link href={href}>{label}</Link></li>)}</ul>
  </section>;
}

export default async function ChannelHome({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const { channel: c, viewer } = data;
  const council = data.war_council.members;
  const posts = [...data.wall_preview.pinned, ...data.wall_preview.latest];
  const schedule = data.schedule_next.items;
  const [activityRes, videosRes] = await Promise.all([
    apiGet<{ items: ActivityItem[]; next_cursor: string | null }>(`/api/channels/${encodeURIComponent(c.username)}/activity`),
    c.has_videos ? apiGet<{ videos: VideoCard[] }>(`/api/channels/${encodeURIComponent(c.username)}/videos?popular=false&offset=0`) : Promise.resolve({ data: null }),
  ]);
  const activity = activityRes.data;
  const videos = (videosRes.data?.videos ?? []).filter(v => v.video.kind !== "VOD").slice(0, 3);
  const intro = data.header?.intro_body ? data.header : null;
  const owner = viewer.is_owner;
  const order: HomeSection[] = c.home_order?.length ? c.home_order : [...HOME_SECTIONS];
  // Each section renders only with content; the owner's empty ones are listed in Finish your page.
  const sections: Record<HomeSection, React.ReactNode> = {
    about_me: (intro || !!data.blocks?.length) && <div key="about_me">
      {intro && <section className="panel section intro-card"><h2>{intro.intro_title || "About this page"}</h2><p className="intro-body">{intro.intro_body}</p></section>}
      {!!data.blocks?.length && <PageBlocks blocks={data.blocks} />}
    </div>,
    war_council: council.length > 0 && <section key="war_council" className="panel section">
      <h2>War Council</h2>
      <ol className="council">{council.map(m => <li key={m.position} className={m.crown ? "crowned" : undefined}><CouncilTile user={m.user} crown={m.crown} /></li>)}</ol>
      {owner && data.war_council.unavailable_count > 0 && <p className="muted">{data.war_council.unavailable_count} member(s) are no longer available and are hidden. <Link href="/studio/channel/war-council">Review</Link></p>}
    </section>,
    wall: (posts.length > 0 || data.wall_preview.viewer.can_post) && <section key="wall" className="panel section">
      <div className="row between"><h2>Wall</h2><Link href={`/${c.username}/wall`} className="button small">{owner ? "Open the Wall" : "Sign the Wall"}</Link></div>
      {posts.length ? posts.map(p => <WallPost key={p.id} post={p} viewer={data.wall_preview.viewer} wallOwner={c.username} />) : <p className="muted">Nobody has signed yet. Be the first.</p>}
    </section>,
    up_next: schedule.length > 0 && <section key="up_next" className="panel section">
      <div className="row between"><h2>Up next</h2>{data.tabs.schedule && <Link href={`/${c.username}/schedule`}>Full schedule</Link>}</div>
      <Occurrences items={schedule} zone={data.schedule_next.timezone} />
    </section>,
    videos: videos.length > 0 && <section key="videos" className="section">
      <div className="row between"><h2>Latest videos</h2><Link href={`/${c.username}/videos`}>All videos</Link></div>
      <VideoGrid items={videos} />
    </section>,
    shine: <Shine key="shine" username={c.username} isOwner={owner} signedIn={!!viewer.signed_in} />,
    achievements: <Achievements key="achievements" username={c.username} />,
    activity: activity && <ActivityFeed key="activity" username={c.username} initial={activity} isOwner={owner} />,
  };
  return <ChannelFrame data={data} path={`/${c.username}`}>
    {!owner && <RelationshipLine data={data} />}
    {owner && <FinishYourPage data={data} />}
    {c.plays && <section className="panel section"><h2>SVER Plays · 24/7</h2><p>Watch the game and vote for its next move.</p><Link className="button" href={`/${c.username}/live`}>Watch and play</Link></section>}
    {order.map(key => sections[key])}
  </ChannelFrame>;
}
