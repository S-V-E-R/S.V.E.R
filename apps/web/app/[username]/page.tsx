import Link from "next/link";
import { ChannelFrame } from "../../components/ChannelFrame";
import { Occurrences } from "../../components/Schedule";
import { UserChip } from "../../components/UserChip";
import { WallPost } from "../../components/Wall";
import { channelMetadata, loadChannel, type ChannelParams } from "../../lib/channel";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username);
}

export default async function ChannelHome({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const { channel: c, viewer } = data;
  const council = data.war_council.members;
  const posts = [...data.wall_preview.pinned, ...data.wall_preview.latest];
  const schedule = data.schedule_next.items;
  return <ChannelFrame data={data} path={`/${c.username}`}>
    {(council.length > 0 || viewer.is_owner) && <section className="panel section">
      <h2>War Council</h2>
      {council.length ? <ol className="council">{council.map(m => <li key={m.position}><UserChip user={m.user} size={56} /></li>)}</ol> : <p className="muted">Pick up to 8 channels for your War Council. <Link href="/studio/channel/war-council">Add members</Link></p>}
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
  </ChannelFrame>;
}
