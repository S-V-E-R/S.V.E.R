"use client";
import { useState } from "react";

/** Share / copy the canonical channel URL (docs/PROFILES.md, P1). Touch devices with the Web Share
 *  API get the native sheet; everything else copies, with a selectable field if copying fails. */
export function ShareButton({ username, displayName }: { username: string; displayName: string }) {
  const [state, setState] = useState<"idle" | "copied" | "manual">("idle");
  const [url, setUrl] = useState("");
  async function share() {
    const link = `${window.location.origin}/${username}`;
    setUrl(link);
    const coarse = typeof window.matchMedia === "function" && window.matchMedia("(pointer: coarse)").matches;
    if (coarse && typeof navigator.share === "function") {
      try { await navigator.share({ title: `${displayName} (@${username}) on S.V.E.R`, url: link }); }
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
    <button type="button" className="small quiet" onClick={share} aria-label={`Share @${username}`}>{state === "copied" ? "Link copied" : "Share"}</button>
    {state === "manual" && <label className="share-fallback"><span>Copy this link</span><input readOnly value={url} ref={el => { el?.focus(); el?.select(); }} onFocus={e => e.currentTarget.select()} /></label>}
    <span role="status" className="sr-only">{state === "copied" ? "Link copied" : ""}</span>
  </span>;
}
