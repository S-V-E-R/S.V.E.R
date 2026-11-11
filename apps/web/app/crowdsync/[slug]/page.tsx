import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { Avatar } from "../../../components/Avatar";
import { AddToBoards, ListingBody, type ListingContent } from "../../../components/Catalog";
import { apiGet } from "../../../lib/server-api";
import "../../../styles/profiles.css";
import "../../../styles/teams.css";

type Listing = ListingContent & { slug: string; developer: { username: string; display_name: string } | null; installs: number };
type Props = { params: Promise<{ slug: string }> };
const load = async (slug: string) => (await apiGet<Listing>(`/api/crowdsync/${encodeURIComponent(slug)}`)).data;

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params;
  const listing = await load(slug);
  if (!listing) return { title: "CrowdSync catalog | S.V.E.R" };
  return { title: `${listing.name} | CrowdSync catalog | S.V.E.R`, description: listing.summary, alternates: { canonical: `https://sver.tv/crowdsync/${listing.slug}` } };
}

// One catalog listing (docs/CROWDSYNC.md "Catalog"). No mockup: built from docs/DESIGN.md with plain panels.
export default async function ListingPage({ params }: Props) {
  const { slug } = await params;
  const listing = await load(slug);
  if (!listing) notFound();
  return <div className="guild-page">
    <p><Link href="/crowdsync">← Games and boards</Link></p>
    <header className="row"><Avatar sizes={listing.icon} name={listing.name} size={96} /><div>
      <span className="eyebrow">{listing.board ? "Board" : "Game with its own board"}</span>
      <h1>{listing.name}</h1>
      <p>{listing.summary}</p>
      <p className="muted">{listing.developer && <>By <Link href={`/${listing.developer.username}`}>{listing.developer.display_name}</Link> · </>}{listing.board ? "Added by" : "Connected to"} {listing.installs.toLocaleString("en-US")} {listing.installs === 1 ? "channel" : "channels"}</p>
    </div></header>
    <section className="panel section">
      {listing.board ? <AddToBoards slug={listing.slug} /> : <>
        <h2>Connect this game to your channel</h2>
        <ol>
          <li>Get the game from the link below and start it.</li>
          <li>Choose <strong>Connect to S.V.E.R</strong> in the game. It shows a short code.</li>
          <li>Enter the code at <Link href="/go">sver.tv/go</Link> and tick <strong>May set my board</strong>, so the game can put its own board on your channel.</li>
        </ol>
        <p className="muted small">Your own board comes back when the game disconnects. You can switch the permission off at any time in <Link href="/studio/board">Studio → Board</Link> or Settings → Connected apps.</p>
      </>}
    </section>
    <section className="panel section"><ListingBody listing={listing} /></section>
  </div>;
}
