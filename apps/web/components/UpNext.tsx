"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { send } from "../lib/client-api";
import type { LiveCard } from "./home/types";

/** Shown when the stream being watched ends: the next live stream in MAGNet's order, after 10 seconds unless cancelled. */
export function UpNext({ username, focused }: { username: string; focused: boolean }) {
  const [next, setNext] = useState<LiveCard | null>(null);
  const [left, setLeft] = useState(10);
  const [cancelled, setCancelled] = useState(false);
  useEffect(() => {
    void send<{ items: LiveCard[] }>("GET", `/api/channels/${encodeURIComponent(username)}/suggestions`).then(r => { if (r.ok) setNext(r.data.items[0] ?? null); });
  }, [username]);
  useEffect(() => {
    if (!next || cancelled) return;
    if (left <= 0) {
      // A full load gives the next channel a fresh player and chat.
      // eslint-disable-next-line @next/next/no-location-assign-relative-destination
      window.location.assign(`/${next.username}${focused ? "/live" : ""}`);
      return;
    }
    const timer = setTimeout(() => setLeft(n => n - 1), 1000);
    return () => clearTimeout(timer);
  }, [next, cancelled, left, focused]);
  if (!next) return null;
  return <div className="up-next panel" role="status">
    <p>The stream ended. {cancelled ? "Up next:" : <>Up next in <strong>{left}</strong>:</>} <Link href={`/${next.username}${focused ? "/live" : ""}`}><strong>{next.display_name}</strong></Link> <span className="muted">· {next.title}</span></p>
    {!cancelled && <button type="button" className="small quiet" onClick={() => setCancelled(true)}>Cancel</button>}
  </div>;
}
