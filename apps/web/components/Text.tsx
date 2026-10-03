"use client";
import { useState, useSyncExternalStore } from "react";

/** Plain user text with https/http URLs turned into nofollow links (docs/PROFILES.md, The Wall: "URLs render as links"). */
export function Linkified({ text }: { text: string }) {
  const parts: React.ReactNode[] = [];
  const pattern = /https?:\/\/[^\s<>"']+/gi;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    const url = match[0].replace(/[.,!?;:)\]]+$/, "");
    const start = match.index ?? 0;
    if (start > last) parts.push(text.slice(last, start));
    parts.push(<a key={start} href={url} rel="nofollow noopener noreferrer ugc" target="_blank">{url}</a>);
    last = start + url.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return <>{parts}</>;
}

const units: [Intl.RelativeTimeFormatUnit, number][] = [["year", 31536000], ["month", 2592000], ["week", 604800], ["day", 86400], ["hour", 3600], ["minute", 60]];
const noSubscribe = () => () => {};
/** Relative time ("5 minutes ago") with the exact time in the viewer's zone as a tooltip. The server render (and the
 *  first client render, so hydration matches) shows the UTC date; the browser then switches to relative and local time. */
export function Ago({ iso, className }: { iso: string; className?: string }) {
  const browser = useSyncExternalStore(noSubscribe, () => true, () => false);
  const [now] = useState(() => Date.now());
  const date = new Date(iso);
  if (!browser) {
    const utc = date.toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short", timeZone: "UTC" });
    return <time className={className} dateTime={iso} title={`${utc} UTC`}>{date.toLocaleDateString("en-US", { dateStyle: "medium", timeZone: "UTC" })}</time>;
  }
  const seconds = Math.round((date.getTime() - now) / 1000);
  const [unit, size] = units.find(([, s]) => Math.abs(seconds) >= s) ?? ["second", 1];
  const label = Math.abs(seconds) < 60 ? "just now" : new Intl.RelativeTimeFormat("en-US", { numeric: "auto" }).format(Math.round(seconds / size), unit);
  const exact = date.toLocaleString("en-US", { month: "short", day: "numeric", year: "numeric", hour: "numeric", minute: "2-digit", timeZoneName: "short" });
  return <time className={className} dateTime={iso} title={exact}>{label}</time>;
}
