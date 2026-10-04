"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { Section, Status, type SaveState } from "./Form";

type Ban = { id: string; reason: string; message_to_user: string; issued_at: string; until: string | null; status: string; current: boolean; appeal_closes_at: string; appeal: null | { status: string; message_to_user: string } };
const date = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "long", timeStyle: "short" });
const STEP_UP = "Confirm your sign-in method again on the Account page, then retry.";

/** The signed-in account's bans and its one appeal per ban (Settings → Standing). */
export function MyBans() {
  const [bans, setBans] = useState<Ban[] | null>(null);
  const load = useCallback(async () => { const r = await send<{ bans: Ban[] }>("GET", "/api/me/bans"); if (r.ok) setBans(r.data.bans); }, []);
  useLoad(load);
  if (!bans || bans.length === 0) return null;
  return <Section title="Account bans">
    <ul className="list">{bans.map(b => <li key={b.id} className="strike">
      <div className="row between"><strong>{b.current ? (b.until ? `Banned until ${date(b.until)}` : "Banned until further review") : b.status === "ACTIVE" ? "Ban ended" : b.status === "OVERTURNED" ? "Ban overturned" : "Ban lifted"}</strong></div>
      <p className="muted">Issued {date(b.issued_at)} · {b.reason}</p>
      {b.current && <p>While banned you can manage account security, read your standing and appeal. Everything else is paused.</p>}
      {b.message_to_user && <p>{b.message_to_user}</p>}
      {b.appeal ? <p><strong>Appeal: {b.appeal.status === "PENDING" ? "under review" : b.appeal.status === "OVERTURNED" ? "ban overturned" : "ban upheld"}</strong>{b.appeal.message_to_user && ` · ${b.appeal.message_to_user}`}</p>
        : b.status === "ACTIVE" && new Date(b.appeal_closes_at) > new Date() ? <Appeal ban={b} onDone={load} />
          : b.current && <p className="muted">The appeal window for this ban closed on {date(b.appeal_closes_at)}.</p>}
    </li>)}</ul>
  </Section>;
}
function Appeal({ ban, onDone }: { ban: Ban; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const body = new FormData(event.currentTarget).get("body");
    const r = await send("POST", `/api/me/bans/${ban.id}/appeal`, { body, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone });
    setState(r.ok ? { saved: "Appeal submitted." } : r);
    if (r.ok) onDone();
  }
  return <form onSubmit={submit}>
    <label className="field"><span>Appeal this ban (once, by {date(ban.appeal_closes_at)})</span><textarea name="body" required maxLength={1000} rows={3} /></label>
    <button type="submit" className="small">Submit appeal</button><Status state={state} />
  </form>;
}

/** Staff: ban an account from its admin standing page. */
export function BanForm({ username, onDone }: { username: string; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const hours = String(f.get("hours") || "").trim();
    if (!window.confirm(`Ban @${username}? This ends every session, hides the channel and stops any stream.`)) return;
    const r = await send("POST", `/api/admin/users/${encodeURIComponent(username)}/ban`, { reason: f.get("reason"), message_to_user: f.get("message") || "", staff_note: f.get("note") || "", hours: hours ? Number(hours) : undefined });
    setState(r.ok ? { saved: "Account banned." } : r.status === 403 ? { error: STEP_UP } : r);
    if (r.ok) onDone();
  }
  return <Section title="Ban account" intro="Separate from strikes. Use after a level-three review or for severe violations. The user can appeal once within 14 days.">
    <form onSubmit={submit}>
      <label className="field"><span>Reason (shown to the user)</span><input name="reason" required maxLength={500} /></label>
      <label className="field"><span>Message to the user (optional)</span><textarea name="message" maxLength={500} rows={2} /></label>
      <label className="field narrow"><span>Hours (leave blank for indefinite)</span><input name="hours" type="number" min={1} max={8760} /></label>
      <label className="field"><span>Staff note (private)</span><textarea name="note" maxLength={500} rows={2} /></label>
      <button type="submit" className="small">Ban account</button><Status state={state} />
    </form>
  </Section>;
}

