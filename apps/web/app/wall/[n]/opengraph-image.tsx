import { ImageResponse } from "next/og";
import { apiOrigin } from "../../../lib/server-api";
import type { WallData } from "../../../lib/wall";

// The share card (docs/WALL.md): the whole Wall with this tile marked. Neutral theme colours.
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const alt = "A tile on the S.V.E.R Wall";

export default async function Image({ params }: { params: Promise<{ n: string }> }) {
  const n = Number((await params).n);
  let wall: WallData | null = null;
  try {
    const response = await fetch(`${apiOrigin()}/api/wall`, { cache: "no-store" });
    if (response.ok) wall = await response.json();
  } catch { /* the card still renders the empty Wall */ }
  const cell = 21, width = 40 * cell, height = 25 * cell;
  const tiles = wall?.tiles ?? [];
  const mine = tiles.find(t => t.n === n && t.state === "live");
  return new ImageResponse(
    <div style={{ display: "flex", width: "100%", height: "100%", background: "#0b0f17", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 20 }}>
      <div style={{ display: "flex", position: "relative", width, height, background: "#121826", border: "2px solid #2a3346" }}>
        {tiles.map(t => <div key={t.n} style={{ position: "absolute", left: t.x * cell, top: t.y * cell, width: t.side * cell, height: t.side * cell, background: t.state === "live" ? "#3a4560" : "#232b3d", border: "1px solid #0b0f17", display: "flex" }}>
          {t.state === "live" && t.image && <img src={t.image} width={t.side * cell - 2} height={t.side * cell - 2} alt="" />}
        </div>)}
        {mine && <div style={{ position: "absolute", left: mine.x * cell - 6, top: mine.y * cell - 6, width: mine.side * cell + 12, height: mine.side * cell + 12, border: "4px solid #c9d2e3", display: "flex" }} />}
      </div>
      <div style={{ width, display: "flex", justifyContent: "space-between", color: "#c9d2e3", fontSize: 24 }}>
        <span>{mine ? `${mine.name} · tile ${mine.n}` : "The S.V.E.R Wall"}</span><span>sver.tv/wall</span>
      </div>
    </div>,
    size,
  );
}
