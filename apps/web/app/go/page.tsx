"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../lib/client-api";

type Pending = { app: { name: string; owner: string }; scopes: { scope: string; text: string }[]; network: string | null };

/** sver.tv/go: approve a desktop, TV or console app with the code it shows (docs/DEVELOPER_PLATFORM.md §1). */
export default function Go() {
  const [code, setCode] = useState("");
  const [pending, setPending] = useState<Pending | null>(null);
  const [message, setMessage] = useState("");
  const [signedOut, setSignedOut] = useState(false);
  const [done, setDone] = useState<boolean | null>(null);
  const check = useCallback(async (value: string) => {
    setMessage(""); setPending(null);
    const r = await send<Pending>("GET", `/api/oauth/device/${encodeURIComponent(value)}`);
    if (r.ok) setPending(r.data);
    else if (r.status === 401) setSignedOut(true);
    else setMessage(r.error);
  }, []);
  const load = useCallback(async () => {
    const given = new URLSearchParams(window.location.search).get("code");
    if (given) { setCode(given); await check(given); }
  }, [check]);
  useLoad(load);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (code.trim()) await check(code.trim());
  }
  async function decide(approve: boolean) {
    const r = await send("POST", `/api/oauth/device/${encodeURIComponent(code.trim())}`, { approve });
    if (r.ok) { setDone(approve); setPending(null); } else setMessage(r.error);
  }
  return <div className="auth-page"><section className="panel stack">
    <h1>Connect a device</h1>
    {signedOut ? <><p>Sign in first, then come back to sver.tv/go and enter the code.</p><Link className="button" href="/login">Log in</Link></>
      : done !== null ? <p role="status">{done ? "Done. Your device is signing in now; you can close this page." : "Cancelled. The device won't be connected."}</p>
      : <>
        <form className="row wrap" onSubmit={submit}>
          <label className="field"><span>The code on your device</span><input value={code} onChange={e => setCode(e.target.value)} autoComplete="off" autoCapitalize="characters" maxLength={9} placeholder="ABC-DEF" required /></label>
          <button type="submit">Continue</button>
        </form>
        {message && <p role="alert" className="form-message error">{message}</p>}
        {pending && <div className="stack">
          <h2>{pending.app.name} wants to use your account</h2>
          <p className="muted">Made by @{pending.app.owner}. The device asking is on {pending.network ?? "a network we couldn't identify"}. If that isn&apos;t your device, or someone sent you this code, choose Cancel.</p>
          <ul>{pending.scopes.map(s => <li key={s.scope}>{s.text}</li>)}{pending.scopes.length === 0 && <li>Confirm that you have a S.V.E.R account</li>}</ul>
          <div className="row"><button type="button" onClick={() => decide(true)}>Allow</button><button type="button" className="quiet" onClick={() => decide(false)}>Cancel</button></div>
        </div>}
      </>}
  </section></div>;
}
