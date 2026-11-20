"use client";
import { useEffect, useState } from "react";
import Link from "next/link";
import { send } from "../../lib/client-api";
import { BellIcon, MessageIcon } from "./Icons";

type Alerts = { unread_reports?: number; new_strikes?: number; notifications?: number };

/** The header's Messages and Notifications buttons. The layout renders once, so the dots refresh on
 *  focus, every minute while the tab is visible, and when the Messages or Notifications page reads
 *  something (`sver:dms-read`, `sver:alerts-read`). */
export function HeaderAlerts({ dms: initialDms, alerts: initialAlerts }: { dms: number; alerts: boolean }) {
  const [dms, setDms] = useState(initialDms);
  const [alerts, setAlerts] = useState(initialAlerts);
  useEffect(() => {
    const refresh = async () => {
      if (document.hidden) return;
      const [unread, me] = await Promise.all([send<{ unread: number }>("GET", "/api/dms/unread"), send<Alerts>("GET", "/api/me/alerts")]);
      if (unread.ok) setDms(unread.data.unread);
      if (me.ok) setAlerts(Number(me.data.unread_reports) > 0 || Number(me.data.new_strikes) > 0 || Number(me.data.notifications) > 0);
    };
    const update = () => void refresh();
    const timer = setInterval(update, 60000);
    const events = ["focus", "sver:dms-read", "sver:alerts-read"];
    for (const name of events) window.addEventListener(name, update);
    document.addEventListener("visibilitychange", update);
    return () => {
      clearInterval(timer);
      for (const name of events) window.removeEventListener(name, update);
      document.removeEventListener("visibilitychange", update);
    };
  }, []);
  return <>
    <Link href="/messages" className="icon-button" aria-label={dms ? `Messages, ${dms} unread` : "Messages"}><MessageIcon />{dms > 0 && <span className="alert-badge" aria-hidden="true" />}</Link>
    <Link href="/notifications" className="icon-button" aria-label={alerts ? "Notifications, new notices" : "Notifications"}><BellIcon />{alerts && <span className="alert-badge" aria-hidden="true" />}</Link>
  </>;
}
