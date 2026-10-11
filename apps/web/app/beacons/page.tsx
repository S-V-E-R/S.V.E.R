import Link from "next/link";
import { BeaconFeed } from "../../components/BeaconFeed";
import { factions, isFaction } from "../../lib/factions";
import type { BeaconFeedPage } from "../../lib/beacons";
import { apiGet } from "../../lib/server-api";
import { currentAccount } from "../session";

export const metadata = { title: "Beacons | S.V.E.R", description: "Short videos from S.V.E.R creators that lead to their live streams." };

/** The Beacons feed (docs/BEACONS.md "The feed"). */
export default async function Beacons({ searchParams }: { searchParams: Promise<{ faction?: string }> }) {
  const tab = (await searchParams).faction;
  const faction = isFaction(tab) ? tab : null;
  const [account, feed] = await Promise.all([currentAccount(), apiGet<BeaconFeedPage>(`/api/beacons/feed${faction ? `?faction=${faction}` : ""}`)]);
  return <>
    <h1 className="sr-only">Beacons</h1>
    <nav className="beacon-tabs" aria-label="Beacon feeds">
      <Link href="/beacons" aria-current={faction ? undefined : "page"}>For you</Link>
      {factions.map(f => <Link key={f.slug} href={`/beacons?faction=${f.slug}`} aria-current={faction === f.slug ? "page" : undefined}>{f.name}</Link>)}
    </nav>
    <BeaconFeed key={faction ?? "all"} initial={feed.data} signedIn={!!account} faction={faction} />
  </>;
}
