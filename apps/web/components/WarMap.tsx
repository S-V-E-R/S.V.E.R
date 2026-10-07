"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { send } from "../lib/client-api";
import { factionInfo } from "../lib/factions";
import { contest, type Genre, type War, utcDate } from "../lib/war";
import { Crest } from "./FactionIdentity";
import { WarStanding } from "./WarStanding";

export function GenreBoard({ genres }: { genres: Genre[] }) {
  return <div className="genre-board">{genres.map(g => { const { top, lead, contested } = contest(g); return <article className="genre-card panel" id={`genre-${g.id}`} key={g.id} data-theme={g.holder ?? "neutral"}><div className="row">{g.holder && <Crest faction={g.holder} size={32} />}<h3>{g.name}</h3></div><p>Held by {g.holder ? <Link href={`/factions/${g.holder}`}>{factionInfo(g.holder).name}</Link> : "nobody · neutral"}{contested && <strong className="contested-tag">Contested</strong>}</p><p className="muted">{top?.score ? `${factionInfo(top.faction).name} leads · ${lead.toFixed(1)}% ahead of second` : "No influence this week."}</p><dl className="genre-scores">{g.scores.map(s => <div key={s.faction}><dt>{factionInfo(s.faction).name}</dt><dd><meter min={0} max={Math.max(1, ...g.scores.map(v => v.score))} value={s.score} aria-label={`${factionInfo(s.faction).name} balanced score`} />{s.score.toLocaleString("en-US", { maximumFractionDigits: 1 })}<span className="sr-only"> balanced score</span></dd></div>)}</dl>{g.neighbors.length > 0 && <p className="muted">Neighbors: {g.neighbors.map(id => genres.find(other => other.id === id)?.name ?? id.replaceAll("_", " ")).join(", ")}</p>}</article>; })}</div>;
}
const R = 70, W = R * Math.sqrt(3), ROW = R * 1.5, PER_ROW = 3, GAP = 56, LABEL = 34, PAD = 16;
const nameLines = (name: string) => name.includes(" & ") ? name.split(" & ") : name.length > 12 && name.includes(" ") ? [name.slice(0, name.lastIndexOf(" ")), name.slice(name.lastIndexOf(" ") + 1)] : [name];
const HEX = (r: number) => { const s = r * Math.sqrt(3) / 2; return `0,${-r} ${s},${-r / 2} ${s},${r / 2} 0,${r} ${-s},${r / 2} ${-s},${-r / 2}`; };
/** Lays out each home region as a honeycomb cluster; clusters sit two on top, then one centered below, like a map. */
function layout(genres: Genre[]) {
  const regions: { home: Genre["home"]; genres: Genre[] }[] = [];
  for (const g of genres) { const last = regions.at(-1); if (last && last.home === g.home) last.genres.push(g); else regions.push({ home: g.home, genres: [g] }); }
  const blobW = (PER_ROW + .5) * W, rows = (n: number) => Math.ceil(n / PER_ROW), blobH = (n: number) => (rows(n) - 1) * ROW + 2 * R;
  const slotRows = Math.ceil(regions.length / 2), rowH = Array.from({ length: slotRows }, (_, r) => Math.max(...regions.slice(r * 2, r * 2 + 2).map(g => blobH(g.genres.length))) + LABEL + GAP);
  const lone = regions.length % 2 === 1 && regions.length > 1;
  const width = (regions.length > 1 ? 2 : 1) * blobW + (regions.length > 1 ? GAP : 0) + PAD * 2;
  const tiles: { genre: Genre; x: number; y: number }[] = [], labels: { home: Genre["home"]; x: number; y: number }[] = [];
  regions.forEach((region, i) => {
    const slotRow = Math.floor(i / 2), centered = lone && i === regions.length - 1;
    const n = region.genres.length, used = Math.max(...Array.from({ length: rows(n) }, (_, r) => Math.min(PER_ROW, n - r * PER_ROW) + (r % 2) * .5)) * W;
    const left = PAD + (centered ? (width - PAD * 2 - blobW) / 2 : (i % 2) * (blobW + GAP)) + (blobW - used) / 2;
    const top = PAD + rowH.slice(0, slotRow).reduce((a, b) => a + b, 0);
    labels.push({ home: region.home, x: left + used / 2, y: top + 20 });
    region.genres.forEach((genre, j) => { const row = Math.floor(j / PER_ROW); tiles.push({ genre, x: left + W / 2 + (j % PER_ROW + (row % 2) * .5) * W, y: top + LABEL + R + row * ROW }); });
  });
  return { tiles, labels, width, height: PAD * 2 + rowH.reduce((a, b) => a + b, 0) - GAP };
}
function HexMap({ genres, selected, onSelect, hidden }: { genres: Genre[]; selected: string | null; onSelect: (id: string) => void; hidden: boolean }) {
  const { tiles, labels, width, height } = layout(genres);
  return <div className={`hex-map ${hidden ? "hidden" : ""}`}><svg viewBox={`0 0 ${width.toFixed(0)} ${height.toFixed(0)}`} role="group" aria-label="Genre territories. Select a hex for its scores.">
    {labels.map(l => <text key={l.home ?? "neutral"} data-theme={l.home ?? "neutral"} className="hex-region" x={l.x} y={l.y} textAnchor="middle">{l.home ? `${factionInfo(l.home).name} home` : "Open ground"}</text>)}
    {tiles.map(({ genre: g, x, y }) => { const c = contest(g); const words = nameLines(g.name); const nameY = 4 - (words.length - 1) * 19 / 2; return <g key={g.id} data-theme={g.holder ?? "neutral"} className={c.contested ? "hex contested" : "hex"} transform={`translate(${x.toFixed(1)} ${y.toFixed(1)})`} role="button" tabIndex={0} aria-label={`${g.name}, held by ${g.holder ? factionInfo(g.holder).name : "nobody"}${c.contested ? ", contested" : ""}`} aria-pressed={selected === g.id} onClick={() => onSelect(g.id)} onKeyDown={e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onSelect(g.id); } }}>
      <polygon className="hex-face" points={HEX(R - 3)} /><polygon className="hex-inset" points={HEX(R - 10)} />
      <text textAnchor="middle" y={-34} className="hex-holder">{g.holder ? factionInfo(g.holder).name : "Neutral"}</text>
      <text textAnchor="middle" y={nameY} className="hex-name">{words.map((w, n) => <tspan key={n} x="0" dy={n ? 19 : 0}>{w}</tspan>)}</text>
      {c.contested ? <text textAnchor="middle" y={40} className="hex-tag">Contested</text> : <text textAnchor="middle" y={40} className="hex-detail">{c.top?.score ? `${c.lead.toFixed(0)}% lead` : "No score yet"}</text>}
    </g>; })}
  </svg></div>;
}
export default function WarMap({ initial }: { initial: War }) {
  const [war, setWar] = useState(initial);
  const [view, setView] = useState<"map" | "board">("map");
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const refresh = async () => { if (document.hidden) return; const r = await send<War>("GET", "/api/factions/war"); if (active) { if (r.ok) { setWar(r.data); setError(""); } else setError("Updates are unavailable. Showing the last loaded standings."); } };
    const timer = setInterval(refresh, 30_000);
    document.addEventListener("visibilitychange", refresh);
    return () => { active = false; clearInterval(timer); document.removeEventListener("visibilitychange", refresh); };
  }, []);
  const sorted = [...war.genres].sort((a, b) => (a.home ?? "z").localeCompare(b.home ?? "z") || a.position - b.position);
  const selectedGenre = war.genres.find(g => g.id === selected);
  return <div className="war-page"><header><p className="eyebrow">The seasonal war</p><h1>War map</h1><p>Play, build and make. Help your faction take ground.</p></header><WarStanding war={war} />
    <div className="war-rules panel"><h2>How ground is taken</h2><ol><li>Verified members earn influence from real streams and Trusted playback. Chat counts too, within limits.</li><li>Influence is balanced by active faction size. Enemy turf and your private council’s target earn bonuses.</li><li>Ownership changes at Monday’s checkpoint, 00:00 UTC. A tie or a narrow lead keeps the holder.</li></ol><p>{war.week && !war.week.completed ? `Next checkpoint: ${utcDate(war.week.ends_at)}.` : "The season is on break."} Borders show neighboring genres; they do not restrict attacks.</p></div>
    {error && <p role="status" className="notice">{error}</p>}<div className="row war-view"><button className="quiet" aria-pressed={view === "map"} onClick={() => setView("map")}>Hex map</button><button className="quiet" aria-pressed={view === "board"} onClick={() => setView("board")}>Genre board</button></div>
    {war.genres.length === 0 ? <p className="notice">Territories will appear when the first season begins.</p> : <>
      <HexMap genres={sorted} selected={selected} onSelect={setSelected} hidden={view === "board"} />
      {view === "map" && selectedGenre && <div className="selected-territory"><GenreBoard genres={[selectedGenre]} /></div>}
      <div className={view === "map" ? "map-board" : ""}><GenreBoard genres={war.genres} /></div>
    </>}
    {!!war.history.length && <details className="panel"><summary>Checkpoint history</summary>{war.history.map(h => <section key={h.ends_at}><h3>{utcDate(h.ends_at)}</h3><ul>{h.genres.map(g => <li key={g.id}>{g.name}: {g.holder ? factionInfo(g.holder).name : "Neutral"}</li>)}</ul></section>)}</details>}
  </div>;
}
