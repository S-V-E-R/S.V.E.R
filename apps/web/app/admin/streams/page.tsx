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
export default function LiveStreams() {
  const [streams, setStreams] = useState<Stream[] | null>(null);
  const [categories, setCategories] = useState<Category[]>([]);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const [s, c] = await Promise.all([send<{ items: Stream[] }>("GET", "/api/admin/streams"), send<{ items: Category[] }>("GET", "/api/admin/categories")]);
    if (s.ok) setStreams(s.data.items); else setMessage(s.error);
    if (c.ok) setCategories(c.data.items);
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
    const result = await send<{ items: Category[] }>("POST", "/api/admin/categories", { name: data.get("name"), genre: data.get("genre") });
    if (result.ok) { setCategories(result.data.items); form.reset(); setMessage("Category added."); } else setMessage(result.error);
  }
  async function change(category: Category, body: { name?: string; active?: boolean }) {
    const result = await send<{ items: Category[] }>("PATCH", `/api/admin/categories/${encodeURIComponent(category.id)}`, body);
    if (result.ok) setCategories(result.data.items); else setMessage(result.error);
  }
  const genres = [...new Set(categories.map(c => c.genre))].sort();
  return <><h1>Live streams</h1>
    {message && <p role="status" className="form-message">{message}</p>}
    <Section title="On now" intro="Every open broadcast. Public is the viewer count shown on the channel; excluded and pending sessions are explained on the Integrity page.">
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
    </Section>
    <Section title="Categories" intro="Creators pick from active categories. Hiding one keeps it on channels that already use it until they change it. A category's genre can't change once created.">
      <form className="row" onSubmit={add}>
        <label className="field"><span>Name</span><input name="name" required maxLength={60} /></label>
        <label className="field"><span>Genre</span><input name="genre" required maxLength={40} list="category-genres" pattern="[a-z_]{2,40}" title="Lowercase letters and underscores" /></label>
        <datalist id="category-genres">{genres.map(g => <option key={g} value={g} />)}</datalist>
        <button type="submit" className="small">Add category</button>
      </form>
      <ul className="list">{categories.map(c => <li key={c.id} className="row between">
        <span>{c.name} <span className="muted">· {c.genre} · {c.channels} channel{c.channels === 1 ? "" : "s"}{c.active ? "" : " · hidden"}</span></span>
        <span className="row">
          <button type="button" className="small quiet" onClick={() => { const name = window.prompt("New name", c.name)?.trim(); if (name && name !== c.name) void change(c, { name }); }}>Rename</button>
          <button type="button" className="small quiet" onClick={() => change(c, { active: !c.active })}>{c.active ? "Hide" : "Show"}</button>
        </span>
      </li>)}</ul>
    </Section>
  </>;
}
