import { GuildStudio } from "../../../components/Guilds";
import type { MyGuilds } from "../../../lib/guilds";
import { apiGet } from "../../../lib/server-api";
import "../../../styles/teams.css";
export default async function Guilds() {
  const { data } = await apiGet<MyGuilds>("/api/me/guilds");
  return data ? <GuildStudio initial={data} /> : <p role="alert">Your guilds could not be loaded.</p>;
}
