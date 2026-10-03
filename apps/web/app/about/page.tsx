import type { Metadata } from "next";
import Link from "next/link";
import SitePage from "../../components/SitePage";

export const metadata: Metadata = {
  title: "About | S.V.E.R",
  description: "A live streaming community for people who play, build and make. Why S.V.E.R exists and what comes next.",
  alternates: { canonical: "https://sver.tv/about" },
};

export default function AboutPage() {
  return <SitePage path="/about" title="About S.V.E.R" intro="A live streaming home for people who play, build and make.">
    <section>
      <h2>A place to find your people</h2>
      <p>Open a stream. Find someone doing something interesting. Stay to talk, learn, follow and come back. That is the experience S.V.E.R is being built around.</p>
      <p>Gaming, art, crafting and making, music, and education belong here. Creators can use the streaming software they already know, and multistreaming is welcome. Our <Link href="/guidelines">Community Guidelines</Link> explain what you can share.</p>
    </section>
    <section>
      <h2>Why Joe started S.V.E.R</h2>
      <p>S.V.E.R began as the platform Joe wanted as a streamer: somewhere people who care about their craft and community can grow without having to be constant self-promoters. The people watching should matter as much as the people broadcasting.</p>
      <p>The name stands for Streaming Vigorously Ensures Revenue. The long-term aim is for creators to earn from their work and for viewers to share in ad revenue. Payments and viewer rewards are planned for a later phase; they are not available in the rebuild today.</p>
    </section>
    <section>
      <h2>A fair turn to be seen</h2>
      <p>Our planned discovery system, MAGNet, gives live streams a rotating place in front of viewers. Viewer count will not decide discovery priority. The aim is simple: when someone goes live, people should have a fair chance to find them.</p>
    </section>
    <section>
      <h2>Three sides. One community.</h2>
      <p>Myria, Aetheron and Glint will bring a seasonal contest over streaming categories to S.V.E.R. Each side offers a different sense of belonging:</p>
      <ul>
        <li><strong>Myria:</strong> competition, grit and earned ground.</li>
        <li><strong>Aetheron:</strong> strategy, learning and mastery.</li>
        <li><strong>Glint:</strong> community, connection and trust.</li>
      </ul>
      <p>Factions will shape identity and discovery while everyone remains free to watch and join any channel. <Link href="/factions">Meet the factions</Link> and learn how the seasonal war will work.</p>
    </section>
    <section>
      <h2>Where things stand</h2>
      <p>S.V.E.R is being rebuilt in stages. Accounts, channel profiles and profile uploads are available. Live streaming and moderation are in progress, with live delivery still being prepared. Factions and discovery come next, followed by past broadcasts, clips and short videos called Beacons.</p>
      <p>The project is open source under AGPL-3.0. You can read the <a href="https://github.com/S-V-E-R/sver">source code</a> and follow the <Link href="/roadmap">roadmap</Link>.</p>
    </section>
    <section>
      <h2>Join in</h2>
      <p><Link href="/signup">Create your account</Link>, visit <Link href="/help">Help & FAQ</Link>, or <Link href="/contact">get in touch</Link>. We want S.V.E.R to be a place people return to because of the people they meet and the things they make together.</p>
    </section>
  </SitePage>;
}
