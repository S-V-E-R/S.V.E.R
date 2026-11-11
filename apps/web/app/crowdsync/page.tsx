import type { Metadata } from "next";
import Link from "next/link";
import { Avatar } from "../../components/Avatar";
import { apiGet } from "../../lib/server-api";
import type { Sizes } from "../../lib/types";
import "../../styles/profiles.css";
import "../../styles/teams.css";

export const metadata: Metadata = {
  title: "CrowdSync catalog | S.V.E.R",
  description: "Games and boards for CrowdSync: let your viewers play along. Reviewed by staff, listed newest first.",
  alternates: { canonical: "https://sver.tv/crowdsync" },
};

type Item = { slug: string; name: string; summary: string; icon: Sizes; sets_own_board: boolean; developer: { username: string; display_name: string } | null; installs: number };

// The CrowdSync catalog (docs/CROWDSYNC.md "Catalog"). No mockup: built from docs/DESIGN.md on the Guilds directory's layout.
export default async function Catalog({ searchParams }: { searchParams: Promise<{ q?: string; cursor?: string }> }) {
  const q = await searchParams;
  const { data } = await apiGet<{ items: Item[]; next_cursor: string | null }>(`/api/crowdsync?${new URLSearchParams({ q: q.q ?? "", cursor: q.cursor ?? "" })}`);
  return <div className="teams-page"><header><span className="eyebrow">CrowdSync</span><h1>Games and boards</h1>
    <p>Games your viewers can play along with, and boards you can add to your own channel. Staff review every listing. They&apos;re shown newest first: nothing here is ranked or paid for.</p>
    <Link className="button quiet" href="/studio/board">My boards</Link> <Link className="button quiet" href="/developers#catalog">List your game</Link></header>
    <form className="row guild-search" action="/crowdsync"><label className="field">Find by name<input type="search" name="q" defaultValue={q.q} maxLength={40} /></label><button>Search</button></form>
    {!data ? <p role="alert">The catalog could not be loaded. Please try again.</p> : <>
      <div className="guild-grid">{data.items.map(item => <article key={item.slug} className="panel guild-card"><Avatar sizes={item.icon} name={item.name} size={64} /><div>
        <h2><Link href={`/crowdsync/${item.slug}`}>{item.name}</Link></h2>
        <p>{item.summary}</p>
        <span className="muted">{item.sets_own_board ? "Game with its own board" : "Board"}{item.developer && ` · by ${item.developer.display_name}`} · {item.installs.toLocaleString("en-US")} {item.installs === 1 ? "channel" : "channels"}</span>
      </div></article>)}</div>
      {data.items.length === 0 && <div className="empty-state"><p><strong>{q.q ? `Nothing matches “${q.q}”.` : "Nothing is listed yet."}</strong></p>{q.q && <p><Link href="/crowdsync">See everything</Link></p>}</div>}
      {data.next_cursor && <Link className="button quiet" href={`/crowdsync?${new URLSearchParams({ q: q.q ?? "", cursor: data.next_cursor })}`}>More</Link>}
    </>}
  </div>;
}
