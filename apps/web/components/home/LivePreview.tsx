"use client";
import { useEffect, useRef, useState } from "react";
import { send } from "../../lib/client-api";
import { startHls } from "../LivePlayer";
import { LiveThumbnail } from "../LiveThumbnail";

/**
 * The carousel's current slide playing live: muted, inline, over the CDN path (HLS), so previews
 * cost no direct-delivery capacity. It sends no heartbeat, so a homepage preview is never counted
 * as a viewer; watching starts on the stream's page. The still stays underneath until the video
 * plays, and is all that shows with reduced motion, data saving, or any playback failure.
 */
export function LivePreview({ username, thumbnail, label }: { username: string; thumbnail?: string | null; label: string }) {
  const video = useRef<HTMLVideoElement>(null);
  const [playing, setPlaying] = useState(false);
  useEffect(() => {
    const element = video.current;
    const saveData = (navigator as Navigator & { connection?: { saveData?: boolean } }).connection?.saveData;
    if (!element || saveData || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    let stop: (() => void) | null = null;
    let cancelled = false;
    const fail = () => { stop?.(); stop = null; if (!cancelled) setPlaying(false); };
    void (async () => {
      const live = await send<{ live: boolean; playback?: { hls: string | null } | null }>("GET", `/api/channels/${encodeURIComponent(username)}/live?transport=hls`);
      const url = live.ok && live.data.live ? live.data.playback?.hls : null;
      if (!url || cancelled) return;
      try {
        stop = await startHls(element, url, fail);
        if (cancelled) { stop(); return; }
        await element.play();
        if (!cancelled) setPlaying(true);
      } catch { fail(); }
    })();
    return () => { cancelled = true; stop?.(); };
  }, [username]);
  return <span className="live-preview">
    <LiveThumbnail src={thumbnail} label={label} />
    <video ref={video} muted playsInline autoPlay aria-hidden="true" tabIndex={-1} className={playing ? "on" : undefined} />
  </span>;
}
