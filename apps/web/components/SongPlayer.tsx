"use client";
import { useState } from "react";
import type { Song } from "../lib/types";

/** Compact song card. The provider iframe loads only after a click and never autoplays. */
export function SongPlayer({ song }: { song: NonNullable<Song> }) {
  const [loaded, setLoaded] = useState(false);
  const src = song.provider === "youtube"
    ? `https://www.youtube-nocookie.com/embed/${encodeURIComponent(song.media_id)}?autoplay=0&rel=0`
    : `https://w.soundcloud.com/player/?url=${encodeURIComponent(`https://api.soundcloud.com/tracks/${song.media_id}`)}&auto_play=false&visual=false`;
  const title = song.title || "Profile song";
  return <section className="song panel" aria-label="Profile song">
    {loaded ? <iframe className="song-frame" src={src} title={title} allow="encrypted-media" referrerPolicy="strict-origin-when-cross-origin" sandbox="allow-scripts allow-same-origin allow-popups" />
      : <button type="button" className="song-start quiet" onClick={() => setLoaded(true)}>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        {song.thumbnail ? <img src={song.thumbnail} alt="" width={64} height={48} /> : <span className="song-icon" aria-hidden="true">♪</span>}
        <span><strong>{title}</strong>{song.artist && <small>{song.artist}</small>}<small>Play on {song.provider === "youtube" ? "YouTube" : "SoundCloud"}</small></span>
      </button>}
  </section>;
}
