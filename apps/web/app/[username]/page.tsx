import Link from "next/link";
import { ChannelFrame } from "../../components/ChannelFrame";
import { Occurrences } from "../../components/Schedule";
import { Crest } from "../../components/FactionIdentity";
import { Avatar } from "../../components/Avatar";
import { WallPost } from "../../components/Wall";
import { ActivityFeed } from "../../components/ActivityFeed";
import { apiGet } from "../../lib/server-api";
import { channelMetadata, loadChannel, type ChannelParams } from "../../lib/channel";
import type { ActivityItem, Chip } from "../../lib/types";

/** A square War Council tile linking to the member's channel; position 1 carries a static crown (docs/PROFILES.md, War Council). */
function CouncilTile({ user, crown }: { user: Chip; crown: boolean }) {
  const body = <>
    {crown && <span className="crown" title="Top spot" aria-label="Top spot">♛</span>}
    <Avatar sizes={user.avatar} name={user.display_name} size={72} />
    {user.faction && <Crest faction={user.faction} size={18} />}<strong>{user.display_name}</strong>
    {user.username && <span className="handle">@{user.username}</span>}
  </>;
  return user.linked && user.username ? <Link className="council-tile" href={`/${user.username}`}>{body}</Link> : <span className="council-tile">{body}</span>;
}

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username);
}

export default async function ChannelHome({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const { channel: c, viewer } = data;
  const council = data.war_council.members;
  const posts = [...data.wall_preview.pinned, ...data.wall_preview.latest];
  const schedule = data.schedule_next.items;
  const activity = (await apiGet<{ items: ActivityItem[]; next_cursor: string | null }>(`/api/channels/${encodeURIComponent(c.username)}/activity`)).data;
  const intro = data.header?.intro_body ? data.header : null;
  return <ChannelFrame data={data} path={`/${c.username}`}>
    {intro && <section className="panel section intro-card"><h2>{intro.intro_title || "About this page"}</h2><p className="intro-body">{intro.intro_body}</p></section>}
    {(council.length > 0 || viewer.is_owner) && <section className="panel section">
      <h2>War Council</h2>
      {council.length ? <ol className="council">{council.map(m => <li key={m.position} className={m.crown ? "crowned" : undefined}><CouncilTile user={m.user} crown={m.crown} /></li>)}</ol> : <p className="muted">Pick up to 8 channels for your War Council. <Link href="/studio/channel/war-council">Add members</Link></p>}
      {viewer.is_owner && data.war_council.unavailable_count > 0 && <p className="muted">{data.war_council.unavailable_count} member(s) are no longer available and are hidden. <Link href="/studio/channel/war-council">Review</Link></p>}
    </section>}
    <section className="panel section">
      <div className="row between"><h2>Wall</h2><Link href={`/${c.username}/wall`} className="button small">Sign the Wall</Link></div>
      {posts.length ? posts.map(p => <WallPost key={p.id} post={p} viewer={data.wall_preview.viewer} />) : <p className="muted">No posts yet.</p>}
    </section>
    {(schedule.length > 0 || viewer.is_owner) && <section className="panel section">
      <div className="row between"><h2>Up next</h2>{data.tabs.schedule && <Link href={`/${c.username}/schedule`}>Full schedule</Link>}</div>
      {schedule.length ? <Occurrences items={schedule} zone={data.schedule_next.timezone} /> : <p className="muted">Add your weekly schedule. <Link href="/studio/channel/schedule">Set schedule</Link></p>}
    </section>}
    {activity && <ActivityFeed username={c.username} initial={activity} isOwner={viewer.is_owner} />}
  </ChannelFrame>;
}
