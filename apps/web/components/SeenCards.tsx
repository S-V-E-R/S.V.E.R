"use client";
import { useEffect } from "react";
import { send } from "../lib/client-api";

/** Counts live cards (`data-broadcast`) that are on screen for a second, once per card per page
 *  view, for streamers' analytics (docs/ANALYTICS.md "Impressions"). Renders nothing. */
export function SeenCards({ surface }: { surface: "home" | "browse" | "search" | "following" | "faction" | "magnet" | "suggestion" }) {
  useEffect(() => {
    if (typeof IntersectionObserver === "undefined") return;
    const counted = new Set<string>();
    const timers = new Map<Element, ReturnType<typeof setTimeout>>();
    let batch: string[] = [];
    const observer = new IntersectionObserver(entries => entries.forEach(entry => {
      const id = (entry.target as HTMLElement).dataset.broadcast;
      if (!id || counted.has(id)) return;
      if (!entry.isIntersecting) { clearTimeout(timers.get(entry.target)); timers.delete(entry.target); return; }
      if (!timers.has(entry.target)) timers.set(entry.target, setTimeout(() => { counted.add(id); batch.push(id); }, 1000));
    }), { threshold: 0.5 });
    const watched = new WeakSet<Element>();
    const scan = () => document.querySelectorAll("[data-broadcast]").forEach(card => { if (!watched.has(card)) { watched.add(card); observer.observe(card); } });
    scan();
    const timer = setInterval(() => {
      scan();
      if (batch.length === 0) return;
      const broadcasts = batch.slice(0, 48);
      batch = batch.slice(48);
      void send("POST", "/api/discovery/seen", { surface, broadcasts });
    }, 3000);
    return () => { clearInterval(timer); observer.disconnect(); timers.forEach(clearTimeout); };
  }, [surface]);
  return null;
}
