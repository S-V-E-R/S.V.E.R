import Link from "next/link";
import { followedOn, type Chip } from "../lib/types";
import { UserChip } from "./UserChip";

export type PeoplePage = { items: { user: Chip & { direct_follow?: boolean }; followed_at: string; guilds?: { name: string; slug: string }[] | null }[]; next_cursor: string | null };
export function People({ page, base }: { page: PeoplePage | null; base: string }) {
  if (!page) return <p className="muted">This list couldn&apos;t be loaded. Please try again.</p>;
  return <>
    {page.items.length === 0 ? <p className="muted">Nobody here yet.</p> : <ul className="people">{page.items.map(i => <li key={i.user.username ?? i.followed_at}><UserChip user={i.user} size={40} /><small className="muted followed-on">Followed <time dateTime={i.followed_at}>{followedOn(i.followed_at)}</time></small></li>)}</ul>}
    <nav className="pager">{page.next_cursor && <Link href={`${base}?cursor=${encodeURIComponent(page.next_cursor)}`}>More</Link>}</nav>
  </>;
}
