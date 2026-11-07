import Link from "next/link";
import type { ReactNode } from "react";
import { Crest } from "./FactionIdentity";
import { factions, factionInfo, type Faction } from "../lib/factions";
import { type War, contest, utcDate, utcDay } from "../lib/war";

const utcTime = (iso: string) => new Intl.DateTimeFormat("en-US", { timeZone: "UTC", month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(new Date(iso)) + " UTC";

export function WarStanding({ war }: { war: War }) {
  const skirmish = war.season && !war.season.finished ? war.skirmish : undefined;
  return <div className="war-standing">{war.season ? <p className="eyebrow">Season {war.season.number} · {war.season.finished ? `Break · next season ${utcDay(war.season.next_starts_at)}` : `Ends ${utcDay(war.season.ends_at)}`}</p> : <p>The first season has not started.</p>}
    {skirmish?.active ? <p className="notice skirmish-notice"><strong>Skirmish Hour is on</strong> until {utcTime(skirmish.active.ends_at)}: influence counts {skirmish.multiplier}× (the usual per-person limits still apply).</p>
      : skirmish?.next && <p className="notice skirmish-notice"><strong>Next Skirmish Hour:</strong> {utcTime(skirmish.next.starts_at)}. For one hour, influence counts {skirmish.multiplier}×.</p>}
    {skirmish?.raid_night?.active ? <p className="notice skirmish-notice"><strong>Raid Night is on</strong> until {utcTime(skirmish.raid_night.active.ends_at)}: viewers who arrive by a raid from another faction earn {skirmish.multiplier}× watch influence.</p>
      : skirmish?.raid_night?.next && <p className="notice skirmish-notice"><strong>Next Raid Night:</strong> {utcTime(skirmish.raid_night.next.starts_at)}. For three hours, cross-faction raids bring {skirmish.multiplier}× watch influence.</p>}
    {skirmish?.hill && <p className="notice skirmish-notice"><strong>King of the Hill: {skirmish.hill.name}.</strong> Influence there counts {skirmish.multiplier}× all season, and whoever holds it when the season ends gets {skirmish.hill.bonus} extra territories. {skirmish.hill.holder ? `${factionInfo(skirmish.hill.holder).name} holds it now.` : "Nobody holds it yet."}</p>}
    {skirmish?.spotlight && <p className="notice skirmish-notice"><strong>Territory Spotlight today:</strong> {skirmish.spotlight.name ?? skirmish.spotlight.genre}. Influence earned there counts {skirmish.multiplier}× until midnight UTC.</p>}
    <div className="war-bar" aria-label="Territories held">{war.scoreboard.filter(s => s.territories > 0).map(s => <span key={s.faction} data-theme={s.faction} style={{ flex: s.territories }} title={`${factionInfo(s.faction).name}: ${s.territories} territories`} />)}</div>
    <ul className="war-scoreboard">{war.scoreboard.map(s => <li key={s.faction}><Link href={`/factions/${s.faction}`}><Crest faction={s.faction} size={24} />{factionInfo(s.faction).name}</Link><strong>{s.territories} {s.territories === 1 ? "territory" : "territories"}</strong><span className="muted" title="Weeks each category was held this season, added up. It breaks ties at the end of the season.">{s.genre_weeks} weeks of ground held</span></li>)}</ul>
  </div>;
}
/** "Myria holds the light." or, for joint winners, "Myria and Glint share the light." (docs/MOTION.md §2). */
export function holdsTheLight(winners: Faction[]) {
  const names = winners.map(w => factionInfo(w).name);
  return names.length === 1 ? `${names[0]} holds the light.` : `${names.slice(0, -1).join(", ")} and ${names.at(-1)} share the light.`;
}
/** After a season ends: "Your side won." for the winners, otherwise the map is redrawn. */
export const seasonLine = (winners: Faction[], faction: Faction | null) => faction && winners.includes(faction) ? "Your side won." : "Next season, the map is redrawn.";

/**
 * The front-line banner at the top of home (Main mockup; docs/DESIGN.md "Home"). Real standings only:
 * with no territories held yet, the bar is left out rather than drawn evenly. Between seasons it
 * names the winners; `replay` is the season reveal's replay button (components/SeasonReveal.tsx).
 */
export function FrontLine({ war, faction, signedIn, replay }: { war: War | null; faction: Faction | null; signedIn: boolean; replay?: ReactNode }) {
  const board = war ? factions.map(f => war.scoreboard.find(s => s.faction === f.slug) ?? { faction: f.slug as Faction, territories: 0, genre_weeks: 0 }) : [];
  const total = board.reduce((n, s) => n + s.territories, 0);
  const ranked = [...board].sort((a, b) => b.territories - a.territories);
  const lead = ranked.length > 1 && ranked[0].territories > ranked[1].territories ? ranked[0] : null;
  const contested = war ? war.genres.filter(g => contest(g).contested).length : 0;
  const finished = war?.season?.finished && war.season.winners.length ? war.season : null;
  // Weeks run from the season start; the first may be partial.
  const week = war?.season && war.week ? Math.max(1, Math.ceil((Date.parse(war.week.ends_at) - Date.parse(war.season.starts_at)) / (7 * 86_400_000))) : null;
  const headline = !war ? "Three factions. One war for every category."
    : !war.season ? "The first season hasn\u2019t started yet."
      : finished ? holdsTheLight(finished.winners)
      : !faction && lead ? `Three factions fight over every category. ${factionInfo(lead.faction).name} leads, ${lead.territories} of ${war.genres.length}.`
      : lead ? `${factionInfo(lead.faction).name} leads with ${lead.territories} ${lead.territories === 1 ? "territory" : "territories"}.${contested ? ` ${contested} contested.` : ""}`
        : total ? `The war is level at ${ranked[0].territories} territories.${contested ? ` ${contested} contested.` : ""}` : "No territory has been taken yet.";
  const sub = finished ? `${seasonLine(finished.winners, faction)} Next season starts ${utcDay(finished.next_starts_at)}.`
    : faction ? `Every stream, watch and chat takes ground for ${factionInfo(faction).name}. Territory changes at weekly checkpoints.`
    : signedIn ? "You haven\u2019t picked a side yet. Choose the faction that sounds like you."
      : "Pick a side, and everything you stream, watch and chat takes ground for it.";
  return <section className="front-line frame" aria-labelledby="front-line-title">
    <div className="front-line-crests">{faction
      ? <Link href={`/factions/${faction}`} aria-label={`${factionInfo(faction).name} hub`}><Crest faction={faction} size={48} /></Link>
      : <>{factions.map(f => <Link key={f.slug} className="pick-side" data-theme={f.slug} href={signedIn ? `/welcome?pick=${f.slug}` : `/signup?faction=${f.slug}`} aria-label={`Enlist in ${f.name}`} title={`Enlist in ${f.name}`}><Crest faction={f.slug} size={40} /></Link>)}<span className="pick-side-caption">Pick a side</span></>}</div>
    <div className="front-line-status">{war?.season && <span className="eyebrow">Season {war.season.number}{week ? ` · Week ${week}` : ""}</span>}<h2 id="front-line-title" tabIndex={-1}>{headline}</h2><p>{sub}{replay && <> {replay}</>}</p></div>
    {total > 0 && <div className="front-line-bar" aria-label="Territories held this season">
      {board.map(s => <span key={s.faction} data-theme={s.faction} style={{ flexGrow: Math.max(s.territories, 0.0001) }}><i aria-hidden="true" /><small>{factionInfo(s.faction).name} {s.territories}</small></span>)}
    </div>}
    {faction ? <Link className="button" href="/war-map">Open the war map</Link>
      : <Link className="button" href={signedIn ? "/welcome" : "/signup"}>{signedIn ? "Pick a side" : "Enlist"}</Link>}
  </section>;
}

/** What already happens after a loss. Dates come from the war payload; nothing here changes scoring. */
export function RecoveryNote({ war }: { war: War | null }) {
  if (!war) return <section className="panel" aria-labelledby="recovery-h"><h2 id="recovery-h">After a loss</h2><p>Standings could not be loaded, so this page will not guess the next checkpoint.</p></section>;
  const open = war.week && !war.week.completed;
  return <section className="panel" aria-labelledby="recovery-h"><h2 id="recovery-h">After a loss</h2>
    <p>Losing a territory does not remove a faction, and it does not change who MAGNet features.</p>
    {open ? <p>This week&apos;s influence counts only until the checkpoint on {utcDate(war.week!.ends_at)}. The next week starts fresh, and members choose a new target. A held genre stays with its holder on a tie or a narrow lead.</p>
      : war.season?.finished ? <p>The season is on break until {utcDay(war.season.next_starts_at)}. Genres return to home turf and influence starts over. Switching factions is open during the break.</p>
        : <p>No checkpoint is on the calendar yet.</p>}
    {war.season && !war.season.finished && <p>This season ends {utcDay(war.season.ends_at)}. Every genre then returns to its home turf.</p>}
  </section>;
}

/** Existing cross-faction freedom and the report, block, and moderation tools already on the site. */
export function RivalryNote({ faction }: { faction?: Faction | null }) {
  return <section className="panel" aria-labelledby="rivalry-h"><h2 id="rivalry-h">Across factions</h2>
    <p>You can watch, follow, and chat with any faction. Influence you earn is for your own side.</p>
    <p>On a channel, <strong>More</strong> includes Block and Report. In chat, Report is on the message. A faction board post can be reported by a member. <Link href="/help#moderation">Moderation tools</Link> · <Link href="/help/safety">Safety</Link> · <Link href="/settings/reports">Your reports</Link>{faction && <> · <Link href={`/factions/${faction}#objective`}>Your faction&apos;s objective</Link></>}</p>
  </section>;
}
