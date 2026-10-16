import { redirect } from "next/navigation";
import { Wallet } from "../../components/Wallet";
import { MySubscriptions } from "../../components/MySubscriptions";
import { SupporterWallet } from "../../components/SupporterWallet";
import { AdValorWallet } from "../../components/Ads";
import { currentAccount } from "../session";
import "../../styles/profiles.css";
import "../../styles/supporters.css";

export const metadata = { title: "Valor | S.V.E.R", robots: { index: false, follow: false } };
export default async function WalletPage({ searchParams }: { searchParams: Promise<{ checkout?: string }> }) {
  if (!(await currentAccount())) redirect("/login");
  const returned = (await searchParams).checkout === "done";
  return <div className="settings-page single"><div className="settings-body">
    <header className="page-head"><span className="eyebrow">Your account</span><h1>Wallet</h1><p className="lead">Your Valor, the channels you subscribe to and your support for S.V.E.R.</p></header>
    <Wallet returned={returned} />
    <MySubscriptions />
    <SupporterWallet />
    <AdValorWallet />
  </div></div>;
}
