"use client";
import { FormEvent, useCallback, useEffect, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";
import { EnableStaffPush } from "../../../components/StaffRemovalAlerts";
import { RecordingEvidence } from "../../../components/RecordedPlayer";
import { BeaconEvidence } from "../../../components/BeaconEvidence";

type Request = { number: string; status: string; received_at: string; deadline: string; resolved_at: string | null; reason: string; media_pending: number; target_count: number };
type Queue = { requests: Request[]; monthly: { received: number; removed: number; overdue: number; median_hours: number | null; longest_hours: number | null } };
type Detail = { number: string; minor: boolean; preservation_reference: string; details: { name: string; email: string; capacity: string; authority: string; locations: string[]; description: string; extra: string; signature: string; signed_on: string; good_faith: boolean }; events: { at: string; action: string; actor: string | null; detail: string }[]; notices: { channel: "email" | "push"; audience: string; state: string; queued_at: string; last_attempt_at: string | null; attempts: number; accepted_at: string | null }[]; targets: { kind: string; id: string }[]; evidence: string[] };
const when = (date: string) => new Date(date).toLocaleString();

function Review({ item, refresh }: { item: Request; refresh: () => Promise<void> }) {
  const [detail, setDetail] = useState<Detail | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(path: string, body: Record<string, unknown>) {
    setBusy(true); setError("");
    const response = await send("POST", path, body);
    setBusy(false);
    if (response.ok) { await refresh(); await open(); } else setError(response.error);
  }
  async function open() {
    const response = await send<Detail>("GET", `/api/admin/take-it-down/${item.number}`);
    if (response.ok) { setDetail(response.data); setError(""); } else setError(response.error);
  }
  async function act(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    const form = new FormData(event.currentTarget);
    await submit(`/api/admin/take-it-down/${item.number}`, { action: form.get("action"), reason: form.get("reason"), minor: form.get("minor") === "on", preservation_reference: form.get("preservation_reference") });
  }
  async function locate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    await submit(`/api/admin/take-it-down/${item.number}/locations`, { location: new FormData(event.currentTarget).get("location") });
  }
  return <>
    {!detail ? <button type="button" className="small quiet" onClick={open}>Review {item.number}</button> : <div>
      <dl><dt>Requester</dt><dd>{detail.details.name} · <a href={`mailto:${detail.details.email}`}>{detail.details.email}</a></dd><dt>Authority</dt><dd>{detail.details.capacity === "shown" ? "Person shown" : detail.details.authority}</dd><dt>Signature</dt><dd>{detail.details.signature}, {detail.details.signed_on}</dd><dt>Good faith confirmed</dt><dd>{detail.details.good_faith ? "Yes" : "No"}</dd></dl>
      <ul>{detail.details.locations.map((url,i) => <li key={i}><a href={url} target="_blank" rel="noreferrer">{url}</a></li>)}</ul>
      <p className="preserve-lines">{detail.details.description}</p><p className="preserve-lines">{detail.details.extra}</p>
      {detail.minor && <p><strong>Person shown was under 18. Preserve evidence.</strong></p>}
      {detail.preservation_reference && <p>Preservation reference: {detail.preservation_reference}</p>}
      <p>{detail.targets.length} located content item(s); {item.media_pending} image group(s) still awaiting storage/cache hiding.</p>
      {detail.targets.filter(t => ["vod","highlight","clip"].includes(t.kind)).map(t => <RecordingEvidence key={t.id} id={t.id} />)}
      {detail.targets.filter(t => t.kind === "beacon").map(t => <BeaconEvidence key={t.id} id={t.id} />)}
      {detail.evidence.length>0 && <details><summary>Quarantined media — restricted staff access</summary><p>Viewing requires a recent sign-in and is recorded. Open only the evidence needed for this review.</p><ul>{detail.evidence.map((key,i)=><li key={key}><a href={`/api/admin/take-it-down/${item.number}/media/${key}`} target="_blank" rel="noreferrer">Review stored image {i+1}</a></li>)}</ul></details>}
      {!item.resolved_at && <form className="editor" onSubmit={locate}><label className="field"><span>Add a precise content location from your review</span><input name="location" type="text" inputMode="url" placeholder="sver.tv/username or a full link" required maxLength={2048} /></label><button type="submit" className="small quiet" disabled={busy}>Locate and hide</button></form>}
      {!item.resolved_at && <form className="editor" onSubmit={act}>
        <label className="field"><span>Action</span><select name="action"><option value="review">Under review — stop named live streams</option><option value="remove">Valid request — permanently remove</option><option value="dismiss">Not valid — restore content</option></select></label>
        <label className="field"><span>Explanation to requester</span><textarea name="reason" required maxLength={1000} rows={3} /></label>
        <label className="check"><input type="checkbox" name="minor" key={String(detail.minor)} defaultChecked={detail.minor} disabled={detail.minor} /> The person shown was under 18 — preserve evidence</label>
        <label className="field"><span>CyberTipline report or legal preservation reference (required for a minor)</span><input name="preservation_reference" key={detail.preservation_reference} defaultValue={detail.preservation_reference} maxLength={300} /></label>
        <p className="muted">Valid removals are permanent. The uploader receives a severe strike and an account-ban review. If you&apos;re asked to confirm it&apos;s you, the action continues once you do.</p>
        <button type="submit" disabled={busy}>{busy ? "Applying…" : "Apply review action"}</button>
      </form>}
      <button type="button" className="small quiet" onClick={open} disabled={busy}>Refresh case record</button>
      <h3>Notifications</h3><p className="muted">Latest 200 notices. Service acceptance does not confirm that the recipient has read an alert.</p>
      <ul>{detail.notices.map((notice,i) => <li key={i}>
        {notice.audience} {notice.channel} — {notice.state === "accepted" ? `Accepted by ${notice.channel} service` : notice.state} · queued {when(notice.queued_at)} · {notice.attempts} attempt(s)
        {notice.last_attempt_at && <> · last attempt {when(notice.last_attempt_at)}</>}
        {notice.accepted_at && <> · accepted {when(notice.accepted_at)}</>}
        {(notice.state === "failed" || notice.state === "expired") && <strong> — Contact the recipient through an available channel.</strong>}
      </li>)}</ul>
      <h3>Record</h3><p className="muted">Latest 200 events, newest first. The full record is retained with the case.</p><ol>{detail.events.map((event,i) => <li key={i}>{when(event.at)} — {event.action.replaceAll("_"," ")}{event.actor && <> · staff {event.actor}</>}{event.detail && <p className="preserve-lines">{event.detail}</p>}</li>)}</ol>
    </div>}
    {error && <p role="alert">{error}</p>}
  </>;
}

