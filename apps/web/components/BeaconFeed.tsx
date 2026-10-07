"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { send } from "../lib/client-api";
import { browserId, compact, type BeaconFeedPage, type BeaconItem } from "../lib/beacons";
import type { Chip } from "../lib/types";
import { Avatar } from "./Avatar";
import { Crest } from "./Crest";
import { ReportForm, TakeDownLink } from "./Report";
import { Turnstile } from "./Turnstile";
import "../styles/beacons.css";

const SOUND = "sver-beacon-sound";
const SEED = "sver-beacon-seed";
const SOUND_EVENT = "sver-beacon-sound";
function soundOn() { try { return sessionStorage.getItem(SOUND) === "on"; } catch { return false; } }
function onSound(change: () => void) { window.addEventListener(SOUND_EVENT, change); return () => window.removeEventListener(SOUND_EVENT, change); }
const HD = "(min-width: 721px) and (min-resolution: 1.5dppx), (min-width: 1100px)";
function onScreen(change: () => void) { const query = window.matchMedia(HD); query.addEventListener("change", change); return () => query.removeEventListener("change", change); }
function session(key: string, make: () => string) {
  try {
    const value = sessionStorage.getItem(key) || make();
    sessionStorage.setItem(key, value);
    return value;
  } catch { return make(); }
}

/**
 * The Beacons feed (docs/BEACONS.md "The feed"): one 9:16 video at a time with a right-side rail.
 * Swipe, scroll or use the arrow keys to move. Videos play muted until tapped, then stay unmuted
 * for the rest of the session. The order comes from the server and never reads counts or money.
 */
