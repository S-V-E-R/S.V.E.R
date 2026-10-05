"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";
import { utcDate, type War } from "../../../lib/war";
import { GenreBoard } from "../../../components/WarMap";
import { UserChip } from "../../../components/UserChip";
import type { Chip } from "../../../lib/types";

type Timeline = { seasons: { number: number; starts_at: string; ends_at: string; next_starts_at: string; finished_at: string | null; winners: string[] }[]; jobs: { id: number; season: number; ends_at: string; completed_at: string | null; attempts: number; error: string | null }[]; switches: { id: number; user: Chip | null; from: string | null; to: string; at: string; reason: string }[] };
type Genre = { id: string; name: string; home: string | null };
export default function Page() {
  const [data, setData] = useState<Timeline | null>(null);
  const [war, setWar] = useState<War | null>(null);
  const [genres, setGenres] = useState<Genre[]>([]);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const [d, w, g] = await Promise.all([send<Timeline>("GET", "/api/admin/factions"), send<War>("GET", "/api/factions/war"), send<{ items: Genre[] }>("GET", "/api/admin/genres")]);
    if (d.ok) setData(d.data); else setMessage(d.error);
    if (w.ok) setWar(w.data); else setMessage(w.error);
    if (g.ok) setGenres(g.data.items); else setMessage(g.error);
  }, []);
  useLoad(load);
  async function change(method: string, path: string, body: unknown) {
    setBusy(true); const r = await send(method, path, body); setMessage(r.ok ? "Saved." : r.error); if (r.ok) await load(); setBusy(false);
  }
  async function genre(event: FormEvent<HTMLFormElement>, id?: string) {
    event.preventDefault(); const f = new FormData(event.currentTarget);
    await change(id ? "PUT" : "POST", `/api/admin/genres${id ? `/${id}` : ""}`, { name: f.get("name"), note: f.get("note") });
  }
  return <><h1>Factions</h1><p>Read-only scores, season timeline and checkpoint recovery. <Link href="/admin/categories">Manage category assignments</Link>.</p>{message && <p className="notice" role="status">{message}</p>}<button className="small quiet" disabled={busy} onClick={() => void load()}>Refresh</button>
    <section className="panel"><h2>Season timeline</h2>{data?.seasons.map(s => <p key={s.number}><strong>Season {s.number}</strong> · {utcDate(s.starts_at)} to {utcDate(s.ends_at)} · {s.finished_at ? `Finished: ${s.winners.join(", ")}` : "Active or scheduled"} · Next: {utcDate(s.next_starts_at)}</p>)}</section>
    <section className="panel"><h2>Checkpoint jobs</h2><p>Retry only processes checkpoints that are due. Completed results cannot be run twice.</p><form onSubmit={e => { e.preventDefault(); void change("POST", "/api/admin/factions/retry", { note: new FormData(e.currentTarget).get("note") }); }}><label className="field"><span>Retry reason</span><input name="note" required maxLength={500} /></label><button className="small quiet" disabled={busy}>Retry due checkpoints</button></form><ul>{data?.jobs.map(j => <li key={j.id}>Season {j.season} · {utcDate(j.ends_at)} · {j.completed_at ? "Completed" : j.error ? "Failed" : "Pending"} · {j.attempts} attempts{j.error && ` · ${j.error}`}</li>)}</ul></section>
    <section><h2>Current scores</h2>{war && <GenreBoard genres={war.genres} />}</section>
    <section className="panel"><h2>Genres</h2><p>New genres start neutral. Home assignments remain fixed.</p><form onSubmit={e => genre(e)}><label className="field"><span>New genre name</span><input name="name" required maxLength={60} /></label><label className="field"><span>Reason</span><input name="note" required maxLength={500} /></label><button className="small" disabled={busy}>Add genre</button></form>{genres.map(g => <details key={g.id}><summary>{g.name} · {g.home ?? "neutral"}</summary><form onSubmit={e => genre(e, g.id)}><label className="field"><span>Name</span><input name="name" defaultValue={g.name} required maxLength={60} /></label><label className="field"><span>Reason</span><input name="note" required maxLength={500} /></label><button className="small quiet" disabled={busy}>Rename</button></form></details>)}</section>
    <section className="panel"><h2>Recent faction switches</h2><ul className="list">{data?.switches.map(s => <li key={s.id}>{s.user ? <UserChip user={s.user} /> : "Unavailable account"}<p>{s.from ?? "No faction"} → {s.to} · {utcDate(s.at)} · {s.reason.replaceAll("_", " ")}</p></li>)}</ul></section>
  </>;
}
