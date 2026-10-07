import type { Metadata } from "next";
import Image from "next/image";
import Link from "next/link";
import SitePage from "../../components/SitePage";
import { FACTIONS, WORLD } from "../../lib/factions";

export const metadata: Metadata = {
  title: "Factions | S.V.E.R",
  description: "Meet Myria, Aetheron and Glint. Discover their creeds, home turf and the seasonal war on S.V.E.R.",
  alternates: { canonical: "https://sver.tv/factions" },
};

const factions = FACTIONS;


export default function FactionsPage() {
  return <SitePage path="/factions" title="Three factions. One family." intro="S.V.E.R is built on a simple truth: people do not just want to watch. They want to belong." wide>
    <p className="notice">Choose a side, meet your faction, and follow the weekly war. <Link href="/war-map">Open the war map</Link>.</p>
    <nav className="site-links" aria-label="Meet each faction">{factions.map(faction =>
      <a key={faction.slug} href={`#${faction.slug}`}>{faction.name}</a>)}</nav>
    <div className="faction-grid">
      {factions.map(faction => <section key={faction.slug} id={faction.slug} className="faction-card frame" data-theme={faction.slug} aria-labelledby={`${faction.slug}-name`}>
        <Image src={`/factions/${faction.slug}.webp`} width={150} height={150} alt="" className="faction-crest" unoptimized />
        <p className="eyebrow">{faction.title}</p>
        <h2 id={`${faction.slug}-name`}>{faction.name}</h2>
        <p className="faction-creed">{faction.creed}</p>
        <p className="faction-values">{faction.values.join(" · ")}</p>
        <p>{faction.belief}</p>
        <h3>Who it speaks to</h3>
        <p>{faction.people}</p>
        <h3>Starting home turf</h3>
        <ul>{faction.turf.map(genre => <li key={genre}>{genre}</li>)}</ul>
        <details><summary>The story of {faction.name}</summary><p>{faction.lore}</p></details>
        <Link href={`/factions/${faction.slug}`} className="button quiet">Visit {faction.name}</Link><Link href={`/welcome?pick=${faction.slug}`} className="button">Join {faction.name}</Link>
      </section>)}
    </div>
    <section className="site-prose" id="shared-truth">
      <h2>Shared truth</h2>
      <p>Every type of creator belongs here. Your faction is not about what you stream. It is about how you show up.</p>
      <p>No faction is better than another. Each is a different expression of the same drive to compete, grow, and build something that matters.</p>
      <p><strong>Myria grinds. Aetheron studies. Glint connects.</strong> None of them are wrong. All of them are necessary.</p>
    </section>
    <section className="site-prose" id="the-ashfall">
      <h2>The Ashfall</h2>
      <p>{WORLD.paragraph}</p>
      <h3>The Accord</h3>
      <p>Every faction swears to the Accord&rsquo;s five terms. They are also how S.V.E.R works.</p>
      <ol>{WORLD.accord.map(term => <li key={term}>{term}</li>)}</ol>
    </section>
    <section className="site-prose" id="the-war">
      <h2>How the war works</h2>
      <p>Categories belong to genres. Each three-month season begins with the home turf above. Territory changes hands only at the weekly checkpoint, Monday at 00:00 UTC, and at the final season checkpoint.</p>
      <ol className="war-steps"><li><strong>Take part.</strong> Verified members earn influence through genuine streaming, watching and chat. Trusted playback is required, and daily limits prevent farming.</li><li><strong>Contest territory.</strong> Enemy territory earns extra influence. Scores are balanced by active faction size. A small lead or a tie keeps the current holder.</li><li><strong>Win the season.</strong> Most territories wins; total weeks holding genres breaks a tie. Members earn a season badge and banner. After a seven-day break, home turf resets.</li></ol>
      <p>Your private War Council chooses a target each week for a bonus the following week. <Link href="/war-map">See current standings</Link>.</p>
    </section>
    <section className="site-prose" id="joining">
      <h2>Choose by what matters to you</h2>
      <p>Your faction is an identity, not a restriction on whom you can watch, follow or talk to. You can stream any allowed category, including another faction’s home turf.</p>
      <p>Choose during signup. You get one free switch within seven days of your original choice; after that, switches open during the breaks between seasons. Your earlier influence stays with the faction that earned it.</p>
      <p>Every faction follows the same <Link href="/guidelines">Community Guidelines</Link>. Competition belongs in the seasonal war; harassment does not.</p>
    </section>
  </SitePage>;
}
