"use client";
import Link from "next/link";
import { useCallback, useEffect, useRef, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { ReportButton, TakeDownLink } from "./Report";
import { Turnstile } from "./Turnstile";
import { UpNext } from "./UpNext";

type Playback = { webrtc: string | null; hls: string | null; preferred: "webrtc" | "hls" };
type Raid = { id: string; status: "countdown" | "cancelled" | "moved" | "failed"; execute_at: string; target: { username: string; display_name: string } };
type Live =
  | { live: false; hosting?: { username: string; display_name: string } }
  | { live: true; broadcast_id: string; state: "LIVE" | "RECONNECTING"; title: string; category: string | null; viewers: number; is_owner: boolean; banned?: boolean; playback: Playback | null; raid?: Raid | null };
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
export function LivePlayer({ username, focused = false, signedIn = false, nested = false, children }: { username: string; focused?: boolean; signedIn?: boolean; nested?: boolean; children?: React.ReactNode }) {
  const [live, setLive] = useState<Live | null>(null);
  const [phase, setPhase] = useState<Phase>("loading");
  const [attempt, setAttempt] = useState(0);
  const video = useRef<HTMLVideoElement>(null);
  // Guests pass a security check once per session before they count (viewer integrity).
  const [sitekey, setSitekey] = useState<string | null>(null);
  const token = useRef("");
  const onToken = useCallback((value: string) => { token.current = value; }, []);
  const path = `/api/channels/${encodeURIComponent(username)}/live`;
  // Raids: pushed over chat for an instant start, otherwise found by the regular poll.
  const [pushed, setPushed] = useState<Raid | null | undefined>(undefined);
  const [stayed, setStayed] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const arrivedFrom = useRef<string | null>(null);
  useEffect(() => { arrivedFrom.current = new URLSearchParams(window.location.search).get("raid"); }, []);
  useEffect(() => {
    const onRaid = (event: Event) => {
      const detail = (event as CustomEvent<{ channel: string; raid: Raid | null }>).detail;
      if (detail.channel === username.toLowerCase()) setPushed(detail.raid);
    };
    window.addEventListener("sver:raid", onRaid);
    return () => window.removeEventListener("sver:raid", onRaid);
  }, [username]);

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
  const webrtc = live?.live ? live.playback?.webrtc ?? null : null;
  const hls = live?.live ? live.playback?.hls ?? null : null;
  const preferred = live?.live ? live.playback?.preferred ?? null : null;
  const isOwner = live?.live ? live.is_owner : false;
  // Remember that this page saw the stream live, so its end can offer the next stream.
  const [ended, setEnded] = useState(false);
  const wasLive = useRef(false);
  useEffect(() => {
    if (live?.live) wasLive.current = true;
    else if (live && wasLive.current && !isOwner) setEnded(true);
  }, [live, isOwner]);
  const polled = live?.live ? live.raid ?? null : null;
  const raid = pushed === undefined ? polled : pushed;
  const counting = raid && raid.status !== "cancelled" && raid.status !== "failed" && stayed !== raid.id ? raid : null;
  const moving = useRef("");

  // Countdown: at zero the player asks whether the raid went ahead, then moves to the target.
  useEffect(() => {
    if (!counting) return;
    const timer = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(timer);
  }, [counting]);
  useEffect(() => {
    if (!counting || isOwner || now < Date.parse(counting.execute_at) || moving.current === counting.id) return;
    moving.current = counting.id;
    void send<Raid>("GET", `/api/raids/${counting.id}`).then(result => {
      // A full page load on purpose: the target gets a fresh player, chat socket and raid link.
      // eslint-disable-next-line @next/next/no-location-assign-relative-destination
      if (result.ok && result.data.status === "moved") window.location.assign(`/${result.data.target.username}${focused ? "/live" : ""}?raid=${counting.id}`);
      else setStayed(counting.id);
    });
  }, [counting, now, isOwner, focused]);

  // Start (or restart, on `attempt`) playback for the current broadcast; the cleanup stops the retired transport.
  useEffect(() => {
    const element = video.current;
    if (!broadcast || !element || (!webrtc && !hls)) return;
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
      const check = token.current;
      token.current = "";
      void send<{ recorded: boolean; needs_turnstile?: boolean }>("POST", `${path}/beat`, { broadcast_id: broadcast, browser_id: browserId(), visible: !document.hidden, media_time: element.currentTime, turnstile: check || undefined, raid: arrivedFrom.current || undefined }).then(async result => {
        if (!result.ok || !result.data.needs_turnstile) { setSitekey(null); return; }
        const config = await send<{ turnstile_site_key: string }>("GET", "/api/auth/config");
        if (config.ok) setSitekey(config.data.turnstile_site_key);
      });
    }, 10000);
    return () => clearInterval(timer);
  }, [broadcast, isOwner, path]);

  if (!live?.live) {
    // An offline channel hosting a live one shows that stream; its viewers count for the target.
    if (live?.hosting && !nested) return <div className="hosting">
      <p className="hosting-bar">Hosting <Link href={`/${live.hosting.username}`}>{live.hosting.display_name}</Link></p>
      <LivePlayer username={live.hosting.username} focused={focused} signedIn={signedIn} nested>{children}</LivePlayer>
    </div>;
    return <>{ended && !nested && <UpNext username={username} focused={focused} />}{children}</>;
  }
  if (live.banned || (!webrtc && !hls)) return <div className="live-player"><p className="panel" role="status">{live.banned ? "You're banned from this channel, so the stream isn't available while you're signed in." : "This stream can't be played here yet."}</p></div>;
  const status = live.state === "RECONNECTING" || phase === "reconnecting" ? "Reconnecting…" : phase === "loading" ? "Loading the stream…" : null;
  return <div className={focused ? "live-player focused" : "live-player"}>
    <video ref={video} controls playsInline aria-label={`${live.title}, live`} />
    {status && <p className="player-status" role="status">{status}</p>}
    {phase === "blocked" && <button type="button" className="player-action" onClick={() => { void video.current?.play().then(() => setPhase("playing")); }}>Play</button>}
    {phase === "failed" && <div className="player-action" role="alert"><p>The stream couldn&apos;t be played.</p><button type="button" onClick={() => setAttempt(n => n + 1)}>Retry</button></div>}
    {sitekey && <Turnstile sitekey={sitekey} action="playback" onToken={onToken} />}
    {counting && <div className="raid-countdown" role="status">
      {isOwner ? <>Raiding <strong>{counting.target.display_name}</strong> in {Math.max(0, Math.ceil((Date.parse(counting.execute_at) - now) / 1000))}s <button type="button" className="small quiet" onClick={() => void send("DELETE", "/api/me/raids").then(r => { if (r.ok) setPushed(null); })}>Cancel raid</button></>
        : <>Raiding <strong>{counting.target.display_name}</strong> in {Math.max(0, Math.ceil((Date.parse(counting.execute_at) - now) / 1000))}s <button type="button" className="small quiet" onClick={() => setStayed(counting.id)}>Stay here</button></>}
    </div>}
    <p className="live-meta"><span className="live badge">Live</span> <strong>{live.title}</strong>{live.category && <span className="muted"> · {live.category}</span>} <span className="muted">· {live.viewers.toLocaleString()} watching</span> {signedIn && !live.is_owner ? <ReportButton target={{ target_type: "live_stream", target_id: live.broadcast_id }} label="Report stream" /> : <TakeDownLink target={{ target_type: "live_stream", target_id: live.broadcast_id }} />}</p>
  </div>;
}
