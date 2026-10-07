"use client";
import { useId, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { factionInfo } from "../lib/factions";
import { accord, founders, homelands, yearAF } from "../lib/lore";
import { contest, type Genre } from "../lib/war";
import { warGeometry, type Territory } from "../lib/warmap";

type Camera = { x: number; y: number; s: number };
const FULL: Camera = { x: 0, y: 0, s: 1 };
const nameLines = (name: string) => name.includes(" & ") ? name.split(" & ") : name.length > 12 && name.includes(" ") ? [name.slice(0, name.lastIndexOf(" ")), name.slice(name.lastIndexOf(" ") + 1)] : [name];
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The RISK-style war map: hex-tiled territories, hover to lift and read standings, select to zoom in. */
export function TerritoryMap({ genres, selected, onSelect, season }: { genres: Genre[]; selected: string | null; onSelect: (id: string | null) => void; season?: number | null }) {
  const layoutKey = genres.map(g => `${g.id}:${g.home}:${g.map?.q},${g.map?.r}`).join("|");
  // Geometry depends only on the layout, not on live scores, so the 30-second refresh never redraws tiles.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const geo = useMemo(() => warGeometry(genres), [layoutKey]);
  const byId = new Map(genres.map(g => [g.id, g]));
  const [hover, setHover] = useState<string | null>(null);
  const [camera, setCamera] = useState<Camera>(FULL);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ x: number; y: number; cam: Camera; moved: boolean } | null>(null);
  const svg = useRef<SVGSVGElement>(null);
  const id = useId().replace(/:/g, "");
  const fit = (c: Camera): Camera => ({ s: c.s, x: clamp(c.x, geo.width * (1 - c.s), 0), y: clamp(c.y, geo.height * (1 - c.s), 0) });
  const focus = (t: Territory): Camera => { const s = clamp(Math.min(geo.width / (t.box.w + 220), geo.height / (t.box.h + 160)), 1.6, 2.6); return fit({ s, x: geo.width / 2 - (t.box.x + t.box.w / 2) * s, y: geo.height / 2 - (t.box.y + t.box.h / 2) * s }); };
  const zoom = (by: number) => setCamera(c => { const s = clamp(c.s * by, 1, 3.5), cx = (geo.width / 2 - c.x) / c.s, cy = (geo.height / 2 - c.y) / c.s; return fit({ s, x: geo.width / 2 - cx * s, y: geo.height / 2 - cy * s }); });
  // Selecting a territory flies the camera to it; clearing the selection returns to the full map.
  const [framed, setFramed] = useState<string | null>(null);
  if (framed !== selected) {
    const t = geo.territories.find(x => x.genre.id === selected);
    setFramed(selected); setCamera(t ? focus(t) : FULL);
  }
  const units = () => geo.width / (svg.current?.getBoundingClientRect().width || geo.width);
  const down = (e: ReactPointerEvent) => { if (e.button === 0) drag.current = { x: e.clientX, y: e.clientY, cam: camera, moved: false }; };
  const move = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d || camera.s <= 1) return;
    const dx = e.clientX - d.x, dy = e.clientY - d.y;
    if (!d.moved && Math.hypot(dx, dy) < 5) return;
    if (!d.moved) { d.moved = true; setDragging(true); setHover(null); (e.currentTarget as Element).setPointerCapture(e.pointerId); }
    setCamera(fit({ s: d.cam.s, x: d.cam.x + dx * units(), y: d.cam.y + dy * units() }));
  };
  const up = () => { setTimeout(() => { drag.current = null; }, 0); setDragging(false); };
  const pick = (genreId: string) => { if (!drag.current?.moved) onSelect(selected === genreId ? null : genreId); };
  const lifted = dragging ? undefined : geo.territories.find(t => t.genre.id === hover);
  const shown = byId.get(hover ?? selected ?? "");
  const label = (t: Territory) => {
    const g = byId.get(t.genre.id) ?? t.genre, c = contest(g), words = nameLines(g.name), capital = g.capital && g.home ? g.home : null, top = capital ? 16 : -((words.length - 1) * 11) / 2 - 2;
    return <g className="territory-label" transform={`translate(${t.x.toFixed(1)} ${t.y.toFixed(1)})`}>
      {capital && <g className="capital" data-theme={capital}>
        {capital === "myria" && <g className="crater"><circle cy={-22} r={19} /><circle cy={-22} r={25} /></g>}
        <circle cy={-22} r={12} className="capital-seal" /><image href={`/factions/${capital}.webp`} x={-10} y={-32} width={20} height={20} />
        <text textAnchor="middle" y={2} className="capital-name">{homelands[capital].city}</text>
      </g>}
      <text textAnchor="middle" y={top}>{words.map((w, n) => <tspan key={n} x="0" dy={n ? 11 : 0}>{w}</tspan>)}</text>
      <text textAnchor="middle" y={top + words.length * 11 + 1} className="territory-detail">{c.contested ? "Contested" : c.top?.score ? `${c.lead.toFixed(0)}% lead` : ""}</text>
    </g>;
  };
  return <div className="war-theater" onKeyDown={e => { if (e.key === "Escape" && selected) onSelect(null); }}><div className="war-stage">
    <svg ref={svg} viewBox={`0 0 ${geo.width} ${geo.height}`} className={dragging ? "dragging" : camera.s > 1 ? "zoomed" : undefined} role="group" aria-label="War map. Each territory is a genre, colored by the faction holding it. Select one to zoom in and see its scores."
      onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up} onPointerLeave={() => setHover(null)}>
      <defs>
        <radialGradient id={`${id}-static`} cx="50%" cy="50%" r="72%"><stop offset="55%" stopColor="#030508" stopOpacity="0" /><stop offset="100%" stopColor="#030508" stopOpacity=".7" /></radialGradient>
        <pattern id={`${id}-hatch`} width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line x1="0" y1="0" x2="0" y2="6" className="hatch-line" /></pattern>
      </defs>
      <rect width={geo.width} height={geo.height} className="war-sea-bg" />
      <g className="war-camera" style={{ transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.s})` }}>
        <path d={geo.sea} className="war-sea" /><path d={geo.shallows} className="war-shallows" />
        {geo.moon && <g className="pale-moon" transform={`translate(${geo.moon.x.toFixed(1)} ${geo.moon.y.toFixed(1)})`} aria-hidden="true"><circle r="19" className="moon-halo" /><path d="M0 -15A15 15 0 1 0 0 15A11 15 0 1 1 0 -15Z" /><text y="30" textAnchor="middle">The Pale Moon</text></g>}
        {geo.territories.map(t => {
          const g = byId.get(t.genre.id) ?? t.genre, c = contest(g);
          return <g key={g.id} data-theme={g.holder ?? "neutral"} className={`territory${c.contested ? " contested" : ""}${selected === g.id ? " selected" : ""}`} role="button" tabIndex={0}
            aria-label={`${g.name}${g.capital && g.home ? `, ${factionInfo(g.home).name} capital` : ""}, held by ${g.holder ? factionInfo(g.holder).name : "nobody"}${c.contested ? ", contested" : ""}`} aria-pressed={selected === g.id}
            onPointerEnter={() => { if (!drag.current?.moved) setHover(g.id); }} onFocus={() => setHover(g.id)} onBlur={() => setHover(null)} onClick={() => pick(g.id)}
            onKeyDown={e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onSelect(selected === g.id ? null : g.id); } }}>
            {t.tones.map((d, n) => <path key={n} d={d} className={`tone tone-${n}`} />)}
            {c.contested && <path d={t.tones.join("")} fill={`url(#${id}-hatch)`} className="hatch" />}
          </g>;
        })}
        <path d={geo.borders} className="war-border" /><path d={geo.frontiers} className="war-frontier" /><path d={geo.coast} className="war-coast" />
        {geo.territories.map(t => { const g = byId.get(t.genre.id) ?? t.genre; return (contest(g).contested || selected === g.id) && <path key={g.id} d={t.outline} className={selected === g.id ? "war-outline selected" : "war-outline contested"} />; })}
        <g className="territory-labels" aria-hidden="true">{geo.territories.map(t => <g key={t.genre.id}>{t.genre.id !== lifted?.genre.id && label(t)}</g>)}</g>
        {geo.regions.map(r => <text key={r.home} data-theme={r.home} className="war-region" x={r.x} y={r.y} textAnchor="middle" aria-hidden="true">{factionInfo(r.home).name}</text>)}
        {lifted && <g className="territory-lift" data-theme={(byId.get(lifted.genre.id) ?? lifted.genre).holder ?? "neutral"} aria-hidden="true" key={lifted.genre.id}>
          <path d={lifted.tones.join("")} className="lift-shadow" transform="translate(3 6)" />
          {lifted.tones.map((d, n) => <path key={n} d={d} className={`tone tone-${n}`} />)}
          <path d={lifted.outline} className="lift-edge" />{label(lifted)}
        </g>}
      </g>
      <rect width={geo.width} height={geo.height} fill={`url(#${id}-static)`} className="war-static" />
      <g className="war-cartouche" transform={`translate(${geo.width - 16} 26)`} aria-hidden="true"><text textAnchor="end" className="cartouche-title">The Known World</text><text textAnchor="end" y="15" className="cartouche-year">{season ? `Season ${season} · ` : ""}{yearAF(season)} AF</text></g>
      <g className="war-compass" transform={`translate(${geo.width - 34} ${geo.height - 36})`} aria-hidden="true"><circle r="20" /><path d="M0 -17L4 0L0 17L-4 0Z M-17 0L0 -3L17 0L0 3Z" /><text y="-23" textAnchor="middle">N</text></g>
    </svg>
    <div className="war-controls"><button type="button" className="quiet" onClick={() => zoom(1.4)} aria-label="Zoom in">+</button><button type="button" className="quiet" onClick={() => zoom(1 / 1.4)} aria-label="Zoom out" disabled={camera.s <= 1}>−</button><button type="button" className="quiet" onClick={() => { onSelect(null); setCamera(FULL); }} disabled={camera.s <= 1 && !selected}>Full map</button></div>
    <ul className="war-legend" aria-label="Map key">{(["myria", "aetheron", "glint"] as const).map(f => <li key={f} data-theme={f}><span className="swatch" />{factionInfo(f).name}</li>)}<li data-theme="neutral"><span className="swatch" />Unclaimed</li><li><span className="swatch hatched" />Contested</li><li><span className="swatch capital-key" />Capital</li><li><span className="swatch static-key" />The Static</li></ul></div>
    <aside className="war-plate" data-theme={shown?.holder ?? "neutral"}>{shown ? <TerritoryPlate genre={shown} /> : <AccordPlate />}</aside>
  </div>;
}

