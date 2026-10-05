"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Stream = { id: string; state: string; started_at: string; title: string; category: string | null; open_reports: number; followers_only_until: string | null;
  channel: { username: string; display_name: string };
  health: { video_codec?: string | null; input_kbps?: number | null; codec_warning?: boolean; bitrate_warning?: boolean; keyframe_warning?: boolean; keyframe_seconds?: number | null; b_frames?: boolean | null };
  counts: { sessions: number; public: number; trusted: number; excluded: number; pending: number } };
type Category = { id: string; name: string; genre: string; active: boolean; channels: number };

const since = (iso: string) => { const m = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000)); return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`; };

/** Staff → Live streams: what's on now (health, counts, reports), an audited stop, and the category catalog. */
export default function LiveStreams({ catalogOnly = false }: { catalogOnly?: boolean }) {
  const [streams, setStreams] = useState<Stream[] | null>(null);
  const [categories, setCategories] = useState<Category[]>([]);
  const [genres, setGenres] = useState<{ id: string; name: string }[]>([]);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const [s, c, g] = await Promise.all([send<{ items: Stream[] }>("GET", "/api/admin/streams"), send<{ items: Category[] }>("GET", "/api/admin/categories"), send<{ items: { id: string; name: string }[] }>("GET", "/api/admin/genres")]);
    if (s.ok) setStreams(s.data.items); else setMessage(s.error);
    if (c.ok) setCategories(c.data.items);
    if (g.ok) setGenres(g.data.items);
  }, []);
  useLoad(load);
  async function stop(stream: Stream) {
    const reason = window.prompt(`Stop @${stream.channel.username}'s stream? Their stream key is revoked and they'll need a new one. Reason (required):`)?.trim();
    if (!reason) return;
    const result = await send("POST", `/api/admin/streams/${stream.id}/stop`, { reason });
    setMessage(result.ok ? `Stopped @${stream.channel.username}'s stream.` : result.error);
    await load();
  }
  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const result = await send<{ items: Category[] }>("POST", "/api/admin/categories", { name: data.get("name"), genre: data.get("genre"), note: data.get("note") });
    if (result.ok) { setCategories(result.data.items); form.reset(); setMessage("Category added."); } else setMessage(result.error);
  }
  async function change(category: Category, body: { name?: string; active?: boolean; genre?: string }) {
    const note = window.prompt("Reason for this category change (required)")?.trim();
    if (!note) return;
    const result = await send<{ items: Category[] }>("PATCH", `/api/admin/categories/${encodeURIComponent(category.id)}`, { ...body, note });
    if (result.ok) setCategories(result.data.items); else setMessage(result.error);
  }
  async function merge(category: Category, into: string, note: string) {
    const r = await send("POST", `/api/admin/categories/${category.id}/merge`, { into, note });
    setMessage(r.ok ? "Categories merged." : r.error); if (r.ok) await load();
  }
  return <><h1>{catalogOnly ? "Categories and genres" : "Live streams"}</h1>
    {message && <p role="status" className="form-message">{message}</p>}
    {!catalogOnly && <Section title="On now" intro="Every open broadcast. Public is the viewer count shown on the channel; excluded and pending sessions are explained on the Integrity page.">
      <button type="button" className="small quiet" onClick={() => void load()}>Refresh</button>
      {!streams ? <p className="loading">Loading…</p> : streams.length === 0 ? <p className="muted">No one is live.</p> :
        <ul className="list">{streams.map(s => <li key={s.id} className="panel inline-panel">
          <p><Link href={`/${s.channel.username}`}><strong>{s.channel.display_name}</strong></Link> <span className="muted">@{s.channel.username} · {s.state} · {since(s.started_at)}</span></p>
          <p>{s.title}{s.category && <span className="muted"> · {s.category}</span>}</p>
          <p className="muted">Public {s.counts.public} · trusted {s.counts.trusted} · pending {s.counts.pending} · excluded {s.counts.excluded} · sessions {s.counts.sessions}</p>
          <p className="muted">{s.health.video_codec ?? "codec not measured"}{s.health.input_kbps != null && ` · ${Math.round(s.health.input_kbps)} Kbps`}{s.health.keyframe_seconds != null && ` · keyframes ${s.health.keyframe_seconds} s`}{s.health.b_frames != null && ` · B-frames ${s.health.b_frames ? "on" : "off"}`}
            {(s.health.codec_warning || s.health.bitrate_warning || s.health.keyframe_warning || s.health.b_frames) && <strong> · OBS warning</strong>}</p>
          {(s.open_reports > 0 || s.followers_only_until) && <p>{s.open_reports > 0 && <Link href="/admin/reports">{s.open_reports} open report{s.open_reports === 1 ? "" : "s"}</Link>}{s.followers_only_until && <span className="muted"> Followers-only chat on</span>}</p>}
          <button type="button" className="small quiet danger-text" onClick={() => stop(s)}>Stop stream</button>
        </li>)}</ul>}
    </Section>}
    {!catalogOnly && <Spotlights />}
    <Section title="Categories" intro="Creators pick from active categories. Hiding one keeps it on channels that already use it until they change it. Genre changes and merges across genres are allowed only between seasons.">
      <form className="row" onSubmit={add}>
        <label className="field"><span>Name</span><input name="name" required maxLength={60} /></label>
        <label className="field"><span>Genre</span><select name="genre" required defaultValue=""><option value="" disabled>Choose a genre</option>{genres.map(g => <option key={g.id} value={g.id}>{g.name}</option>)}</select></label>
        <label className="field"><span>Reason</span><input name="note" required maxLength={500} /></label>
        <button type="submit" className="small">Add category</button>
      </form>
      <ul className="list">{categories.map(c => <li key={c.id} className="row between">
        <span>{c.name} <span className="muted">· {c.genre} · {c.channels} channel{c.channels === 1 ? "" : "s"}{c.active ? "" : " · hidden"}</span></span>
        <span className="row">
          <button type="button" className="small quiet" onClick={() => { const name = window.prompt("New name", c.name)?.trim(); if (name && name !== c.name) void change(c, { name }); }}>Rename</button>
          <button type="button" className="small quiet" onClick={() => change(c, { active: !c.active })}>{c.active ? "Hide" : "Show"}</button>
        <details><summary>Move or merge</summary><form onSubmit={e => { e.preventDefault(); void change(c, { genre: String(new FormData(e.currentTarget).get("genre")) }); }}><label className="field"><span>Genre</span><select name="genre" defaultValue={c.genre}>{genres.map(g => <option key={g.id} value={g.id}>{g.name}</option>)}</select></label><button className="small quiet">Move category</button></form><form onSubmit={e => { e.preventDefault(); const d = new FormData(e.currentTarget); if (window.confirm(`Merge ${c.name} into the selected category? Existing channels will move to it.`)) void merge(c, String(d.get("into")), String(d.get("note"))); }}><label className="field"><span>Merge into</span><select name="into" required defaultValue=""><option value="" disabled>Choose a category</option>{categories.filter(d => d.id !== c.id && d.active).map(d => <option key={d.id} value={d.id}>{d.name}</option>)}</select></label><label className="field"><span>Reason</span><input name="note" required maxLength={500} /></label><button className="small quiet">Merge category</button></form></details>
        </span>
      </li>)}</ul>
    </Section>
  </>;
}

