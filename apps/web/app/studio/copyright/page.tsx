"use client";
import { useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";
import { CopyrightForm } from "../../../components/CopyrightForm";
type Case = { id: string; status: string; created_at: string; reason: string | null; restore_after: string | null; restore_by: string | null; notice?: Record<string, string | boolean> };
export default function Copyright() {
  const [cases, setCases] = useState<Case[]>([]);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => { const result = await send<{ cases: Case[] }>("GET", "/api/me/copyright"); if (result.ok) setCases(result.data.cases); else setMessage(result.error); }, []);
  useLoad(load);
  return <><h1>Copyright cases</h1><p>Read notices affecting your recordings and submit a counter-notice if a removal was a mistake or misidentification.</p>{!cases.length && <p>No copyright cases to show.</p>}{cases.map(c => <section className="panel video-manage" key={c.id}><h2>Case {c.id.slice(0, 8)}</h2><p>{c.status.replaceAll("_", " ")} · {new Date(c.created_at).toLocaleDateString()}</p>{c.reason && <p>{c.reason}</p>}{c.restore_after && <p>Counter-notice restoration window: {new Date(c.restore_after).toLocaleDateString()}–{new Date(c.restore_by!).toLocaleDateString()}. Other valid holds and normal recording retention still apply.</p>}{c.notice && <details><summary>Copyright notice</summary><dl>{Object.entries(c.notice).filter(([, value]) => typeof value === "string").map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{String(value)}</dd></div>)}</dl></details>}{c.status === "REMOVED" && <details><summary>Submit a counter-notice</summary><CopyrightForm caseId={c.id} location={String(c.notice?.location ?? "")} onSaved={() => void load()} /></details>}</section>)}{message && <p role="alert">{message}</p>}</>;
}
