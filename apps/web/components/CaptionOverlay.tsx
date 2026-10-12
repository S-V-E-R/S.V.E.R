"use client";
import { useEffect, useRef, useState, useSyncExternalStore, type RefObject } from "react";
import { send } from "../lib/client-api";

export type CaptionStyle = { size: "s" | "m" | "l"; x: number; y: number };
const SIZES: CaptionStyle["size"][] = ["s", "m", "l"];
const clamp = (n: number) => Math.min(1, Math.max(0, n));

// The viewer's own size and position, remembered per browser (in memory when storage is blocked).
const listeners = new Set<() => void>();
let memory: string | null = null;
function savedRaw(): string | null {
  if (memory !== null) return memory;
  try { return localStorage.getItem("sver-caption-style"); } catch { return null; }
}
function setSaved(style: CaptionStyle | null) {
  memory = style ? JSON.stringify(style) : "";
  try { if (style) localStorage.setItem("sver-caption-style", memory); else localStorage.removeItem("sver-caption-style"); } catch { /* storage blocked: this page only */ }
  listeners.forEach(listener => listener());
}
function parse(raw: string | null): CaptionStyle | null {
  try {
    const s = raw ? JSON.parse(raw) as CaptionStyle : null;
    return s && SIZES.includes(s.size) && Number.isFinite(s.x) && Number.isFinite(s.y) ? { size: s.size, x: clamp(s.x), y: clamp(s.y) } : null;
  } catch { return null; }
}

/**
 * Live captions drawn over the player instead of by the browser, so each viewer can resize them and
 * drag them anywhere on the video (arrow keys move them too). They start where the channel's
 * default puts them; the owner can save their own placement as that default. In native fullscreen
 * only the video is shown, so the browser draws the captions there.
 */
export function CaptionOverlay({ video, on, channel, isOwner }: { video: RefObject<HTMLVideoElement | null>; on: boolean; channel?: CaptionStyle; isOwner: boolean }) {
  const raw = useSyncExternalStore(cb => { listeners.add(cb); return () => { listeners.delete(cb); }; }, savedRaw, () => null);
  const mine = parse(raw);
  const style = mine ?? channel ?? { size: "m", x: 0.5, y: 0.85 };
  const [lines, setLines] = useState<string[]>([]);
  const [box, setBox] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const [note, setNote] = useState("");
  const dragging = useRef(false);

  // Keep the caption track decoding; draw it ourselves, or let the browser draw it in fullscreen.
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    const tracks = element.textTracks;
    const update = () => {
      const fullscreen = document.fullscreenElement === element;
      const text: string[] = [];
      for (const t of Array.from(tracks)) {
        if (t.kind !== "captions" && t.kind !== "subtitles") continue;
        t.mode = !on ? "disabled" : fullscreen ? "showing" : "hidden";
        if (on && !fullscreen) for (const cue of Array.from(t.activeCues ?? [])) if (cue instanceof VTTCue && cue.text.trim()) text.push(...cue.text.split("\n"));
      }
      setLines(text);
    };
    const attach = () => { for (const t of Array.from(tracks)) t.oncuechange = update; update(); };
    attach();
    tracks.addEventListener("addtrack", attach);
    document.addEventListener("fullscreenchange", update);
    return () => {
      tracks.removeEventListener("addtrack", attach);
      document.removeEventListener("fullscreenchange", update);
      for (const t of Array.from(tracks)) t.oncuechange = null;
    };
  }, [video, on]);

  // Sit exactly over the video element (the player also holds the bar under it).
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    const measure = () => setBox({ left: element.offsetLeft, top: element.offsetTop, width: element.offsetWidth, height: element.offsetHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [video]);

  if (!on || !box) return null;
  const move = (x: number, y: number) => setSaved({ ...style, x: clamp(x), y: clamp(y) });
  const fromPointer = (event: React.PointerEvent) => {
    const rect = video.current?.getBoundingClientRect();
    if (rect) move((event.clientX - rect.left) / rect.width, (event.clientY - rect.top) / rect.height);
  };
  const keys: Record<string, [number, number]> = { ArrowLeft: [-0.02, 0], ArrowRight: [0.02, 0], ArrowUp: [0, -0.02], ArrowDown: [0, 0.02] };
  async function saveDefault() {
    const result = await send<CaptionStyle>("PUT", "/api/me/stream/captions", style);
    setNote(result.ok ? "Saved as your channel's default." : result.error);
  }

  return <div className="caption-layer" style={box}>
    {lines.length > 0 && <div
      className={`caption-box caption-${style.size}`}
      style={{ left: `${style.x * 100}%`, top: `${style.y * 100}%` }}
      tabIndex={0}
      role="group"
      aria-label="Captions. Drag them, or use the arrow keys, to move them."
      onPointerDown={event => { dragging.current = true; event.currentTarget.setPointerCapture(event.pointerId); }}
      onPointerMove={event => { if (dragging.current) fromPointer(event); }}
      onPointerUp={() => { dragging.current = false; }}
      onPointerCancel={() => { dragging.current = false; }}
      onKeyDown={event => { const k = keys[event.key]; if (k) { event.preventDefault(); move(style.x + k[0], style.y + k[1]); } }}
    >
      <p aria-live="off">{lines.map((line, i) => <span key={i}>{line}</span>)}</p>
      <div className="caption-tools" onPointerDown={event => event.stopPropagation()}>
        {SIZES.map(size => <button key={size} type="button" className="small quiet" aria-pressed={style.size === size} onClick={() => setSaved({ ...style, size })}>{size.toUpperCase()}</button>)}
        {mine && <button type="button" className="small quiet" onClick={() => setSaved(null)}>Reset</button>}
        {isOwner && <button type="button" className="small quiet" onClick={() => void saveDefault()}>Save as channel default</button>}
      </div>
    </div>}
    {note && <p className="caption-note" role="status">{note}</p>}
  </div>;
}
