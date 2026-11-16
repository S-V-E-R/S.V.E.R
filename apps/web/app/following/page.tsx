import Link from "next/link";
import { redirect } from "next/navigation";
import { FollowingList } from "../../components/FollowingList";
import type { PeoplePage } from "../../components/People";
import { SectionHead, StreamGrid } from "../../components/home/Shelves";
import type { LiveCard } from "../../components/home/types";
import { apiGet } from "../../lib/server-api";
import { currentAccount } from "../session";
import "../../styles/profiles.css";
import "../../styles/home.css";

export const metadata = { title: "Following | S.V.E.R", robots: { index: false, follow: false } };
export default async function Following({ searchParams }: { searchParams: Promise<{ cursor?: string }> }) {
  const account = await currentAccount();
  if (!account) redirect("/login");
  const cursor = (await searchParams).cursor;
  // Live channels first as stream cards, then everyone (docs/DESIGN.md "Following"); later pages are the list only.
  const [people, home] = await Promise.all([
    apiGet<PeoplePage>(`/api/me/following${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`),
    cursor ? null : apiGet<{ following: LiveCard[] }>("/api/discovery/home"),
  ]);
  const page = people.data;
  const live = home?.data?.following ?? [];
  return <div className="settings-page single"><div className="settings-body">
    <header className="page-head"><span className="eyebrow">Your channels</span><h1>Following</h1><p className="lead">Who&apos;s live now, then everyone you follow. <Link href="/guilds">Find a guild</Link> to follow its live members.</p></header>
    {!cursor && <section className="home-section" aria-labelledby="fol-live-h">
      <SectionHead id="fol-live-h" title="Live now" />
      {live.length ? <StreamGrid streams={live} viewerFaction={account.faction ?? null} /> : <p className="empty-state">Nobody you follow is live right now. <Link href="/browse">Browse live channels</Link></p>}
    </section>}
    <section className="panel section following-all">{cursor && <Link href="/following">Back to start</Link>}<FollowingList page={page} /></section>
  </div></div>;
}
