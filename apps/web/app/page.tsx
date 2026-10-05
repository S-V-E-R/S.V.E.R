import Image from "next/image";
import Link from "next/link";
import { Rotation } from "../components/home/Rotation";
import { StreamCard } from "../components/home/StreamCard";
import type { LiveCard } from "../components/home/types";
import { apiGet } from "../lib/server-api";
import { crestSrc, factionOf, FACTIONS } from "../lib/factions";
import { currentAccount } from "./session";
import "../styles/home.css";

/**
 * Home (docs/DESIGN.md "Home", the Main mockup). Sections whose modules aren't built yet
 * (Beacons, Territories, clips, war standings) are left out rather than shown with made-up data;
 * each module adds its section in the mockup's order when it ships.
 */
export default async function Home() {
  const account = await currentAccount();
  const live = (await apiGet<{ now: LiveCard[]; fresh: LiveCard[] }>("/api/live")).data ?? { now: [], fresh: [] };
  const mine = factionOf(account?.faction);
  const viewerFaction = account?.faction ?? null;

  return <div className="home">
    <section className="front frame" aria-label="Your front">
      <div className="front-crests" aria-hidden="true">
        {mine ? <Image src={crestSrc(mine.slug)} width={48} height={48} alt="" unoptimized /> : FACTIONS.map(f => <Image key={f.slug} src={crestSrc(f.slug)} width={34} height={34} alt="" unoptimized />)}
      </div>
      <div className="front-text">
        {mine
          ? <><strong>{mine.name} stands ready. {mine.creed}</strong><span>Season 1 opens the war over every category. Until then, stream, watch and bring your people in.</span></>
          : account
            ? <><strong>You haven&apos;t picked a side yet.</strong><span>Myria, Aetheron and Glint will fight over every category. Choose the one that sounds like you.</span></>
            : <><strong>Three factions. One war for every category.</strong><span>Pick a side, and everything you stream, watch and chat will take ground for it.</span></>}
      </div>
      {mine
        ? <Link href="/factions" className="button">Your side</Link>
        : <Link href={account ? "/choose-side" : "/signup"} className="button">Pick a side</Link>}
    </section>

    {live.now.length > 0 && <section aria-labelledby="rot-h" className="home-section">
      <SectionHead id="rot-h" title="Live rotation" note="Every live stream gets a turn here, whether it has 3 viewers or 3,000." level={1} />
      <Rotation streams={live.now.slice(0, 8)} viewerFaction={viewerFaction} />
    </section>}

    <section aria-labelledby="live-h" className="home-section">
      <SectionHead id="live-h" title="Live now" note="Never ordered by viewer count" level={live.now.length ? 2 : 1} />
      {live.now.length
        ? <div className="stream-grid">{live.now.map(s => <StreamCard key={s.username} s={s} viewerFaction={viewerFaction} />)}</div>
        : <div className="empty-live panel">
          <p><strong>Nobody is live right now.</strong> When someone goes live, they show up here first.</p>
          {account ? <Link href="/studio/stream" className="button">Go live</Link> : <Link href="/signup" className="button">Enlist</Link>}
        </div>}
    </section>

    {live.fresh.length > 0 && <section aria-labelledby="new-h" className="home-section">
      <SectionHead id="new-h" title="Just went live" note="Fresh streams get a head start." />
      <div className="stream-grid">{live.fresh.map(s => <StreamCard key={s.username} s={s} viewerFaction={viewerFaction} fresh />)}</div>
    </section>}
  </div>;
}

function SectionHead({ id, title, note, level = 2 }: { id: string; title: string; note?: string; level?: 1 | 2 }) {
  const H = level === 1 ? "h1" : "h2";
  return <div className="section-head"><H id={id}>{title}</H><span className="dia" aria-hidden="true" />{note && <span className="section-note">{note}</span>}<span className="rule" aria-hidden="true" /></div>;
}
