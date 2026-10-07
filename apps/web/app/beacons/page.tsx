import { BeaconFeed } from "../../components/BeaconFeed";
import type { BeaconFeedPage } from "../../lib/beacons";
import { apiGet } from "../../lib/server-api";
import { currentAccount } from "../session";

export const metadata = { title: "Beacons | S.V.E.R", description: "Short videos from S.V.E.R creators that lead to their live streams." };

/** The Beacons feed (docs/BEACONS.md "The feed"). */
export default async function Beacons() {
  const [account, feed] = await Promise.all([currentAccount(), apiGet<BeaconFeedPage>("/api/beacons/feed")]);
  return <>
    <h1 className="sr-only">Beacons</h1>
    <BeaconFeed initial={feed.data} signedIn={!!account} />
  </>;
}
