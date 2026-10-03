"use client";
import Link from "next/link";
import { useState } from "react";
import { send } from "../lib/client-api";
import type { ActivityItem, Chip } from "../lib/types";
import { Ago } from "./Text";
import { UserChip } from "./UserChip";

type Page = { items: ActivityItem[]; next_cursor: string | null };
const PREVIEW = 5;

/** Renderers for the kinds this build knows (docs/PROFILES.md, P7). Later modules add entries;
 *  an unknown kind renders nothing. */
const renderers: Record<string, (item: ActivityItem, owner: string) => React.ReactNode> = {
  follow: i => i.subject && <>Followed <UserChip user={i.subject} size={20} /></>,
  wall_post: (i, owner) => i.subject && (i.subject.username === owner ? <>Posted on <Link href={`/${owner}/wall`}>their Wall</Link></> : <>Signed <WallOf user={i.subject} /></>),
  war_council: i => <>Updated their War Council{typeof i.data.count === "number" ? ` (${i.data.count} member${i.data.count === 1 ? "" : "s"})` : ""}</>,
  song: i => <>Set their profile song{typeof i.data.title === "string" && i.data.title ? <> to <q>{i.data.title}</q>{typeof i.data.artist === "string" && i.data.artist ? ` by ${i.data.artist}` : ""}</> : ""}</>,
  schedule: (_, owner) => <>Updated <Link href={`/${owner}/schedule`}>their schedule</Link></>,
};
function WallOf({ user }: { user: Chip }) {
  return user.username ? <Link href={`/${user.username}/wall`}>{user.display_name}&apos;s Wall</Link> : <>a Wall</>;
}

/** "Recent activity" on the channel Home tab: 5 at first, then Show more pages through the feed. */
export function ActivityFeed({ username, initial, isOwner }: { username: string; initial: Page; isOwner: boolean }) {
  const [items, setItems] = useState(initial.items.filter(i => renderers[i.kind]));
  const [cursor, setCursor] = useState(initial.next_cursor);
  const [shown, setShown] = useState(PREVIEW);
  const [error, setError] = useState("");
  if (!items.length && !isOwner) return null;
  async function more() {
    if (shown < items.length) { setShown(shown + 20); return; }
    if (!cursor) return;
    const r = await send<Page>("GET", `/api/channels/${encodeURIComponent(username)}/activity?cursor=${encodeURIComponent(cursor)}`);
    if (!r.ok) { setError(r.error); return; }
    setItems([...items, ...r.data.items.filter(i => renderers[i.kind])]);
    setCursor(r.data.next_cursor);
    setShown(shown + 20);
  }
  return <section className="panel section activity" aria-label="Recent activity">
    <h2>Recent activity</h2>
    {items.length ? <ul className="list activity-list">{items.slice(0, shown).map(i => <li key={i.id} data-kind={i.kind}><span>{renderers[i.kind](i, username)}</span> <Ago className="muted" iso={i.created_at} /></li>)}</ul> : <p className="muted">Your recent activity appears here.</p>}
    {(shown < items.length || cursor) && <button type="button" className="small quiet" onClick={more}>Show more</button>}
    {error && <p role="alert" className="form-message">{error}</p>}
  </section>;
}
