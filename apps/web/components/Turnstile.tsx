"use client";
import Script from "next/script";
import { useEffect, useRef, useState } from "react";

declare global { interface Window { turnstile?: { render(el: HTMLElement, options: Record<string, unknown>): string; remove(id: string): void } } }

/** `size="compact"` (150×140) fits narrow places such as a co-stream tile. */
export function Turnstile({ sitekey, action, onToken, size = "normal" }: { sitekey: string; action: string; onToken: (token: string) => void; size?: "normal" | "compact" }) {
  const root = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    if (!ready || !window.turnstile || !root.current) return;
    const id = window.turnstile.render(root.current, { sitekey, action, size, theme: "dark", callback: onToken, "expired-callback": () => onToken(""), "error-callback": () => onToken("") });
    return () => window.turnstile?.remove(id);
  }, [ready, sitekey, action, onToken, size]);
  return <><Script src="https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit" onReady={() => setReady(true)} /><div ref={root} className="turnstile" /></>;
}
