"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import type Hls from "hls.js";
import { send } from "../lib/client-api";
import type { Chapter, VideoPage } from "../lib/videos";
import { duration } from "../lib/videos";
import { Turnstile } from "./Turnstile";
import "../styles/videos.css";

export function RecordedPlayer({ id, ageAck = false, chapters = [], onTime, autoPlay = false, review = false }: { id: string; ageAck?: boolean; chapters?: Chapter[]; onTime?: (ms: number) => void; autoPlay?: boolean; review?: boolean }) {
  const video = useRef<HTMLVideoElement>(null);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [sitekey, setSitekey] = useState<string | null>(null);
  const token = useRef("");
  const onToken = useCallback((value: string) => { token.current = value; }, []);
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    let stopped = false, loading = false;
    let hls: Hls | null = null;
    let seek: (() => void) | undefined;
    async function refresh() {
      if (loading || stopped) return;
      loading = true;
      const result = await send<VideoPage>("GET", review ? `/api/admin/videos/${id}/review` : `/api/videos/${id}?age_ack=${ageAck}`);
      if (stopped) return;
      if (!result.ok) { setError(result.error); loading = false; return; }
      const at = element!.currentTime, resume = !element!.paused || autoPlay;
      if (seek) element!.removeEventListener("loadedmetadata", seek);
      seek = () => { element!.currentTime = at; if (resume) void element!.play().catch(() => {}); };
      element!.addEventListener("loadedmetadata", seek, { once: true });
      try {
        if (result.data.video.kind === "CLIP" || element!.canPlayType("application/vnd.apple.mpegurl")) element!.src = result.data.playback;
        else {
          const { default: Hls } = await import("hls.js");
          if (stopped) return;
          if (!Hls.isSupported()) throw new Error("This browser cannot play the recording.");
          if (!hls) {
            hls = new Hls({ enableWorker: true, startPosition: 0 });
            hls.on(Hls.Events.ERROR, (_event, data) => { if (data.fatal) { hls?.stopLoad(); setError("Playback was interrupted. Retry to refresh your access."); } });
            hls.attachMedia(element!);
          }
          hls.loadSource(result.data.playback);
        }
        element!.poster = result.data.thumbnail ?? "";
        setError("");
      } catch (error) { setError(error instanceof Error ? error.message : "Playback could not start."); }
      loading = false;
    }
    void refresh();
    // Refresh before the five-minute ticket expiry, retaining the viewer's position.
    const timer = setInterval(() => void refresh(), 240000);
    return () => { stopped = true; clearInterval(timer); if (seek) element.removeEventListener("loadedmetadata", seek); hls?.destroy(); element.removeAttribute("src"); element.load(); };
  }, [id, ageAck, attempt, autoPlay, review]);
  useEffect(() => {
    if (review) return;
    const element = video.current;
    if (!element) return;
    let browser: string;
    try { browser = localStorage.getItem("sver-browser") || crypto.randomUUID(); localStorage.setItem("sver-browser", browser); } catch { browser = crypto.randomUUID(); }
    let last = -1, sending = false, stopped = false;
    async function beat(boundary = false) {
      if (sending || stopped || (!boundary && element!.paused) || element!.readyState < 2 || element!.currentTime === last) return;
      sending = true;
      last = element!.currentTime;
      const check = token.current; token.current = "";
      const result = await send<{ needs_turnstile?: boolean }>("POST", `/api/videos/${id}/beat`, { browser_id: browser, media_time: element!.currentTime, visible: !document.hidden, age_ack: ageAck, turnstile: check || undefined });
      sending = false;
      if (stopped) return;
      if (result.ok && result.data.needs_turnstile) { const config = await send<{ turnstile_site_key: string }>("GET", "/api/auth/config"); if (config.ok) setSitekey(config.data.turnstile_site_key); }
      else setSitekey(null);
    }
    const boundary = () => { void beat(true); };
    for (const event of ["playing", "pause", "ended"]) element.addEventListener(event, boundary);
    const timer = setInterval(() => void beat(), 10000);
    return () => { stopped = true; clearInterval(timer); for (const event of ["playing", "pause", "ended"]) element.removeEventListener(event, boundary); };
  }, [id, ageAck, review]);
  return <div className="recorded-player">
    <video ref={video} controls controlsList="nodownload" playsInline preload="metadata" aria-label="Recording player" onTimeUpdate={e => onTime?.(Math.floor(e.currentTarget.currentTime * 1000))} onError={() => setError("Playback was interrupted. Retry to refresh your access.")} />
    {error && <p className="notice" role="alert">{error} <button type="button" className="small quiet" onClick={() => setAttempt(n => n + 1)}>Retry</button></p>}
    {sitekey && <Turnstile sitekey={sitekey} action="playback" onToken={onToken} />}
    {chapters.length > 0 && <nav className="video-chapters" aria-label="Chapters">{chapters.map(c => <button className="small quiet" type="button" key={c.id} onClick={() => { if (video.current) video.current.currentTime = c.offset_ms / 1000; }}><time>{duration(c.offset_ms)}</time> {c.label}</button>)}</nav>}
  </div>;
}

export function RecordingEvidence({ id }: { id: string }) {
  const [open, setOpen] = useState(false);
  return <div><button type="button" className="small quiet" onClick={() => setOpen(value => !value)} aria-expanded={open}>{open ? "Close recording evidence" : "Review recording evidence"}</button>{open && <RecordedPlayer id={id} review />}</div>;
}
