"use client";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { LivePlayer } from "./LivePlayer";
import { HypeChat } from "./HypeChat";
import { StreamCard } from "./home/StreamCard";
import type { LiveCard } from "./home/types";
import { LiveThumbnail } from "./LiveThumbnail";

type Featured = { stream: LiveCard; kind: string; reason: string | null; since: string; moves_on_by: string | null; holding: boolean; squad: { mode: "MERGED" | "SEPARATE"; members: LiveCard[] } | null };
type Lane = { id: string; name: string; enabled: boolean; featured: Featured | null; next: { stream: LiveCard; reason: string | null; switch_at: string } | null; others: LiveCard[] };
type LaneLink = { id: string; name: string; enabled: boolean; featuring: string | null };

/**
 * A MAGNet channel (docs/MAGNET.md "What the viewer sees"): one player on the featured
 * stream with a one-line reason, a 5-second countdown with a still of the next stream and Stay,
 * and a holding card for viewers who can't watch the featured channel. A merged co-stream is
 * featured as one unit with tabs between its members (still one player).
 */
export function MagnetHype({ lane, account, viewerFaction }: { lane: string; account: string | null; viewerFaction: string | null }) {
  const signedIn = !!account;
  const [state, setState] = useState<Lane | null>(null);
  const [lanes, setLanes] = useState<LaneLink[]>([]);
  const [error, setError] = useState("");
  const [now, setNow] = useState(() => Date.now());
  // The co-stream member tab a viewer picked, for the feature it was picked in.
  const [tab, setTab] = useState<{ of: string; username: string } | null>(null);
  const load = useCallback(async () => {
    const result = await send<Lane>("GET", `/api/magnet/${encodeURIComponent(lane)}`);
    if (result.ok) { setState(result.data); setError(""); } else setError(result.error);
  }, [lane]);
  useLoad(load);
  useEffect(() => { void send<{ lanes: LaneLink[] }>("GET", "/api/magnet").then(r => { if (r.ok) setLanes(r.data.lanes); }); }, []);
  // Poll for switches; tick faster during a countdown.
  const counting = state?.next ? Date.parse(state.next.switch_at) : null;
  useEffect(() => {
    const timer = setInterval(() => { if (!document.hidden) void load(); }, counting ? 1500 : 5000);
    return () => clearInterval(timer);
  }, [load, counting]);
  useEffect(() => {
    if (!counting) return;
    const timer = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(timer);
  }, [counting]);
  const featured = state?.featured;
  const merged = featured?.squad?.mode === "MERGED" ? featured.squad.members : [];
  const shown = (tab && tab.of === featured?.since && merged.find(m => m.username === tab.username)) || featured?.stream;
  const left = counting ? Math.max(0, Math.ceil((counting - now) / 1000)) : 0;
  const minutes = featured?.moves_on_by ? Math.max(1, Math.ceil((Date.parse(featured.moves_on_by) - now) / 60000)) : null;
  return <div className="magnet-hype channel watch"><div className="watch-main">
    <nav className="browse-genres" aria-label="MAGNet lanes">{lanes.filter(l => l.enabled).map(l => <Link key={l.id} href={l.id === "global" ? "/magnet" : `/magnet/${l.id}`} aria-current={l.id === lane ? "page" : undefined} className="chip">{l.name}</Link>)}</nav>
    {error && <p className="notice" role="alert">{error}</p>}
    {state && !state.enabled && <p className="notice">This MAGNet lane is paused. Try another lane or <Link href="/browse">browse live streams</Link>.</p>}
    {state?.enabled && !featured && <div className="empty-live panel"><p><strong>Nothing is live in this lane right now.</strong> MAGNet starts the moment someone goes live.</p><p><Link href="/browse" className="button quiet">Browse</Link> <Link href="/war-map" className="button quiet">War map</Link></p></div>}
    {featured && <>
      {featured.holding
        ? <div className="magnet-holding panel" role="status">
          <p><strong>This stream isn&apos;t available to you.</strong> MAGNet moves on in about {minutes ?? 8} minute{minutes === 1 ? "" : "s"}. Chat resumes when MAGNet moves on.</p>
          {state!.others.length > 0 && <><p>Other live streams in this lane:</p><div className="stream-grid">{state!.others.map(s => <StreamCard key={s.username} s={s} viewerFaction={viewerFaction} />)}</div></>}
        </div>
        : <div className="watch-player frame"><LivePlayer key={shown!.broadcast_id} username={shown!.username} focused signedIn={signedIn} magnetLane={lane} /></div>}
      {merged.length > 1 && <div className="board-screens" role="tablist" aria-label="Co-stream members">{merged.map(m => <button key={m.username} type="button" role="tab" aria-selected={m.username === shown!.username} className={m.username === shown!.username ? "small" : "small quiet"} onClick={() => setTab({ of: featured.since, username: m.username })}>{m.display_name}</button>)}</div>}
      <p className="magnet-why"><span className="magnet-mark-inline" aria-hidden="true">MAGNet</span> <Link href={`/${shown!.username}`}><strong>{shown!.display_name}</strong></Link> · {shown!.title}{shown!.category && <span className="muted"> · {shown!.category}</span>}{shown!.label && <span className="tag-label">{shown!.label}</span>}<br /><span className="muted">{featured.reason}</span>
        {featured.squad?.mode === "SEPARATE" && featured.squad.members.length > 0 && <><br /><span className="muted">Co-streaming with {featured.squad.members.map((m, i) => <span key={m.username}>{i > 0 && ", "}<Link href={`/${m.username}/live`}>{m.display_name}</Link></span>)}</span></>}</p>
      {state!.next && <div className="magnet-countdown panel" role="status">
        <span className="magnet-next-thumbnail"><LiveThumbnail src={state!.next.stream.thumbnail} label={state!.next.stream.category ?? state!.next.stream.display_name} /></span>
        <p>Up next in <strong>{left}</strong>: <strong>{state!.next.stream.display_name}</strong> · {state!.next.reason}</p>
        <Link className="button small" href={`/${shown!.username}/live`}>Stay with {shown!.display_name}</Link>
      </div>}
    </>}
  </div>
  <HypeChat lane={lane} account={account} upNext={state?.next ? { name: state.next.stream.display_name, seconds: left } : null} />
  </div>;
}