function TerritoryPlate({ genre: g }: { genre: Genre }) {
  const c = contest(g), max = Math.max(1, ...g.scores.map(s => s.score)), founder = founders[g.id];
  return <>
    <p className="eyebrow">{g.capital && g.home ? `${homelands[g.home].city} · capital of ${factionInfo(g.home).name}` : g.home ? `${factionInfo(g.home).name} home turf` : "Open ground"}</p>
    <h3>{g.name}</h3>
    <p>Held by {g.holder ? factionInfo(g.holder).name : "nobody, under the Accord"}{c.contested && <strong className="contested-tag">Contested</strong>}</p>
    <dl className="genre-scores">{[...g.scores].sort((a, b) => b.score - a.score).map(s => <div key={s.faction} data-theme={s.faction}><dt>{factionInfo(s.faction).name}</dt><dd><meter min={0} max={max} value={s.score} aria-label={`${factionInfo(s.faction).name} balanced score`} />{s.score.toLocaleString("en-US", { maximumFractionDigits: 1 })}</dd></div>)}</dl>
    <p className="muted">{c.top?.score ? `${factionInfo(c.top.faction).name} leads by ${c.lead.toFixed(1)}%.` : "No influence this week."}</p>
    {(g.capital && g.home || founder) && <div className="plate-lore">
      {g.capital && g.home && <p>{homelands[g.home].about} Home of {homelands[g.home].relic}.</p>}
      {founder && <p><strong>{founder.name}.</strong> {founder.deed}</p>}
    </div>}
    {g.holder && <p className="battle-cry">“{homelands[g.holder].cry}”</p>}
  </>;
}

/** Shown before anything is hovered: the terms the war is fought under. */
function AccordPlate() {
  return <>
    <p className="eyebrow">Sworn in 41 AF</p>
    <h3>The Accord</h3>
    <ol className="accord-terms">{accord.map(term => <li key={term}>{term}</li>)}</ol>
    <p className="muted">Hover a territory for this week’s standings. Select one to zoom in.</p>
  </>;
}
