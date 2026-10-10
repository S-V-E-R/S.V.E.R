"use client";
import { useEffect, useState } from "react";

/** A GIF as chat stores it: KLIPY media URLs, loaded straight from KLIPY (docs/COMMUNITY.md "GIFs in chat"). */
export type Gif = { slug: string; still: string; play: string; width: number; height: number };
/** The channel's GIF setting from the chat snapshot; null when nobody can send GIFs here. */
export type GifAccess = { key: string; who: "everyone" | "followers" | "subscribers" } | null;
type Picked = Gif & { title: string };
type Size = { url: string; width: number; height: number };
type Item = { slug: string; title: string; type: string; file: { sm: { jpg: Size; webp: Size } } };

/** A still frame until hovered or tapped, so chat stays light and never autoplays. With reduced motion, only a tap plays it. */
export function GifImage({ gif, alt }: { gif: Gif; alt: string }) {
  const [playing, setPlaying] = useState(false);
  const calm = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  return <button type="button" className="chat-gif" aria-pressed={playing} aria-label={`${alt}, ${playing ? "playing" : "play"}`}
    onClick={() => setPlaying(p => !p)} onMouseEnter={() => { if (!calm()) setPlaying(true); }} onMouseLeave={() => { if (!calm()) setPlaying(false); }}>
    {/* eslint-disable-next-line @next/next/no-img-element -- KLIPY's terms require loading their URLs as given */}
    <img src={playing ? gif.play : gif.still} alt="" width={gif.width} height={gif.height} loading="lazy" />
  </button>;
}

/** KLIPY wants a stable per-user id; send a hash, never the username. */
async function customerId(account: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`sver:${account.toLowerCase()}`));
  return Array.from(new Uint8Array(digest).slice(0, 16), b => b.toString(16).padStart(2, "0")).join("");
}

/** Search panel. KLIPY's terms have the browser call their API and show results in their order. */
export function GifPicker({ access, account, onPick, onClose }: { access: NonNullable<GifAccess>; account: string; onPick: (gif: Picked) => void; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<Item[] | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let stale = false;
    const timer = setTimeout(async () => {
      try {
        const q = query.trim();
        const params = new URLSearchParams({ per_page: "24", content_filter: "high", format_filter: "jpg,webp", customer_id: await customerId(account) });
        if (q) params.set("q", q);
        const response = await fetch(`https://api.klipy.com/api/v1/${encodeURIComponent(access.key)}/gifs/${q ? "search" : "trending"}?${params}`);
        const json = await response.json() as { data?: { data?: Item[] } };
        if (!stale) { setItems((json.data?.data ?? []).filter(i => i.type === "gif" && i.file?.sm?.jpg && i.file.sm.webp)); setFailed(!response.ok); }
      } catch { if (!stale) setFailed(true); }
    }, 300);
    return () => { stale = true; clearTimeout(timer); };
  }, [query, access.key, account]);
  return <div className="gif-picker" role="dialog" aria-label="GIFs" onKeyDown={e => { if (e.key === "Escape") onClose(); }}>
    <div className="gif-picker-head">
      <label htmlFor="gif-search" className="sr-only">Search GIFs</label>
      <input id="gif-search" type="search" placeholder="Search KLIPY" autoFocus value={query} onChange={e => setQuery(e.target.value)} maxLength={100} />
      <button type="button" className="small quiet" onClick={onClose}>Close</button>
    </div>
    {access.who !== "everyone" && <p className="muted small">GIFs here are for {access.who}.</p>}
    {failed ? <p className="muted small">GIF search isn&apos;t answering. Try again shortly.</p>
      : items === null ? <p className="muted small">Loading…</p>
      : items.length === 0 ? <p className="muted small">No GIFs found.</p>
      : <ul className="gif-grid">{items.map(i => <li key={i.slug}><button type="button" title={i.title} onClick={() => onPick({ slug: i.slug, title: i.title, still: i.file.sm.jpg.url, play: i.file.sm.webp.url, width: i.file.sm.jpg.width, height: i.file.sm.jpg.height })}>
        {/* eslint-disable-next-line @next/next/no-img-element -- KLIPY's terms require loading their URLs as given */}
        <img src={i.file.sm.jpg.url} alt={i.title} loading="lazy" />
      </button></li>)}</ul>}
    <p className="muted small gif-credit">Powered by KLIPY</p>
  </div>;
}
