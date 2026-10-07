import Link from "next/link";
import { Crest } from "./FactionIdentity";
import { factions, factionInfo, type Faction } from "../lib/factions";
import { type War, contest, utcDate } from "../lib/war";

export function WarStanding({ war }: { war: War }) {
  return <div className="war-standing">{war.season ? <p className="eyebrow">Season {war.season.number} · {war.season.finished ? `Break · next season ${utcDate(war.season.next_starts_at)}` : `Ends ${utcDate(war.season.ends_at)}`}</p> : <p>The first season has not started.</p>}
    <div className="war-bar" aria-label="Territories held">{war.scoreboard.filter(s => s.territories > 0).map(s => <span key={s.faction} data-theme={s.faction} style={{ flex: s.territories }} title={`${factionInfo(s.faction).name}: ${s.territories} territories`} />)}</div>
    <ul className="war-scoreboard">{war.scoreboard.map(s => <li key={s.faction}><Link href={`/factions/${s.faction}`}><Crest faction={s.faction} size={24} />{factionInfo(s.faction).name}</Link><strong>{s.territories}</strong><span className="muted">{s.genre_weeks} genre-weeks</span></li>)}</ul>
  </div>;
}
/**
 * The front-line banner at the top of home (Main mockup; docs/DESIGN.md "Home"). Real standings only:
 * with no territories held yet, the bar is left out rather than drawn evenly.
 */
export function FrontLine({ war, faction, signedIn }: { war: War | null; faction: Faction | null; signedIn: boolean }) {
  const board = war ? factions.map(f => war.scoreboard.find(s => s.faction === f.slug) ?? { faction: f.slug as Faction, territories: 0, genre_weeks: 0 }) : [];
  const total = board.reduce((n, s) => n + s.territories, 0);
  const ranked = [...board].sort((a, b) => b.territories - a.territories);
  const lead = ranked.length > 1 && ranked[0].territories > ranked[1].territories ? ranked[0] : null;
  const contested = war ? war.genres.filter(g => contest(g).contested).length : 0;
  const headline = !war ? "Three factions. One war for every category."
    : !war.season ? "The first season hasn\u2019t started yet."
      : lead ? `${factionInfo(lead.faction).name} leads with ${lead.territories} ${lead.territories === 1 ? "territory" : "territories"}.${contested ? ` ${contested} contested.` : ""}`
        : total ? `The war is level at ${ranked[0].territories} territories.${contested ? ` ${contested} contested.` : ""}` : "No territory has been taken yet.";
  const sub = faction ? `Every stream, watch and chat takes ground for ${factionInfo(faction).name}. Territory changes at weekly checkpoints.`
    : signedIn ? "You haven\u2019t picked a side yet. Choose the faction that sounds like you."
      : "Pick a side, and everything you stream, watch and chat takes ground for it.";
  return <section className="front-line frame" aria-labelledby="front-line-title">
    <div className="front-line-crests">{faction
      ? <Link href={`/factions/${faction}`} aria-label={`${factionInfo(faction).name} hub`}><Crest faction={faction} size={48} /></Link>
      : factions.map(f => <Link key={f.slug} href={signedIn ? `/welcome?pick=${f.slug}` : `/signup?faction=${f.slug}`} aria-label={`Enlist in ${f.name}`}><Crest faction={f.slug} size={40} /></Link>)}</div>
    <div className="front-line-status"><h2 id="front-line-title">{headline}</h2><p>{sub}</p></div>
    {total > 0 && <div className="front-line-bar" aria-label="Territories held this season">
      {board.map(s => <span key={s.faction} data-theme={s.faction} style={{ flexGrow: Math.max(s.territories, 0.0001) }}><i aria-hidden="true" /><small>{factionInfo(s.faction).name} {s.territories}</small></span>)}
    </div>}
    {faction ? <Link className="button" href="/war-map">Hold the line</Link>
      : <Link className="button" href={signedIn ? "/welcome" : "/signup"}>{signedIn ? "Pick a side" : "Enlist"}</Link>}
  </section>;
}
