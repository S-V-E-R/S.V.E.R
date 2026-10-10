"use client";
import { useEffect, useState } from "react";

export type Banner = { message: string; ends_at: string | null; updated_at: string };
const KEY = "sver-banner-dismissed";

/** Staff's site-wide message (docs/ADMIN.md "Site banner"). Dismissing hides this version of it. */
export function SiteBanner({ banner }: { banner: Banner }) {
  const [dismissed, setDismissed] = useState(true);
  useEffect(() => {
    let seen: string | null = null;
    try { seen = localStorage.getItem(KEY); } catch { /* storage blocked: show it */ }
    setDismissed(seen === banner.updated_at);
  }, [banner.updated_at]);
  if (dismissed) return null;
  function dismiss() {
    try { localStorage.setItem(KEY, banner.updated_at); } catch { /* storage blocked: hide for this page */ }
    setDismissed(true);
  }
  return <div className="site-banner" role="status">
    <p>{banner.message}</p>
    <button type="button" className="quiet small" onClick={dismiss}>Dismiss<span className="sr-only"> this notice</span></button>
  </div>;
}
