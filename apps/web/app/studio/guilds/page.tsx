import { GuildStudio } from "../../../components/Guilds";
import type { MyGuilds } from "../../../lib/guilds";
import { apiGet } from "../../../lib/server-api";
import { currentAccount } from "../../session";
import "../../../styles/teams.css";
export default async function Guilds() {
  const { data } = await apiGet<MyGuilds>("/api/me/guilds");
  const account = await currentAccount();
  return data && account ? <GuildStudio initial={data} account={account.username} /> : <p role="alert">Your guilds could not be loaded.</p>;
}
