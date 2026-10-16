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
    <header className="page-head row between"><div><span className="eyebrow">Your account</span><h1>Notifications</h1><p className="lead">Go-lives from people you follow, replies, reports and account news.</p></div><Link className="button small quiet" href="/settings/notifications">Notification settings</Link></header>
    <section className="panel section"><NotificationList reports={alerts?.unread_reports ?? 0} strikes={alerts?.new_strikes ?? 0} /></section>
  </div></div>;
}
