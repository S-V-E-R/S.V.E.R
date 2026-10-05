import Link from "next/link";
import { Crest } from "../components/FactionIdentity";
import { FrontLine } from "../components/WarStanding";
import type { War } from "../lib/war";
import { factionInfo } from "../lib/factions";
import { apiGet } from "../lib/server-api";
import { ShelfHeading, StreamShelf, type StreamDirectory } from "../components/StreamShelf";
import { LiveSpotlight } from "../components/LiveSpotlight";
import type { PeoplePage } from "../components/People";
import { MagnetMark } from "../components/shell/Icons";
import { currentAccount } from "./session";

export const metadata = { title: "Live streams · S.V.E.R" };

export default async function Home({ searchParams }: { searchParams: Promise<{ page?: string }> }) {
  const params = await searchParams;
  const page = /^\d{1,6}$/.test(params.page ?? "") ? Math.min(Number(params.page), 100_000) : 0;
  const [account, result, catalog, standings] = await Promise.all([
    currentAccount(), apiGet<StreamDirectory>(`/api/streams?page=${page}`),
    apiGet<{ categories: { id: string; name: string; genre: string }[] }>("/api/categories"), apiGet<War>("/api/factions/war"),
  ]);
  const directory = result.data;
  const live = directory?.live ?? [];
  const justStarted = live.filter(stream => Date.parse(directory!.as_of) - Date.parse(stream.started_at) < 3_600_000).slice(0, 6);
  const following = account ? (await apiGet<PeoplePage>("/api/me/following")).data : null;
  const followed = new Set(following?.items.map(item => item.user.username));
  const followingLive = live.filter(stream => followed.has(stream.user.username));
  return <div className="home-page">
    <h1 className="sr-only">Live streams on S.V.E.R</h1>
    <FrontLine war={standings.data} faction={account?.faction ?? null} signedIn={!!account} />
    <section aria-labelledby="spotlight-title">
      <ShelfHeading id="spotlight-title" title="Live spotlight" description="Find someone playing, building or making." />
      {live.length ? <LiveSpotlight streams={live} /> : <div className="spotlight frame"><div className="shelf-empty"><h2>{directory ? "Nothing live right now." : "Streams couldn’t be loaded."}</h2><p>{directory ? "Visit a recent channel below, or get ready for your own stream." : "Please try again in a moment."}</p><Link className="button quiet" href={directory ? "/help" : "/"}>{directory ? "Streaming help" : "Try again"}</Link></div></div>}
      <p className="shelf-note"><MagnetMark /> MAGNet rotation is coming. Live channels are currently listed by start time, never viewer count.</p>
    </section>
    {followingLive.length > 0 && <section aria-labelledby="home-following"><ShelfHeading id="home-following" title="Following · live" href="/following" /><StreamShelf streams={followingLive} /></section>}
    <section aria-labelledby="live-now"><ShelfHeading id="live-now" title="Live now" description="Watch a stream in one click." />
      {live.length ? <StreamShelf streams={live} /> : <p className="shelf-empty frame">{directory ? "Nothing live right now." : "Live channels are temporarily unavailable."} <Link href="/help">Get ready to stream</Link></p>}
      {(page > 0 || directory?.has_more) && <nav className="pager" aria-label="Live stream pages">{page > 0 && <Link href={`/?page=${page - 1}#live-now`}>Previous</Link>}{directory?.has_more && <Link href={`/?page=${page + 1}#live-now`}>More live streams</Link>}</nav>}
    </section>
    <section className="beacons-shelf frame" aria-labelledby="beacons-title"><ShelfHeading id="beacons-title" title="Beacons" description="Short videos from the community." href="/roadmap" link="On the roadmap" /><p className="muted">Beacons will appear here when creators can publish short videos.</p></section>
    <section aria-labelledby="just-live"><ShelfHeading id="just-live" title="Just went live" description="Streams started in the last hour." />
      {justStarted.length ? <StreamShelf streams={justStarted} /> : <p className="shelf-empty frame">{directory ? "No streams started in the last hour." : "Recent starts are temporarily unavailable."}</p>}
    </section>
    {directory && !live.length && directory.recent.length > 0 && <section aria-labelledby="recent-channels"><ShelfHeading id="recent-channels" title="Recently live" description="Visit a channel and follow for the next stream." /><StreamShelf streams={directory.recent.slice(0, 8)} /></section>}
    <section aria-labelledby="territories-title"><ShelfHeading id="territories-title" title="Territories" description="Categories for people who play, build and make." href="/war-map" link="View the map" />
      <div className="territory-grid">{catalog.data?.categories.slice(0, 8).map(category => { const holder = standings.data?.genres.find(g => g.id === category.genre)?.holder; return <Link className="territory-tile" href={`/war-map#genre-${category.genre}`} key={category.id} data-theme={holder ?? "neutral"}>{holder && <Crest faction={holder} size={32} />}<span className="eyebrow">{category.genre.replaceAll("_", " ")}</span><h3>{category.name}</h3><span>{holder ? `Held by ${factionInfo(holder).name}` : "Neutral territory"}</span></Link>; })}</div>
      {!catalog.data?.categories.length && <p className="shelf-empty frame">The category catalog is unavailable. <Link href="/war-map">View the war map</Link></p>}
    </section>
    <section aria-labelledby="clips-title"><ShelfHeading id="clips-title" title="Latest clips" description="Moments from the community." href="/roadmap" link="On the roadmap" /><p className="shelf-empty frame">Clips will appear here when clipping is available.</p></section>
  </div>;
}
