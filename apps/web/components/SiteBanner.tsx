"use client";
import { useState, useSyncExternalStore } from "react";

export type Banner = { message: string; ends_at: string | null; updated_at: string };
const KEY = "sver-banner-dismissed";
const quiet = () => () => {};
function stored(): string | null {
  try { return localStorage.getItem(KEY); } catch { return null; }
}

/** Staff's site-wide message (docs/ADMIN.md "Site banner"). Dismissing hides this version of it. */
export function SiteBanner({ banner }: { banner: Banner }) {
  // On the server (and before hydration) it counts as dismissed, so a dismissed banner never flashes.
  const seen = useSyncExternalStore(quiet, stored, () => banner.updated_at);
  const [closed, setClosed] = useState(false);
  if (closed || seen === banner.updated_at) return null;
  function dismiss() {
    try { localStorage.setItem(KEY, banner.updated_at); } catch { /* storage blocked: hide for this page */ }
    setClosed(true);
  }
  return <div className="site-banner" role="status">
    <p>{banner.message}</p>
    <button type="button" className="quiet small" onClick={dismiss}>Dismiss<span className="sr-only"> this notice</span></button>
  </div>;
}
