"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";

export type Readiness = { steps: { key: string; label: string; done: boolean; href: string }[]; done: number; total: number; complete: boolean; dismissed: boolean };

/** One-line page readiness reminder on Studio channel pages (docs/PROFILES.md, P3). Owner-only by
 *  construction: it reads the signed-in user's own readiness. Hidden once complete or dismissed. */
export function ReadinessBanner() {
  const path = usePathname();
  const [data, setData] = useState<Readiness | null>(null);
  // Re-read on each Studio channel page so finished steps update the count.
  const load = useCallback(async () => {
    if (!path.startsWith("/studio/channel/")) return;
    const r = await send<Readiness>("GET", "/api/me/readiness");
    if (r.ok) setData(r.data);
  }, [path]);
  useLoad(load);
  if (!path.startsWith("/studio/channel/") || !data || data.complete || data.dismissed) return null;
  async function dismiss() {
    const r = await send<{ dismissed: boolean }>("PUT", "/api/me/readiness", { dismissed: true });
    if (r.ok && data) setData({ ...data, dismissed: true });
  }
  return <div className="panel readiness-banner" role="region" aria-label="Page readiness">
    <span>Page readiness: <strong>{data.done} of {data.total}</strong> done</span>
    <Link href="/studio/channel">Finish your page</Link>
    <button type="button" className="small quiet" onClick={dismiss}>Dismiss</button>
  </div>;
}
