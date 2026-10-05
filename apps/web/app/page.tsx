import Image from "next/image";
import Link from "next/link";
import { Rotation } from "../components/home/Rotation";
import { RecentChannels, SectionHead, StreamGrid } from "../components/home/Shelves";
import { StreamCard } from "../components/home/StreamCard";
import type { LiveCard, Recent } from "../components/home/types";
import { apiGet } from "../lib/server-api";
import { crestSrc, factionOf, FACTIONS } from "../lib/factions";
import { currentAccount } from "./session";
import "../styles/home.css";

type Spotlight = { kind: "staff" | "first_stream" | "returning"; reason: string; stream: LiveCard | null; user?: { username: string; display_name: string } };
type Home = { live: LiveCard[]; following: LiveCard[]; faction: LiveCard[] | null; fresh: LiveCard[]; spotlights: Spotlight[]; recent: Recent[] };

/**
 * Home (docs/DESIGN.md "Home", docs/MAGNET.md "Homepage"). Every list is in MAGNet's fair rotation:
 * each live stream reaches the top row within a cycle, never by viewer count.
 */
export default async function Home() {
  const account = await currentAccount();
  const home = (await apiGet<Home>("/api/discovery/home")).data;
  const mine = factionOf(account?.faction);
  const viewerFaction = account?.faction ?? null;
  const live = home?.live ?? [];

  return <div className="home">
    <section className="front frame" aria-label="Your front">
      <div className="front-crests" aria-hidden="true">
        {mine ? <Image src={crestSrc(mine.slug)} width={48} height={48} alt="" unoptimized /> : FACTIONS.map(f => <Image key={f.slug} src={crestSrc(f.slug)} width={34} height={34} alt="" unoptimized />)}
      </div>
      <div className="front-text">
        {mine
          ? <><strong>{mine.name} stands ready. {mine.creed}</strong><span>Every stream, watch and chat takes ground for your side.</span></>
          : account
            ? <><strong>You haven&apos;t picked a side yet.</strong><span>Myria, Aetheron and Glint fight over every category. Choose the one that sounds like you.</span></>
            : <><strong>Three factions. One war for every category.</strong><span>Pick a side, and everything you stream, watch and chat will take ground for it.</span></>}
      </div>
      {mine
        ? <Link href="/war-map" className="button">War map</Link>
        : <Link href={account ? "/welcome" : "/signup"} className="button">Pick a side</Link>}
    </section>

    {!home && <p className="notice" role="alert">Live channels couldn&apos;t be loaded. Please refresh.</p>}

    {(home?.spotlights.length ?? 0) > 0 && <section aria-labelledby="spot-h" className="home-section">
      <SectionHead id="spot-h" title="Spotlight" note="First streams, returning creators and staff picks" />
      <div className="stream-grid">{home!.spotlights.map((s, i) => s.stream
        ? <div key={s.stream.username} className="spotlight-card"><p className="spotlight-reason">{s.reason}</p><StreamCard s={s.stream} viewerFaction={viewerFaction} /></div>
        : s.user && <Link key={`staff-${i}`} href={`/${s.user.username}`} className="spotlight-card offline panel"><p className="spotlight-reason">{s.reason}</p><strong>{s.user.display_name}</strong><span className="muted">Offline now · follow for the next stream</span></Link>)}</div>
    </section>}

    {(home?.following.length ?? 0) > 0 && <section aria-labelledby="fol-h" className="home-section">
      <SectionHead id="fol-h" title="Following · live" href="/following" link="Everyone you follow" />
      <StreamGrid streams={home!.following} viewerFaction={viewerFaction} />
    </section>}

    {live.length > 0 && <section aria-labelledby="rot-h" className="home-section">
      <SectionHead id="rot-h" title="Live rotation" note="Every live stream gets a turn here, whether it has 3 viewers or 3,000." level={1} />
      <Rotation streams={live.slice(0, 8)} viewerFaction={viewerFaction} />
    </section>}

    <section aria-labelledby="live-h" className="home-section">
      <SectionHead id="live-h" title="Live now" note="Fair rotation, never ordered by viewer count" level={live.length ? 2 : 1} href="/browse" link="Browse" />
      {live.length
        ? <StreamGrid streams={live} viewerFaction={viewerFaction} />
        : <div className="empty-live panel">
          <p><strong>Nobody is live right now.</strong> These channels were live recently; follow them to hear when they&apos;re back.</p>
          {home && <RecentChannels recent={home.recent} />}
          <p><Link href="/war-map" className="button quiet">See the war map</Link> {account ? <Link href="/studio/stream" className="button">Go live</Link> : <Link href="/signup" className="button">Enlist</Link>}</p>
        </div>}
    </section>

    {mine && (home?.faction?.length ?? 0) > 0 && <section aria-labelledby="fac-h" className="home-section">
      <SectionHead id="fac-h" title={`From ${mine.name}`} href={`/browse?faction=${mine.slug}`} link="All of your side" />
      <StreamGrid streams={home!.faction!} viewerFaction={viewerFaction} />
    </section>}

    {(home?.fresh.length ?? 0) > 0 && <section aria-labelledby="new-h" className="home-section">
      <SectionHead id="new-h" title="Just went live" note="Fresh streams get a head start." />
      <StreamGrid streams={home!.fresh} viewerFaction={viewerFaction} />
    </section>}
  </div>;
}
