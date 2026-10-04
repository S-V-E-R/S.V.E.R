"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { setupLabel } from "../../../lib/setup-parts";

type Entry = { id: string; category: string; name: string; created_at: string; submitted_by: string | null; uses: number; part: string | null; reviewed_by: string | null; reviewed_at: string | null; similar: string[] };
type Queue = { status: string; pending: number; items: Entry[] };
const when = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
const STATUSES = [["PENDING", "Waiting"], ["APPROVED", "Added"], ["DISMISSED", "Dismissed"]] as const;

/** Splits a typed name into a brand guess (first word) and model for the approve form. */
const guess = (name: string) => { const [brand, ...rest] = name.trim().split(/\s+/); return rest.length ? { brand, model: rest.join(" ") } : { brand: "", model: brand || "" }; };

function Review({ entry, onDone }: { entry: Entry; onDone: (notice: string) => void }) {
  const [state, setState] = useState<SaveState>({});
  const g = guess(entry.name);
  async function decide(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const submitter = (event.nativeEvent as SubmitEvent).submitter as HTMLButtonElement | null;
    const f = new FormData(event.currentTarget);
    const decision = submitter?.value === "dismiss" ? { decision: "dismiss" } : { decision: "approve", brand: f.get("brand"), model: f.get("model") };
    const r = await send<{ status: string; part?: { name: string }; linked?: number }>("POST", `/api/admin/parts/${encodeURIComponent(entry.id)}/decision`, decision);
    if (!r.ok) return setState(r);
    // The entry leaves the Waiting list on reload, so the result shows at the top of the page.
    onDone(r.data.status === "APPROVED" ? `Added “${r.data.part?.name}” to the list; ${r.data.linked} setup entr${r.data.linked === 1 ? "y" : "ies"} linked.` : `Dismissed “${entry.name}”. Setups keep the entry as typed.`);
  }
  return <form onSubmit={decide} className="part-review">
    <div className="row wrap">
      <label className="field"><span>Brand</span><input name="brand" defaultValue={g.brand} required maxLength={40} aria-invalid={state.field === "brand"} /></label>
      <label className="field"><span>Model</span><input name="model" defaultValue={g.model} required maxLength={80} aria-invalid={state.field === "model"} /></label>
    </div>
    <div className="row"><button type="submit" className="small" value="approve">Add to the parts list</button><button type="submit" className="small quiet" value="dismiss" formNoValidate>Dismiss</button></div>
    <Status state={state} />
  </form>;
}

export default function PartsQueue() {
  const [status, setStatus] = useState<string>("PENDING");
  const [data, setData] = useState<Queue | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const load = useCallback(async () => {
    const r = await send<Queue>("GET", `/api/admin/parts?status=${status}`);
    if (r.ok) { setData(r.data); setError(""); } else setError(r.error);
  }, [status]);
  useLoad(load);
  return <><h1>Setup parts</h1>
    <p className="muted">Custom entries users typed in the setup parts picker. They already show on those users&apos; pages. Add one to SVER&apos;s parts list (tidy the brand and model first) or dismiss it. Dismissed names stay on the setups as typed and aren&apos;t queued again.</p>
    <nav className="row tight" aria-label="Queue status">{STATUSES.map(([key, name]) => <button key={key} type="button" className={`small${status === key ? "" : " quiet"}`} aria-pressed={status === key} onClick={() => { setStatus(key); setNotice(""); }}>{name}{key === "PENDING" && data ? ` (${data.pending})` : ""}</button>)}</nav>
    {error && <p className="form-message" role="alert">{error}</p>}
    {notice && <p className="muted" role="status">{notice}</p>}
    {!data ? <p className="loading">Loading…</p> : data.items.length === 0 ? <p className="panel section muted">{status === "PENDING" ? "No custom parts waiting." : "Nothing here yet."}</p>
      : data.items.map(e => <Section key={e.id} title={`${setupLabel(e.category)} · ${e.name}`}>
        <p className="muted">{e.submitted_by ? `First added by @${e.submitted_by}` : "Added by a deleted account"} · {when(e.created_at)} · used in {e.uses} setup{e.uses === 1 ? "" : "s"}</p>
        {status === "PENDING" && e.similar.length > 0 && <p className="muted">Already in the list: {e.similar.join(", ")}</p>}
        {status === "PENDING" ? <Review entry={e} onDone={n => { setNotice(n); load(); }} />
          : <p>{status === "APPROVED" ? `Added as “${e.part}”` : "Dismissed"}{e.reviewed_by && ` by @${e.reviewed_by}`}{e.reviewed_at && ` · ${when(e.reviewed_at)}`}</p>}
      </Section>)}
  </>;
}
