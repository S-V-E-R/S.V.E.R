"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { Crest } from "../Crest";
import { factionOf } from "../../lib/factions";
import { scene, type LiveCard } from "./types";
import { LiveThumbnail } from "../LiveThumbnail";

/**
 * The rotation carousel (Main mockup). Every live stream gets a turn; it never orders by viewer
 * count. MAGNet (Module 5) replaces the simple hourly shuffle the API supplies today.
 */
export function Rotation({ streams, viewerFaction }: { streams: LiveCard[]; viewerFaction: string | null }) {
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const n = streams.length;
  useEffect(() => {
    if (n < 2 || paused || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = setInterval(() => setIndex(i => (i + 1) % n), 12000);
    return () => clearInterval(timer);
  }, [n, paused]);
  if (!n) return null;
  const cur = streams[index % n];
  const next = streams[(index + 1) % n];
  const prev = streams[(index + n - 1) % n];
  const f = factionOf(cur.faction);
  const ally = !!viewerFaction && cur.faction === viewerFaction;
  return <div className="rotation" onMouseEnter={() => setPaused(true)} onMouseLeave={() => setPaused(false)} onFocus={() => setPaused(true)} onBlur={() => setPaused(false)}>
    <div className="rotation-row">
    {n > 1 && <span className="rotation-peek" style={{ background: scene(prev.username) }} aria-hidden="true" />}
    <div className="rotation-card frame" aria-live="polite">
      <Link href={`/${cur.username}`} className="rotation-stage" style={{ background: scene(cur.username) }} aria-label={`Watch ${cur.display_name}`}>
        <LiveThumbnail key={cur.broadcast_id ?? cur.username} src={cur.thumbnail} label={cur.category ?? cur.display_name} />
        <span className="stream-tags"><span className="tag-live">Live</span>{ally && <span className="tag-ally">Ally</span>}</span>
      </Link>
      <div className="rotation-info">
        <div className="rotation-who">
          <Crest faction={cur.faction} initial={cur.display_name.slice(0, 1).toUpperCase()} size={42} label={f?.name} />
          <span><span className="rotation-name">{cur.display_name}</span>{f && <span className="rotation-faction" style={{ color: f.color }}>{f.name} · {f.title}</span>}</span>
        </div>
        <p className="rotation-title">{cur.title}</p>
        <div className="chips">{cur.category && <span className="chip">{cur.category}</span>}<span className="chip">{cur.viewers.toLocaleString()} watching</span></div>
        <div className="rotation-actions"><Link href={`/${cur.username}`} className="button">Watch</Link></div>
        {n > 1 && <p className="rotation-next">Up next in rotation: <strong>{next.display_name}</strong>{next.category ? ` · ${next.category}` : ""}</p>}
      </div>
    </div>
    {n > 1 && <span className="rotation-peek" style={{ background: scene(next.username) }} aria-hidden="true" />}
    </div>
    {n > 1 && <div className="rotation-controls">
      <button type="button" className="rotation-arrow prev" aria-label="Previous stream" onClick={() => setIndex(i => (i + n - 1) % n)}><svg width="18" height="18" viewBox="0 0 12 12" fill="none" stroke="currentColor" aria-hidden="true"><path d="M7.5 2.5 4 6l3.5 3.5" /></svg></button>
      <div className="rotation-dots" aria-hidden="true">{streams.map((s, i) => <span key={s.username} className={i === index % n ? "on" : undefined} />)}</div>
      <button type="button" className="rotation-arrow next" aria-label="Next stream" onClick={() => setIndex(i => (i + 1) % n)}><svg width="18" height="18" viewBox="0 0 12 12" fill="none" stroke="currentColor" aria-hidden="true"><path d="M4.5 2.5 8 6 4.5 9.5" /></svg></button>
    </div>}
  </div>;
}
