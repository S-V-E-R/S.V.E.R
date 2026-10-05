import Link from "next/link";
import { Crest } from "./FactionIdentity";
import { factions, factionInfo, type Faction } from "../lib/factions";
import { type War, utcDate } from "../lib/war";

export function WarStanding({ war }: { war: War }) {
  return <div className="war-standing">{war.season ? <p className="eyebrow">Season {war.season.number} · {war.season.finished ? `Break · next season ${utcDate(war.season.next_starts_at)}` : `Ends ${utcDate(war.season.ends_at)}`}</p> : <p>The first season has not started.</p>}
    <div className="war-bar" aria-label="Territories held">{war.scoreboard.filter(s => s.territories > 0).map(s => <span key={s.faction} data-theme={s.faction} style={{ flex: s.territories }} title={`${factionInfo(s.faction).name}: ${s.territories} territories`} />)}</div>
    <ul className="war-scoreboard">{war.scoreboard.map(s => <li key={s.faction}><Link href={`/factions/${s.faction}`}><Crest faction={s.faction} size={24} />{factionInfo(s.faction).name}</Link><strong>{s.territories}</strong><span className="muted">{s.genre_weeks} genre-weeks</span></li>)}</ul>
  </div>;
}
export function FrontLine({ war, faction, signedIn }: { war: War | null; faction: Faction | null; signedIn: boolean }) {
  const leading = war ? [...war.scoreboard].sort((a, b) => b.territories - a.territories) : [];
  const lead = leading.length > 1 && leading[0].territories > leading[1].territories ? leading[0] : null;
  return <section className="front-line frame" aria-labelledby="front-line-title">
    <div className="front-line-crests">{factions.map(f => <Link key={f.slug} href={signedIn ? `/factions/${f.slug}` : `/signup?faction=${f.slug}`} aria-label={signedIn ? `Visit ${f.name}` : `Enlist in ${f.name}`}><Crest faction={f.slug} size={44} /></Link>)}</div>
    <div className="front-line-status"><h2 id="front-line-title">{faction ? `${factionInfo(faction).name}, take your place.` : "Three factions. Pick your side."}</h2><p>{lead ? `${factionInfo(lead.faction).name} holds ${lead.territories} territories.` : "Every contribution helps your side."} Territory changes at weekly checkpoints.</p>{war ? <WarStanding war={war} /> : <p>Standings are temporarily unavailable.</p>}</div>
    <Link className="button quiet" href="/war-map">Read the map</Link>{!faction && <Link className="button" href={signedIn ? "/choose-faction" : "/signup"}>Enlist</Link>}
    {!!war?.previous_winners.length && <p className="season-feature">Last season’s champions: {war.previous_winners.map((f, i) => <span key={f}>{i > 0 && " · "}<Link href={`/factions/${f}`}>{factionInfo(f).name}</Link></span>)}</p>}
  </section>;
}
