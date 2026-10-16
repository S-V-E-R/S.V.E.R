import Link from "next/link";
import { Crest } from "./Crest";
import { factionOf, type Faction } from "../lib/factions";

/**
 * Whose ground a stream is on (docs/DESIGN.md "Watch page"): the genre, who holds it this week and
 * what that means for the viewer. Watching on enemy ground earns half again as much influence
 * (docs/FACTIONS.md), so the strip says so; it never ranks or promotes the stream.
 */
export function TurfStrip({ genre, holder, viewer }: {
  genre: { id: string; name: string };
  holder: Faction | null;
  viewer: Faction | null;
}) {
  const held = factionOf(holder);
  const mine = factionOf(viewer);
  const where = <Link href={`/war-map#genre-${genre.id}`}>{genre.name}</Link>;
  let note: React.ReactNode;
  if (!held) note = "Unclaimed ground";
  else if (!mine) note = <>Held by {held.name}</>;
  else if (held.slug === mine.slug) note = <>Held by {held.name} · <strong>ally turf</strong></>;
  else note = <>Held by {held.name} · <strong>enemy turf</strong>: your watching here counts 1.5× for {mine.name}</>;
  return <p className="turf-strip" data-theme={held?.slug ?? undefined}>
    {held && <Crest faction={held.slug} initial="" size={20} label={`Held by ${held.name}`} />}
    <span>{where} · {note}</span>
  </p>;
}
