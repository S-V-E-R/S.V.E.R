"use client";
import { FormEvent, useState } from "react";
import { send } from "../../lib/client-api";
import { Turnstile } from "../../components/Turnstile";

type Status = { number: string; status: string; received_at?: string; resolved_at?: string; reason?: string; message?: string };
const labels: Record<string,string> = { received: "Received", under_review: "Under review", removed: "Removed", not_removed: "Not removed" };
const when = (date: string) => new Date(date).toLocaleString();

export default function RequestForms({ sitekey, location }: { sitekey: string; location: string }) {
  const [token, setToken] = useState("");
  const [statusToken, setStatusToken] = useState("");
  const [generation, setGeneration] = useState(0);
  const [statusGeneration, setStatusGeneration] = useState(0);
  const [capacity, setCapacity] = useState("shown");
  const [receipt, setReceipt] = useState<Status | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState("");
  const [lookupError, setLookupError] = useState("");
  const [busy, setBusy] = useState(false);
  const [looking, setLooking] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    const data = new FormData(event.currentTarget);
    setBusy(true); setError("");
    const response = await send<Status>("POST", "/api/take-it-down", {
      name: data.get("name"), email: data.get("email"), capacity, authority: data.get("authority") || "",
      locations: String(data.get("locations")).split(/\r?\n/).map(s => s.trim()).filter(Boolean),
      description: data.get("description"), good_faith: data.get("good_faith") === "on",
      signature: data.get("signature"), signed_on: data.get("signed_on"), extra: data.get("extra"), turnstile_token: token,
    });
    setBusy(false); setToken(""); setGeneration(n => n+1);
    if (response.ok) setReceipt(response.data); else setError(response.error);
  }
  async function lookup(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (looking) return;
    const data = new FormData(event.currentTarget);
    setLooking(true); setLookupError(""); setStatus(null);
    const response = await send<Status>("POST", "/api/take-it-down/status", { number: data.get("number"), email: data.get("email"), turnstile_token: statusToken });
    setLooking(false); setStatusToken(""); setStatusGeneration(n => n+1);
    if (response.ok) setStatus(response.data); else setLookupError(response.error);
  }
  return <>
    {receipt ? <div className="panel" role="status"><h3>Request received</h3><p>Your request number is <strong>{receipt.number}</strong>. Keep it and the email address you used to check your request below.</p><p>We have queued your confirmation email. Check your spam folder if it does not arrive.</p></div> :
      <form onSubmit={submit} className="editor" aria-label="Request removal">
        <label className="field"><span>Your name</span><input name="name" autoComplete="name" required maxLength={150} /></label>
        <label className="field"><span>Your email</span><input name="email" type="email" autoComplete="email" required maxLength={320} /></label>
        <label className="field"><span>Who is asking?</span><select name="capacity" value={capacity} onChange={e => setCapacity(e.target.value)}><option value="shown">I am the person shown</option><option value="authorized">I am authorized to act for them</option></select></label>
        {capacity === "authorized" && <label className="field"><span>How are you authorized to act for the person shown?</span><textarea name="authority" required maxLength={1000} rows={3} /></label>}
        <label className="field"><span>S.V.E.R links, one per line</span><textarea name="locations" defaultValue={location} required maxLength={41000} rows={4} aria-describedby="location-help" /></label>
        <p id="location-help" className="muted">Up to 20 links. Give a username, time in a stream or description below if a link alone is not enough. Do not send a copy of the image.</p>
        <label className="field"><span>Details that help us find the content (optional)</span><textarea name="description" maxLength={4000} rows={4} /></label>
        <label className="check"><input type="checkbox" name="good_faith" required /> I believe in good faith that this content was shared without consent of the person shown.</label>
        <label className="field"><span>Electronic signature: type your full name</span><input name="signature" required maxLength={150} autoComplete="name" /></label>
        <label className="field"><span>Signature date</span><input name="signed_on" type="date" required /></label>
        <label className="field"><span>Other locations or information (optional)</span><textarea name="extra" maxLength={4000} rows={3} /></label>
        <Turnstile key={generation} sitekey={sitekey} action="take_down" onToken={setToken} />
        <button disabled={busy || !token} type="submit">{busy ? "Sending…" : "Request removal"}</button>
        {error && <p role="alert">{error}</p>}
      </form>}
    <h3 id="status">Check your request</h3>
    <form onSubmit={lookup} className="editor" aria-label="Check removal request">
      <label className="field"><span>Request number</span><input name="number" required maxLength={40} defaultValue={receipt?.number || ""} placeholder="TID-2026-000123" /></label>
      <label className="field"><span>Email used for the request</span><input name="email" type="email" autoComplete="email" required maxLength={320} /></label>
      <Turnstile key={statusGeneration} sitekey={sitekey} action="take_down_status" onToken={setStatusToken} />
      <button disabled={looking || !statusToken} type="submit">{looking ? "Checking…" : "Check status"}</button>
      {lookupError && <p role="alert">{lookupError}</p>}
      {status && <div role="status"><p><strong>{status.number}: {labels[status.status] || status.status}</strong></p>{status.received_at && <p>Received: {when(status.received_at)}</p>}{status.resolved_at && <p>Decision: {when(status.resolved_at)}</p>}{status.reason && <p>{status.reason}</p>}</div>}
    </form>
  </>;
}
