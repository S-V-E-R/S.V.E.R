import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "About | S.V.E.R",
  description: "A live streaming community for people who play, build and make. Why S.V.E.R exists and how it works.",
  alternates: { canonical: "https://sver.tv/about" },
};

const how = [
  ["Fair discovery", "MAGNet gives every live stream a turn at the top of the homepage, whether it has 3 viewers or 3,000. Viewer count never decides who gets seen."],
  ["Three factions", "Myria, Aetheron and Glint fight a seasonal war over categories. Verified members earn influence by streaming, watching, and chatting. Discovery still gives every stream a fair turn."],
  ["Creators first", "Streamers keep 65% to 80% of every subscription, rising with their creator tier, and S.V.E.R pays the card fees. Payday is every two weeks, with Early Pay in between."],
  ["Stream your way", "Use the software you already use, over RTMP, WHIP or SRT. Restream the same broadcast to up to three other destinations, with nothing re-encoded."],
  ["Chat you can trust", "Channel moderators, timeouts and bans, slow mode, link blocking, banned words, followers-only chat and account-age limits. Viewer counts only count real playback."],
  ["Viewers matter too", "Watching is free and needs no account. Viewers earn loyalty in the channels they support, and when ads arrive they will share 5% of what ads earn."],
] as const;

// Layout follows the About board of the design canvas (docs/design/README.md).
export default function AboutPage() {
  return <div className="about-page">
    <section className="about-hero" aria-label="About S.V.E.R">
      <span className="eyebrow">About S.V.E.R</span>
      <h1>Streaming Vigorously Ensures Revenue.</h1>
      <p className="about-lead">A live streaming home for people who play, build and make, where showing up and doing the work is how you get seen.</p>
    </section>

    <section aria-labelledby="stand" className="about-section">
      <h2 id="stand" className="rule-heading">What we stand for<span className="dia" aria-hidden="true" /></h2>
      <blockquote className="about-quote"><p>At S.V.E.R you forge your destiny, with those around you or on your own. You only go as far as you allow yourself.</p></blockquote>
    </section>

    <section aria-labelledby="why" className="about-section">
      <h2 id="why" className="rule-heading">Why it exists<span className="dia" aria-hidden="true" /></h2>
      <div className="about-why">
        <div className="about-prose">
          <p>S.V.E.R began as the platform Joe wanted as a streamer: somewhere people who care about their craft and their community can grow without having to be constant self-promoters. The people watching should matter as much as the people broadcasting.</p>
          <p>Open a stream. Find someone doing something interesting. Stay to talk, learn, follow and come back. Gaming, art, crafting and making, music and education all belong here, and our <Link href="/guidelines">Community Guidelines</Link> explain what you can share.</p>
          <p>No audience required to get started. Show up, pick a side, and go.</p>
        </div>
        <aside className="about-founder frame">
          <span className="eyebrow">Founder</span>
          <span className="about-founder-name">Joe</span>
          <span className="muted">@JoeTheChode · Aetheron</span>
          <span className="muted small-text">Streamer, community builder, and the person building S.V.E.R in the open.</span>
        </aside>
      </div>
    </section>

    <section aria-labelledby="how" className="about-section">
      <h2 id="how" className="rule-heading">How it works<span className="dia" aria-hidden="true" /></h2>
      <div className="about-cards">{how.map(([title, text]) => <div key={title} className="panel about-card"><h3>{title}</h3><p>{text}</p></div>)}</div>
    </section>

    <section aria-labelledby="open" className="about-section">
      <h2 id="open" className="rule-heading">Built in the open<span className="dia" aria-hidden="true" /></h2>
      <div className="about-prose">
        <p>S.V.E.R is open source under the GNU AGPL. Anyone can read the code, report a problem or contribute. Live streaming, factions, MAGNet discovery, subscriptions, past broadcasts, clips and Beacons are all live; the <Link href="/roadmap">roadmap</Link> shows what is being built next.</p>
        <p>The money terms are public on <Link href="/help/creator-tiers">Creator tiers and payouts</Link>, and the <Link href="/guidelines">Community Guidelines</Link> explain how rules are enforced and appealed. <Link href="/help/safety">Safety and fair discovery</Link> lists every moderation tool and how MAGNet keeps viewbots out. We never use your streams, clips or chat to train AI (see the <Link href="/terms">Terms</Link>).</p>
        <p>S.V.E.R is built with AI assistance, mostly Anthropic&apos;s Claude, with Joe deciding what gets built and what ships. Changes Claude helped write are credited to it in the commit history on GitHub.</p>
      </div>
      <div className="about-links">
        <a href="https://github.com/S-V-E-R/S.V.E.R" className="button quiet">Read the code on GitHub</a>
        <Link href="/roadmap" className="button quiet">See the roadmap</Link>
        <Link href="/wall" className="button quiet">Take a seat on the Wall</Link>
        <Link href="/supporters" className="button quiet">Support S.V.E.R</Link>
        <Link href="/contact" className="button quiet">Get in touch</Link>
        <Link href="/open-data" className="button quiet">Open data</Link>
        <Link href="/credits" className="button quiet">Open-source credits</Link>
      </div>
    </section>

    <section className="site-cta frame about-cta" aria-labelledby="cta">
      <div><h2 id="cta">Pick a side.</h2><p className="muted">Watching is free and needs no account.</p></div>
      <div className="about-links"><Link href="/signup" className="button">Enlist</Link><Link href="/" className="button quiet">Watch live</Link></div>
    </section>
  </div>;
}
