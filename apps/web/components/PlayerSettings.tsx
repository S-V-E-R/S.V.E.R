"use client";
import { type RefObject, useEffect, useState } from "react";
import { send } from "../lib/client-api";

/** "Live 1 h 12 min", refreshed every 30 seconds. */
export function Uptime({ since }: { since: string }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 30_000); return () => clearInterval(timer); }, []);
  const m = Math.max(0, Math.floor((now - Date.parse(since)) / 60_000));
  return <span className="muted" title={`Started ${new Date(since).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`}>Live {m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`}</span>;
}

type Transport = "webrtc" | "hls" | null;
const PROBLEMS: [string, string][] = [["stutter", "It stutters or freezes"], ["no_audio", "No sound"], ["wont_load", "It won't load"], ["out_of_sync", "Sound and picture are out of sync"], ["blurry", "It's blurry"], ["distorted", "The picture or sound is distorted"]];

/** What this player can measure right now. How far behind the live edge is known only over HLS. */
function measure(video: HTMLVideoElement | null, transport: Transport) {
  if (!video) return null;
  const quality = video.getVideoPlaybackQuality?.();
  const edge = video.seekable.length ? video.seekable.end(video.seekable.length - 1) : NaN;
  const behind = transport === "hls" && Number.isFinite(edge) ? Math.max(0, edge - video.currentTime) : null;
  return { width: video.videoWidth, height: video.videoHeight, dropped: quality?.droppedVideoFrames ?? null, total: quality?.totalVideoFrames ?? null, behind };
}

/**
 * Player settings: "Stats for nerds" over the video, and a one-tap "Report playback problem" with
 * fixed choices, sent with what the player measured so staff can tell encoder from delivery.
 */
export function PlayerSettings({ username, broadcast, video, transport, browserId }: { username: string; broadcast: string; video: RefObject<HTMLVideoElement | null>; transport: Transport; browserId: () => string }) {
  const [open, setOpen] = useState(false);
  const [stats, setStats] = useState(false);
  const [reporting, setReporting] = useState(false);
  const [message, setMessage] = useState("");
  const [, tick] = useState(0);
  useEffect(() => {
    if (!stats) return;
    const timer = setInterval(() => tick(n => n + 1), 1000);
    return () => clearInterval(timer);
  }, [stats]);
  const now = stats ? measure(video.current, transport) : null;
  async function report(kind: string) {
    const m = measure(video.current, transport);
    const result = await send("POST", `/api/channels/${encodeURIComponent(username)}/live/problem`, {
      broadcast_id: broadcast, browser_id: browserId(), kind, transport: transport ?? undefined,
      delay_ms: m?.behind != null ? Math.round(m.behind * 1000) : undefined, height: m?.height || undefined, dropped_frames: m?.dropped ?? undefined,
    });
    setReporting(false);
    setMessage(result.ok ? "Thanks. Staff can see this report with what your player measured." : result.error);
  }
  return <>
    <span className="menu player-menu">
      <button type="button" className="small quiet" aria-haspopup="true" aria-expanded={open} onClick={() => setOpen(!open)}>Settings <span aria-hidden="true">▾</span></button>
      {open && <span className="menu-list panel" role="menu">
        <button type="button" role="menuitemcheckbox" aria-checked={stats} className="link-button" onClick={() => { setStats(!stats); setOpen(false); }}>Stats for nerds</button>
        <button type="button" role="menuitem" className="link-button" onClick={() => { setReporting(true); setMessage(""); setOpen(false); }}>Report playback problem</button>
      </span>}
    </span>
    {now && <dl className="player-stats" aria-label="Playback stats">
      <dt>Delivery</dt><dd>{transport === "webrtc" ? "WebRTC (direct)" : transport === "hls" ? "HLS (CDN)" : "Starting"}</dd>
      <dt>Resolution</dt><dd>{now.height ? `${now.width}×${now.height}` : "—"}</dd>
      {now.behind != null && <><dt>Behind live</dt><dd>{now.behind.toFixed(1)} s</dd></>}
      {now.total != null && <><dt>Dropped frames</dt><dd>{now.dropped?.toLocaleString()} of {now.total.toLocaleString()}</dd></>}
      <dd><button type="button" className="small quiet" onClick={() => setStats(false)}>Close</button></dd>
    </dl>}
    {reporting && <fieldset className="panel inline-panel playback-problem">
      <legend>What went wrong?</legend>
      {PROBLEMS.map(([kind, label]) => <button key={kind} type="button" className="small quiet" onClick={() => void report(kind)}>{label}</button>)}
      <button type="button" className="small quiet" onClick={() => setReporting(false)}>Cancel</button>
    </fieldset>}
    {message && <span role="status" className="small-text">{message}</span>}
  </>;
}
