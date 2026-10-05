import Link from "next/link";
import { Avatar } from "./Avatar";
import type { Chip, Sizes } from "../lib/types";

export type StreamCardData = { user: Chip; banner: Sizes; started_at: string; title: string; category: string | null; viewers: number; live: boolean };
export type StreamDirectory = { live: StreamCardData[]; recent: StreamCardData[]; has_more: boolean; as_of: string };

export function ShelfHeading({ id, title, description, href, link = "View all" }: { id: string; title: string; description?: string; href?: string; link?: string }) {
  return <div className="shelf-heading"><h2 id={id}>{title}</h2><span className="heading-diamond" aria-hidden="true" />{description && <p>{description}</p>}<span className="heading-rule" />{href && <Link href={href}>{link}</Link>}</div>;
}

export function StreamCard({ stream }: { stream: StreamCardData }) {
  const href = `/${stream.user.username}${stream.live ? "/live" : ""}`;
  const banner = Object.values(stream.banner ?? {})[0];
  return <article className="stream-card">
    <Link href={href} className="stream-thumbnail" aria-label={`${stream.live ? "Watch" : "Visit"} ${stream.user.display_name}`}>
      {/* A channel banner is the fallback until live thumbnails are available. */}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {banner ? <img src={banner} alt="" loading="lazy" /> : <span className="stream-category-art">{stream.category || stream.user.display_name}</span>}
      <span className={stream.live ? "badge live" : "badge"}>{stream.live ? "Live" : "Offline"}</span>
      {stream.live && <span className="stream-viewers">{stream.viewers.toLocaleString()} watching</span>}
    </Link>
    <div className="stream-card-info"><Avatar sizes={stream.user.avatar} name={stream.user.display_name} size={32} /><div>
      <h3><Link href={href}>{stream.title}</Link></h3><Link className="stream-creator" href={`/${stream.user.username}`}>{stream.user.display_name}</Link>
      {stream.category && <span className="category-chip">{stream.category}</span>}
    </div></div>
  </article>;
}

export function StreamShelf({ streams }: { streams: StreamCardData[] }) {
  return <div className="stream-grid">{streams.map(stream => <StreamCard key={stream.user.username} stream={stream} />)}</div>;
}
