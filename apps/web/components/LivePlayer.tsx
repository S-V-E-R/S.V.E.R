"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import { send, useLoad } from "../lib/client-api";

type Playback = { webrtc: string | null; hls: string | null; preferred: "webrtc" | "hls" };
type Live =
  | { live: false }
  | { live: true; broadcast_id: string; state: "LIVE" | "RECONNECTING"; title: string; category: string | null; viewers: number; is_owner: boolean; playback: Playback };
type Phase = "loading" | "playing" | "reconnecting" | "blocked" | "failed";

let cachedBrowserId = "";
/** Random first-party ID so a guest counts once across tabs. Never derived from the IP. */
function browserId() {
  if (cachedBrowserId) return cachedBrowserId;
  try {
    cachedBrowserId = localStorage.getItem("sver-browser") || crypto.randomUUID();
    localStorage.setItem("sver-browser", cachedBrowserId);
  } catch {
    cachedBrowserId = crypto.randomUUID();
  }
  return cachedBrowserId;
}

/** SRS WHEP: one recvonly offer, the answer comes back in the response body. */
async function startWebRtc(video: HTMLVideoElement, url: string, onFatal: () => void): Promise<() => void> {
  const pc = new RTCPeerConnection();
  pc.addTransceiver("video", { direction: "recvonly" });
  pc.addTransceiver("audio", { direction: "recvonly" });
  const stream = new MediaStream();
  pc.ontrack = event => { stream.addTrack(event.track); video.srcObject = stream; };
  pc.onconnectionstatechange = () => { if (pc.connectionState === "failed" || pc.connectionState === "disconnected") onFatal(); };
  try {
    await pc.setLocalDescription(await pc.createOffer());
    const response = await fetch(url, { method: "POST", body: pc.localDescription?.sdp, headers: { "Content-Type": "application/sdp" } });
    if (!response.ok) throw new Error("WebRTC playback was refused.");
    await pc.setRemoteDescription({ type: "answer", sdp: await response.text() });
  } catch (error) {
    pc.close();
    throw error;
  }
  return () => { pc.close(); video.srcObject = null; };
}

async function startHls(video: HTMLVideoElement, url: string, onFatal: () => void): Promise<() => void> {
  if (video.canPlayType("application/vnd.apple.mpegurl")) {
    video.src = url;
    return () => { video.removeAttribute("src"); video.load(); };
  }
  const { default: Hls } = await import("hls.js");
  if (!Hls.isSupported()) throw new Error("This browser can't play the stream.");
  const hls = new Hls({ lowLatencyMode: true });
  hls.on(Hls.Events.ERROR, (_event, data) => { if (data.fatal) onFatal(); });
  hls.loadSource(url);
  hls.attachMedia(video);
  return () => hls.destroy();
}

/** Resolves when media is actually playing, rejects after `ms` (the WebRTC startup timeout). */
function playingWithin(video: HTMLVideoElement, ms: number) {
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => { video.removeEventListener("playing", done); reject(new Error("timeout")); }, ms);
    function done() { clearTimeout(timer); resolve(); }
    video.addEventListener("playing", done, { once: true });
  });
}

/**
 * Live player for a channel. Shows `children` (the offline banner) when the channel isn't live.
 * WebRTC first when offered; on failure or an 8-second startup timeout it falls back to HLS.
 * Uses native controls for keyboard, fullscreen, volume and captions.
 */
export function LivePlayer({ username, focused = false, children }: { username: string; focused?: boolean; children?: React.ReactNode }) {
  const [live, setLive] = useState<Live | null>(null);
  const [phase, setPhase] = useState<Phase>("loading");
  const [attempt, setAttempt] = useState(0);
  const video = useRef<HTMLVideoElement>(null);
  const path = `/api/channels/${encodeURIComponent(username)}/live`;

  const load = useCallback(async () => {
    const result = await send<Live>("GET", path);
    if (result.ok) setLive(result.data);
  }, [path]);
  useLoad(load);
  useEffect(() => {
    const timer = setInterval(() => { if (!document.hidden) void load(); }, 15000);
    return () => clearInterval(timer);
  }, [load]);

  const broadcast = live?.live ? live.broadcast_id : null;
  // Plain strings so a poll returning the same URLs does not restart playback.
  const webrtc = live?.live ? live.playback.webrtc : null;
  const hls = live?.live ? live.playback.hls : null;
  const preferred = live?.live ? live.playback.preferred : null;
  const isOwner = live?.live ? live.is_owner : false;

  // Start (or restart, on `attempt`) playback for the current broadcast; the cleanup stops the retired transport.
  useEffect(() => {
    const element = video.current;
    if (!broadcast || !element) return;
    let stop: (() => void) | null = null;
    let cancelled = false;
    const fail = () => { if (!cancelled) setPhase("reconnecting"); };
    (async () => {
      setPhase("loading");
      const order = preferred === "webrtc" ? [webrtc, hls] : [hls, webrtc];
      for (const url of order) {
        if (!url || cancelled) continue;
        try {
          stop = url === webrtc ? await startWebRtc(element, url, fail) : await startHls(element, url, fail);
          if (cancelled) break;
          const started = playingWithin(element, 8000);
          await element.play().catch(() => { if (!cancelled) setPhase("blocked"); });
          await started;
          if (!cancelled) setPhase("playing");
          return;
        } catch {
          stop?.();
          stop = null;
        }
      }
      if (!cancelled) setPhase("failed");
    })();
    return () => { cancelled = true; stop?.(); };
  }, [broadcast, webrtc, hls, preferred, attempt]);

  // Retry a dropped transport a few seconds later; the broadcast's 60-second reconnect grace keeps it live meanwhile.
  useEffect(() => {
    if (phase !== "reconnecting") return;
    const timer = setTimeout(() => setAttempt(n => n + 1), 3000);
    return () => clearTimeout(timer);
  }, [phase]);

  // Heartbeat only while media is actually advancing; the owner's own preview is never counted.
  useEffect(() => {
    if (!broadcast || isOwner) return;
    let last = -1;
    const timer = setInterval(() => {
      const element = video.current;
      if (!element || element.paused || element.currentTime <= last) return;
      last = element.currentTime;
      void send("POST", `${path}/beat`, { broadcast_id: broadcast, browser_id: browserId() });
    }, 10000);
    return () => clearInterval(timer);
  }, [broadcast, isOwner, path]);

  if (!live?.live) return <>{children}</>;
  const status = live.state === "RECONNECTING" || phase === "reconnecting" ? "Reconnecting…" : phase === "loading" ? "Loading the stream…" : null;
  return <div className={focused ? "live-player focused" : "live-player"}>
    <video ref={video} controls playsInline aria-label={`${live.title}, live`} />
    {status && <p className="player-status" role="status">{status}</p>}
    {phase === "blocked" && <button type="button" className="player-action" onClick={() => { void video.current?.play().then(() => setPhase("playing")); }}>Play</button>}
    {phase === "failed" && <div className="player-action" role="alert"><p>The stream couldn&apos;t be played.</p><button type="button" onClick={() => setAttempt(n => n + 1)}>Retry</button></div>}
    <p className="live-meta"><span className="live badge">Live</span> <strong>{live.title}</strong>{live.category && <span className="muted"> · {live.category}</span>} <span className="muted">· {live.viewers.toLocaleString()} watching</span></p>
  </div>;
}
