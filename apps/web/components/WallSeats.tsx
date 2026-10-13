"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { FormEvent, useEffect, useMemo, useState } from "react";
import { send } from "../lib/client-api";
import { dollars, place, sizeLabel, statusLabel, type MyTile, type WallData, type WallSize, type WallTile } from "../lib/wall";
import { Status, type SaveState } from "./Form";

/** Zoomed sections are 10 columns by 10 rows, entered from 20 bands of 10 × 5 on the overview. */
const ZOOM = 10;
const BAND_ROWS = 5;

/** Which tile covers each cell. */
function occupancy(tiles: WallTile[]) {
  const cells = new Map<string, WallTile>();
  for (const t of tiles) for (let y = t.y; y < t.y + t.side; y++) for (let x = t.x; x < t.x + t.side; x++) cells.set(`${x},${y}`, t);
  return cells;
}
const tileLabel = (t: WallTile) => t.state === "live" ? `${t.name}, ${sizeLabel[t.size].toLowerCase()} at ${place(t)}` : t.state === "claiming" ? `Being claimed, ${place(t)}` : `Taken, ${place(t)}`;

function TileFace({ tile, px }: { tile: WallTile; px: number }) {
  if (tile.state !== "live" || !tile.image) return <span className={`wall-tile-blank ${tile.state}`} aria-hidden="true" />;
  // eslint-disable-next-line @next/next/no-img-element -- media CDN images at their stored sizes
  return <img src={tile.image} alt="" width={px} height={px} loading="lazy" decoding="async" />;
}

/** The full grid. Taken tiles are links; the 20 bands behind them open a zoomed section. */
function Overview({ data, onZoom }: { data: WallData; onZoom: (x: number, y: number) => void }) {
  const bands = [];
  for (let y = 0; y < data.rows; y += BAND_ROWS) for (let x = 0; x < data.columns; x += ZOOM) bands.push({ x, y });
  return <div className="wall-overview" style={{ aspectRatio: `${data.columns} / ${data.rows}` }}>
    {bands.map(b => <button key={`${b.x},${b.y}`} type="button" className="wall-band" style={{ left: `${b.x / data.columns * 100}%`, top: `${b.y / data.rows * 100}%`, width: `${ZOOM / data.columns * 100}%`, height: `${BAND_ROWS / data.rows * 100}%` }}
      onClick={() => onZoom(b.x, Math.min(b.y, data.rows - ZOOM))} aria-label={`Zoom into columns ${b.x + 1} to ${b.x + ZOOM}, rows ${b.y + 1} to ${b.y + BAND_ROWS}`} />)}
    {data.tiles.map(t => {
      const style = { left: `${t.x / data.columns * 100}%`, top: `${t.y / data.rows * 100}%`, width: `${t.side / data.columns * 100}%`, height: `${t.side / data.rows * 100}%` };
      return t.state === "live"
        ? <Link key={t.n} href={`/wall/${t.n}`} className="wall-tile" style={style} aria-label={tileLabel(t)} title={t.message ? `${t.name}: ${t.message}` : t.name}><TileFace tile={t} px={25 * t.side} /></Link>
        : <span key={t.n} className="wall-tile" style={style} title={tileLabel(t)}><TileFace tile={t} px={25 * t.side} /></span>;
    })}
  </div>;
}

