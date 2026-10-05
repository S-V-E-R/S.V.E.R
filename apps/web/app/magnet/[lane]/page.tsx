import { MagnetHype } from "../../../components/MagnetHype";
import { currentAccount } from "../../session";
import "../../../styles/home.css";

export const metadata = { title: "MAGNet | S.V.E.R" };
export default async function LaneHype({ params }: { params: Promise<{ lane: string }> }) {
  const { lane } = await params;
  const account = await currentAccount();
  return <div className="home"><h1>MAGNet</h1><p className="muted">One lane per genre, each with its own rotation.</p>
    <MagnetHype lane={lane} account={account?.username ?? null} viewerFaction={account?.faction ?? null} /></div>;
}
