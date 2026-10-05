import { notFound } from "next/navigation";
import { GuildView } from "../../../components/Guilds";
import type { GuildPage } from "../../../lib/guilds";
import { apiGet } from "../../../lib/server-api";
import { currentAccount } from "../../session";
import "../../../styles/profiles.css";
import "../../../styles/teams.css";
export const metadata = { title: "Guild | S.V.E.R" };
export default async function Guild({ params }: { params: Promise<{ name: string }> }) {
  const { name } = await params;
  const [result, account] = await Promise.all([apiGet<GuildPage>(`/api/guilds/${encodeURIComponent(name)}`), currentAccount()]);
  if (result.status === 404) notFound();
  return result.data ? <GuildView initial={result.data} account={account?.username ?? null} /> : <p role="alert">This guild could not be loaded. Please try again.</p>;
}
