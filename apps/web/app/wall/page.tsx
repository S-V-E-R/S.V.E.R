import type { Metadata } from "next";
import Link from "next/link";
import WallSeats from "../../components/WallSeats";
import { apiGet } from "../../lib/server-api";
import { stageName, type WallData } from "../../lib/wall";
import "../../styles/wall.css";

export const metadata: Metadata = {
  title: "The Wall | S.V.E.R",
  description: "1,000 seats on one wall, bought once to help build S.V.E.R. A seat buys this tile and nothing else.",
  alternates: { canonical: "https://sver.tv/wall" },
};

// The Wall (docs/WALL.md). Neutral theme, one framed header, no mockup: built from docs/DESIGN.md.
export default async function WallPage() {
  const { data } = await apiGet<WallData>("/api/wall");
  return <div className="wall-page" data-theme="neutral">
    <header className="wall-header frame">
      <span className="eyebrow">The Wall</span>
      <h1>Coin can&apos;t buy the light. It can help build the hall.</h1>
      <p>One thousand seats, each bought once and kept for the life of the site. Everyone who takes one is helping build S.V.E.R.</p>
      <p><strong>A seat buys its tile on this page and nothing else.</strong> No placement anywhere else on S.V.E.R, no ranking, no faction points, no badges and no chat perks.</p>
      <p>Rather support S.V.E.R every month? <Link href="/supporters">Become a supporter</Link>.</p>
      {data ? <p className="wall-count">{data.full ? "The Wall is full." : <>{data.taken.toLocaleString("en-US")} of 1,000 seats taken · {stageName(data.stage)}{data.next_stage_at != null && <> · prices rise at {data.next_stage_at}</>}</>}{!data.sales_open && !data.full && " · seats aren't on sale yet"}</p>
        : <p role="alert">The Wall couldn&apos;t be loaded. Please try again shortly.</p>}
    </header>
    {data && <WallSeats initial={data} />}
  </div>;
}
