"use client";
import { useEffect } from "react";
import { creditName } from "../lib/share";

const KEY = "sver-ref";

/** Remember ?ref= from a share link until enlist, in this tab only. No request, no cookie. */
export function ReferralCapture() {
  useEffect(() => {
    const name = creditName(new URLSearchParams(window.location.search).get("ref"));
    if (!name) return;
    try { sessionStorage.setItem(KEY, name); } catch { /* private mode */ }
  }, []);
  return null;
}

export function storedReferral(): string {
  try {
    const fromUrl = creditName(new URLSearchParams(window.location.search).get("ref"));
    if (fromUrl) return fromUrl;
    return creditName(sessionStorage.getItem(KEY)) ?? "";
  } catch {
    return "";
  }
}
