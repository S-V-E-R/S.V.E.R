"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import "../styles/boards.css";

export type BoardEvent = {
  type: "board_effect" | "board_input" | "board";
  control?: string; label?: string; effect?: string; user?: { username: string }; text?: string | null;
  goal?: { progress: number; target: number; reached: boolean } | null;
  stream_ms?: number | null; at?: number; overlay?: boolean; test?: boolean; x?: number; y?: number;
  /** Skills, emote combos and Surge levels carry their own caption; combos carry the emote image. */
  caption?: string; image?: string; skill?: string; surge?: number;
};
type Shown = BoardEvent & { key: number };

const PARTICLES: Record<string, string> = { confetti: "🎉", hearts: "❤️", fireworks: "🎆", stars: "⭐", rain: "💧" };
const STICKERS: Record<string, string> = { "sticker-crown": "👑", "sticker-heart": "💖", "sticker-trophy": "🏆" };
// Sound Skills are synthesized, so no audio files are needed: [frequency, start seconds].
const TUNES: Record<string, [number, number][]> = {
  "sound-chime": [[880, 0], [1320, 0.12]],
  "sound-fanfare": [[523, 0], [659, 0.15], [784, 0.3], [1047, 0.45]],
};
let audio: AudioContext | null = null;
function playSound(effect: string) {
  const tune = TUNES[effect];
  if (!tune) return;
  try {
    audio ??= new AudioContext();
    const start = audio.currentTime;
    for (const [frequency, at] of tune) {
      const tone = audio.createOscillator();
      const gain = audio.createGain();
      tone.type = "triangle";
      tone.frequency.value = frequency;
      gain.gain.setValueAtTime(0.0001, start + at);
      gain.gain.exponentialRampToValueAtTime(0.2, start + at + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + at + 0.4);
      tone.connect(gain).connect(audio.destination);
      tone.start(start + at);
      tone.stop(start + at + 0.45);
    }
  } catch { /* audio unavailable */ }
}
export const REDUCE_KEY = "sver:reduce-effects";
/** The viewer's "reduce effects" toggle or their system's reduced-motion setting. */
export function reduced(): boolean {
  try { return localStorage.getItem(REDUCE_KEY) === "1" || matchMedia("(prefers-reduced-motion: reduce)").matches; } catch { return false; }
}

/** Board effects: a caption naming who used what, and the preset animation unless effects are reduced. */
export function EffectStage({ events, calm }: { events: Shown[]; calm: boolean }) {
  return <div className="board-stage" aria-live="polite">
    {events.map(e => {
      const animate = !calm && !!e.effect && e.effect !== "none" && (!e.goal || e.goal.reached);
      const particle = animate ? PARTICLES[e.effect!] : undefined;
      const sticker = animate ? STICKERS[e.effect!] : undefined;
      const shower = animate && e.effect === "emote-shower" && e.image;
      return <div key={e.key} className={animate ? `board-effect effect-${e.effect}` : "board-effect"}>
        {particle && <div className="board-particles" aria-hidden="true">{Array.from({ length: 18 }, (_, i) => <span key={i} style={{ left: `${(i * 37) % 100}%`, animationDelay: `${(i % 6) * 0.12}s` }}>{particle}</span>)}</div>}
        {shower && <div className="board-particles" aria-hidden="true">{Array.from({ length: 18 }, (_, i) => <span key={i} style={{ left: `${(i * 37) % 100}%`, animationDelay: `${(i % 6) * 0.12}s`, backgroundImage: `url(${JSON.stringify(e.image)})` }} className="emote-particle" />)}</div>}
        {sticker && <div className="board-sticker" aria-hidden="true">{sticker}</div>}
        <p className="board-caption">{e.caption ?? <><strong>{e.user?.username}</strong> {e.goal ? (e.goal.reached ? `completed ${e.label}!` : `added to ${e.label} (${e.goal.progress}/${e.goal.target})`) : `used ${e.label}`}</>}{e.text && <>: <q>{e.text}</q></>}</p>
      </div>;
    })}
  </div>;
}

/** The effects on screen; each stays four seconds. */
export function useShown() {
  const [shown, setShown] = useState<Shown[]>([]);
  const next = useRef(0);
  const add = useCallback((e: BoardEvent) => {
    if (e.effect?.startsWith("sound-") && !reduced()) playSound(e.effect);
    const key = ++next.current;
    setShown(list => [...list.slice(-4), { ...e, key }]);
    setTimeout(() => setShown(list => list.filter(x => x.key !== key)), 4000);
  }, []);
  return [shown, add] as const;
}

/**
 * How far behind real time this player shows the stream, in milliseconds.
 * ponytail: WebRTC is about 0.5 s; HLS uses the distance from the live edge plus 2 s of
 * packaging. Sync to EXT-X-PROGRAM-DATE-TIME if measured drift ever matters.
 */
function delay(video: HTMLVideoElement | null, transport: "webrtc" | "hls" | null) {
  if (transport !== "hls" || !video) return 500;
  const ranges = video.seekable;
  const edge = ranges.length ? ranges.end(ranges.length - 1) : video.currentTime;
  return Math.round((Math.max(0, edge - video.currentTime) + 2) * 1000);
}
let latest = 500;
/** This page's player delay in milliseconds, so poll windows can close by stream time. */
export const playerDelay = () => latest;

/**
 * Effects over the live player, shown when this viewer's video reaches the moment of the press, so
 * CDN and WebRTC viewers see them in step with the picture. Skipped while the streamer's OBS
 * overlay shows them inside the video (never both), and never shows test presses.
 */
export function PlayerEffects({ username, video, transport }: { username: string; video: React.RefObject<HTMLVideoElement | null>; transport: "webrtc" | "hls" | null }) {
  const [shown, add] = useShown();
  useEffect(() => {
    const timers: number[] = [];
    const on = (event: Event) => {
      const { channel, event: e } = (event as CustomEvent<{ channel: string; event: BoardEvent }>).detail;
      if (channel !== username.toLowerCase() || e.type !== "board_effect" || e.overlay || e.test) return;
      timers.push(window.setTimeout(() => add(e), delay(video.current, transport)));
    };
    window.addEventListener("sver:board", on);
    const measure = setInterval(() => { latest = delay(video.current, transport); }, 2000);
    return () => { window.removeEventListener("sver:board", on); timers.forEach(clearTimeout); clearInterval(measure); };
  }, [username, video, transport, add]);
  if (!shown.length) return null;
  return <EffectStage events={shown} calm={reduced()} />;
}
