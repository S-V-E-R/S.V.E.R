"use client";
import type { Occurrence } from "../lib/types";

/** Occurrences shown in the viewer's own zone, with the channel's zone noted. */
export function Occurrences({ items, zone }: { items: Occurrence[]; zone: string | null }) {
  const day = (iso: string) => new Date(iso).toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" });
  const time = (iso: string) => new Date(iso).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  return <>
    <ul className="occurrences">{items.map(o => <li key={`${o.kind}-${o.start_at}`}><strong suppressHydrationWarning>{day(o.start_at)}</strong><span suppressHydrationWarning>{time(o.start_at)} – {time(o.end_at)}</span><span>{o.label || (o.kind === "event" ? "Event" : "Stream")}</span>{o.kind === "event" && <span className="badge">Event</span>}</li>)}</ul>
    {zone && <p className="muted small-print">Times shown in your time zone. Channel time zone: {zone.replaceAll("_", " ")}.</p>}
  </>;
}
