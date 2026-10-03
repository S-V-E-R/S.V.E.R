"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { parseRoadmap, type Roadmap } from "./data";

export default function RoadmapProgress({ initial }: { initial: Roadmap | null }) {
  const [roadmap, setRoadmap] = useState(initial);
  const [unavailable, setUnavailable] = useState(!initial);

  useEffect(() => {
    let stopped = false;
    let pending = false;
    let request: AbortController | null = null;
    async function refresh() {
      if (document.hidden || pending) return;
      pending = true;
      request = new AbortController();
      const timeout = setTimeout(() => request?.abort(), 8000);
      try {
        const response = await fetch("/api/roadmap", { cache: "no-store", credentials: "omit", signal: request.signal });
        if (!response.ok) throw new Error("Roadmap unavailable");
        const next = parseRoadmap(await response.json());
        if (!stopped) {
          setRoadmap(previous => previous?.revision === next.revision ? previous : next);
          setUnavailable(false);
        }
      } catch {
        if (!stopped) setUnavailable(true);
      } finally {
        clearTimeout(timeout);
        pending = false;
      }
    }
    void refresh();
    const interval = setInterval(() => void refresh(), 30000);
    const onVisible = () => void refresh();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      stopped = true;
      clearInterval(interval);
      request?.abort();
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);

  return <section className="roadmap-core" aria-labelledby="core-title">
    <h2 id="core-title">The core platform</h2>
    <p>We verify each module against its working requirements before calling it done. The first five modules make the platform functional; VODs, clips and Beacons complete the core.</p>
    <p className="site-updated" role="status">{unavailable
      ? roadmap ? "Updates are temporarily unavailable. Showing the last received progress; we’ll keep trying." : "Progress is temporarily unavailable. We’ll keep trying to reconnect."
      : "Live roadmap · Checks for updates every 30 seconds."}</p>
    {roadmap && <>
      <p><strong>{roadmap.items.filter(item => item.status === "Done").length} of {roadmap.items.length} milestones complete</strong></p>
      <ol className="roadmap-track" start={0}>
        {roadmap.items.map(item => <li key={item.id} id={item.id} className={`roadmap-item ${item.status === "Done" ? "is-done" : item.status === "In progress" ? "is-progress" : item.status === "Started" ? "is-started" : "is-planned"}`}>
          <div className="roadmap-heading"><div><span className="eyebrow">Module {String(item.number).padStart(2, "0")}</span><h3>{item.name}</h3></div><span className="roadmap-status">{item.status}</span></div>
          <p>{item.detail}</p>
          {item.id === "factions" && <Link href="/factions">Meet the three factions</Link>}
        </li>)}
      </ol>
    </>}
  </section>;
}
