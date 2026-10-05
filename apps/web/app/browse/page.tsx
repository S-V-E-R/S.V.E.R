import Link from "next/link";
import { RecentChannels, SectionHead, StreamGrid } from "../../components/home/Shelves";
import type { LiveCard, Recent } from "../../components/home/types";
import { apiGet } from "../../lib/server-api";
import { FACTIONS, isFaction } from "../../lib/factions";
import { currentAccount } from "../session";
import "../../styles/home.css";

type Category = { id: string; name: string; live: number };
type Genre = { id: string; name: string; home: string | null; live: number; categories: Category[] };

export const metadata = { title: "Browse | S.V.E.R" };

/** Browse by genre, then category, each in fair rotation, with a faction filter (docs/MAGNET.md "Browse"). */
export default async function Browse({ searchParams }: { searchParams: Promise<{ genre?: string; category?: string; faction?: string }> }) {
  const params = await searchParams;
  const faction = isFaction(params.faction) ? params.faction : null;
  const filter = new URLSearchParams();
  if (params.genre) filter.set("genre", params.genre);
  if (params.category) filter.set("category", params.category);
  if (faction) filter.set("faction", faction);
  const [account, browse, list] = await Promise.all([
    currentAccount(),
    apiGet<{ genres: Genre[]; live: number }>("/api/discovery/browse"),
    apiGet<{ items: LiveCard[]; recent: Recent[] }>(`/api/discovery/live?${filter}`),
  ]);
  const genres = browse.data?.genres ?? [];
  const genre = genres.find(g => g.id === params.genre);
  const category = genres.flatMap(g => g.categories).find(c => c.id === params.category);
  const link = (change: Record<string, string | null>) => {
    const next = new URLSearchParams(filter);
    for (const [key, value] of Object.entries(change)) { if (value) next.set(key, value); else next.delete(key); }
    const query = next.toString();
    return query ? `/browse?${query}` : "/browse";
  };
  const title = category?.name ?? genre?.name ?? "Browse";
  return <div className="home browse">
    <SectionHead id="browse-h" title={title} note={`${browse.data?.live ?? 0} live now · fair rotation, never by viewer count`} level={1} />
    <nav className="browse-filters" aria-label="Faction filter">
      <Link href={link({ faction: null })} aria-current={!faction ? "page" : undefined} className="chip">All factions</Link>
      {FACTIONS.map(f => <Link key={f.slug} href={link({ faction: f.slug })} aria-current={faction === f.slug ? "page" : undefined} className="chip">{f.name}</Link>)}
    </nav>
    <nav className="browse-genres" aria-label="Genres">
      <Link href={link({ genre: null, category: null })} aria-current={!genre && !category ? "page" : undefined} className="chip">Everything</Link>
      {genres.map(g => <Link key={g.id} href={link({ genre: g.id, category: null })} aria-current={genre?.id === g.id && !category ? "page" : undefined} className="chip">{g.name} <span className="muted">{g.live}</span></Link>)}
    </nav>
    {genre && <nav className="browse-categories" aria-label={`${genre.name} categories`}>{genre.categories.map(c => <Link key={c.id} href={link({ category: c.id })} aria-current={category?.id === c.id ? "page" : undefined} className="chip">{c.name} <span className="muted">{c.live}</span></Link>)}</nav>}
    {!list.data ? <p className="notice" role="alert">Live channels couldn&apos;t be loaded. Please refresh.</p>
      : list.data.items.length ? <StreamGrid streams={list.data.items} viewerFaction={account?.faction ?? null} />
        : <div className="empty-live panel"><p><strong>Nobody is live here right now.</strong> These channels were live recently.</p><RecentChannels recent={list.data.recent} /><p><Link href="/war-map" className="button quiet">See the war map</Link></p></div>}
  </div>;
}
