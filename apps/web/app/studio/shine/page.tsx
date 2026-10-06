"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Stream = { id: string; charity: string; url: string; started_at: string; live: boolean; raised_cents: number | null; proof_url: string | null; status: "none" | "submitted" | "verified" | "rejected" | "revoked"; review_note: string | null };
type Data = { charity_name: string | null; charity_url: string | null; streams: Stream[] };
const STATUS = { none: "Not submitted", submitted: "Waiting for staff", verified: "Good Works badge", rejected: "Not verified", revoked: "Badge revoked" };

/** Creator Studio → Shine: charity streams and Good Works badges (docs/SUPPORT.md "Shine"). */
export default function StudioShine() {
  const [data, setData] = useState<Data | null>(null);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [state, setState] = useState<SaveState>({});
  const show = (d: Data) => { setData(d); setName(d.charity_name ?? ""); setUrl(d.charity_url ?? ""); };
  const load = useCallback(async () => {
    const r = await send<Data>("GET", "/api/me/shine");
    if (r.ok) show(r.data); else setState(r);
  }, []);
  useLoad(load);
  async function save(event: FormEvent, clear = false) {
    event.preventDefault();
    setBusy(true);
    const r = await send<Data>("PUT", "/api/me/shine", clear ? {} : { charity_name: name, charity_url: url });
    setBusy(false); setState(r.ok ? { saved: clear ? "Charity removed." : "Charity saved." } : r);
    if (r.ok) show(r.data);
  }
  async function submit(event: FormEvent<HTMLFormElement>, stream: Stream) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    const r = await send<Data>("POST", `/api/me/shine/${encodeURIComponent(stream.id)}/submit`, { raised_cents: Math.round(Number(form.get("raised")) * 100), proof_url: form.get("proof") });
    setBusy(false); setState(r.ok ? { saved: "Submitted for staff to verify." } : r);
    if (r.ok) show(r.data);
  }
  if (!data) return state.error ? <Status state={state} /> : <p className="loading">Loading…</p>;
  return <><h1>Shine</h1>
    <p>Let your light shine. Mark your streams as charity streams: viewers see a Shine banner with a button to the charity&apos;s own donation page. S.V.E.R never collects or holds charity money.</p>
    <Section title="Charity stream" intro="Applies to your current stream if you're live, and to your next streams until you remove it. Use a registered charity and a link to its own donation page or a recognized fundraiser for it.">
      <form onSubmit={save} className="reward-form">
        <label className="field"><span>Charity</span><input value={name} maxLength={80} onChange={e => setName(e.target.value)} placeholder="For example: American Red Cross" /></label>
        <label className="field"><span>Donation page</span><input type="url" value={url} maxLength={500} onChange={e => setUrl(e.target.value)} placeholder="https://" /></label>
        <button disabled={busy || !name || !url}>Save charity</button>
        {data.charity_name && <button type="button" className="quiet" disabled={busy} onClick={e => save(e, true)}>Remove charity</button>}
      </form>
    </Section>
    <Section title="Good Works" intro="After a charity stream ends, submit the amount raised with a link to the charity's receipt or fundraiser page. Staff verify it and add a Good Works badge to your channel.">
      {data.streams.length === 0 ? <p className="muted">No charity streams yet.</p> : <ul className="list">{data.streams.map(stream => <li key={stream.id}>
        <p><strong>{stream.charity}</strong> · {new Date(stream.started_at).toLocaleDateString()}{stream.live && " · live now"} · {STATUS[stream.status]}{stream.raised_cents !== null && ` · $${(stream.raised_cents / 100).toLocaleString(undefined, { minimumFractionDigits: 2 })}`}</p>
        {stream.review_note && <p className="muted">Staff note: {stream.review_note}</p>}
        {!stream.live && (stream.status === "none" || stream.status === "rejected") && <form className="reward-form" onSubmit={e => submit(e, stream)}>
          <label className="field narrow"><span>Amount raised ($)</span><input name="raised" type="number" min={0.01} step={0.01} required /></label>
          <label className="field"><span>Proof (receipt or fundraiser page)</span><input name="proof" type="url" required placeholder="https://" /></label>
          <button disabled={busy}>Submit for verification</button>
        </form>}
      </li>)}</ul>}
    </Section>
    <Status state={state} />
  </>;
}
