import Link from "next/link";
import { RecentChannels, SectionHead, StreamGrid } from "../../components/home/Shelves";
import type { LiveCard, Recent } from "../../components/home/types";
import { apiGet } from "../../lib/server-api";
import { FACTIONS, factionOf, isFaction } from "../../lib/factions";
import { Crest } from "../../components/Crest";
import { holders, TerritoryGrid, type BrowseGenre } from "../../components/Territories";
import type { War } from "../../lib/war";
import { currentAccount } from "../session";
import "../../styles/home.css";


export const metadata = { title: "Browse | S.V.E.R" };

/**
 * Browse (docs/DESIGN.md "Browse"; docs/MAGNET.md "Browse"): genre tabs grouped by holding faction,
 * each with its crest, then 3:4 category tiles with the holder's crest and live count. A category
 * lists its live streams in fair rotation, with a faction filter.
 */
export default async function Browse({ searchParams }: { searchParams: Promise<{ genre?: string; category?: string; faction?: string }> }) {
  const params = await searchParams;
  const faction = isFaction(params.faction) ? params.faction : null;
  const filter = new URLSearchParams();
  if (params.genre) filter.set("genre", params.genre);
  if (params.category) filter.set("category", params.category);
  if (faction) filter.set("faction", faction);
  const [account, browse, list, warRes] = await Promise.all([
    currentAccount(),
    apiGet<{ genres: BrowseGenre[]; live: number }>("/api/discovery/browse"),
    apiGet<{ items: LiveCard[]; recent: Recent[] }>(`/api/discovery/live?${filter}`),
    apiGet<War>("/api/factions/war"),
  ]);
  const war = warRes.data ?? null;
  const held = holders(war);
  const order = (f: string | null | undefined) => { const i = FACTIONS.findIndex(x => x.slug === f); return i < 0 ? FACTIONS.length : i; };
  const genres = [...(browse.data?.genres ?? [])].sort((a, b) => order(held.get(a.id)) - order(held.get(b.id)));
  const genre = genres.find(g => g.id === params.genre);
  const category = genres.flatMap(g => g.categories).find(c => c.id === params.category);
  const link = (change: Record<string, string | null>) => {
    const next = new URLSearchParams(filter);
    for (const [key, value] of Object.entries(change)) { if (value) next.set(key, value); else next.delete(key); }
    const query = next.toString();
    return query ? `/browse?${query}` : "/browse";
  };
  const title = category?.name ?? genre?.name ?? "Browse";
  const viewerFaction = account?.faction ?? null;
  return <div className="home browse">
    <SectionHead id="browse-h" title={title} note={`${browse.data?.live ?? 0} live now · fair rotation, never by viewer count`} level={1} />
    <nav className="genre-tabs" aria-label="Genres">
      <Link href={link({ genre: null, category: null })} aria-current={!genre && !category ? "page" : undefined}>Everything</Link>
      {genres.map(g => { const f = factionOf(held.get(g.id)); return <Link key={g.id} href={link({ genre: g.id, category: null })} aria-current={genre?.id === g.id && !category ? "page" : undefined}>
        {f && <Crest faction={f.slug} initial="" size={20} label={`Held by ${f.name}`} />}{g.name} <span className="muted">{g.live}</span></Link>; })}
    </nav>
    {!category && <TerritoryGrid genres={genre ? [genre] : genres} war={war} viewerFaction={viewerFaction} href={(g, c) => link({ genre: g.id, category: c.id })} />}
    <SectionHead id="browse-live-h" title={category ? "Live now" : genre ? `Live in ${genre.name}` : "Live now"} note="Ordered by MAGNet, not by viewer count" />
    <nav className="browse-filters" aria-label="Faction filter">
      <Link href={link({ faction: null })} aria-current={!faction ? "page" : undefined} className="chip">All factions</Link>
      {FACTIONS.map(f => <Link key={f.slug} href={link({ faction: f.slug })} aria-current={faction === f.slug ? "page" : undefined} className="chip">{f.name}</Link>)}
    </nav>
    {!list.data ? <p className="notice" role="alert">Live channels couldn&apos;t be loaded. Please refresh.</p>
      : list.data.items.length ? <StreamGrid streams={list.data.items} viewerFaction={viewerFaction} />
        : <div className="empty-live panel"><p><strong>Nothing live here right now.</strong> These channels were live recently.</p><RecentChannels recent={list.data.recent} /><p><Link href="/war-map" className="button quiet">See the war map</Link></p></div>}
  </div>;
}
