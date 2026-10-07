"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { send } from "../lib/client-api";
import { factionInfo, factionOf, factions } from "../lib/factions";
import { contest, type Genre, type War, utcDate } from "../lib/war";
import { Crest } from "./FactionIdentity";
import { WarStanding } from "./WarStanding";

/**
 * The hex map grouped into each faction's homeland (docs/LORE.md): the Kiln, Selenne and Aurel,
 * then any genre with no home faction as open ground. Rows of four hexes under each region's name.
 */
function homelandLayout(genres: Genre[]) {
  const order = [...factions.map(f => f.slug as string), null];
  const regions: { key: string; faction: string | null; name: string; note: string; y: number }[] = [];
  const hexes: { g: Genre; x: number; y: number }[] = [];
  let y = 0;
  for (const home of order) {
    const members = genres.filter(g => (g.home ?? null) === home).sort((a, b) => a.position - b.position);
    if (!members.length) continue;
    const f = home ? factionOf(home) : null;
    regions.push({ key: home ?? "open", faction: home, name: f ? f.homeland[0].toUpperCase() + f.homeland.slice(1) : "Open ground", note: f ? `${f.name}\u2019s homeland \u00b7 ${f.homelandNote}` : "No faction\u2019s homeland \u00b7 kept neutral under the Accord", y });
    members.forEach((g, i) => hexes.push({ g, x: 102 + (i % 4) * 202 + (Math.floor(i / 4) % 2) * 8, y: y + 56 + 82 + Math.floor(i / 4) * 154 }));
    y += 56 + Math.ceil(members.length / 4) * 154 + 24;
  }
  return { regions, hexes, height: y + 10 };
}

export function GenreBoard({ genres }: { genres: Genre[] }) {
  return <div className="genre-board">{genres.map(g => { const { top, lead, contested } = contest(g); return <article className="genre-card panel" id={`genre-${g.id}`} key={g.id} data-theme={g.holder ?? "neutral"}><div className="row">{g.holder && <Crest faction={g.holder} size={32} />}<h3>{g.name}</h3></div><p>Held by {g.holder ? <Link href={`/factions/${g.holder}`}>{factionInfo(g.holder).name}</Link> : "nobody · neutral"}{contested && <strong className="contested-tag">Contested</strong>}</p><p className="muted">{top?.score ? `${factionInfo(top.faction).name} leads · ${lead.toFixed(1)}% ahead of second` : "No influence this week."}</p><dl className="genre-scores">{g.scores.map(s => <div key={s.faction}><dt>{factionInfo(s.faction).name}</dt><dd><meter min={0} max={Math.max(1, ...g.scores.map(v => v.score))} value={s.score} aria-label={`${factionInfo(s.faction).name} balanced score`} />{s.score.toLocaleString("en-US", { maximumFractionDigits: 1 })}<span className="sr-only"> balanced score</span></dd></div>)}</dl>{g.home && <p className="muted">In {factionOf(g.home)?.homeland}, {factionInfo(g.home).name}&rsquo;s homeland</p>}{g.neighbors.length > 0 && <p className="muted">Neighbors: {g.neighbors.map(id => genres.find(other => other.id === id)?.name ?? id.replaceAll("_", " ")).join(", ")}</p>}</article>; })}</div>;
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
  const layout = homelandLayout(war.genres);
  const selectedGenre = war.genres.find(g => g.id === selected);
  return <div className="war-page"><header><p className="eyebrow">The seasonal war</p><h1>War map</h1><p>Play, build and make. Help your faction take ground.</p></header><WarStanding war={war} />
    <div className="war-rules panel"><h2>How ground is taken</h2><ol><li>Verified members earn influence from real streams and Trusted playback. Chat counts too, within limits.</li><li>Influence is balanced by active faction size. Enemy turf and your private council’s target earn bonuses.</li><li>Ownership changes at Monday’s checkpoint, 00:00 UTC. A tie or a narrow lead keeps the holder.</li></ol><p>{war.week && !war.week.completed ? `Next checkpoint: ${utcDate(war.week.ends_at)}.` : "The season is on break."} Borders show neighboring genres; they do not restrict attacks.</p></div>
    {error && <p role="status" className="notice">{error}</p>}<div className="row war-view"><button className="quiet" aria-pressed={view === "map"} onClick={() => setView("map")}>Hex map</button><button className="quiet" aria-pressed={view === "board"} onClick={() => setView("board")}>Genre board</button></div>
    {war.genres.length === 0 ? <p className="notice">Territories will appear when the first season begins.</p> : <>
      <div className={`hex-map ${view === "board" ? "hidden" : ""}`}><svg viewBox={`0 0 820 ${layout.height}`} role="group" aria-label="Genre territories by homeland. Select a hex for its scores.">{layout.regions.map(r => <g key={r.key} className="homeland" data-theme={r.faction ?? "neutral"}><text x="10" y={r.y + 24} className="homeland-name">{r.name}</text><text x="10" y={r.y + 42} className="homeland-note">{r.note}</text></g>)}{layout.hexes.map(({ g, x, y }) => { const c = contest(g); const words = g.name.split(" & "); return <g key={g.id} data-theme={g.holder ?? "neutral"} className={c.contested ? "hex contested" : "hex"} transform={`translate(${x} ${y})`} role="button" tabIndex={0} aria-label={`${g.name}, held by ${g.holder ?? "nobody"}${c.contested ? ", contested" : ""}`} aria-pressed={selected === g.id} onClick={() => setSelected(g.id)} onKeyDown={e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setSelected(g.id); } }}><polygon points="-92,-42 0,-76 92,-42 92,42 0,76 -92,42" /><text textAnchor="middle" y={-26} className="hex-holder">{g.holder ?? "Neutral"}</text><text textAnchor="middle" y={-4}>{words.map((w, n) => <tspan key={n} x="0" dy={n ? 20 : 0}>{w}</tspan>)}</text><text textAnchor="middle" y={45} className="hex-detail">{c.contested ? "CONTESTED · " : ""}{c.top?.score ? `${c.lead.toFixed(1)}% lead` : "No score yet"}</text></g>; })}</svg></div>
      {view === "map" && selectedGenre && <div className="selected-territory"><GenreBoard genres={[selectedGenre]} /></div>}
      <div className={view === "map" ? "map-board" : ""}><GenreBoard genres={war.genres} /></div>
    </>}
    {!!war.history.length && <details className="panel"><summary>Checkpoint history</summary>{war.history.map(h => <section key={h.ends_at}><h3>{utcDate(h.ends_at)}</h3><ul>{h.genres.map(g => <li key={g.id}>{g.name}: {g.holder ? factionInfo(g.holder).name : "Neutral"}</li>)}</ul></section>)}</details>}
  </div>;
}
