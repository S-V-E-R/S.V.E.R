"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import type { Sizes } from "../lib/types";
import { Avatar } from "./Avatar";
import { Section } from "./Form";

type Item = { id: string; kind: "live"; created_at: string; read: boolean; live: boolean; channel: { username: string; display_name: string; avatar: Sizes } };
type Settings = { site: boolean; push: boolean; email: boolean; push_devices: number; push_key: string };

/** The notifications list (go-live alerts, last 30 days). Opening it marks everything read. */
export function NotificationList({ reports, strikes }: { reports: number; strikes: number }) {
  const [items, setItems] = useState<Item[] | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const result = await send<{ items: Item[] }>("GET", "/api/me/notifications");
    if (!result.ok) { setError(result.error); return; }
    setItems(result.data.items);
    if (result.data.items.some(item => !item.read)) await send("POST", "/api/me/notifications/read");
  }, []);
  useLoad(load);
  return <>
    {reports > 0 && <p className="panel inline-panel"><Link href="/settings/reports">Action was taken on {reports === 1 ? "a report" : `${reports} reports`} you made.</Link></p>}
    {strikes > 0 && <p className="panel inline-panel"><Link href="/settings/standing">There&apos;s a new notice about your account standing.</Link></p>}
    {error && <p role="alert" className="form-message error">{error}</p>}
    {!items ? <p className="loading">Loading…</p> : items.length === 0
      ? <p className="muted">No notifications yet. Channels you follow show up here when they go live.</p>
      : <ul className="list notifications">{items.map(item => <li key={item.id} className={item.read ? "row" : "row unread"}>
        <Avatar sizes={item.channel.avatar} name={item.channel.display_name} size={40} />
        <span><Link href={`/${item.channel.username}`}><strong>{item.channel.display_name}</strong> {item.live ? "is live" : "went live"}</Link>
          <br /><time className="muted" dateTime={item.created_at}>{new Date(item.created_at).toLocaleString()}</time></span>
        {!item.read && <span className="sr-only">New</span>}
      </li>)}</ul>}
  </>;
}

/** Subscribes this browser to push through the site's service worker. */
async function subscribe(publicKey: string) {
  if (!("serviceWorker" in navigator) || !("PushManager" in window)) throw new Error("This browser doesn't support push alerts. On iPhone or iPad, add S.V.E.R to your home screen first.");
  if (!publicKey) throw new Error("Push alerts aren't available yet.");
  if (await Notification.requestPermission() !== "granted") throw new Error("Allow notifications for S.V.E.R in your browser settings first.");
  const registration = await navigator.serviceWorker.register("/staff-push.js", { scope: "/" });
  await navigator.serviceWorker.ready;
  const key = Uint8Array.from(atob(publicKey.replaceAll("-", "+").replaceAll("_", "/")), char => char.charCodeAt(0));
  const subscription = await registration.pushManager.getSubscription() || await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
  const saved = await send("POST", "/api/me/push", subscription.toJSON());
  if (!saved.ok) throw new Error(saved.error);
}
async function currentEndpoint() {
  if (!("serviceWorker" in navigator)) return null;
  const registration = await navigator.serviceWorker.getRegistration("/");
  return (await registration?.pushManager.getSubscription())?.endpoint ?? null;
}

/** Settings → Notifications: the three delivery methods and push on this browser. */
export function NotificationSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [here, setHere] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const result = await send<Settings>("GET", "/api/me/notifications/settings");
    if (result.ok) setSettings(result.data); else setMessage(result.error);
    setHere(await currentEndpoint().catch(() => null));
  }, []);
  useLoad(load);
  async function save(change: Partial<Settings>) {
    if (!settings) return;
    const next = { ...settings, ...change };
    const result = await send<Settings>("PUT", "/api/me/notifications/settings", { site: next.site, push: next.push, email: next.email });
    if (result.ok) { setSettings(result.data); setMessage("Saved."); } else setMessage(result.error);
  }
  async function togglePush() {
    if (!settings) return;
    setBusy(true); setMessage("");
    try {
      if (here) {
        await send("DELETE", "/api/me/push", { endpoint: here });
        setMessage("Push alerts are off on this browser.");
      } else {
        await subscribe(settings.push_key);
        setMessage("Push alerts are on for this browser.");
      }
      await load();
    } catch (error) { setMessage(error instanceof Error ? error.message : "Push alerts couldn't be changed."); }
    setBusy(false);
  }
  if (!settings) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <>
    <Section title="Go-live alerts" intro="When a channel you follow goes live (at most once every 6 hours per channel). Turn alerts off for one channel with the bell next to Following on its page.">
      <label className="checkbox"><input type="checkbox" checked={settings.site} onChange={e => save({ site: e.target.checked })} /> In-site notifications (the bell at the top)</label>
      <label className="checkbox"><input type="checkbox" checked={settings.push} onChange={e => save({ push: e.target.checked })} /> Browser push notifications</label>
      <label className="checkbox"><input type="checkbox" checked={settings.email} onChange={e => save({ email: e.target.checked })} /> Email (every email has a one-click unsubscribe)</label>
    </Section>
    <Section title="This browser" intro={`Push alerts are on for ${settings.push_devices} browser${settings.push_devices === 1 ? "" : "s"} on your account.`}>
      <button type="button" className="small" disabled={busy || (!here && !settings.push)} onClick={togglePush}>{here ? "Turn off push on this browser" : "Turn on push on this browser"}</button>
    </Section>
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}

/** The bell next to Following: go-live alerts for this one channel, without unfollowing. */
export function AlertBell({ username, initial }: { username: string; initial: boolean }) {
  const [on, setOn] = useState(initial);
  const [error, setError] = useState("");
  async function toggle() {
    const result = await send<{ alerts: boolean }>("PATCH", `/api/channels/${encodeURIComponent(username)}/follow`, { alerts: !on });
    if (result.ok) { setOn(result.data.alerts); setError(""); } else setError(result.error);
  }
  return <>
    <button type="button" className="small quiet" aria-pressed={on} aria-label={on ? `Go-live alerts on for @${username}` : `Go-live alerts off for @${username}`} title={on ? "Turn off go-live alerts" : "Turn on go-live alerts"} onClick={toggle}>{on ? "🔔" : "🔕"}</button>
    {error && <p role="alert" className="form-message">{error}</p>}
  </>;
}

/** The page an email's unsubscribe link opens; the token is in the link. */
export function Unsubscribe() {
  const [state, setState] = useState<"ready" | "busy" | "done" | string>("ready");
  async function confirm() {
    setState("busy");
    const token = new URLSearchParams(window.location.search).get("token") ?? "";
    const result = await send("POST", `/api/notifications/unsubscribe?token=${encodeURIComponent(token)}`);
    setState(result.ok ? "done" : result.error);
  }
  if (state === "done") return <p role="status">You won&apos;t get go-live emails anymore. You can turn them back on in <Link href="/settings/notifications">notification settings</Link>.</p>;
  return <>
    <p>Stop all go-live emails from S.V.E.R? In-site and push alerts aren&apos;t affected.</p>
    <button type="button" disabled={state === "busy"} onClick={confirm}>Stop go-live emails</button>
    {state !== "ready" && state !== "busy" && <p role="alert" className="form-message error">{state}</p>}
  </>;
}
