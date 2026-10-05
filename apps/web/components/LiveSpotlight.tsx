"use client";
import Link from "next/link";
import { useState } from "react";
import { StreamCard, type StreamCardData } from "./StreamShelf";

/** Manual carousel: browsing previews never opens playback sessions or counts as watching. */
export function LiveSpotlight({ streams }: { streams: StreamCardData[] }) {
  const [index, setIndex] = useState(0);
  const current = index % streams.length;
  const stream = streams[current];
  return <div className="spotlight frame" role="region" aria-roledescription="carousel" aria-label="Live spotlight">
    <StreamCard stream={stream} />
    <div className="spotlight-copy"><p className="eyebrow">Live now</p><h2>{stream.title}</h2><p>{stream.user.display_name}{stream.category && ` · ${stream.category}`}</p><Link className="button" href={`/${stream.user.username}/live`}>Watch stream</Link>
      {streams.length > 1 && <div className="spotlight-controls"><button type="button" className="quiet" aria-label="Previous stream" onClick={() => setIndex((current + streams.length - 1) % streams.length)}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><path d="m15 5-7 7 7 7" /></svg></button><span aria-live="polite" aria-atomic="true">{current + 1} / {streams.length}</span><button type="button" className="quiet" aria-label="Next stream" onClick={() => setIndex((current + 1) % streams.length)}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><path d="m9 5 7 7-7 7" /></svg></button></div>}
    </div>
  </div>;
}
