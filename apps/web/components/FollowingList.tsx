"use client";
import Link from "next/link";
import { useState } from "react";
import { send } from "../lib/client-api";
import { followedOn } from "../lib/types";
import type { PeoplePage } from "./People";
import { UserChip } from "./UserChip";

/** The viewer's own /following list: each row has Unfollow (and Follow again until the page is left). */
export function FollowingList({ page }: { page: PeoplePage | null }) {
  const [unfollowed, setUnfollowed] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  if (!page) return <p className="muted">This list couldn&apos;t be loaded. Please try again.</p>;
  async function toggle(username: string, following: boolean) {
    setBusy(username);
    setError("");
    const result = await send(following ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(username)}`);
    setBusy(null);
    if (!result.ok) { setError(result.error); return; }
    const next = new Set(unfollowed);
    if (next.has(username)) next.delete(username); else next.add(username);
    setUnfollowed(next);
  }
  return <>
    {page.items.length === 0 ? <p className="muted">Channels you follow appear here.</p> : <ul className="people">{page.items.map(i => {
      const name = i.user.username;
      const following = (i.user.direct_follow !== false) !== (name ? unfollowed.has(name) : false);
      return <li key={name ?? i.followed_at} className="person-row">
        <UserChip user={i.user} size={40} />
        <small className="muted followed-on">{i.guilds?.length ? <>Via {i.guilds.map((g, n) => <span key={g.slug}>{n > 0 && ", "}<Link href={`/g/${g.slug}`}>{g.name}</Link></span>)}</> : following ? <>Followed <time dateTime={i.followed_at}>{followedOn(i.followed_at)}</time></> : "Unfollowed"}</small>
        {name && <button type="button" className={following ? "small quiet" : "small"} disabled={busy === name} onClick={() => toggle(name, following)} aria-label={`${following ? "Unfollow" : "Follow"} @${name}`}>{following ? "Unfollow" : "Follow"}</button>}
      </li>;
    })}</ul>}
    {error && <p role="alert" className="form-message">{error}</p>}
    <nav className="pager">{page.next_cursor && <Link href={`/following?cursor=${encodeURIComponent(page.next_cursor)}`}>More</Link>}</nav>
  </>;
}
