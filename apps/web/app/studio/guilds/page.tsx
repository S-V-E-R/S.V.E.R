import { GuildStudio } from "../../../components/Guilds";
import { SquadStudio, type MySquads } from "../../../components/Squads";
import type { MyGuilds } from "../../../lib/guilds";
import { apiGet } from "../../../lib/server-api";
import "../../../styles/teams.css";
/** Guilds and co-streams on one Teams page (navigation review, October 2026). */
export default async function Guilds() {
  const [guilds, squads] = await Promise.all([apiGet<MyGuilds>("/api/me/guilds"), apiGet<MySquads>("/api/me/squads")]);
  return <>
    {guilds.data ? <GuildStudio initial={guilds.data} /> : <p role="alert">Your guilds could not be loaded.</p>}
    {squads.data ? <SquadStudio initial={squads.data} /> : <p role="alert">Co-streams could not be loaded.</p>}
  </>;
}
