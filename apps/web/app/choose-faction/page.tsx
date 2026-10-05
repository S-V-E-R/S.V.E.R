import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import { apiGet } from "../../lib/server-api";
import { isFaction } from "../../lib/factions";
import ChooseFaction, { type Membership } from "./choose";

export const metadata = { title: "Choose your side · S.V.E.R" };
export default async function Page({ searchParams }: { searchParams: Promise<{ faction?: string }> }) {
  const params = await searchParams;
  const account = await currentAccount();
  if (!account) redirect(`/signup${isFaction(params.faction) ? `?faction=${params.faction}` : ""}`);
  const result = await apiGet<Membership>("/api/me/faction");
  if (!result.data) return <p className="notice" role="alert">Faction choices could not be loaded. Please reload this page.</p>;
  return <ChooseFaction membership={result.data} verified={account.email_verified} preferred={isFaction(params.faction) ? params.faction : null} />;
}
