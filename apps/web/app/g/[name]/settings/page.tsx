import { notFound, redirect } from "next/navigation";
import { GuildView } from "../../../../components/Guilds";
import type { GuildPage } from "../../../../lib/guilds";
import { apiGet } from "../../../../lib/server-api";
import { currentAccount } from "../../../session";
import "../../../../styles/profiles.css";
import "../../../../styles/teams.css";
export const metadata = { title: "Manage guild | S.V.E.R", robots: { index: false, follow: false } };
export default async function Settings({ params }: { params: Promise<{ name: string }> }) {
  const account = await currentAccount();
  if (!account) redirect("/login");
  const { name } = await params;
  const result = await apiGet<GuildPage>(`/api/guilds/${encodeURIComponent(name)}`);
  if (result.status === 404) notFound();
  if (result.data && !result.data.viewer.can_manage) redirect(`/g/${encodeURIComponent(name)}`);
  return result.data ? <GuildView initial={result.data} account={account.username} settings /> : <p role="alert">Guild settings could not be loaded.</p>;
}
