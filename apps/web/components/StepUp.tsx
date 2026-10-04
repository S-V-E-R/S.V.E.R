"use client";
import Link from "next/link";
import { FormEvent, useState } from "react";
import { send } from "../lib/client-api";

/** True when an API error means "sign in again before this staff or security action". */
export const needsStepUp = (result: { ok: boolean; status?: number; error?: string }) =>
  !result.ok && result.status === 403 && /confirm your sign-in/i.test(result.error ?? "");

/**
 * Inline re-confirmation for staff actions, which need a sign-in within the last 5 minutes.
 * Confirms the password, then runs `onConfirmed` (usually the action that was refused).
 */
export function StepUp({ onConfirmed }: { onConfirmed: () => void }) {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function confirm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true); setError("");
    const password = new FormData(event.currentTarget).get("password");
    const result = await send("POST", "/api/auth/reauth", { password });
    setBusy(false);
    if (result.ok) onConfirmed(); else setError(result.error);
  }
  return <form className="editor" onSubmit={confirm}>
    <p><strong>Confirm it&apos;s you to continue.</strong> Staff actions need a sign-in within the last 5 minutes.</p>
    <label className="field"><span>Password</span><input name="password" type="password" autoComplete="current-password" required /></label>
    <button type="submit" className="small" disabled={busy}>{busy ? "Confirming…" : "Confirm and continue"}</button>
    <p className="muted">Signed in with Google, Twitch or Discord only? Confirm on <Link href="/account">Account security</Link>, then try again.</p>
    {error && <p role="alert">{error}</p>}
  </form>;
}
