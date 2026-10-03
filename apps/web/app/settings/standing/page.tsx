"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { MyBans } from "../../../components/Bans";
import { send, useLoad } from "../../../lib/client-api";
import { reasons } from "../../../lib/types";

type Strike = { id: string; reason: string; severity: string; content: { field: string | null; value: unknown }; penalty: string; penalty_until: string | null; level: number; message_to_user: string; issued_at: string; expires_at: string; status: "active" | "expired" | "overturned"; acknowledged: boolean; appeal_closes_at: string; can_appeal: boolean; note?: string; appeal: null | { status: string; created_at: string; body: string; message_to_user: string | null; signed: string | null } };
type Standing = { level: number; restriction: null | { until: string | null; indefinite: boolean }; strikes: Strike[] };
const penalties: Record<string, string> = { WARNING: "Warning", RESTRICT_72H: "72-hour restriction", RESTRICT_INDEFINITE: "Restriction until further review" };
const LEVEL2 = "This restriction lasts 72 hours. Appeals are usually decided after it ends. If your appeal succeeds, the strike is removed from your record and no longer counts toward future penalties.";
const date = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "long", timeStyle: "short" });

function Content({ value }: { value: unknown }) {
  if (value === null || value === undefined) return null;
  if (typeof value === "string") return <p>{value}</p>;
  return <pre className="snapshot">{JSON.stringify(value, null, 2)}</pre>;
}

function Appeal({ strike, onDone }: { strike: Strike; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const body = new FormData(event.currentTarget).get("body");
    const result = await send("POST", `/api/me/strikes/${strike.id}/appeal`, { body, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone });
    setState(result.ok ? { saved: "Appeal submitted." } : result);
    if (result.ok) onDone();
  }
  return <form onSubmit={submit} className="appeal">
    {strike.level === 2 && <p className="notice">{LEVEL2}</p>}
    <label className="field"><span>Tell us why this strike should be reviewed</span><textarea name="body" required maxLength={1000} rows={4} /><small>You can appeal each strike once, until {date(strike.appeal_closes_at)}.</small></label>
    <button type="submit" className="small">Submit appeal</button>
    <Status state={state} />
  </form>;
}

export default function StandingPage() {
  const [data, setData] = useState<Standing | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const result = await send<Standing>("GET", "/api/me/standing");
    if (result.ok) setData(result.data); else setError(result.error);
  }, []);
  useLoad(load);
  async function acknowledge(id: string) {
    const result = await send("POST", `/api/me/strikes/${id}/acknowledge`);
    if (result.ok) load();
  }
  if (!data) return <p className="loading">{error || "Loading…"}</p>;
  const unacknowledged = data.strikes.filter(s => !s.acknowledged && s.status !== "overturned");
  return <><h1>Account standing</h1>
    {unacknowledged.map(s => <div key={s.id} className="panel notice" role="alert"><p>You received a strike on {date(s.issued_at)} for {reasons.find(r => r[0] === s.reason)?.[1].toLowerCase() || s.reason}.</p><button type="button" className="small" onClick={() => acknowledge(s.id)}>I understand</button></div>)}
    <Section title="Standing">
      {data.restriction ? <p>Your channel is restricted {data.restriction.indefinite ? "until further review" : `until ${date(data.restriction.until!)}`}. While restricted, your channel is hidden and you can&apos;t edit it, post or report. You can still appeal.</p> : <p>{data.level === 0 ? "Your account is in good standing." : `Active strikes: ${data.level}.`}</p>}
      <p className="muted small-print">Strikes stop counting after 90 days (365 days for severe violations). A first strike is a warning, a second is a 72-hour restriction and a third is a restriction until further review.</p>
    </Section>
    <MyBans />
    <Section title="Strikes">
      {data.strikes.length === 0 ? <p className="muted">No strikes.</p> : <ul className="list">{data.strikes.map(s => <li key={s.id} className="strike">
        <div className="row between"><strong>{penalties[s.penalty] || s.penalty} · {reasons.find(r => r[0] === s.reason)?.[1] || s.reason}</strong><span className="badge">{s.status === "active" ? "Active" : s.status === "expired" ? "Expired" : "Overturned"}</span></div>
        <p className="muted">Issued {date(s.issued_at)}{s.penalty_until && ` · Restricted until ${date(s.penalty_until)}`} · Counts until {date(s.expires_at)}</p>
        {s.level === 2 && <p className="notice">{LEVEL2}</p>}
        {s.message_to_user && <p>{s.message_to_user}</p>}
        <details><summary>The content involved</summary>{s.content?.field && <p className="muted">{s.content.field.replaceAll("_", " ")}</p>}<Content value={s.content?.value} /></details>
        {s.appeal ? <div className="appeal-status"><p><strong>Appeal: {s.appeal.status === "PENDING" ? "under review" : s.appeal.status === "OVERTURNED" ? "strike overturned" : "strike upheld"}</strong></p>{s.appeal.message_to_user && <p>{s.appeal.message_to_user} <span className="muted">— {s.appeal.signed}</span></p>}</div>
          : s.can_appeal ? <Appeal strike={s} onDone={load} /> : s.status === "active" && <p className="muted">The appeal window for this strike closed on {date(s.appeal_closes_at)}.</p>}
      </li>)}</ul>}
    </Section></>;
}
