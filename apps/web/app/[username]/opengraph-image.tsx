import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { ImageResponse } from "next/og";
import sharp from "sharp";
import { apiGet } from "../../lib/server-api";
import { factionInfo } from "../../lib/factions";
import type { Channel } from "../../lib/types";

// The channel's share card: what X, Discord, Bluesky and messages show for a sver.tv/{name} link.
// It sells the streamer first and S.V.E.R second, in the owner's faction theme (docs/DESIGN.md
// colour table; neutral steel without a faction).
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const alt = "A channel on S.V.E.R";

const THEMES = {
  neutral: { accent: "#C9CED6", light: "#ECEEF2", deep: "#12151C" },
  myria: { accent: "#FF9A1F", light: "#FFC56E", deep: "#2A1604" },
  aetheron: { accent: "#A68BFF", light: "#D3C6FF", deep: "#1C1038" },
  glint: { accent: "#E9C35A", light: "#F8E6A8", deep: "#0A1A3C" },
};
const font = (file: string) => readFile(join(process.cwd(), "assets/fonts", file));
const clip = (text: string, max: number) => text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
/** Media is WebP, which the card renderer can't draw: re-encode it as a PNG data URL (null on any failure, so the initial shows). */
async function png(url: string | undefined) {
  if (!url) return null;
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(5000) });
    if (!response.ok) return null;
    const image = await sharp(Buffer.from(await response.arrayBuffer())).png().toBuffer();
    return `data:image/png;base64,${image.toString("base64")}`;
  } catch { return null; }
}

export default async function Image({ params }: { params: Promise<{ username: string }> }) {
  const { username } = await params;
  const [{ status, data }, cinzel, barlow, barlowBold] = await Promise.all([
    apiGet<Channel>(`/api/channels/${encodeURIComponent(username)}`).catch(() => ({ status: 0, data: null })),
    font("Cinzel-Bold.ttf"), font("Barlow-Regular.ttf"), font("Barlow-SemiBold.ttf"),
  ]);
  const c = status === 200 ? data?.channel : undefined;
  const t = THEMES[c?.faction ?? "neutral"];
  const faction = c?.faction ? factionInfo(c.faction) : null;
  const avatar = await png(c?.avatar?.["400"]);
  const bio = c?.bio?.trim() ? clip(c.bio.trim().replace(/\s+/g, " "), 150) : "Live on S.V.E.R. Come find your people.";
  // Corner brackets (docs/DESIGN.md "Framed panel"), drawn as two borders on a square.
  const corner = (top: boolean, left: boolean) => <div style={{ position: "absolute", [top ? "top" : "bottom"]: 28, [left ? "left" : "right"]: 28, width: 44, height: 44, display: "flex", borderColor: t.accent, borderStyle: "solid", borderWidth: 0, [top ? "borderTopWidth" : "borderBottomWidth"]: 4, [left ? "borderLeftWidth" : "borderRightWidth"]: 4 }} />;
  return new ImageResponse(
    <div style={{ display: "flex", width: "100%", height: "100%", position: "relative", background: `linear-gradient(135deg, ${t.deep} 0%, #07080B 70%)`, color: "#F2F0EA", fontFamily: "Barlow" }}>
      {corner(true, true)}{corner(true, false)}{corner(false, true)}{corner(false, false)}
      <div style={{ position: "absolute", top: 56, right: 72, display: "flex", fontFamily: "Cinzel", fontSize: 34, letterSpacing: 4, color: t.light }}>S.V.E.R</div>
      {c ? <div style={{ display: "flex", alignItems: "center", gap: 56, padding: "0 96px", width: "100%" }}>
        <div style={{ display: "flex", flexShrink: 0, width: 280, height: 280, borderRadius: 140, border: `6px solid ${t.accent}`, padding: 8, background: "#07080B" }}>
          {avatar
            ? <img src={avatar} width={252} height={252} style={{ borderRadius: 126, objectFit: "cover" }} alt="" />
            : <div style={{ display: "flex", width: 252, height: 252, borderRadius: 126, background: t.deep, alignItems: "center", justifyContent: "center", fontFamily: "Cinzel", fontSize: 120, color: t.light }}>{c.display_name.slice(0, 1).toUpperCase()}</div>}
        </div>
        <div style={{ display: "flex", flexDirection: "column", width: 676 }}>
          {c.live && <div style={{ display: "flex", alignSelf: "flex-start", background: "#E5262E", color: "#fff", fontWeight: 600, fontSize: 24, padding: "4px 14px", marginBottom: 14 }}>{c.streaming_category ? `LIVE · ${clip(c.streaming_category, 32)}` : "LIVE"}</div>}
          <div style={{ display: "flex", fontFamily: "Cinzel", fontSize: c.display_name.length > 16 ? 56 : 72, lineHeight: 1.1, color: t.light }}>{clip(c.display_name, 28)}</div>
          <div style={{ display: "flex", fontWeight: 600, fontSize: 30, color: t.accent, marginTop: 6 }}>@{c.username}</div>
          <div style={{ display: "flex", fontSize: 28, lineHeight: 1.35, color: "#A3A6AE", marginTop: 20, maxHeight: 114, overflow: "hidden" }}>{bio}</div>
          <div style={{ display: "flex", alignItems: "center", gap: 18, marginTop: 28, fontSize: 24, color: "#8E929B" }}>
            {faction && <span style={{ color: t.accent, fontWeight: 600 }}>{faction.name} · {faction.title}</span>}
            <span>{c.follower_count.toLocaleString("en-US")} {c.follower_count === 1 ? "follower" : "followers"}</span>
          </div>
        </div>
      </div> : <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", padding: "0 96px" }}>
        <div style={{ display: "flex", fontFamily: "Cinzel", fontSize: 72, color: t.light }}>Find your people.</div>
        <div style={{ display: "flex", fontFamily: "Cinzel", fontSize: 72, color: t.accent }}>Forge your legacy.</div>
      </div>}
      <div style={{ position: "absolute", bottom: 52, left: 96, right: 96, display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 24 }}>
        <span style={{ color: "#8E929B" }}>Live streams for people who play, build and make</span>
        <span style={{ display: "flex", fontWeight: 600, color: "#0A0A0C", background: t.accent, padding: "6px 16px" }}>{c ? `sver.tv/${c.username}` : "sver.tv"}</span>
      </div>
    </div>,
    {
      ...size,
      fonts: [
        { name: "Cinzel", data: cinzel, weight: 700 },
        { name: "Barlow", data: barlow, weight: 400 },
        { name: "Barlow", data: barlowBold, weight: 600 },
      ],
    },
  );
}
