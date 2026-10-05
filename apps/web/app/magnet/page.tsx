import { MagnetHype } from "../../components/MagnetHype";
import { currentAccount } from "../session";
import "../../styles/home.css";

export const metadata = { title: "MAGNet | S.V.E.R" };
export default async function GlobalHype() {
  const account = await currentAccount();
  return <div className="home"><h1>MAGNet</h1><p className="muted">MAGNet moves you to whichever stream is having a moment, and gives every stream its turn.</p>
    <MagnetHype lane="global" account={account?.username ?? null} viewerFaction={account?.faction ?? null} /></div>;
}
