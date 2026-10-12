"use client";
import { useState, type ReactNode } from "react";

/** Share / copy a page's canonical URL (docs/PROFILES.md, P1): channels, clips, recordings, Beacons.
 *  Touch devices with the Web Share API get the native sheet; everything else copies (the button
 *  says "Link copied"), with a selectable field if copying fails. */
export function ShareButton({ path, title, className = "small quiet", label = "Share" }: { path: string; title: string; className?: string; label?: ReactNode }) {
  const [state, setState] = useState<"idle" | "copied" | "manual">("idle");
  const [url, setUrl] = useState("");
  async function share() {
    const link = `${window.location.origin}${path}`;
    setUrl(link);
    const coarse = typeof window.matchMedia === "function" && window.matchMedia("(pointer: coarse)").matches;
    if (coarse && typeof navigator.share === "function") {
      try { await navigator.share({ title, url: link }); }
      catch (error) { if ((error as Error)?.name !== "AbortError") setState("manual"); }
      return;
    }
    try {
      if (!navigator.clipboard) throw new Error("no clipboard");
      await navigator.clipboard.writeText(link);
      setState("copied");
      setTimeout(() => setState(s => s === "copied" ? "idle" : s), 2000);
    } catch {
      setState("manual");
    }
  }
  return <span className="share">
    <button type="button" className={className} onClick={share} aria-label={`Share ${title}`}>{state === "copied" ? "Link copied" : label}</button>
    {state === "manual" && <label className="share-fallback"><span>Copy this link</span><input readOnly value={url} ref={el => { el?.focus(); el?.select(); }} onFocus={e => e.currentTarget.select()} /></label>}
    <span role="status" className="sr-only">{state === "copied" ? "Link copied" : ""}</span>
  </span>;
}
