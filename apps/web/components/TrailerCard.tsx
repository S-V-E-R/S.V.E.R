"use client";
import { useState } from "react";
import { RecordedPlayer } from "./RecordedPlayer";
import type { Trailer } from "../lib/types";

/** The channel trailer as a 16:9 framed card: thumbnail, title and Play. The player (a
 *  youtube-nocookie iframe or our own recorded player) is created only after the visitor presses
 *  Play; nothing autoplays on page load (docs/PROFILES.md, "Channel trailer"). */
export function TrailerCard({ trailer, heading = "Trailer" }: { trailer: NonNullable<Trailer>; heading?: string }) {
  const [playing, setPlaying] = useState(false);
  const title = trailer.title || "Channel trailer";
  return <section className="panel section trailer" aria-label="Channel trailer">
    <h2>{heading}</h2>
    {playing
      ? trailer.provider === "youtube"
        ? <iframe className="trailer-frame" src={`https://www.youtube-nocookie.com/embed/${encodeURIComponent(trailer.media_id)}?autoplay=1&rel=0`} title={title} allow="autoplay; encrypted-media; fullscreen" allowFullScreen referrerPolicy="strict-origin-when-cross-origin" sandbox="allow-scripts allow-same-origin allow-popups" />
        : <RecordedPlayer id={trailer.media_id} autoPlay />
      : <button type="button" className="trailer-start" onClick={() => setPlaying(true)} aria-label={`Play ${title}`}>
        {/* Own-recording thumbnails are signed and served by the API, never the image optimizer. */}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        {trailer.thumbnail && <img src={trailer.thumbnail} alt="" width={640} height={360} />}
        <span className="trailer-play"><svg aria-hidden="true" width="12" height="12" viewBox="0 0 12 12"><path d="M3 2l7 4-7 4z" fill="currentColor" /></svg> Play</span>
      </button>}
    <p className="trailer-title"><strong>{title}</strong></p>
  </section>;
}
