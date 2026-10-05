import { notFound } from "next/navigation";
import { SquadView, type Squad } from "../../../components/Squads";
import { apiGet } from "../../../lib/server-api";
import { currentAccount } from "../../session";
import "../../../styles/profiles.css";
import "../../../styles/teams.css";
export const metadata = { title: "Co-stream | S.V.E.R" };
export default async function CoStream({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const [result, account] = await Promise.all([apiGet<Squad>(`/api/squads/${encodeURIComponent(id)}`), currentAccount()]);
  if (result.status === 404) notFound();
  return result.data ? <SquadView initial={result.data} account={account?.username ?? null} /> : <p role="alert">This co-stream could not be loaded.</p>;
}
