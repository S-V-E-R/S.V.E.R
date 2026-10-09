import Link from "next/link";
import { Avatar } from "../Avatar";
import { StreamCard } from "./StreamCard";
import type { LiveCard, Recent } from "./types";

export function SectionHead({ id, title, note, level = 2, href, link }: { id: string; title: string; note?: string; level?: 1 | 2; href?: string; link?: string }) {
  const H = level === 1 ? "h1" : "h2";
  return <div className="section-head"><H id={id}>{title}</H><span className="dia" aria-hidden="true" />{note && <span className="section-note">{note}</span>}<span className="rule" aria-hidden="true" />{href && <Link href={href} className="section-link">{link ?? "See all"}</Link>}</div>;
}

export function StreamGrid({ streams, viewerFaction, languages }: { streams: LiveCard[]; viewerFaction: string | null; languages?: string[] }) {
  return <div className="stream-grid">{streams.map(s => <StreamCard key={s.username} s={s} viewerFaction={viewerFaction} fresh={!!s.fresh} languages={languages} />)}</div>;
}

/** Nothing live: never an empty page (docs/MAGNET.md "Nothing live"). */
export function RecentChannels({ recent }: { recent: Recent[] }) {
  if (!recent.length) return null;
  return <ul className="recent-channels">{recent.map(r => r.user.username && <li key={r.user.username}>
    <Link href={`/${r.user.username}`}><Avatar sizes={r.user.avatar} name={r.user.display_name} size={40} /><span><strong>{r.user.display_name}</strong><span className="muted">Live {new Date(r.ended_at).toLocaleDateString()}</span></span></Link>
  </li>)}</ul>;
}
