import Link from "next/link";
import { Avatar } from "../../components/Avatar";
import { SectionHead } from "../../components/home/Shelves";
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
    <SectionHead id="search-h" title={q ? `Results for “${q}”` : "Search"} level={1} />
    <form action="/search" className="search-form" role="search"><label htmlFor="search-q" className="sr-only">Search channels, categories and Beacons</label><input id="search-q" name="q" type="search" defaultValue={q} minLength={2} maxLength={50} required /><button type="submit">Search</button></form>
    {q.length > 0 && q.length < 2 && <p className="muted">Type at least 2 characters.</p>}
    {result && !result.data && <p className="notice" role="alert">Search isn&apos;t available right now. Please try again.</p>}
    {result?.data && <>
      <section aria-labelledby="ch-h"><h2 id="ch-h">Channels</h2>
        {result.data.channels.length ? <ul className="search-results">{result.data.channels.map(c => c.username && <li key={c.username}><Link href={c.live ? `/${c.username}/live` : `/${c.username}`}><Avatar sizes={c.avatar} name={c.display_name} size={40} /><span><strong>{c.display_name}</strong> <span className="muted">@{c.username}</span></span>{c.live && <span className="tag-live">Live</span>}</Link></li>)}</ul> : <p className="muted">No channels match.</p>}
      </section>
      <section aria-labelledby="cat-h"><h2 id="cat-h">Categories</h2>
        {result.data.categories.length ? <ul className="search-results">{result.data.categories.map(c => <li key={c.id}><Link href={`/browse?category=${encodeURIComponent(c.id)}`}><strong>{c.name}</strong></Link></li>)}</ul> : <p className="muted">No categories match.</p>}
      </section>
      <section aria-labelledby="bea-h"><h2 id="bea-h">Beacons</h2>
        {result.data.beacons?.length ? <BeaconGrid items={result.data.beacons} /> : <p className="muted">No Beacons match.</p>}
      </section>
    </>}
  </div>;
}
