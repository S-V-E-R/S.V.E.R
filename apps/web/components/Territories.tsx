import Link from "next/link";
import { Crest } from "./Crest";
import { factionOf } from "../lib/factions";
import { contest, type War } from "../lib/war";

/** `image`: the category's cover art by width (300, 600), or null. */
export type BrowseCategory = { id: string; name: string; live: number; image?: Record<string, string> | null };
export type BrowseGenre = { id: string; name: string; home: string | null; live: number; categories: BrowseCategory[] };

/** Holder of each genre this week, from the war (docs/FACTIONS.md). */
export function holders(war: War | null): Map<string, string | null> {
  return new Map((war?.genres ?? []).map(g => [g.id, g.holder]));
}

function turf(holder: string | null | undefined, viewer: string | null): string {
  if (!holder) return "Unclaimed";
  const f = factionOf(holder);
  if (!viewer) return `Held by ${f?.name ?? holder}`;
  return holder === viewer ? "Your turf · defend it" : `Enemy turf · held by ${f?.name ?? holder}`;
}

/**
 * Category tiles at 3:4 with the holding faction's crest in the corner and the live count
 * (docs/DESIGN.md "Home" Territories and "Browse"). Genres come in war-map order; categories
 * stay alphabetical inside a genre, never ranked by how many people are live.
 */
export function TerritoryGrid({ genres, war, viewerFaction, limit, href, shelf }: {
  genres: BrowseGenre[]; war: War | null; viewerFaction: string | null; limit?: number; shelf?: boolean;
  href: (genre: BrowseGenre, category: BrowseCategory) => string;
}) {
  const held = holders(war);
  const tiles = genres.flatMap(g => g.categories.map(c => ({ g, c, holder: held.get(g.id) ?? null })));
  const shown = limit ? tiles.slice(0, limit) : tiles;
  if (!shown.length) return null;
  return <ul className={shelf ? "territory-grid shelf" : "territory-grid"}>{shown.map(({ g, c, holder }) => {
    const f = factionOf(holder);
    const ground = war?.genres.find(w => w.id === g.id);
    const contested = !!ground && contest(ground).contested;
    const art = c.image?.["300"];
    return <li key={c.id}><Link href={href(g, c)} className={`territory-tile${contested ? " contested" : ""}${art ? " has-art" : ""}`} data-theme={f?.slug ?? undefined}>
      {/* eslint-disable-next-line @next/next/no-img-element -- media CDN image, already sized */}
      {art && <img className="territory-art" src={art} srcSet={Object.entries(c.image!).map(([w, url]) => `${url} ${w}w`).join(", ")} sizes="(max-width: 700px) 45vw, 180px" alt="" loading="lazy" decoding="async" />}
      <span className="territory-crest">{f && <Crest faction={f.slug} initial="" size={30} label={`Held by ${f.name}`} />}</span>
      <h3>{c.name}</h3>
      <span className="territory-turf">{turf(holder, viewerFaction)}{contested && <strong className="contested-tag">Contested</strong>}</span>
      <span className="territory-live">{c.live} live · {g.name}</span>
    </Link></li>;
  })}</ul>;
}