type Queue = {
  bans: { id: string; username: string; reason: string; issued_at: string; until: string | null; issued_by: string | null; review_requested_at: string | null }[];
  appeals: { id: string; username: string; body: string; created_at: string; ban: { reason: string; until: string | null; staff_note: string } }[];
};
/** Staff queue: bans in force (with re-review requests first) and pending ban appeals. */
export function BanQueue() {
  const [queue, setQueue] = useState<Queue | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<Queue>("GET", "/api/admin/bans"); if (r.ok) setQueue(r.data); }, []);
  useLoad(load);
  const done = (r: { ok: boolean; status?: number; error?: string }, ok: string) => { setState(r.ok ? { saved: ok } : { error: r.status === 403 && !r.error?.includes("Another") ? STEP_UP : r.error }); load(); };
  async function lift(id: string) {
    const note = window.prompt("Staff note for lifting this ban");
    if (note) done(await send("POST", `/api/admin/bans/${id}/lift`, { staff_note: note }), "Ban lifted.");
  }
  async function decide(event: FormEvent<HTMLFormElement>, id: string) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    done(await send("POST", `/api/admin/ban-appeals/${id}/decision`, { outcome: f.get("outcome"), staff_note: f.get("note"), message_to_user: f.get("message") || "" }), "Decision saved.");
  }
  if (!queue) return <p className="loading">Loading…</p>;
  return <><h1>Bans</h1><Status state={state} />
    <Section title="Appeals waiting">{queue.appeals.length === 0 ? <p className="muted">No ban appeals waiting.</p> : queue.appeals.map(a => <form key={a.id} className="panel" onSubmit={e => decide(e, a.id)}>
      <p><strong>@{a.username}</strong> · {a.ban.reason} · {a.ban.until ? `until ${date(a.ban.until)}` : "indefinite"}</p>
      <blockquote>{a.body}</blockquote>{a.ban.staff_note && <p className="muted">Staff note: {a.ban.staff_note}</p>}
      <fieldset className="row"><legend className="sr-only">Outcome</legend><label className="row"><input type="radio" name="outcome" value="upheld" required /> Uphold</label><label className="row"><input type="radio" name="outcome" value="overturned" /> Overturn</label></fieldset>
      <label className="field"><span>Staff note (required)</span><textarea name="note" required maxLength={500} rows={2} /></label>
      <label className="field"><span>Message to the user (optional)</span><textarea name="message" maxLength={500} rows={2} /></label>
      <button type="submit" className="small">Decide</button>
    </form>)}</Section>
    <Section title="Bans in force">{queue.bans.length === 0 ? <p className="muted">No accounts are banned.</p> : <ul className="list">{queue.bans.map(b => <li key={b.id} className="row between">
      <span><Link href={`/admin/users/${b.username}`}>@{b.username}</Link> · {b.reason} · {b.until ? `until ${date(b.until)}` : "indefinite"}{b.issued_by && ` · by @${b.issued_by}`}{b.review_requested_at && <strong> · Re-review: a related strike was overturned</strong>}</span>
      <button type="button" className="small quiet" onClick={() => lift(b.id)}>Lift</button>
    </li>)}</ul>}</Section>
  </>;
}

/** Staff: replace an impersonating username with a neutral one. The page moves to the new name. */
export function UsernameResetForm({ username }: { username: string }) {
  const router = useRouter();
  const [state, setState] = useState<SaveState>({});
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const reason = new FormData(event.currentTarget).get("reason");
    if (!window.confirm(`Reset @${username}'s username? They get a neutral name, and @${username} is held for 90 days with no redirect.`)) return;
    const r = await send<{ username: string }>("POST", `/api/admin/users/${encodeURIComponent(username)}/username-reset`, { reason });
    if (r.ok) router.push(`/admin/users/${r.data.username}`);
    else setState(r.status === 403 ? { error: STEP_UP } : r);
  }
  return <Section title="Reset username" intro="For impersonation. The account keeps its followers, content and sessions; the old name stops working and can't be claimed for 90 days. The reason is shown to the user.">
    <form onSubmit={submit}>
      <label className="field"><span>Reason (shown to the user)</span><input name="reason" required maxLength={500} /></label>
      <button type="submit" className="small">Reset username</button><Status state={state} />
    </form>
  </Section>;
}
