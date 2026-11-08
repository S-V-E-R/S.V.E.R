import { apiGet } from "../../lib/server-api";
import type { War } from "../../lib/war";
import WarMap from "../../components/WarMap";
import { currentAccount } from "../session";
export const metadata = { title: "War map · S.V.E.R" };
export default async function Page() {
  const [result, account] = await Promise.all([apiGet<War>("/api/factions/war"), currentAccount()]);
  return result.data ? <WarMap initial={result.data} faction={account?.faction ?? null} /> : <section className="panel"><h1>War map</h1><p>Standings could not be loaded. Please try again.</p></section>;
}
