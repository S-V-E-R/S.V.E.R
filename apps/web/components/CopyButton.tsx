"use client";
import { useState } from "react";

/** Copies a short value (a sponsor discount code) to the clipboard. */
export function CopyButton({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try { await navigator.clipboard.writeText(value); setCopied(true); setTimeout(() => setCopied(false), 2000); } catch { setCopied(false); }
  }
  return <button type="button" className="link-button" onClick={copy} aria-label={`Copy ${label}`}>{copied ? "Copied" : "Copy"}</button>;
}
