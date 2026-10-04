import type { Metadata } from "next";
import Image from "next/image";
import Link from "next/link";
import SitePage from "../../components/SitePage";

export const metadata: Metadata = {
  title: "Factions | S.V.E.R",
  description: "Meet Myria, Aetheron and Glint. Discover their creeds, home turf and the planned seasonal war on S.V.E.R.",
  alternates: { canonical: "https://sver.tv/factions" },
};

const factions = [
  {
    slug: "myria", name: "Myria", title: "The Vanguard",
    creed: "Earn everything. Accept nothing.",
    belief: "We believe nothing is given. Everything is earned through discipline and conviction.",
    values: "Discipline · Conviction · Endurance",
    people: "Competitors, speedrunners, challenge hunters and makers who keep working at their craft, even when nobody is watching.",
    turf: ["FPS & battle royale", "Fighting", "Sports & racing", "Speedrunning", "Crafting & making"],
    lore: "Myria was not founded. It was forged by people who refused to quit. Where you started matters less than whether you show up when it gets hard. Your word is your bond; your progress is your proof.",
  },
  {
    slug: "aetheron", name: "Aetheron", title: "The Arcane",
    creed: "Always learning. Never finished.",
    belief: "We believe mastery comes from curiosity. Every answer is the start of a better question.",
    values: "Curiosity · Mastery · Discovery",
    people: "Strategists, artists, educators, developers and theorycrafters who learn by testing, asking better questions and sharing what they find.",
    turf: ["RTS & MOBA", "Strategy & 4X", "Card & board", "Puzzle & simulation", "Art", "Education & coding"],
    lore: "Aetheron moves by knowledge. Its people study how systems work and how creators improve, then pass that understanding on. Mastery is the goal, discovery is the fuel, and there is always more to learn.",
  },
  {
    slug: "glint", name: "Glint", title: "The Sovereign",
    creed: "All are welcome. None are forgotten.",
    belief: "We believe the strongest force on any platform is a room where everyone belongs.",
    values: "Belonging · Trust · Momentum",
    people: "Musicians, co-op teams, cozy gamers and community builders who remember the newcomer and leave room for one more.",
    turf: ["Community events", "MMOs & RPGs", "Co-op & party", "Cozy & sandbox", "Music"],
    lore: "Glint builds its strength wherever people gather. A room where everyone feels welcome can become a community that lasts. Trust connects its people, and lifting someone else helps the whole side move forward.",
  },
] as const;

export default function FactionsPage() {
  return <SitePage path="/factions" title="Three factions. One family." intro="S.V.E.R is built on a simple truth: people do not just want to watch. They want to belong." wide>
    <p className="notice">Faction enrollment and the seasonal war are planned for Module 4. You can create an account now; choosing your side will come later. <Link href="/roadmap#factions">Follow the roadmap</Link>.</p>
    <nav className="site-links" aria-label="Meet each faction">{factions.map(faction =>
      <a key={faction.slug} href={`#${faction.slug}`}>{faction.name}</a>)}</nav>
    <div className="faction-grid">
      {factions.map(faction => <section key={faction.slug} id={faction.slug} className="faction-card frame" data-theme={faction.slug} aria-labelledby={`${faction.slug}-name`}>
        <Image src={`/factions/${faction.slug}.webp`} width={150} height={150} alt="" className="faction-crest" unoptimized />
        <p className="eyebrow">{faction.title}</p>
        <h2 id={`${faction.slug}-name`}>{faction.name}</h2>
        <p className="faction-creed">{faction.creed}</p>
        <p className="faction-values">{faction.values}</p>
        <p>{faction.belief}</p>
        <h3>Who it speaks to</h3>
        <p>{faction.people}</p>
        <h3>Starting home turf</h3>
        <ul>{faction.turf.map(genre => <li key={genre}>{genre}</li>)}</ul>
        <details><summary>The story of {faction.name}</summary><p>{faction.lore}</p></details>
        <a href="#joining" className="button quiet" aria-label={`About joining ${faction.name}`}>About joining</a>
      </section>)}
    </div>
    <section className="site-prose" id="shared-truth">
      <h2>Shared truth</h2>
      <p>Every type of creator belongs here. Your faction is not about what you stream. It is about how you show up.</p>
      <p>No faction is better than another. Each is a different expression of the same drive to compete, grow, and build something that matters.</p>
      <p><strong>Myria grinds. Aetheron studies. Glint connects.</strong> None of them are wrong. All of them are necessary.</p>
    </section>
    <section className="site-prose" id="the-war">
      <h2>How the war will work</h2>
      <p>Each streaming category belongs to a genre. Factions begin with the home turf above, then compete over three-month seasons. At the end of a season, each genre goes to the faction with the most influence in it after balancing for active faction size.</p>
      <ol className="war-steps">
        <li><strong>Take part.</strong> Verified accounts will contribute through streaming, watching and participating in chat. Watch time will count real playback sessions, and chat contributions will be limited to discourage farming.</li>
        <li><strong>Contest territory.</strong> Streaming in another faction’s territory will earn extra influence. Size balancing gives smaller factions a chance to compete.</li>
        <li><strong>Carry the result forward.</strong> The winning side will hold that genre in the next season. Territory will help guide discovery, with standings and contributions visible in the faction hubs.</li>
      </ol>
      <p>The war map, live standings and contribution tools will arrive with the working faction system. <Link href="/roadmap#magnet">MAGNet</Link> will bring fair discovery rotation: viewer count will never decide who gets a turn.</p>
    </section>
    <section className="site-prose" id="joining">
      <h2>Choose by what matters to you</h2>
      <p>Your faction will be an identity, not a restriction on whom you can watch, follow or talk to. You will be able to stream any allowed category, including another faction’s home turf.</p>
      <p>When enrollment opens, the planned rule is one free switch during your first seven days, then switches between seasons. Creating an account today does not enroll you in a faction or reserve a side.</p>
      <p>Every faction follows the same <Link href="/guidelines">Community Guidelines</Link>. Competition belongs in the seasonal war; harassment does not.</p>
    </section>
  </SitePage>;
}
