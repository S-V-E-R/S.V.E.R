import Link from "next/link";
import { Crest } from "../Crest";
import { factionOf } from "../../lib/factions";
import { scene, uptime, type LiveCard } from "./types";
import { LiveThumbnail } from "../LiveThumbnail";

/** A live channel card for the homepage grids (Main mockup: "Live now", "Just went live"). */
export function StreamCard({ s, viewerFaction, fresh = false }: { s: LiveCard; viewerFaction: string | null; fresh?: boolean }) {
  const f = factionOf(s.faction);
  const ally = !!viewerFaction && s.faction === viewerFaction;
  return <Link href={`/${s.username}`} className={ally ? "stream-card ally" : "stream-card"}>
    <span className="stream-thumb" style={{ background: scene(s.username) }}>
      <LiveThumbnail src={s.thumbnail} label={s.category ?? s.display_name} />
      <span className="stream-tags"><span className="tag-live">Live</span>{ally && <span className="tag-ally">Ally</span>}{s.label && <span className="tag-label">{s.label}</span>}</span>
      {fresh
        ? <span className="stream-started">Started {uptime(s.started_at)} ago</span>
        : <><span className="stream-viewers">{s.viewers.toLocaleString()} watching</span><span className="stream-uptime">{uptime(s.started_at)}</span></>}
    </span>
    <span className="stream-meta">
      <Crest faction={s.faction} initial={s.display_name.slice(0, 1).toUpperCase()} size={32} label={f?.name} />
      <span className="stream-text">
        <span className="stream-title">{s.title}</span>
        <span className="stream-name">{s.display_name}</span>
        {s.category && <span className="stream-category">{s.category}</span>}
      </span>
    </span>
  </Link>;
}
