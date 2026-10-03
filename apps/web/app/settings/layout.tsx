import Link from "next/link";
import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import "../../styles/profiles.css";

export const metadata = { title: "Settings | S.V.E.R", robots: { index: false, follow: false } };
export default async function SettingsLayout({ children }: { children: React.ReactNode }) {
  if (!(await currentAccount())) redirect("/login");
  return <div className="settings-page">
    <nav className="settings-nav" aria-label="Settings"><Link href="/settings/profile">Profile</Link><Link href="/settings/blocked">Blocked users</Link><Link href="/settings/reports">My reports</Link><Link href="/settings/standing">Account standing</Link><Link href="/account">Account security</Link></nav>
    <div className="settings-body">{children}</div>
  </div>;
}