/** A 10 × 10 section with cells of at least 44 px: pick a size, then a free spot. */
function Zoom({ data, origin, size, onPan, onPick, picking }: {
  data: WallData; origin: { x: number; y: number }; size: WallSize; picking: boolean;
  onPan: (dx: number, dy: number) => void; onPick: (x: number, y: number) => void;
}) {
  const taken = useMemo(() => occupancy(data.tiles), [data.tiles]);
  const side = data.prices.find(p => p.size === size)?.side ?? 1;
  const fits = (x: number, y: number) => {
    if (x + side > data.columns || y + side > data.rows) return false;
    for (let cy = y; cy < y + side; cy++) for (let cx = x; cx < x + side; cx++) if (taken.has(`${cx},${cy}`)) return false;
    return true;
  };
  const cells = [];
  for (let y = origin.y; y < origin.y + ZOOM; y++) for (let x = origin.x; x < origin.x + ZOOM; x++) {
    const tile = taken.get(`${x},${y}`);
    if (tile) {
      // A tile's top-left cell carries its picture and its link; the rest are covered.
      const corner = tile.x === x && tile.y === y || (x === origin.x && tile.x < origin.x && (tile.y === y || y === origin.y)) || (y === origin.y && tile.y < origin.y && tile.x === x);
      if (!corner) continue;
      const left = Math.max(tile.x, origin.x), top = Math.max(tile.y, origin.y);
      const w = Math.min(tile.x + tile.side, origin.x + ZOOM) - left, h = Math.min(tile.y + tile.side, origin.y + ZOOM) - top;
      if (left !== x || top !== y) continue;
      const style = { gridColumn: `${left - origin.x + 1} / span ${w}`, gridRow: `${top - origin.y + 1} / span ${h}` };
      cells.push(tile.state === "live"
        ? <Link key={`t${tile.n}`} href={`/wall/${tile.n}`} className="wall-zoom-tile" style={style} aria-label={tileLabel(tile)} title={tile.message ?? undefined}><TileFace tile={tile} px={200} /></Link>
        : <span key={`t${tile.n}`} className="wall-zoom-tile" style={style}><TileFace tile={tile} px={200} /><span className="wall-zoom-state">{tile.state === "claiming" ? "Being claimed" : tile.state === "review" ? "Taken · in review" : "Taken"}</span></span>);
      continue;
    }
    const free = fits(x, y);
    cells.push(<button key={`${x},${y}`} type="button" className="wall-cell" style={{ gridColumn: x - origin.x + 1, gridRow: y - origin.y + 1 }}
      disabled={!picking || !free} onClick={() => onPick(x, y)}
      aria-label={picking ? (free ? `Choose ${sizeLabel[size].toLowerCase()} at ${place({ x, y })}` : `No room for a ${size} at ${place({ x, y })}`) : `Open, ${place({ x, y })}`} />);
  }
  return <section className="wall-zoom" aria-label={`Columns ${origin.x + 1} to ${origin.x + ZOOM}, rows ${origin.y + 1} to ${origin.y + ZOOM}`}>
    <div className="wall-zoom-head row between"><h2>Columns {origin.x + 1}–{origin.x + ZOOM} · rows {origin.y + 1}–{origin.y + ZOOM}</h2>
      <div className="row wall-pan"><button type="button" className="small quiet" disabled={origin.x === 0} onClick={() => onPan(-5, 0)} aria-label="Move left">←</button><button type="button" className="small quiet" disabled={origin.y === 0} onClick={() => onPan(0, -5)} aria-label="Move up">↑</button><button type="button" className="small quiet" disabled={origin.y + ZOOM >= data.rows} onClick={() => onPan(0, 5)} aria-label="Move down">↓</button><button type="button" className="small quiet" disabled={origin.x + ZOOM >= data.columns} onClick={() => onPan(5, 0)} aria-label="Move right">→</button></div></div>
    <div className="wall-zoom-scroll"><div className="wall-zoom-grid">{cells}</div></div>
  </section>;
}

function Countdown({ until, onEnd }: { until: string; onEnd: () => void }) {
  const [now, setNow] = useState<number | null>(null);
  useEffect(() => { const tick = () => setNow(Date.now()); tick(); const timer = setInterval(tick, 1000); return () => clearInterval(timer); }, []);
  const left = now === null ? null : Math.max(0, Math.floor((Date.parse(until) - now) / 1000));
  useEffect(() => { if (left === 0) onEnd(); }, [left, onEnd]);
  if (left === null) return null;
  return <span className="badge">Held for {Math.floor(left / 60)}:{String(left % 60).padStart(2, "0")}</span>;
}

/** Fill in a held tile and pay, or change a paid one (docs/WALL.md "Fill it in"). */
export function TileForm({ tile, mode, onDone }: { tile: MyTile; mode: "checkout" | "edit"; onDone?: () => void }) {
  const router = useRouter();
  const [state, setState] = useState<SaveState>({});
  const [busy, setBusy] = useState(false);
  const draft = mode === "edit" && tile.pending ? tile.pending : tile;
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    const r = await send<{ url?: string }>(mode === "checkout" ? "POST" : "PUT", `/api/wall/${tile.n}${mode === "checkout" ? "/checkout" : ""}`, new FormData(event.currentTarget));
    setBusy(false);
    if (!r.ok) { setState(r); return; }
    if (r.data.url) { window.location.assign(r.data.url); return; }
    setState({ saved: tile.status === "live" ? "Sent for review. Your tile stays as it is until the change is approved." : "Sent for review." });
    onDone?.();
    router.refresh();
  }
  return <form className="wall-form" onSubmit={submit}>
    <label className="field"><span>Picture (square, at least {tile.min_picture} px; PNG, JPEG or WebP){mode === "edit" && " · leave empty to keep the current one"}</span><input type="file" name="file" accept="image/png,image/jpeg,image/webp" required={mode === "checkout"} aria-invalid={state.field === "file" || undefined} /></label>
    <label className="field"><span>Name</span><input name="name" required maxLength={40} defaultValue={draft.name ?? ""} aria-invalid={state.field === "name" || undefined} /></label>
    <label className="field"><span>Link (optional, https only)</span><input name="link" type="url" inputMode="url" pattern="https://.*" maxLength={2048} defaultValue={draft.link ?? ""} placeholder="https://" aria-invalid={state.field === "link" || undefined} /></label>
    <label className="field"><span>Message (optional, shown on hover or tap)</span><input name="message" maxLength={140} defaultValue={draft.message ?? ""} aria-invalid={state.field === "message" || undefined} /></label>
    <label className="check"><input type="checkbox" name="crest" defaultChecked={!!tile.crest} /> Show my faction&apos;s crest in the corner (decoration only)</label>
    <div className="row wrap"><button disabled={busy}>{mode === "checkout" ? `Pay ${dollars(tile.price_cents)}` : tile.status === "live" ? "Send change for review" : "Resubmit for review"}</button><Status state={state} /></div>
    {mode === "checkout" && <p className="muted small">The price is set when you start paying. Your tile shows as &ldquo;taken · in review&rdquo; until staff approve it, and you can ask for a full refund any time before then.</p>}
  </form>;
}

