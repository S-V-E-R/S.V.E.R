"use client";
import { useRef, useState } from "react";
import type { Song } from "../lib/types";

const origins = { youtube: "https://www.youtube-nocookie.com", soundcloud: "https://w.soundcloud.com" } as const;

/** Messages that set the owner's default volume (0-100) through the provider's player API. */
export function volumeMessages(provider: "youtube" | "soundcloud", volume: number) {
  const value = Math.max(0, Math.min(100, Math.round(volume)));
  return provider === "youtube"
    ? [JSON.stringify({ event: "listening", id: "sver-song" }), JSON.stringify({ event: "command", func: "setVolume", args: [value] })]
    : [JSON.stringify({ method: "setVolume", value })];
}

/** Compact song card. The provider iframe is created only after the visitor clicks play; playback then starts
 *  at the owner's default volume. Nothing autoplays on page load (docs/PROFILES.md, "Profile song"). */
export function SongPlayer({ song }: { song: NonNullable<Song> }) {
  const [loaded, setLoaded] = useState(false);
  const frame = useRef<HTMLIFrameElement>(null);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const title = song.title || "Profile song";
  const src = () => song.provider === "youtube"
    ? `${origins.youtube}/embed/${encodeURIComponent(song.media_id)}?autoplay=1&rel=0&enablejsapi=1&origin=${encodeURIComponent(window.location.origin)}`
    : `${origins.soundcloud}/player/?url=${encodeURIComponent(`https://api.soundcloud.com/tracks/${song.media_id}`)}&auto_play=true&visual=false`;
  // Players finish initialising after the frame's load event, so the volume is sent a few times.
  function applyVolume() {
    timers.current.forEach(clearTimeout);
    timers.current = [0, 600, 1500, 3000].map(delay => setTimeout(() => {
      const target = frame.current?.contentWindow;
      if (target) for (const message of volumeMessages(song.provider, song.volume ?? 70)) target.postMessage(message, origins[song.provider]);
    }, delay));
  }
  return <section className="song panel" aria-label="Profile song">
    {loaded ? <iframe ref={frame} className="song-frame" src={src()} title={title} allow="autoplay; encrypted-media" onLoad={applyVolume} referrerPolicy="strict-origin-when-cross-origin" sandbox="allow-scripts allow-same-origin allow-popups" />
      : <button type="button" className="song-start quiet" onClick={() => setLoaded(true)}>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        {song.thumbnail ? <img src={song.thumbnail} alt="" width={64} height={48} /> : <span className="song-icon" aria-hidden="true">♪</span>}
        <span><strong>{title}</strong>{song.artist && <small>{song.artist}</small>}<small>Play on {song.provider === "youtube" ? "YouTube" : "SoundCloud"}</small></span>
      </button>}
  </section>;
}
