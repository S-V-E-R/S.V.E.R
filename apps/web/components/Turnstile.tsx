"use client";
import Script from "next/script";
import { useEffect, useRef, useState } from "react";

declare global { interface Window { turnstile?: { render(el: HTMLElement, options: Record<string, unknown>): string; remove(id: string): void } } }

export function Turnstile({ sitekey, action, onToken }: { sitekey: string; action: string; onToken: (token: string) => void }) {
  const root = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    if (!ready || !window.turnstile || !root.current) return;
    const id = window.turnstile.render(root.current, { sitekey, action, theme: "dark", callback: onToken, "expired-callback": () => onToken(""), "error-callback": () => onToken("") });
    return () => window.turnstile?.remove(id);
  }, [ready, sitekey, action, onToken]);
  return <><Script src="https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit" onReady={() => setReady(true)} /><div ref={root} className="turnstile" /></>;
}
