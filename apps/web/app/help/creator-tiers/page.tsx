import type { Metadata } from "next";
import Link from "next/link";
import SitePage from "../../../components/SitePage";
import CreatorTierTable from "../../../components/CreatorTiers";

export const metadata: Metadata = {
  title: "Creator tiers | S.V.E.R",
  description: "What each S.V.E.R creator tier pays, how long it keeps VODs, and what it takes to reach it.",
  alternates: { canonical: "https://sver.tv/help/creator-tiers" },
};

/** Public creator tier ladder (docs/SUPPORT.md "Creator tiers"; numbers from tiers.rs). */
export default function CreatorTiersPage() {
  return <SitePage path="/help/creator-tiers" title="Creator tiers" intro="Scout, Trailblazer, Pioneer and Pathfinder: what each tier pays and what it takes to reach it." wide>
    <CreatorTierTable />
    <section>
      <h2>How tiers work</h2>
      <ul>
        <li>Every streamer starts at Scout.</li>
        <li>Tiers are checked every Monday at 00:01 Eastern. You move up to the highest tier whose every requirement you meet, so you can skip a tier.</li>
        <li>Streams, stream hours, average viewers, unique viewers and days active count the last 90 days. Followers and subscribers count what you have at the check.</li>
        <li>Average and unique viewers count Trusted viewers only, so viewbots never help anyone climb.</li>
        <li>Your tier never goes down.</li>
        <li>Promotion is paused while a staff review of your streams is open.</li>
        <li>Trailblazer, Pioneer and Pathfinder show as a badge on your channel and beside your name in chat.</li>
      </ul>
    </section>
    <section>
      <h2>What you earn</h2>
      <ul>
        <li>You keep your tier&apos;s share of the full price of each subscription and gift sub. S.V.E.R pays the card fees out of its share.</li>
        <li>You earn 0.8¢ for every Valor spent on you, through tributes, Skills and subscriptions paid with Valor, at every tier.</li>
        <li>Ad revenue sharing is coming. Its terms will be published before ads pay out.</li>
        <li>Earnings are paid every two weeks through Stripe Connect, with Early Pay in between. To earn, you need a verified email, authenticator two-factor authentication and finished payout setup; there&apos;s no follower or hours minimum.</li>
      </ul>
      <p>See your own tier and progress in <Link href="/studio/payouts">Creator Studio → Payouts</Link>. The <Link href="/terms#section-8">Terms of Service</Link> cover earnings and payouts in full.</p>
    </section>
  </SitePage>;
}
