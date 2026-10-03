import Link from "next/link";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { Occurrences } from "../../../components/Schedule";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";
import type { Occurrence } from "../../../lib/types";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Schedule");
}
export default async function ScheduleTab({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const name = data.channel.username;
  const page = (await apiGet<{ timezone: string | null; items: Occurrence[] }>(`/api/channels/${encodeURIComponent(name)}/schedule`)).data;
  return <ChannelFrame data={data} path={`/${name}/schedule`}>
    <section className="panel section">
      <h2>Schedule</h2>
      {!page ? <p className="muted">The schedule couldn&apos;t be loaded. Please try again.</p>
        : page.items.length ? <Occurrences items={page.items} zone={page.timezone} />
        : <p className="muted schedule-empty">No streams scheduled this week.{data.viewer.is_owner ? <> <Link href="/studio/channel/schedule">Set your schedule</Link></> : " Check back later."}</p>}
    </section>
  </ChannelFrame>;
}
