"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { send } from "../lib/client-api";
import { factionInfo, factionOf, type Faction } from "../lib/factions";
import { contest, type Genre, type War, utcDate } from "../lib/war";
import { Crest } from "./FactionIdentity";
import { TerritoryMap } from "./TerritoryMap";
import { RecoveryNote, RivalryNote, WarStanding } from "./WarStanding";

export function GenreBoard({ genres }: { genres: Genre[] }) {
  return <div className="genre-board">{genres.map(g => { const { top, lead, contested } = contest(g); return <article className="genre-card panel" id={`genre-${g.id}`} key={g.id} data-theme={g.holder ?? "neutral"}><div className="row">{g.holder && <Crest faction={g.holder} size={32} />}<h3>{g.name}</h3></div><p>Held by {g.holder ? <Link href={`/factions/${g.holder}`}>{factionInfo(g.holder).name}</Link> : "nobody · neutral"}{contested && <strong className="contested-tag">Contested</strong>}</p><p className="muted">{top?.score ? `${factionInfo(top.faction).name} leads · ${lead.toFixed(1)}% ahead of second` : "No influence this week."}</p><dl className="genre-scores">{g.scores.map(s => <div key={s.faction}><dt>{factionInfo(s.faction).name}</dt><dd><meter min={0} max={Math.max(1, ...g.scores.map(v => v.score))} value={s.score} aria-label={`${factionInfo(s.faction).name} balanced score`} />{s.score.toLocaleString("en-US", { maximumFractionDigits: 1 })}<span className="sr-only"> balanced score</span></dd></div>)}</dl>{g.home && <p className="muted">In {factionOf(g.home)?.homeland}, {factionInfo(g.home).name}&rsquo;s homeland</p>}{g.neighbors.length > 0 && <p className="muted">Neighbors: {g.neighbors.map(id => genres.find(other => other.id === id)?.name ?? id.replaceAll("_", " ")).join(", ")}</p>}</article>; })}</div>;
}
export default function WarMap({ initial, faction }: { initial: War; faction?: Faction | null }) {
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
  // How ground is taken sits beside the Accord on the map, and stands alone on the genre board.
  const rules = <><h2>How ground is taken</h2><ol><li>Verified members earn influence from a confirmed stream, from playback that counts, and from chat while that playback counts. Payments do not.</li><li>Influence is balanced by active faction size. Enemy turf and the target a faction&apos;s members vote for earn more. The vote is not ordered by influence.</li><li>Ownership changes at the checkpoint. A tie or a narrow lead keeps the holder. Losing a genre does not remove anyone.</li></ol><p>{war.week && !war.week.completed ? `Next checkpoint: ${utcDate(war.week.ends_at)}. That week&apos;s influence then stops, and members choose a new target.` : "The season is on break. Genres return to home turf and influence starts over."} Borders show neighboring genres. They do not decide who can earn influence.</p></>;
  return <div className="war-page"><header><p className="eyebrow">The seasonal war</p><h1>War map</h1><p>Play, build and make. Help your faction take ground.</p></header>
    {error && <p role="status" className="notice">{error}</p>}<div className="row war-view"><button className="quiet" aria-pressed={view === "map"} onClick={() => setView("map")}>Map</button><button className="quiet" aria-pressed={view === "board"} onClick={() => setView("board")}>Genre board</button></div>
    {war.genres.length === 0 ? <p className="notice">Territories will appear when the first season begins.</p> : <>
      <div className={`war-map-view ${view === "board" ? "hidden" : ""}`}><TerritoryMap genres={sorted} selected={selected} onSelect={setSelected} season={war.season?.number} rules={rules} /></div>
      {view === "board" && <div className="war-rules panel">{rules}</div>}
    </>}
    <WarStanding war={war} />
    <RecoveryNote war={war} />
    <RivalryNote faction={faction} />
    {war.genres.length > 0 && <div className={view === "map" ? "map-board" : ""}><GenreBoard genres={war.genres} /></div>}
    {war.genres.length === 0 && <div className="war-rules panel">{rules}</div>}
    {!!war.history.length && <details className="panel"><summary>Checkpoint history</summary>{war.history.map(h => <section key={h.ends_at}><h3>{utcDate(h.ends_at)}</h3><ul>{h.genres.map(g => <li key={g.id}>{g.name}: {g.holder ? factionInfo(g.holder).name : "Neutral"}</li>)}</ul></section>)}</details>}
  </div>;
}