export default function TakeItDownQueue() {
  const [queue, setQueue] = useState<Queue | null>(null);
  const [error, setError] = useState("");
  const [now, setNow] = useState<number | null>(null);
  const load = useCallback(async () => {
    const response = await send<Queue>("GET", "/api/admin/take-it-down");
    if (response.ok) { setQueue(response.data); setError(""); setNow(Date.now()); } else setError(response.error);
  }, []);
  useLoad(load);
  useEffect(() => { const timer = setInterval(() => { void load(); },30000); return () => clearInterval(timer); },[load]);
  return <section className="panel"><h1>Take It Down requests</h1><p>Review urgent requests first. The 48-hour deadline includes weekends and holidays. Requester details are private.</p>
    <EnableStaffPush />
    {error && <p role="alert">{error}</p>}
    {queue && <><p>This month: {queue.monthly.received} received · {queue.monthly.removed} removed · {queue.monthly.overdue} over 48 hours. Median removal: {queue.monthly.median_hours?.toFixed(1) ?? "—"} hours; longest: {queue.monthly.longest_hours?.toFixed(1) ?? "—"} hours.</p>
      {queue.requests.length === 0 && <p>No requests.</p>}
      {queue.requests.map(item => {
        const minutes = now === null ? null : Math.ceil((Date.parse(item.deadline)-now)/60000);
        return <article className="panel" key={item.number}><h2>{item.number}</h2><p>{item.status.replaceAll("_"," ")} · received {when(item.received_at)}</p>
          {!item.resolved_at && <p role={minutes !== null && minutes<0 ? "alert" : undefined}><strong>{minutes === null ? "Deadline" : minutes<0 ? `Overdue by ${Math.abs(minutes)} minutes` : `${Math.floor(minutes/60)}h ${minutes%60}m remaining`}</strong> — {when(item.deadline)}</p>}
          {item.reason && <p>{item.reason}</p>}<Review item={item} refresh={load} /></article>;
      })}</>}
  </section>;
}
