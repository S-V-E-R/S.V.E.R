import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import { ReadinessBanner } from "../../components/Readiness";
import { StudioNav } from "../../components/StudioNav";
import "../../styles/profiles.css";

export const metadata = { title: "Creator Studio | S.V.E.R", robots: { index: false, follow: false } };
export default async function StudioLayout({ children }: { children: React.ReactNode }) {
  const account = await currentAccount();
  if (!account) redirect("/login");
  return <div className="settings-page">
    <StudioNav username={account.username} />
    <div className="settings-body"><ReadinessBanner />{children}</div>
  </div>;
}
