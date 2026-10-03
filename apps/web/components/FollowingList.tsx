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
  async function toggle(username: string) {
    const following = !unfollowed.has(username);
    setBusy(username);
    setError("");
    const result = await send(following ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(username)}`);
    setBusy(null);
    if (!result.ok) { setError(result.error); return; }
    const next = new Set(unfollowed);
    if (following) next.add(username); else next.delete(username);
    setUnfollowed(next);
  }
  return <>
    {page.items.length === 0 ? <p className="muted">Channels you follow appear here.</p> : <ul className="people">{page.items.map(i => {
      const name = i.user.username;
      const gone = name ? unfollowed.has(name) : false;
      return <li key={name ?? i.followed_at} className="person-row">
        <UserChip user={i.user} size={40} />
        <small className="muted followed-on">{gone ? "Unfollowed" : <>Followed <time dateTime={i.followed_at}>{followedOn(i.followed_at)}</time></>}</small>
        {name && <button type="button" className={gone ? "small" : "small quiet"} disabled={busy === name} onClick={() => toggle(name)} aria-label={`${gone ? "Follow" : "Unfollow"} @${name}`}>{gone ? "Follow" : "Unfollow"}</button>}
      </li>;
    })}</ul>}
    {error && <p role="alert" className="form-message">{error}</p>}
    <nav className="pager">{page.next_cursor && <Link href={`/following?cursor=${encodeURIComponent(page.next_cursor)}`}>More</Link>}</nav>
  </>;
}
