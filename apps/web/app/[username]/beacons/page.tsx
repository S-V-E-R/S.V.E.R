import Link from "next/link";
import { BeaconGrid } from "../../../components/BeaconGrid";
import { ChannelFrame } from "../../../components/ChannelFrame";
import type { BeaconItem } from "../../../lib/beacons";
import { loadChannel, channelMetadata, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";

export async function generateMetadata({ params }: { params: ChannelParams }) { return channelMetadata((await params).username, "Beacons"); }

/** The channel's Beacons tab (docs/BEACONS.md "Where Beacons appear"), newest first. */
export default async function ChannelBeacons({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ offset?: string }> }) {
  const data = await loadChannel((await params).username);
  const username = data.channel.username;
  const query = await searchParams;
  const offset = /^\d{1,6}$/.test(query.offset ?? "") ? Math.min(100000, Number(query.offset)) : 0;
  const result = await apiGet<{ items: BeaconItem[]; has_more: boolean }>(`/api/channels/${encodeURIComponent(username)}/beacons?offset=${offset}`);
  return <ChannelFrame data={data} path={`/${username}/beacons`}>
    <h2>Beacons</h2>
    {!result.data ? <p role="alert">Beacons could not be loaded. Please refresh.</p>
      : result.data.items.length ? <BeaconGrid items={result.data.items} showChannel={false} />
        : <p className="muted">{data.viewer.is_owner ? <>No Beacons yet. <Link href="/studio/beacons">Make one from a clip</Link>.</> : "No Beacons to show yet."}</p>}
    <nav className="video-actions" aria-label="Beacon pages">
      {offset > 0 && <Link href={`/${username}/beacons?offset=${Math.max(0, offset - 24)}`}>Newer Beacons</Link>}
      {result.data?.has_more && <Link href={`/${username}/beacons?offset=${offset + 24}`}>Older Beacons</Link>}
    </nav>
  </ChannelFrame>;
}
