import { apiGet } from "../../lib/server-api";
import type { War } from "../../lib/war";
import WarMap from "../../components/WarMap";
import { publicMeta } from "../../lib/seo";
export const metadata = publicMeta("War map | S.V.E.R", "Which faction holds each category this week. Ground changes. A voice is never silenced.", "/war-map");
export default async function Page() {
  const result = await apiGet<War>("/api/factions/war");
  return result.data ? <WarMap initial={result.data} /> : <section className="panel"><h1>War map</h1><p>Standings could not be loaded. Please try again.</p></section>;
}
