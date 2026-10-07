"use client";
import { StatusMark } from "../../../components/shell/Icons";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import type { Readiness } from "../../../components/Readiness";
import { send, useLoad } from "../../../lib/client-api";

/** Channel overview: the page readiness checklist (docs/PROFILES.md, P3). */
export default function ChannelOverview() {
  const [data, setData] = useState<Readiness | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<Readiness>("GET", "/api/me/readiness"); if (r.ok) setData(r.data); else setState(r); }, []);
  useLoad(load);
  async function remind(dismissed: boolean) {
    const r = await send("PUT", "/api/me/readiness", { dismissed });
    setState(r.ok ? { saved: dismissed ? "Reminder hidden." : "The reminder is back on your Studio pages." } : r);
    if (r.ok) load();
  }
  if (!data) return <><h1>Channel overview</h1>{state.error ? <Status state={state} /> : <p className="loading">Loading…</p>}</>;
  return <><h1>Channel overview</h1>
    <Section title="Page readiness" intro="Only you can see this. Finish these steps so visitors get the full picture of your channel.">
      <p className="readiness-count" aria-live="polite">{data.complete ? "Your page is ready." : `${data.done} of ${data.total} done`}</p>
      <progress max={data.total} value={data.done} aria-label="Page readiness" />
      <ol className="list readiness-list">{data.steps.map(s => <li key={s.key} data-step={s.key} data-done={s.done} className="row between">
        <span><StatusMark ok={s.done} />{s.label}</span>
        {!s.done && <Link href={s.href}>Do this</Link>}
      </li>)}</ol>
      {!data.complete && (data.dismissed ? <button type="button" className="small quiet" onClick={() => remind(false)}>Show the reminder again</button> : <button type="button" className="small quiet" onClick={() => remind(true)}>Hide the reminder</button>)}
      <Status state={state} />
    </Section>
  </>;
}
