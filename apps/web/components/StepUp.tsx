"use client";
import { FormEvent, useEffect, useRef, useState } from "react";
import { send, setStepUpHandler } from "../lib/client-api";

/**
 * Staff confirmation prompt, mounted once by the admin layout. Staff tools stay unlocked for 15
 * minutes after a confirmation and each staff action extends that (up to 8 hours). When a request
 * is refused because the window lapsed, this asks for an authenticator code (or the password) and
 * the refused request then retries on its own.
 */
export function StaffStepUp() {
  const [open, setOpen] = useState(false);
  const [usePassword, setUsePassword] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  // One pending confirmation is shared by every request refused while the prompt is open.
  const pending = useRef<{ promise: Promise<boolean>; resolve: (ok: boolean) => void } | null>(null);
  useEffect(() => {
    setStepUpHandler(() => {
      if (!pending.current) {
        let resolve: (ok: boolean) => void = () => {};
        const promise = new Promise<boolean>(r => { resolve = r; });
        pending.current = { promise, resolve };
        setError(""); setOpen(true);
      }
      return pending.current.promise;
    });
    return () => setStepUpHandler(null);
  }, []);
  function finish(ok: boolean) {
    pending.current?.resolve(ok);
    pending.current = null;
    setOpen(false); setUsePassword(false);
  }
  async function confirm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = String(new FormData(event.currentTarget).get("secret") ?? "");
    setBusy(true); setError("");
    const result = usePassword
      ? await send("POST", "/api/auth/reauth", { password: value })
      : await send("POST", "/api/admin/confirm", { code: value });
    setBusy(false);
    if (result.ok) finish(true); else setError(result.error);
  }
  if (!open) return null;
  return <div className="step-up" role="dialog" aria-modal="true" aria-labelledby="step-up-title">
    <form className="panel" onSubmit={confirm}>
      <h2 id="step-up-title">Confirm it&apos;s you</h2>
      <p>Staff tools lock after 15 minutes without a staff action. Confirm to continue; what you were doing finishes on its own.</p>
      <label className="field">
        <span>{usePassword ? "Password" : "Authenticator or recovery code"}</span>
        {usePassword
          ? <input key="password" name="secret" type="password" autoComplete="current-password" required autoFocus />
          : <input key="code" name="secret" inputMode="numeric" autoComplete="one-time-code" required autoFocus maxLength={64} />}
      </label>
      <div className="row">
        <button type="submit" disabled={busy}>{busy ? "Confirming…" : "Confirm"}</button>
        <button type="button" className="quiet" onClick={() => { setUsePassword(!usePassword); setError(""); }}>{usePassword ? "Use a code instead" : "Use my password instead"}</button>
        <button type="button" className="quiet" onClick={() => finish(false)}>Cancel</button>
      </div>
      {error && <p role="alert">{error}</p>}
    </form>
  </div>;
}
