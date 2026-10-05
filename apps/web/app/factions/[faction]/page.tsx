import Link from "next/link";
import { notFound } from "next/navigation";
import { apiGet } from "../../../lib/server-api";
import { isFaction, factionInfo } from "../../../lib/factions";
import type { War } from "../../../lib/war";
import type { Chip } from "../../../lib/types";
import { Crest } from "../../../components/FactionIdentity";
import { WarStanding } from "../../../components/WarStanding";
import { GenreBoard } from "../../../components/WarMap";
import { StreamShelf, type StreamCardData } from "../../../components/StreamShelf";
import { UserChip } from "../../../components/UserChip";
import { FactionCommunity, MemberDirectory, RefreshFaction } from "../../../components/FactionCommunity";

type Hub = War & { is_member: boolean; live: StreamCardData[]; weekly_leaders: { user: Chip; influence: number }[]; season_leaders: { user: Chip; influence: number }[] };
export default async function Page({ params }: { params: Promise<{ faction: string }> }) {
  const { faction } = await params;
  if (!isFaction(faction)) notFound();
  const f = factionInfo(faction);
  const { data } = await apiGet<Hub>(`/api/factions/${faction}`);
  if (!data) return <section className="panel"><h1>{f.name}</h1><p>This hub could not be loaded. Please try again.</p></section>;
  return <div className="faction-hub" data-theme={faction}>
    <RefreshFaction />
    <header className="hub-header frame"><Crest faction={faction} size={150} /><div><p className="eyebrow">{f.title}</p><h1>{f.name}</h1><p className="faction-creed">{f.creed}</p><p>{f.belief}</p><Link className="button quiet" href="/choose-faction">{data.is_member ? "Your allegiance" : `Join ${f.name}`}</Link></div></header>
    <WarStanding war={data} />
    <section><h2>Live with {f.name}</h2><p className="muted">Every channel gets a turn. Picks rotate every 30 seconds; viewer count does not set the order.</p>{data.live.length ? <StreamShelf streams={data.live} /> : <p className="panel">No faction streams are live right now.</p>}</section>
    <section><h2>Contributions</h2><div className="contribution-columns">{([["This week", data.weekly_leaders], ["This season", data.season_leaders]] as const).map(([label, leaders]) => <div className="panel" key={label}><h3>{label}</h3>{leaders.length ? <ol className="faction-leaders">{leaders.map(l => <li key={l.user.username}><UserChip user={l.user} /><strong>{l.influence.toLocaleString()}<small> influence</small></strong></li>)}</ol> : <p>No contributions yet.</p>}</div>)}</div><p className="muted">Influence belongs to the faction you represented when you earned it.</p></section>
    <section><h2>Genre standings</h2><GenreBoard genres={data.genres} /><Link href="/war-map">View the full war map</Link></section>
    {data.is_member ? <FactionCommunity faction={faction} genres={data.genres} votingOpen={!!data.week && !data.week.completed} /> : <p className="panel">The War Council, community board and moderator elections are private to {f.name} members.</p>}
    <MemberDirectory faction={faction} />
  </div>;
}
