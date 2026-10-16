import { MagnetHype } from "../../components/MagnetHype";
import { currentAccount } from "../session";
import "../../styles/home.css";

export const metadata = { title: "MAGNet | S.V.E.R" };
export default async function GlobalHype() {
  const account = await currentAccount();
  return <div className="home"><header className="page-head"><span className="eyebrow">MAGNet</span><h1>Every stream gets a turn</h1><p className="lead">MAGNet moves you to whichever stream is having a moment, and gives every stream its turn: 3 viewers or 3,000.</p></header>
    <MagnetHype lane="global" account={account?.username ?? null} viewerFaction={account?.faction ?? null} /></div>;
}
