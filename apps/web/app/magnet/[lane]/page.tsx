import { MagnetHype } from "../../../components/MagnetHype";
import { currentAccount } from "../../session";
import "../../../styles/home.css";

export const metadata = { title: "MAGNet | S.V.E.R" };
export default async function LaneHype({ params }: { params: Promise<{ lane: string }> }) {
  const { lane } = await params;
  const account = await currentAccount();
  return <div className="home"><header className="page-head"><span className="eyebrow">MAGNet</span><h1>Every stream gets a turn</h1><p className="lead">Each lane runs its own rotation: a side&apos;s streams, or one genre&apos;s.</p></header>
    <MagnetHype lane={lane} account={account?.username ?? null} viewerFaction={account?.faction ?? null} /></div>;
}
