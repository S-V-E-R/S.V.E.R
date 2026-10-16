"use client";
import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { browserId } from "../lib/beacons";
import "../styles/ads.css";

/** An ad handed over by /api/ads/serve, with its single-use impression ticket (docs/ADVERTISERS.md). */
export type ServedAd = { ticket: string; kind: "video" | "image"; src: string; poster: string; title: string; advertiser: string | null; click: string; min_ms: number; duration_ms: number | null };

export async function fetchAd(placement: "break" | "feed" | "site", channel?: string): Promise<ServedAd | null> {
  const r = await send<{ ad: ServedAd | null }>("POST", "/api/ads/serve", { placement, channel, browser_id: browserId() });
  return r.ok ? r.data.ad : null;
}

/** Reports the impression once it really happened: at least half the ad on screen, the page
 * visible and, for video, playback advancing, for the ad's minimum time. The server checks again. */
function useImpression(ad: ServedAd, box: RefObject<HTMLElement | null>, media: RefObject<HTMLVideoElement | null>) {
  useEffect(() => {
    const element = box.current;
    if (!element) return;
    let inView = false;
    let seen = 0;
    let last = performance.now();
    let lastTime = -1;
    let sent = false;
    const observer = new IntersectionObserver(([entry]) => { inView = (entry?.intersectionRatio ?? 0) >= 0.5; }, { threshold: [0, 0.5, 1] });
    observer.observe(element);
    const timer = setInterval(() => {
      const now = performance.now();
      const step = now - last;
      last = now;
      const video = media.current;
      const advancing = ad.kind === "image" || (!!video && !video.paused && video.currentTime !== lastTime);
      if (video) lastTime = video.currentTime;
      if (inView && !document.hidden && advancing) seen += step;
      if (!sent && seen >= ad.min_ms + 150) {
        sent = true;
        void send("POST", "/api/ads/impressions", { ticket: ad.ticket, browser_id: browserId(), visible_ms: Math.round(seen), visible: true });
      }
    }, 250);
    return () => { clearInterval(timer); observer.disconnect(); };
  }, [ad, box, media]);
}

