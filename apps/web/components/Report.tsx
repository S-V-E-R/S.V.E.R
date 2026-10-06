"use client";
import { FormEvent, useState } from "react";
import { send } from "../lib/client-api";
import { reasons } from "../lib/types";

export type ReportTarget = { target_type: "profile" | "wall_post" | "wall_reply" | "fan_art" | "setup_photo" | "chat_message" | "live_stream" | "emote" | "faction_post" | "guild" | "guild_emblem" | "vod" | "highlight" | "clip"; target_id: string; field?: string };
export function TakeDownLink({ target }: { target: ReportTarget }) {
  const query = new URLSearchParams({ report: target.target_type, id: target.target_id, ...(target.field ? { field: target.field } : {}) });
  const location = `https://sver.tv/${target.target_type === "profile" ? encodeURIComponent(target.target_id) : "take-it-down"}?${query}`;
  return <a className="link-button" href={`/take-it-down?${new URLSearchParams({ location })}`}>Intimate image (Take It Down)</a>;
}
const fields: [string, string][] = [["display_name", "Display name"], ["username", "Username"], ["avatar", "Avatar"], ["banner", "Banner"], ["bio", "Bio"], ["status", "Status"], ["mood", "Mood"], ["links", "Links"], ["song", "Profile song"], ["war_council", "War Council"], ["sponsors", "Sponsors"], ["setup", "Streaming setup"], ["blocks", "About blocks"], ["header", "Page header text"]];

/** Report form (reason, optional note; channel reports also pick the part of the channel). */
export function ReportForm({ target, onDone }: { target: ReportTarget; onDone: () => void }) {
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    const result = await send<{ message: string }>("POST", "/api/reports", { ...target, field: target.target_type === "profile" ? form.get("field") : undefined, reason: form.get("reason"), note: form.get("note") || "" });
    setBusy(false);
    setMessage(result.ok ? result.data.message : result.error);
    if (result.ok) setTimeout(onDone, 1500);
  }
  return <form className="report-form" onSubmit={submit}>
    {target.target_type !== "faction_post" && <p><TakeDownLink target={target} /></p>}
    {target.target_type === "profile" && <label className="field"><span>What are you reporting?</span><select name="field" required defaultValue="">{[<option key="" value="" disabled>Choose…</option>, ...fields.map(([v, l]) => <option key={v} value={v}>{l}</option>)]}</select></label>}
    <label className="field"><span>Reason</span><select name="reason" required defaultValue="">{[<option key="" value="" disabled>Choose…</option>, ...reasons.map(([v, l]) => <option key={v} value={v}>{l}</option>)]}</select></label>
    <label className="field"><span>Anything else? (optional)</span><textarea name="note" maxLength={500} rows={3} /></label>
    <div className="row"><button type="submit" className="small" disabled={busy}>Send report</button><button type="button" className="small quiet" onClick={onDone}>Cancel</button></div>
    {message && <p role="status" className="form-message">{message}</p>}
  </form>;
}

/** "Report" text button that expands the form inline. */
export function ReportButton({ target, label = "Report" }: { target: ReportTarget; label?: string }) {
  const [open, setOpen] = useState(false);
  return <>{open ? <ReportForm target={target} onDone={() => setOpen(false)} /> : <button type="button" className="link-button" onClick={() => setOpen(true)}>{label}</button>}</>;
}
