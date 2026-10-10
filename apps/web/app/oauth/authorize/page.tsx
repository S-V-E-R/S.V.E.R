"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Consent = { app: { name: string; owner: string }; scopes: { scope: string; text: string }[] };

/** The consent screen for an app asking to use the person's account (docs/DEVELOPER_PLATFORM.md §1). */
export default function Authorize() {
  const [consent, setConsent] = useState<Consent | null>(null);
  const [error, setError] = useState("");
  const [signedOut, setSignedOut] = useState(false);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const r = await send<Consent>("GET", `/api/oauth/authorize${window.location.search}`);
    if (r.ok) setConsent(r.data);
    else if (r.status === 401) setSignedOut(true);
    else setError(r.error);
  }, []);
  useLoad(load);
  async function decide(approve: boolean) {
    setBusy(true);
    const params = Object.fromEntries(new URLSearchParams(window.location.search));
    const r = await send<{ redirect: string }>("POST", "/api/oauth/authorize", { ...params, approve });
    if (r.ok) window.location.assign(r.data.redirect); else { setError(r.error); setBusy(false); }
  }
  return <div className="auth-page"><section className="panel stack">
    {signedOut ? <><h1>Sign in to continue</h1><p>An app wants to use your S.V.E.R account. Sign in first, then come back to this page.</p><Link className="button" href="/login">Log in</Link></>
      : error ? <><h1>This app can&apos;t connect</h1><p role="alert" className="form-message error">{error}</p></>
      : !consent ? <p className="loading">Loading…</p>
      : <>
        <h1>{consent.app.name} wants to use your account</h1>
        <p className="muted">Made by @{consent.app.owner}. It isn&apos;t made by S.V.E.R.</p>
        <p>It will be able to:</p>
        <ul>{consent.scopes.map(s => <li key={s.scope}>{s.text}</li>)}{consent.scopes.length === 0 && <li>Confirm that you have a S.V.E.R account</li>}</ul>
        <p className="muted small">It can never spend your Valor, see your email or password, or move money. You can remove it any time in Settings → Connected apps.</p>
        <div className="row"><button type="button" disabled={busy} onClick={() => decide(true)}>Allow</button><button type="button" className="quiet" disabled={busy} onClick={() => decide(false)}>Cancel</button></div>
      </>}
  </section></div>;
}
