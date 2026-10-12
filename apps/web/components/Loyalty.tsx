"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { send } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { UserChip } from "./UserChip";

export const LOYALTY = ["Newcomer", "Regular", "Devoted", "Veteran", "Legend"];
type Me = { rank: number; earned: number; from: number; balance: number; week: number; next: { name: string; at: number } | null };
type Loyalty = { me: Me | null; leaders: { user: Chip; points: number }[] };

// The pill and the leaders box share one request per channel; a minute later they ask again.
const cache = new Map<string, { at: number; data: Promise<Loyalty | null> }>();
function fetchLoyalty(username: string) {
  const hit = cache.get(username);
  if (hit && Date.now() - hit.at < 55_000) return hit.data;
  const data = send<Loyalty>("GET", `/api/channels/${encodeURIComponent(username)}/loyalty`).then(r => r.ok ? r.data : null);
  cache.set(username, { at: Date.now(), data });
  return data;
}
function useLoyalty(username: string) {
  const [data, setData] = useState<Loyalty | null>(null);
  useEffect(() => {
    let live = true;
    const load = () => { if (!document.hidden) void fetchLoyalty(username).then(d => { if (live && d) setData(d); }); };
    load();
    const timer = setInterval(load, 60_000);
    return () => { live = false; clearInterval(timer); };
  }, [username]);
  return data;
}

/** The viewer's standing in this channel for the streamer bar: loyalty rank, progress and Engagement Valor. */
export function StandingPill({ username }: { username: string }) {
  const me = useLoyalty(username)?.me;
  if (!me) return null;
  const share = me.next ? Math.min(100, Math.round(((me.earned - me.from) / (me.next.at - me.from)) * 100)) : 100;
  const label = me.next ? `${(me.next.at - me.earned).toLocaleString()} more Engagement Valor to ${me.next.name}` : "Top loyalty rank";
  return <div className="standing-pill" title={label}>
    <span className="standing-rank">{LOYALTY[me.rank]}</span>
    <progress max={100} value={share} aria-label={label} />
    <span className="standing-balance"><strong>{me.balance.toLocaleString()}</strong> EV</span>
  </div>;
}

/** Top of chat: this week's most loyal viewers here, by Engagement Valor earned (watching, chatting), never by spending. */
export function EffortLeaders({ username }: { username: string }) {
  const data = useLoyalty(username);
  if (!data?.leaders.length) return null;
  return <details className="effort-leaders" open>
    <summary>Most loyal this week</summary>
    <ol>{data.leaders.map((l, i) => <li key={l.user.username ?? i}><span className="leader-place">{i + 1}</span><UserChip user={l.user} size={24} /><span className="muted">{l.points.toLocaleString()}</span></li>)}</ol>
    {data.me && data.me.week > 0 && <p className="small-text muted">You: {data.me.week.toLocaleString()} this week</p>}
    {!data.me && <p className="small-text muted"><Link href="/login">Sign in</Link> and watch or chat to earn a place.</p>}
  </details>;
}
