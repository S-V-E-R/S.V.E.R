import type { Metadata } from "next";
import Link from "next/link";
import SitePage from "../../components/SitePage";
import RoadmapProgress from "./progress";
import { parseRoadmap, type Roadmap } from "./data";

export const metadata: Metadata = {
  title: "Roadmap | S.V.E.R",
  description: "What is available on S.V.E.R, what is being built and what comes next: accounts, profiles, live streams, factions and fair discovery.",
  alternates: { canonical: "https://sver.tv/roadmap" },
};

export default async function RoadmapPage() {
  let roadmap: Roadmap | null = null;
  try {
    const response = await fetch(`${process.env.API_INTERNAL_ORIGIN || "http://127.0.0.1:8080"}/api/roadmap`, { cache: "no-store", signal: AbortSignal.timeout(5000) });
    if (response.ok) roadmap = parseRoadmap(await response.json());
  } catch {
    // The live client retries; never substitute invented completion statuses.
  }
  return <SitePage path="/roadmap" title="The road ahead" intro="What you can use today, what we are working on, and what comes next." wide>
    <p className="notice">This is the build order, not a set of release dates. Progress below follows the roadmap published with the running server.</p>
    <RoadmapProgress initial={roadmap} />
    <section aria-labelledby="pages-title">
      <h2 id="pages-title">Alongside the modules</h2>
      <p className="site-prose">The <Link href="/about">About</Link>, <Link href="/help">Help & FAQ</Link>, faction information and policy pages are being restored alongside the platform. The anonymous Take It Down request form and removal workflow are still to be implemented. Legal and support information is also being reviewed before wider launch.</p>
    </section>
    <section aria-labelledby="later-title">
      <h2 id="later-title">After the core</h2>
      <p>These follow the seven modules above. They are planned work, with no promised release date.</p>
      <div className="roadmap-later">
        <section className="frame"><p className="eyebrow">Phase 2</p><h3>Rewards & creator tools</h3><p>Valor and progression, creator subscriptions and payouts, analytics and stronger abuse detection.</p></section>
        <section className="frame"><p className="eyebrow">Later</p><h3>More ways to take part</h3><p>Community interactions, achievements, native alerts and overlays, followed by viewer revenue sharing and faction events.</p></section>
      </div>
    </section>
    <section className="site-prose" aria-labelledby="updates-title">
      <h2 id="updates-title">Follow the work</h2>
      <p>S.V.E.R is open source under AGPL-3.0. The <a href="https://github.com/S-V-E-R/S.V.E.R">source repository</a> and <a href="https://github.com/S-V-E-R/S.V.E.R/blob/main/docs/ROADMAP.md">development roadmap</a> track the work. If something available today is broken, <Link href="/contact">contact support</Link>.</p>
    </section>
  </SitePage>;
}
