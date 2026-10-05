import Link from "next/link";
import { apiGet } from "../../lib/server-api";
import type { Guild } from "../../lib/guilds";
import { GuildCard } from "../../components/Guilds";
import "../../styles/profiles.css";
import "../../styles/teams.css";
export const metadata = { title: "Guilds | S.V.E.R" };
export default async function Guilds({ searchParams }: { searchParams: Promise<{ q?: string; offset?: string }> }) {
  const q = await searchParams;
  const query = new URLSearchParams({ q: q.q ?? "", offset: q.offset ?? "0" });
  const { data } = await apiGet<{ items: Guild[]; next_offset: number | null }>(`/api/guilds?${query}`);
  return <div className="teams-page"><header><span className="eyebrow">Play. Build. Make. Together.</span><h1>Guilds</h1><p>Find a stream team across all three factions.</p><Link className="button quiet" href="/studio/guilds">My guilds &amp; applications</Link></header>
    <form className="row guild-search" action="/guilds"><label className="field">Find a guild<input type="search" name="q" defaultValue={q.q} maxLength={100} /></label><button>Search</button></form>
    {!data ? <p role="alert">Guilds could not be loaded. Please try again.</p> : <><div className="guild-grid">{data.items.map(g => <GuildCard key={g.id} guild={g} />)}</div>{data.items.length === 0 && <p>No guilds found. Start a team in Creator Studio.</p>}{data.next_offset !== null && <Link className="button quiet" href={`/guilds?${new URLSearchParams({ q: q.q ?? "", offset: String(data.next_offset) })}`}>More guilds</Link>}</>}
  </div>;
}
