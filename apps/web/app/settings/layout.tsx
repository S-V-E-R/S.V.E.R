import { SideMenu, type MenuGroup } from "../../components/SideMenu";
import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import "../../styles/profiles.css";

const MENU: MenuGroup[] = [[null, [["/settings/profile", "Your page"], ["/settings/notifications", "Notifications"], ["/settings/preferences", "Preferences"], ["/settings/blocked", "Blocked users"], ["/settings/reports", "My reports"], ["/settings/standing", "Account standing"], ["/settings/connections", "Connected apps"], ["/settings/developer", "Developer"], ["/account", "Account security"]]]];

export const metadata = { title: "Settings | S.V.E.R", robots: { index: false, follow: false } };
export default async function SettingsLayout({ children }: { children: React.ReactNode }) {
  if (!(await currentAccount())) redirect("/login");
  return <div className="settings-page">
    <SideMenu label="Settings" groups={MENU} />
    <div className="settings-body">{children}</div>
  </div>;
}