type Spotlight = { id: string; username: string; reason: string; starts_at: string; ends_at: string; ended_early_at: string | null; active: boolean };

/** Staff spotlights (docs/MAGNET.md "Spotlights"): a public reason, at most 14 days, one active per channel, 7-day cooldown. */
function Spotlights() {
  const [items, setItems] = useState<Spotlight[] | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const result = await send<{ items: Spotlight[] }>("GET", "/api/admin/spotlights");
    if (result.ok) setItems(result.data.items); else setMessage(result.error);
  }, []);
  useLoad(load);
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const result = await send<{ items: Spotlight[] }>("POST", "/api/admin/spotlights", { username: String(data.get("username") ?? "").trim(), reason: String(data.get("reason") ?? "").trim(), days: Number(data.get("days")) });
    if (result.ok) { setItems(result.data.items); form.reset(); setMessage("Spotlight added."); } else setMessage(result.error);
  }
  async function end(id: string) {
    const result = await send<{ items: Spotlight[] }>("POST", `/api/admin/spotlights/${id}/end`);
    if (result.ok) { setItems(result.data.items); setMessage("Spotlight ended."); } else setMessage(result.error);
  }
  return <Section title="Spotlights" intro="Staff picks shown on the homepage with a short public reason. At most 14 days, one at a time per channel, then 7 days before the same channel again. First streams and returning creators are spotlighted automatically.">
    <form className="row" onSubmit={create}>
      <label className="field"><span>Channel</span><input name="username" required maxLength={26} autoComplete="off" /></label>
      <label className="field"><span>Public reason</span><input name="reason" required maxLength={120} /></label>
      <label className="field"><span>Days</span><input name="days" type="number" min={1} max={14} defaultValue={7} required /></label>
      <button type="submit" className="small">Spotlight</button>
    </form>
    {message && <p role="status" className="form-message">{message}</p>}
    {!items ? <p className="loading">Loading…</p> : items.length === 0 ? <p className="muted">No staff spotlights yet.</p> :
      <ul className="list">{items.map(s => <li key={s.id} className="row between">
        <span><Link href={`/${s.username}`}>@{s.username}</Link> · {s.reason} <span className="muted">· {s.active ? `until ${new Date(s.ends_at).toLocaleDateString()}` : s.ended_early_at ? "ended early" : "ended"}</span></span>
        {s.active && <button type="button" className="small quiet" onClick={() => end(s.id)}>End now</button>}
      </li>)}</ul>}
  </Section>;
}
