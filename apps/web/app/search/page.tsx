import Link from "next/link";
import { Avatar } from "../../components/Avatar";
import { apiGet } from "../../lib/server-api";
import type { Chip } from "../../lib/types";
import { BeaconGrid } from "../../components/BeaconGrid";
import type { BeaconItem } from "../../lib/beacons";
import "../../styles/home.css";

type Result = { query: string; channels: Chip[]; categories: { id: string; name: string; genre: string }[]; beacons?: BeaconItem[] };

export const metadata = { title: "Search | S.V.E.R", robots: { index: false, follow: false } };

/** Channels (live first), categories and Beacons by name (docs/MAGNET.md "Search", docs/BEACONS.md). */
export default async function Search({ searchParams }: { searchParams: Promise<{ q?: string }> }) {
  const q = ((await searchParams).q ?? "").trim();
  const result = q.length >= 2 ? await apiGet<Result>(`/api/search?q=${encodeURIComponent(q.slice(0, 50))}`) : null;
  return <div className="home search-page">
    <header className="page-head"><span className="eyebrow">Search</span><h1 id="search-h">{q.length >= 2 ? `Results for “${q}”` : "Search S.V.E.R"}</h1><p className="lead">Channels, categories and Beacons by name. Live channels come first.</p></header>
    {q.length < 2 && <div className="empty-state"><p><strong>{q ? "Type at least 2 characters." : "Search channels, categories and Beacons."}</strong> Use the search bar at the top of the page.</p><p><Link href="/browse" className="button quiet">Browse live streams</Link></p></div>}
    {result && !result.data && <p className="notice" role="alert">Search isn&apos;t available right now. Please try again.</p>}
    {result?.data && <>
      <section aria-labelledby="ch-h"><h2 id="ch-h">Channels</h2>
        {result.data.channels.length ? <ul className="search-results">{result.data.channels.map(c => c.username && <li key={c.username}><Link href={c.live ? `/${c.username}/live` : `/${c.username}`}><Avatar sizes={c.avatar} name={c.display_name} size={40} /><span><strong>{c.display_name}</strong> <span className="muted">@{c.username}</span></span>{c.live && <span className="tag-live">Live</span>}</Link></li>)}</ul> : <div className="empty-state"><p>No channels match.</p><p><Link href="/browse">Browse live streams</Link></p></div>}
      </section>
      <section aria-labelledby="cat-h"><h2 id="cat-h">Categories</h2>
        {result.data.categories.length ? <ul className="search-results">{result.data.categories.map(c => <li key={c.id}><Link href={`/browse?category=${encodeURIComponent(c.id)}`}><strong>{c.name}</strong></Link></li>)}</ul> : <div className="empty-state"><p>No categories match.</p><p><Link href="/browse">See every category</Link></p></div>}
      </section>
      <section aria-labelledby="bea-h"><h2 id="bea-h">Beacons</h2>
        {result.data.beacons?.length ? <BeaconGrid items={result.data.beacons} /> : <div className="empty-state"><p>No Beacons match.</p><p><Link href="/beacons">See all Beacons</Link></p></div>}
      </section>
    </>}
  </div>;
}
