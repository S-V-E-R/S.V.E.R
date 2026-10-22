"use client";
import { useEffect, useState, type ReactNode } from "react";
import { redditIntent, shareQuery, shareUrl, xIntent } from "../lib/share";

/** Share / copy a page (docs/PROFILES.md P1, plus X, Reddit and a Discord paste).
 *  Touch devices with the Web Share API get the native sheet; everything else copies.
 *  `canonical` keeps the copied URL query-free (channel pages). X, Reddit and Discord
 *  still carry ?ref= when the sharer is signed in. */
export function ShareButton({ path, title, className = "small quiet", label = "Share", credit = null, canonical = false, params, networks = false }: {
  path: string; title: string; className?: string; label?: ReactNode; credit?: string | null; canonical?: boolean;
  params?: () => Record<string, string | undefined>; networks?: boolean;
}) {
  const [state, setState] = useState<"idle" | "copied" | "manual" | "discord">("idle");
  const [url, setUrl] = useState("");
  const [origin, setOrigin] = useState("https://sver.tv");
  useEffect(() => {
    // Origin exists only in the browser. Reading it during render would disagree with the server HTML.
    // eslint-disable-next-line react-hooks/set-state-in-effect -- sync the share links to this host after hydration
    setOrigin(window.location.origin);
  }, []);

  function query(primary: boolean) {
    return shareUrl(origin, path, shareQuery(credit, params?.() ?? {}), primary, canonical);
  }
  async function copy(link: string, which: "copied" | "discord") {
    setUrl(link);
    try {
      if (!navigator.clipboard) throw new Error("no clipboard");
      await navigator.clipboard.writeText(link);
      setState(which);
      setTimeout(() => setState(s => s === which ? "idle" : s), 2000);
    } catch {
      setState("manual");
    }
  }
  async function share() {
    const link = query(true);
    setUrl(link);
    const coarse = typeof window.matchMedia === "function" && window.matchMedia("(pointer: coarse)").matches;
    if (coarse && typeof navigator.share === "function") {
      try { await navigator.share({ title, url: link }); }
      catch (error) { if ((error as Error)?.name !== "AbortError") setState("manual"); }
      return;
    }
    await copy(link, "copied");
  }
  const credited = query(false);
  return <span className="share">
    <button type="button" className={className} onClick={() => void share()} aria-label={`Share ${title}`}>{state === "copied" ? "Link copied" : label}</button>
    {networks && <span className="share-networks">
      <a href={xIntent(credited, title)} target="_blank" rel="noopener noreferrer">X</a>
      <a href={redditIntent(credited, title)} target="_blank" rel="noopener noreferrer">Reddit</a>
      <button type="button" className="link-button" onClick={() => void copy(credited, "discord")}>{state === "discord" ? "Link copied" : "Discord"}</button>
    </span>}
    {state === "manual" && <label className="share-fallback"><span>Copy this link</span><input readOnly value={url} ref={el => { el?.focus(); el?.select(); }} onFocus={e => e.currentTarget.select()} /></label>}
    <span role="status" className="sr-only">{state === "copied" || state === "discord" ? "Link copied" : ""}</span>
  </span>;
}
