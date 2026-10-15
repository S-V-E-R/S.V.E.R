import type { Metadata } from "next";
import Link from "next/link";
import { Crest } from "../../components/Crest";
import { SupporterBadge, WolfMark } from "../../components/Supporter";
import { SupporterTiers } from "../../components/SupporterTiers";
import { factionOf } from "../../lib/factions";
import { apiGet } from "../../lib/server-api";
import { founderNumber, type SupportersData } from "../../lib/supporters";
import "../../styles/supporters.css";

export const metadata: Metadata = {
  title: "Support S.V.E.R | S.V.E.R",
  description: "A monthly subscription to S.V.E.R itself. All of the money goes to the platform, and the perks are only how you look.",
  alternates: { canonical: "https://sver.tv/supporters" },
};

const NEVER = [
  "A better place on MAGNet, home, the sidebar, Browse or search.",
  "Faction influence or war ground.",
  "XP, levels, Valor of either kind or Engagement Valor.",
  "Chat priority, a way past slow mode or chat gates, or extra emote slots.",
  "Anything a streamer or moderator controls, like unbans or skipped queues.",
];

// Platform supporters and Founders (docs/SUPPORTERS.md). Neutral theme, no mockup: built from docs/DESIGN.md.
export default async function SupportersPage({ searchParams }: { searchParams: Promise<{ subscribed?: string }> }) {
  const [{ data }, params] = await Promise.all([apiGet<SupportersData>("/api/supporters"), searchParams]);
  const full = data && data.founders_claimed >= data.founders_total;
  return <div className="supporters-page" data-theme="neutral">
    <header className="supporters-header frame">
      <span className="eyebrow"><WolfMark size={14} /> Support S.V.E.R</span>
      <h1>Wear the Wolf&apos;s grey.</h1>
      <p>The Grey Wolf is S.V.E.R itself, and its people wore grey because they belonged to no colour. Supporting is a monthly subscription to the site. All of the money goes to running and building S.V.E.R, and none of it to creators&apos; ledgers.</p>
      <p>What you get is only how you look: a Wolf badge in chat and on your card, a silver player-card frame and &ldquo;Supporting since&rdquo; on your profile. Your faction&apos;s crest and colours always stay.</p>
      {params.subscribed === "1" && <p role="status" className="supporters-thanks">Thank you. Your marks appear as soon as Stripe confirms the payment, usually within a minute.</p>}
    </header>

    {data ? <SupporterTiers initial={data} /> : <p role="alert">Supporting couldn&apos;t be loaded. Please try again shortly.</p>}

    <section className="supporters-never panel" aria-labelledby="never">
      <h2 id="never">What supporting never gives</h2>
      <ul>{NEVER.map(n => <li key={n}>{n}</li>)}</ul>
      <p className="muted">Supporters and everyone else are ranked, counted and moderated the same way. Want to back a creator instead? Subscribe on their channel.</p>
    </section>

    {data && <section className="supporters-founders" aria-labelledby="founders">
      <h2 id="founders"><WolfMark size={18} /> Founders</h2>
      <p>The first {data.founders_total} accounts to pay for a Greycloak month get a numbered Founder mark, kept for good, even after cancelling. A refunded or disputed payment gives the number back, and staff can&apos;t hand them out.</p>
      <p className="supporters-count">{full ? `All ${data.founders_total} Founder spots are claimed.` : `${data.founders_claimed} of ${data.founders_total} Founder spots claimed.`}</p>
      {data.founders.length > 0 ? <ol className="founders-list">{data.founders.map(f => {
        const faction = factionOf(f.faction);
        return <li key={f.number}>
          <span className="founders-number">{founderNumber(f.number)}</span>
          {f.username ? <><Crest faction={f.faction} initial={f.username.slice(0, 1).toUpperCase()} size={24} label={faction?.name} /><Link href={`/${f.username}`}>{f.username}</Link></> : <span className="muted">Founder {founderNumber(f.number)}</span>}
        </li>;
      })}</ol> : <p className="muted">No Founders yet. The first Greycloak becomes Founder #001.</p>}
    </section>}

    <p className="supporters-foot muted"><SupporterBadge tier="wolfsworn" /> Wolfsworn · <SupporterBadge tier="greycloak" /> Greycloak · <span>Also see <Link href="/wall">the Wall</Link>, a one-time seat that helps build the hall.</span></p>
  </div>;
}
