"use client";
import type { ReactNode } from "react";

/** Field-level error/status line used by every settings and Studio section. */
export function Status({ state }: { state: { error?: string; field?: string; saved?: string } }) {
  if (state.error) return <p role="alert" className="form-message error">{state.error}</p>;
  if (state.saved) return <p role="status" className="form-message">{state.saved}</p>;
  return null;
}
export function Section({ title, intro, children }: { title: string; intro?: string; children: ReactNode }) {
  return <section className="panel section"><h2>{title}</h2>{intro && <p className="intro">{intro}</p>}{children}</section>;
}
export type SaveState = { error?: string; field?: string; saved?: string };
export const STALE = "This changed in another tab. Reload to see the latest.";
