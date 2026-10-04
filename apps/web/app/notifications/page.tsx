import { redirect } from "next/navigation";
import Link from "next/link";
import { NotificationList } from "../../components/Notifications";
import { apiGet } from "../../lib/server-api";
import { currentAccount } from "../session";
import "../../styles/profiles.css";

export const metadata = { title: "Notifications | S.V.E.R", robots: { index: false, follow: false } };
export default async function Notifications() {
  if (!(await currentAccount())) redirect("/login");
  const alerts = (await apiGet<{ unread_reports?: number; new_strikes?: number }>("/api/me/alerts")).data;
  return <div className="settings-page single"><div className="settings-body">
    <div className="row between"><h1>Notifications</h1><Link href="/settings/notifications">Settings</Link></div>
    <section className="panel section"><NotificationList reports={alerts?.unread_reports ?? 0} strikes={alerts?.new_strikes ?? 0} /></section>
  </div></div>;
}
