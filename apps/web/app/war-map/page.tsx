import { apiGet } from "../../lib/server-api";
import type { War } from "../../lib/war";
import WarMap from "../../components/WarMap";
export const metadata = { title: "War map · S.V.E.R" };
export default async function Page() {
  const result = await apiGet<War>("/api/factions/war");
  return result.data ? <WarMap initial={result.data} /> : <section className="panel"><h1>War map</h1><p>Standings could not be loaded. Please try again.</p></section>;
}
