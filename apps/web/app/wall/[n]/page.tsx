import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { Crest } from "../../../components/Crest";
import { ReportButton } from "../../../components/Report";
import { ShareButton } from "../../../components/ShareButton";
import { currentAccount } from "../../session";
import { TileOwner } from "../../../components/WallTileOwner";
import { apiGet } from "../../../lib/server-api";
import { place, sizeLabel, tileLinkRel, type MyTile, type WallTile } from "../../../lib/wall";
import "../../../styles/wall.css";

type Page = { tile: WallTile; mine?: MyTile };
const number = (n: string) => /^\d{1,9}$/.test(n) ? n : null;

export async function generateMetadata({ params }: { params: Promise<{ n: string }> }): Promise<Metadata> {
  const n = number((await params).n);
  const { data } = n ? await apiGet<Page>(`/api/wall/${n}`) : { data: null };
  const title = data?.tile.state === "live" ? `${data.tile.name} on the Wall | S.V.E.R` : "The Wall | S.V.E.R";
  return { title, description: "One of 1,000 seats on the S.V.E.R Wall.", alternates: { canonical: `https://sver.tv/wall/${n}` }, robots: data?.tile.state === "live" ? undefined : { index: false } };
}

export default async function WallTilePage({ params, searchParams }: { params: Promise<{ n: string }>; searchParams: Promise<{ checkout?: string }> }) {
  const n = number((await params).n);
  if (!n) notFound();
  const [{ data }, account] = await Promise.all([apiGet<Page>(`/api/wall/${n}`), currentAccount()]);
  if (!data) notFound();
  const { tile, mine } = data;
  const paid = (await searchParams).checkout === "done";
  const live = tile.state === "live";
  return <div className="wall-page" data-theme="neutral">
    <p><Link href="/wall">← The Wall</Link></p>
    <article className="wall-tile-page frame">
      <div className="wall-tile-art" style={{ "--side": tile.side } as React.CSSProperties}>
        {/* eslint-disable-next-line @next/next/no-img-element -- media CDN image */}
        {live && tile.image_large ? <img src={tile.image_large} alt="" width={400} height={400} /> : <span className="wall-tile-blank review" aria-hidden="true" />}
        {live && tile.crest && <span className="wall-tile-crest"><Crest faction={tile.crest} initial="" size={40} label="Owner's faction" /></span>}
      </div>
      <div className="wall-tile-info">
        <span className="eyebrow">Tile {tile.n} · {sizeLabel[tile.size]}</span>
        <h1>{live ? tile.name : mine?.name ?? "Your tile"}</h1>
        {live && tile.message && <p className="wall-tile-message">{tile.message}</p>}
        {live && tile.link && <p><a href={tile.link} rel={tileLinkRel} target="_blank">{tile.link.replace(/^https:\/\//, "")}</a></p>}
        <dl className="wall-facts">
          {tile.owner && <><dt>Owner</dt><dd><Link href={`/${tile.owner}`}>@{tile.owner}</Link></dd></>}
          {tile.approved_at && <><dt>Claimed</dt><dd>{new Date(tile.approved_at).toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric", timeZone: "UTC" })}</dd></>}
          <dt>Place</dt><dd>{place(tile)}, {tile.side}×{tile.side} cells</dd>
        </dl>
        {live && <div className="row wrap"><ShareButton path={`/wall/${tile.n}`} title={`${tile.name} on the S.V.E.R Wall`} credit={account?.username ?? null} networks /><ReportButton target={{ target_type: "wall_tile", target_id: String(tile.n) }} label="Report this seat" /></div>}
      </div>
    </article>
    {paid && mine && <p role="status" className="panel section">Payment received. Your tile shows as &ldquo;taken · in review&rdquo; until staff approve it.</p>}
    {mine && <TileOwner tile={mine} />}
  </div>;
}
