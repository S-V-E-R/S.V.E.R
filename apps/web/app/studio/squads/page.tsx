import { SquadStudio, type MySquads } from "../../../components/Squads";
import { apiGet } from "../../../lib/server-api";
import "../../../styles/teams.css";
export default async function Squads() {
  const { data } = await apiGet<MySquads>("/api/me/squads");
  return data ? <SquadStudio initial={data} /> : <p role="alert">Co-streams could not be loaded.</p>;
}
