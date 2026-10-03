"use client";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { reasons } from "../../../lib/types";

type Row = { id: string; target_type: string; username: string | null; reason: string; note: string; created_at: string; status: "under_review" | "action_taken" | "closed"; notice: string | null; unread: boolean };
type Page = { reports: Row[]; next_cursor: string | null; email_updates: boolean; email_available: boolean };
const kinds: Record<string, string> = { profile: "Channel", wall_post: "Wall post", wall_reply: "Wall reply", fan_art: "Fan art", setup_photo: "Setup photo" };
const statuses = { under_review: "Under review", action_taken: "Action taken", closed: "Closed" };

export default function Reports() {
  const [page, setPage] = useState<Page | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [error, setError] = useState("");
  const load = useCallback(async (cursor?: string) => {
    const result = await send<Page>("GET", `/api/me/reports${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`);
    if (!result.ok) return setError(result.error);
    setPage(result.data);
    setRows(r => cursor ? [...r, ...result.data.reports] : result.data.reports);
    // Opening the page marks every action-taken notice as seen; the dots stay for this view.
    if (!cursor) await send("POST", "/api/me/reports/seen");
  }, []);
  useLoad(load);
  async function toggle(enabled: boolean) {
    const result = await send<{ email_updates: boolean }>("PUT", "/api/me/reports/email", { enabled });
    if (result.ok && page) setPage({ ...page, email_updates: result.data.email_updates });
  }
  const reason = (r: string) => reasons.find(x => x[0] === r)?.[1] || r;
  return <><h1>My reports</h1>
    <Section title="Reports you've sent" intro="Only you can see this page. Reports from the last 180 days are shown.">
      {error && <p role="alert" className="form-message error">{error}</p>}
      {page?.email_available && <label className="checkbox"><input type="checkbox" checked={page.email_updates} onChange={e => toggle(e.target.checked)} /> Email me when action is taken on my reports</label>}
      {!page ? <p className="loading">Loading…</p> : rows.length === 0 ? <p className="muted">You haven&apos;t reported anything.</p> :
        <ul className="list">{rows.map(r => <li key={r.id} className="report-row">
          <div className="row between"><strong>{kinds[r.target_type] || r.target_type} · {r.username ? `@${r.username}` : "Deleted user"}</strong><span className="badge">{r.unread && <span className="dot" aria-label="New" />} {statuses[r.status]}</span></div>
          <p className="muted">{reason(r.reason)} · {new Date(r.created_at).toLocaleDateString(undefined, { dateStyle: "medium" })}</p>
          {r.note && <p>{r.note}</p>}
          {r.notice && <p className="notice">{r.notice}</p>}
        </li>)}</ul>}
      {page?.next_cursor && <button type="button" className="small quiet" onClick={() => load(page.next_cursor!)}>More</button>}
    </Section></>;
}