/** The held spot: countdown, form and release. */
function Claim({ tile, price, onEnd }: { tile: MyTile; price: number | null; onEnd: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function release() {
    const r = await send("DELETE", `/api/wall/${tile.n}/hold`);
    if (r.ok) onEnd(); else setState(r);
  }
  return <section className="panel section wall-claim" aria-labelledby="claim-title">
    <div className="row between wrap"><h2 id="claim-title">Your {sizeLabel[tile.size].toLowerCase()} at {place(tile)}</h2><Countdown until={tile.hold_until} onEnd={onEnd} /></div>
    <p className="muted">Fill it in and pay within {15} minutes. Everyone else sees this spot as being claimed meanwhile.</p>
    <TileForm tile={{ ...tile, price_cents: price }} mode="checkout" />
    <button type="button" className="quiet small" onClick={release}>Give the spot back</button><Status state={state} />
  </section>;
}

export default function Wall({ initial }: { initial: WallData }) {
  const [data, setData] = useState(initial);
  const [origin, setOrigin] = useState<{ x: number; y: number } | null>(null);
  const [size, setSize] = useState<WallSize>("seat");
  const [state, setState] = useState<SaveState>({});
  const held = data.mine.find(t => t.hold_active) ?? null;
  async function reload() {
    const r = await send<WallData>("GET", "/api/wall");
    if (r.ok) setData(r.data);
  }
  async function pick(x: number, y: number) {
    setState({});
    const r = await send<{ tile: MyTile }>("POST", "/api/wall/holds", { x, y, size });
    if (!r.ok) { setState(r); await reload(); return; }
    await reload();
  }
  const zoomTo = (x: number, y: number) => setOrigin({ x: Math.max(0, Math.min(data.columns - ZOOM, x)), y: Math.max(0, Math.min(data.rows - ZOOM, y)) });
  const live = data.tiles.filter(t => t.state === "live");
  const price = held ? data.prices.find(p => p.size === held.size)?.cents ?? null : null;
  const buying = data.sales_open && data.can_buy && !data.full;
  return <>
    <Overview data={data} onZoom={zoomTo} />
    <p className="muted small wall-hint">Choose part of the Wall to zoom in. {data.signed_in ? "" : "Sign in to claim a seat."}</p>
    {origin && <>
      {buying && !held && <fieldset className="wall-sizes"><legend>Size</legend>{data.prices.map(p => {
        const soldOut = p.limit != null && p.taken != null && p.taken >= p.limit;
        return <label key={p.size} className="check"><input type="radio" name="size" value={p.size} checked={size === p.size} disabled={soldOut} onChange={() => setSize(p.size)} /> {sizeLabel[p.size]} ({p.side}×{p.side}) · {dollars(p.cents)}{soldOut ? " · all taken" : ""}</label>;
      })}</fieldset>}
      <Zoom data={data} origin={origin} size={size} picking={buying && !held} onPick={pick} onPan={(dx, dy) => zoomTo(origin.x + dx, origin.y + dy)} />
      <Status state={state} />
    </>}
    {held && <Claim tile={held} price={price} onEnd={reload} />}
    {data.mine.filter(t => t.status !== "held" && t.status !== "expired").length > 0 && <section className="panel section" aria-labelledby="mine-title"><h2 id="mine-title">Your tiles</h2>
      <ul className="list">{data.mine.filter(t => t.status !== "held" && t.status !== "expired").map(t => <li key={t.n} className="row between wrap"><span><Link href={`/wall/${t.n}`}>{t.name ?? `Tile ${t.n}`}</Link> · {sizeLabel[t.size]} at {place(t)}</span><span className="badge">{statusLabel[t.status]}</span></li>)}</ul></section>}
    <section className="wall-list" aria-labelledby="wall-list-title">
      <h2 id="wall-list-title" className="rule-heading">Every tile<span className="dia" aria-hidden="true" /></h2>
      {live.length === 0 ? <p className="muted">No tiles yet.</p> : <ol>{live.map(t => <li key={t.n}>
        <Link href={`/wall/${t.n}`}>{t.name}</Link> <span className="muted">· {sizeLabel[t.size]} at {place(t)}</span>
        {t.link && <> · <a href={t.link} rel="sponsored nofollow ugc noopener" target="_blank">{new URL(t.link).host.replace(/^www\./, "")}</a></>}
        {t.message && <p>{t.message}</p>}
      </li>)}</ol>}
    </section>
  </>;
}