export function BeaconFeed({ initial, start = null, signedIn, gateId = null }: { initial: BeaconFeedPage | null; start?: BeaconItem | null; signedIn: boolean; gateId?: string | null }) {
  const [items, setItems] = useState<BeaconItem[]>(() => {
    const rest = (initial?.items ?? []).filter(i => i.beacon.id !== start?.beacon.id);
    return start ? [start, ...rest] : rest;
  });
  const [live, setLive] = useState<Chip[]>(initial?.live ?? []);
  const [index, setIndex] = useState(0);
  const [more, setMore] = useState(initial?.has_more ?? true);
  const [next, setNext] = useState(initial?.next ?? 0);
  const [ageAck, setAgeAck] = useState(false);
  // Sound stays on for the rest of the session once the viewer taps (server render: muted).
  const sound = useSyncExternalStore(onSound, soundOn, () => false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(initial ? "" : "Beacons couldn't be loaded. Please refresh.");
  const seed = useRef("");
  const fetching = useRef(false);

  useEffect(() => {
    seed.current = session(SEED, () => Math.random().toString(36).slice(2, 12));
  }, []);

  const loadMore = useCallback(async (reset = false) => {
    if (fetching.current) return;
    fetching.current = true;
    setLoading(true);
    const offset = reset ? 0 : next;
    const result = await send<BeaconFeedPage>("GET", `/api/beacons/feed?seed=${seed.current || "0"}&offset=${offset}&age_ack=${ageAck}`);
    fetching.current = false;
    setLoading(false);
    if (!result.ok) { setError(result.error); return; }
    setError("");
    setMore(result.data.has_more);
    setNext(result.data.next);
    if (offset === 0) setLive(result.data.live);
    setItems(current => {
      const kept = reset ? current.slice(0, 1).filter(i => i.beacon.id === start?.beacon.id) : current;
      const seen = new Set(kept.map(i => i.beacon.id));
      return [...kept, ...result.data.items.filter(i => !seen.has(i.beacon.id))];
    });
  }, [next, ageAck, start]);

  // A feed opened on one Beacon continues into the viewer's rotation.
  const started = useRef(false);
  useEffect(() => { if (!initial && !started.current) { started.current = true; void loadMore(true); } }, [initial, loadMore]);
  // Passing the 18+ gate opens the linked Beacon and refreshes the feed with 18+ Beacons included.
  const confirmAge = useCallback(async () => {
    setAgeAck(true);
    if (!gateId) return;
    const result = await send<BeaconItem>("GET", `/api/beacons/${gateId}?age_ack=true`);
    if (result.ok) { setItems(current => [result.data, ...current.filter(i => i.beacon.id !== gateId)]); setIndex(0); }
    else setError(result.error);
  }, [gateId]);
  useEffect(() => { if (more && items.length - index <= 3) void loadMore(); }, [index, items.length, more, loadMore]);

  const move = useCallback((step: number) => setIndex(i => Math.max(0, Math.min(items.length - 1, i + step))), [items.length]);
  useEffect(() => {
    function key(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
      if (event.key === "ArrowDown" || event.key === "j") { event.preventDefault(); move(1); }
      if (event.key === "ArrowUp" || event.key === "k") { event.preventDefault(); move(-1); }
    }
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [move]);
  const touch = useRef<number | null>(null);
  const wheel = useRef(0);

  const current = items[index];
  // Keep the address shareable as the viewer moves.
  useEffect(() => {
    if (current && window.location.pathname !== `/beacons/${current.beacon.id}`) window.history.replaceState(null, "", `/beacons/${current.beacon.id}`);
  }, [current]);

  const [soundHere, setSoundHere] = useState(false);
  function unmute() {
    setSoundHere(true);
    try { sessionStorage.setItem(SOUND, "on"); } catch { /* storage blocked: this page only */ }
    window.dispatchEvent(new Event(SOUND_EVENT));
  }

  return <div className="beacon-feed">
    {live.length > 0 && <nav className="beacon-live-row" aria-label="Live now">
      <span className="eyebrow">Live now</span>
      <ul>{live.map(c => c.username && <li key={c.username}><Link href={`/${c.username}/live`}><Avatar sizes={c.avatar} name={c.display_name} size={44} /><span className="beacon-live-name">{c.display_name}</span></Link></li>)}</ul>
    </nav>}
    <div className="beacon-stage"
      onTouchStart={e => { touch.current = e.touches[0]?.clientY ?? null; }}
      onTouchEnd={e => { const from = touch.current; touch.current = null; const to = e.changedTouches[0]?.clientY; if (from === null || to === undefined) return; if (from - to > 50) move(1); else if (to - from > 50) move(-1); }}
      onWheel={e => { const now = Date.now(); if (Math.abs(e.deltaY) < 30 || now - wheel.current < 600) return; wheel.current = now; move(e.deltaY > 0 ? 1 : -1); }}>
      {current
        && !(gateId && !ageAck)
        ? <BeaconPlayer key={current.beacon.id} item={current} sound={sound || soundHere} onUnmute={unmute} ageAck={ageAck} signedIn={signedIn} />
        : gateId && !ageAck
          ? <div className="beacon-empty panel"><p>This Beacon is for viewers aged 18 and over.</p><button type="button" onClick={() => void confirmAge()}>I am 18 or older</button> <Link href="/beacons" className="button quiet">Back to Beacons</Link></div>
          : <div className="beacon-empty panel">
          {error ? <p role="alert">{error}</p> : loading ? <p role="status">Loading Beacons…</p> : <>
            <h2>No Beacons yet</h2>
            <p>Creators post short videos here that lead to their streams. Until then, see who&apos;s live.</p>
            <p><Link className="button" href="/browse">Browse live channels</Link></p>
          </>}
        </div>}
      <div className="beacon-nav">
        <button type="button" className="quiet" onClick={() => move(-1)} disabled={index === 0} aria-label="Previous Beacon">↑</button>
        <button type="button" className="quiet" onClick={() => move(1)} disabled={index >= items.length - 1} aria-label="Next Beacon">↓</button>
      </div>
    </div>
  </div>;
}

function BeaconPlayer({ item, sound, onUnmute, ageAck, signedIn }: { item: BeaconItem; sound: boolean; onUnmute: () => void; ageAck: boolean; signedIn: boolean }) {
  const router = useRouter();
  const video = useRef<HTMLVideoElement>(null);
  const [liked, setLiked] = useState(item.liked);
  const [likes, setLikes] = useState(item.beacon.likes);
  const [paused, setPaused] = useState(false);
  const [panel, setPanel] = useState<"report" | "more" | null>(null);
  const [message, setMessage] = useState("");
  const [followed, setFollowed] = useState(false);
  const [sitekey, setSitekey] = useState<string | null>(null);
  const token = useRef("");
  const onToken = useCallback((value: string) => { token.current = value; }, []);
  const b = item.beacon, c = item.channel;
  // 1080×1920 on large or dense screens, 720×1280 otherwise.
  const hd = useSyncExternalStore(onScreen, () => window.matchMedia(HD).matches, () => false);

  // View heartbeat: only visible, advancing playback counts (docs/BEACONS.md "Counts").
  useEffect(() => {
    const element = video.current;
    if (!element || !item.playback || item.is_owner) return;
    const browser = browserId();
    let last = -1, sending = false, stopped = false;
    async function beat(force = false) {
      if (sending || stopped || element!.readyState < 2 || (!force && (element!.paused || element!.currentTime === last))) return;
      sending = true;
      last = element!.currentTime;
      const check = token.current; token.current = "";
      const result = await send<{ needs_turnstile?: boolean }>("POST", `/api/beacons/${b.id}/beat`, { browser_id: browser, media_time: element!.currentTime, visible: !document.hidden && !element!.paused, age_ack: ageAck, turnstile: check || undefined });
      sending = false;
      if (stopped || !result.ok) return;
      if (result.data.needs_turnstile) { const config = await send<{ turnstile_site_key: string }>("GET", "/api/auth/config"); if (config.ok) setSitekey(config.data.turnstile_site_key); }
      else setSitekey(null);
    }
    const now = () => { void beat(true); };
    element.addEventListener("playing", now);
    const timer = setInterval(() => void beat(), 2000);
    return () => { stopped = true; clearInterval(timer); element.removeEventListener("playing", now); };
  }, [b.id, item.playback, item.is_owner, ageAck]);

  useEffect(() => { if (video.current) video.current.muted = !sound; }, [sound]);

  function tap() {
    const element = video.current;
    if (!element) return;
    if (!sound) { onUnmute(); element.muted = false; void element.play().catch(() => {}); return; }
    if (element.paused) void element.play().catch(() => {}); else element.pause();
  }
  async function like() {
    if (!signedIn) { router.push(`/login?next=${encodeURIComponent(`/beacons/${b.id}`)}`); return; }
    const result = await send<{ liked: boolean; likes: number }>(liked ? "DELETE" : "PUT", `/api/beacons/${b.id}/like`);
    if (result.ok) { setLiked(result.data.liked); setLikes(result.data.likes); } else setMessage(result.error);
  }
  async function goLive() {
    const result = await send<{ url: string }>("POST", `/api/beacons/${b.id}/live`, { browser_id: browserId(), age_ack: ageAck });
    router.push(result.ok ? result.data.url : `/${c.username}/live`);
  }
  async function follow() {
    if (!signedIn) { router.push(`/login?next=${encodeURIComponent(`/beacons/${b.id}`)}`); return; }
    const result = await send("PUT", `/api/follows/${c.username}`);
    if (result.ok) { setFollowed(true); setMessage(`Following ${c.display_name}.`); await send("POST", `/api/beacons/${b.id}/followed`); }
    else setMessage(result.error);
  }
  async function share() {
    const url = `${window.location.origin}/beacons/${b.id}`;
    try {
      if (navigator.share) await navigator.share({ title: b.title, url });
      else { await navigator.clipboard.writeText(url); setMessage("Link copied."); }
    } catch { /* the share sheet was dismissed */ }
  }
  async function mute() {
    const result = await send("PUT", `/api/beacons/mutes/${c.username}`);
    setMessage(result.ok ? `You won't see ${c.display_name}'s Beacons in your feed.` : result.error);
    setPanel(null);
  }

  return <article className="beacon" aria-roledescription="Beacon" aria-label={`${b.title} by ${c.display_name}`}>
    <div className="beacon-video">
      {item.playback
        ? <video ref={video} src={hd ? item.playback.hd : item.playback.sd} poster={item.thumbnail ?? undefined} autoPlay muted={!sound} loop playsInline preload="auto" controlsList="nodownload nofullscreen" disablePictureInPicture
            onPlay={() => setPaused(false)} onPause={() => setPaused(true)} onClick={tap} aria-label={sound ? "Pause or play" : "Unmute"} />
        : <div className="beacon-gate"><p role="status">{b.status === "PROCESSING" ? "This Beacon is still processing." : "This Beacon isn't available."}</p></div>}
      {item.playback && !sound && <button type="button" className="beacon-unmute small" onClick={tap}>Tap for sound</button>}
      {item.playback && sound && paused && <span className="beacon-paused" aria-hidden="true">Paused</span>}
      <div className="beacon-caption">
        <h2>{b.title}</h2>
        <p>{c.username ? <Link href={`/${c.username}`}>@{c.username}</Link> : c.display_name}{b.category && <> · {b.category}</>}</p>
        {item.clipper?.username && <p className="muted">Clipped by <Link href={`/${item.clipper.username}`}>{item.clipper.display_name}</Link></p>}
        {item.charity && <p className="beacon-shine"><span className="eyebrow">Shine</span> {item.charity.name}{item.charity.url && <> · <a href={item.charity.url} target="_blank" rel="noopener noreferrer">Donate</a></>}</p>}
        {b.clip_id && <p><Link href={`/clips/${b.clip_id}`} className="muted">From a clip</Link></p>}
      </div>
    </div>
    <aside className="beacon-rail" aria-label="Beacon actions">
      {c.username && <Link href={`/${c.username}`} className="beacon-creator" aria-label={`${c.display_name}'s channel`}>
        <Avatar sizes={c.avatar} name={c.display_name} size={48} />
        {c.faction && <span className="beacon-crest"><Crest faction={c.faction} initial="" size={22} label={`${c.faction} faction`} /></span>}
      </Link>}
      {!item.is_owner && !followed && c.username && <button type="button" className="beacon-action" onClick={() => void follow()} aria-label={`Follow ${c.display_name}`}><span aria-hidden="true">＋</span><span>Follow</span></button>}
      {c.live && c.username && <button type="button" className="beacon-action beacon-live" onClick={() => void goLive()}><span className="tag-live">Live</span><span>Live now</span></button>}
      <button type="button" className="beacon-action" onClick={() => void like()} aria-pressed={liked} aria-label={liked ? "Unlike" : "Like"}><span aria-hidden="true">{liked ? "♥" : "♡"}</span><span>{compact(likes)}</span></button>
      <span className="beacon-action beacon-stat" aria-label={`${b.views} views`}><span aria-hidden="true">▶</span><span>{compact(b.views)}</span></span>
      <button type="button" className="beacon-action" onClick={() => void share()}><span aria-hidden="true">↗</span><span>Share</span></button>
      <button type="button" className="beacon-action" onClick={() => setPanel(panel === "more" ? null : "more")} aria-expanded={panel === "more"} aria-label="More actions"><span aria-hidden="true">⋯</span><span>More</span></button>
    </aside>
    {panel === "more" && <div className="beacon-panel panel" role="dialog" aria-label="More actions">
      {signedIn && !item.is_owner && <button type="button" className="link-button" onClick={() => setPanel("report")}>Report this Beacon</button>}
      {signedIn && !item.is_owner && <button type="button" className="link-button" onClick={() => void mute()}>Mute {c.display_name}</button>}
      {!signedIn && <p><Link href="/login">Sign in</Link> to report or mute.</p>}
      {!item.is_owner && <TakeDownLink target={{ target_type: "beacon", target_id: b.id }} />}
      {item.is_owner && <Link href="/studio/beacons">Manage in Creator Studio</Link>}
      <button type="button" className="small quiet" onClick={() => setPanel(null)}>Close</button>
    </div>}
    {panel === "report" && <div className="beacon-panel panel" role="dialog" aria-label="Report this Beacon"><ReportForm target={{ target_type: "beacon", target_id: b.id }} onDone={() => setPanel(null)} /></div>}
    {message && <p className="beacon-message" role="status">{message}</p>}
    {sitekey && <Turnstile sitekey={sitekey} action="playback" onToken={onToken} />}
  </article>;
}
