import { redirect } from "next/navigation";
import { Wallet } from "../../components/Wallet";
import { currentAccount } from "../session";
import "../../styles/profiles.css";

export const metadata = { title: "Valor | S.V.E.R", robots: { index: false, follow: false } };
export default async function WalletPage({ searchParams }: { searchParams: Promise<{ checkout?: string }> }) {
  if (!(await currentAccount())) redirect("/login");
  const returned = (await searchParams).checkout === "done";
  return <div className="settings-page single"><div className="settings-body">
    <h1>Valor</h1>
    <Wallet returned={returned} />
  </div></div>;
}
