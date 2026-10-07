import Link from "next/link";
import { beaconPath, compact, type BeaconItem } from "../lib/beacons";
import "../styles/beacons.css";

/** 9:16 Beacon cards for the home shelf, the channel tab and search (docs/DESIGN.md "Home"). */
export function BeaconGrid({ items, showChannel = true }: { items: BeaconItem[]; showChannel?: boolean }) {
  return <ul className="beacon-grid">{items.map(({ beacon: b, channel, thumbnail }) => <li key={b.id} className="beacon-card">
    <Link href={beaconPath(b.id)} className="beacon-poster" aria-label={`Watch ${b.title}`}>
      {/* Signed private thumbnails come from the API, never the public image optimizer. */}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {thumbnail ? <img src={thumbnail} alt="" loading="lazy" width={270} height={480} /> : <span>{b.mature ? "18+" : "Beacon"}</span>}
      {channel.live && <span className="tag-live">Live</span>}
      <span className="beacon-views">{compact(b.views)} views</span>
    </Link>
    <h3><Link href={beaconPath(b.id)}>{b.title}</Link></h3>
    {showChannel && channel.username && <Link href={`/${channel.username}`} className="muted">{channel.display_name}</Link>}
  </li>)}</ul>;
}
