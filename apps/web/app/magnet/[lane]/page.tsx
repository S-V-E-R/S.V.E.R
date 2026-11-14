import { MagnetHype } from "../../../components/MagnetHype";
import { currentAccount } from "../../session";
import "../../../styles/home.css";

import type { Metadata } from "next";
import { publicMeta } from "../../../lib/seo";
import { isFaction, factionInfo } from "../../../lib/factions";

export async function generateMetadata({ params }: { params: Promise<{ lane: string }> }): Promise<Metadata> {
  const { lane } = await params;
  const name = isFaction(lane) ? factionInfo(lane).name : "MAGNet";
  return publicMeta(`${name} MAGNet | S.V.E.R`, `The ${name} MAGNet lane gives every stream in it a turn, whether it has 3 viewers or 3,000.`, `/magnet/${encodeURIComponent(lane)}`);
}
export default async function LaneHype({ params }: { params: Promise<{ lane: string }> }) {
  const { lane } = await params;
  const account = await currentAccount();
  return <div className="home"><header className="page-head"><span className="eyebrow">MAGNet</span><h1>Every stream gets a turn</h1><p className="lead">Each lane runs its own rotation: a side&apos;s streams, or one genre&apos;s.</p></header>
    <MagnetHype lane={lane} account={account?.username ?? null} viewerFaction={account?.faction ?? null} /></div>;
}
