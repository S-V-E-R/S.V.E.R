"use client";
import { useEffect, useRef, useState } from "react";

/** A still only: visible cards refresh once a minute without creating playback sessions. */
export function LiveThumbnail({ src, label }: { src?: string | null; label: string }) {
  const frame = useRef<HTMLSpanElement>(null);
  const [version, setVersion] = useState(0);
  const [failed, setFailed] = useState<string | null>(null);
  const url = src ? `${src}${src.includes("?") ? "&" : "?"}v=${version}` : null;
  useEffect(() => {
    if (!src || !frame.current) return;
    let visible = false;
    let refreshedAt = Date.now();
    const refresh = () => {
      const now = Date.now();
      if (visible && !document.hidden && now - refreshedAt >= 60000) {
        refreshedAt = now;
        setVersion(Math.floor(now / 60000));
      }
    };
    const observer = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; refresh(); });
    observer.observe(frame.current);
    const timer = setInterval(refresh, 60000);
    document.addEventListener("visibilitychange", refresh);
    return () => { observer.disconnect(); clearInterval(timer); document.removeEventListener("visibilitychange", refresh); };
  }, [src]);
  return <span className="live-thumbnail" ref={frame} aria-hidden="true">
    <span className="stream-thumb-mark">{label}</span>
    {/* eslint-disable-next-line @next/next/no-img-element -- small server-generated still, never video */}
    {url && <img src={url} alt="" loading="lazy" decoding="async" hidden={failed === url} onError={() => setFailed(url)} />}
  </span>;
}