/** One ad, always labelled with its advertiser. */
export function AdFrame({ ad, className = "", muted = true, onEnd }: { ad: ServedAd; className?: string; muted?: boolean; onEnd?: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  const media = useRef<HTMLVideoElement>(null);
  useImpression(ad, box, media);
  return <aside ref={box} className={`ad ${className}`} aria-label={`Ad from ${ad.advertiser ?? "an advertiser"}`}>
    <span className="ad-label">Ad{ad.advertiser ? ` · ${ad.advertiser}` : ""}</span>
    {ad.kind === "video"
      ? <video ref={media} src={ad.src} poster={ad.poster} autoPlay playsInline muted={muted} onEnded={onEnd} aria-label={ad.title} />
      : <img src={ad.src} alt={ad.title} />}
    <a className="button small ad-link" href={ad.click} target="_blank" rel="sponsored noopener">{ad.title}</a>
  </aside>;
}

/** A streamer's ad break over the player: the stream keeps running behind it (muted meanwhile). */
export function AdBreak({ username, endsAt, muted, onActive }: { username: string; endsAt: string; muted: boolean; onActive: (on: boolean) => void }) {
  const [ad, setAd] = useState<ServedAd | null>(null);
  const [left, setLeft] = useState(() => Math.max(0, Math.round((Date.parse(endsAt) - Date.now()) / 1000)));
  const [done, setDone] = useState(false);
  useEffect(() => {
    let stopped = false;
    void fetchAd("break", username).then(served => {
      if (stopped) return;
      setAd(served);
      onActive(!!served);
    });
    return () => { stopped = true; onActive(false); };
  }, [username, endsAt, onActive]);
  useEffect(() => {
    const timer = setInterval(() => {
      const seconds = Math.max(0, Math.round((Date.parse(endsAt) - Date.now()) / 1000));
      setLeft(seconds);
      if (seconds === 0) { setDone(true); onActive(false); }
    }, 1000);
    return () => clearInterval(timer);
  }, [endsAt, onActive]);
  if (!ad || done) return null;
  return <div className="ad-break">
    <AdFrame ad={ad} muted={muted} onEnd={() => { setDone(true); onActive(false); }} />
    <span className="ad-countdown" role="status">Back to the stream in {left} s</span>
  </div>;
}

/** The labelled site slot on Home and Browse (signed-in viewers only). */
export function AdSlot({ signedIn }: { signedIn: boolean }) {
  const [ad, setAd] = useState<ServedAd | null>(null);
  useEffect(() => {
    if (!signedIn) return;
    let stopped = false;
    void fetchAd("site").then(served => { if (!stopped) setAd(served); });
    return () => { stopped = true; };
  }, [signedIn]);
  return ad ? <AdFrame ad={ad} className="ad-site" /> : null;
}

type StudioAdsView = { enabled: boolean; can_earn: boolean; live: boolean; running: { id: string; ends_at: string } | null; next_break_at: string | null; lengths: number[]; breaks: { id: string; starts_at: string; impressions: number }[]; earnings: Earned };
type Earned = { total_tenths: number; days: { day: string; tenths: number; impressions: number }[] };
const money = (tenths: number) => `$${(tenths / 1000).toFixed(3)}`;
/** Creator Studio: the ads switch and Run ad break. Never automatic, never a pre-roll. */
export function StudioAds() {
  const [view, setView] = useState<StudioAdsView | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => { const r = await send<StudioAdsView>("GET", "/api/me/ads"); if (r.ok) setView(r.data); }, []);
  useLoad(load);
  async function toggle(enabled: boolean) {
    const r = await send("PUT", "/api/me/ads", { enabled });
    setMessage(r.ok ? (enabled ? "Ads are on. Start a break whenever it suits your stream." : "Ads are off.") : r.error);
    void load();
  }
  async function start(seconds: number) {
    const r = await send("POST", "/api/me/ads/breaks", { seconds });
    setMessage(r.ok ? `A ${seconds}-second ad break is running.` : r.error);
    void load();
  }
  if (!view) return null;
  return <section className="panel section" aria-labelledby="ads-h">
    <h2 id="ads-h">Ads</h2>
    <p className="muted small">Ads are off until you turn them on, and only play during breaks you start: never before your stream. Subscribers never see them. You earn your tier&apos;s share of each verified impression.</p>
    {!view.can_earn ? <p className="small">Set up payouts first: ad earnings go to your payout balance.</p>
      : <label className="check"><input type="checkbox" checked={view.enabled} onChange={e => void toggle(e.target.checked)} /> Run ads on my channel</label>}
    {view.enabled && view.can_earn && <div className="row wrap">
      {view.lengths.map(s => <button key={s} type="button" className="small" disabled={!view.live || !!view.running || !!view.next_break_at} onClick={() => void start(s)}>Run {s}-second break</button>)}
    </div>}
    {view.enabled && !view.live && <p className="muted small">Go live to start a break.</p>}
    {view.next_break_at && !view.running && <p className="muted small">Next break available at {new Date(view.next_break_at).toLocaleTimeString()}.</p>}
    {view.earnings.total_tenths > 0 && <p className="small">Ad earnings: <strong>{money(view.earnings.total_tenths)}</strong> in all{view.earnings.days.length > 0 ? `, ${money(view.earnings.days[0]!.tenths)} on ${view.earnings.days[0]!.day}` : ""}. They join your payout balance.</p>}
    {view.breaks.length > 0 && <ul className="list small">{view.breaks.map(b => <li key={b.id}>{new Date(b.starts_at).toLocaleString()} · {b.impressions} verified impressions</li>)}</ul>}
    {message && <p role="status" className="form-message">{message}</p>}
  </section>;
}

/** Wallet: the viewer's Ad Valor, their 5% of the ads they watched (docs/ADS.md "Ad Valor"). */
export function AdValorWallet() {
  const [view, setView] = useState<{ balance_tenths: number; earned: Earned } | null>(null);
  const load = useCallback(async () => { const r = await send<{ balance_tenths: number; earned: Earned }>("GET", "/api/me/ad-valor"); if (r.ok) setView(r.data); }, []);
  useLoad(load);
  if (!view || (view.balance_tenths === 0 && view.earned.days.length === 0)) return null;
  return <section className="panel section" aria-labelledby="adv-h">
    <h2 id="adv-h">Ad Valor</h2>
    <p>You have <strong>{money(view.balance_tenths)}</strong> from the ads you watched. It will pay for subscriptions, and cash out once you&apos;re 18.</p>
    {view.earned.days.length > 0 && <ul className="list small">{view.earned.days.map(d => <li key={d.day}>{d.day} · {d.impressions} ads · {money(d.tenths)}</li>)}</ul>}
  </section>;
}
