"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { duration, videoPath, type Video } from "../../../lib/videos";
import type { Chip } from "../../../lib/types";
import "../../../styles/videos.css";
type Settings = { recording: boolean; visibility: string; clip_permission: string; clip_approval: boolean; chat_replay: boolean; mature: boolean; copyright_restricted: boolean };
type Library = { settings: Settings; videos: Video[]; has_more: boolean; editors: Chip[]; configured: boolean; highlight_used_ms: number; highlight_limit_ms: number; retention_hours: number; username: string; editing: Video[] };
export default function Videos() {
  const [data, setData] = useState<Library | null>(null);
  const [message, setMessage] = useState("");
  const [saving, setSaving] = useState(false);
  const [filter, setFilter] = useState("ALL");
  const [offset, setOffset] = useState(0);
  const [download, setDownload] = useState("");
  const load = useCallback(async () => { const result = await send<Library>("GET", `/api/me/videos?kind=${filter}&offset=${offset}`); if (result.ok) setData(result.data); else setMessage(result.error); }, [filter, offset]);
  useLoad(load);
  async function save(change: Partial<Settings>) { if (!data || saving) return; setSaving(true); setMessage(""); const settings = { ...data.settings, ...change }; setData({ ...data, settings }); const result = await send("PUT", "/api/me/videos", settings); setSaving(false); if (result.ok) { setMessage("Saved. Recording and default visibility apply to the next broadcast."); } else { setData(data); setMessage(result.error); } }
  async function editor(username: string, enabled: boolean) { const result = await send("PUT", "/api/me/videos/editors", { username, enabled }); setMessage(result.ok ? "Editor permissions saved." : result.error); if (result.ok) await load(); }
  if (!data) return <p role="status">{message || "Loading recordings…"}</p>;
  const s = data.settings;
  return <><h1>Videos &amp; clips</h1>
    {!data.configured && <p className="notice">Recording storage is not available yet.</p>}
    {s.copyright_restricted && <p className="notice">Recording and clipping are restricted following copyright review. See your copyright cases.</p>}
    <Section title="Recording" intro={`Past broadcasts are kept for ${data.retention_hours} hours after the stream ends. Highlights are permanent.`}>
      <label className="checkbox"><input type="checkbox" disabled={saving} checked={s.recording} onChange={e => void save({ recording: e.target.checked })} />Record my broadcasts</label>
      <label className="field"><span>Default visibility</span><select disabled={saving} value={s.visibility} onChange={e => void save({ visibility: e.target.value })}><option value="PUBLIC">Public</option><option value="SUBSCRIBERS">Subscribers</option><option value="PRIVATE">Private: you and your mods</option></select></label>
      <label className="checkbox"><input type="checkbox" disabled={saving} checked={s.mature} onChange={e => void save({ mature: e.target.checked })} />My recordings are for viewers aged 18 and over</label>
      <label className="checkbox"><input type="checkbox" disabled={saving} checked={s.chat_replay} onChange={e => void save({ chat_replay: e.target.checked })} />Allow chat replay</label>
      <p className="muted">Turning recording off keeps only a short live window and the last two minutes for clipping.</p>
    </Section>
    <Section title="Clips" intro="Set who can clip your channel and whether clips need review.">
      <label className="field"><span>Who can clip?</span><select disabled={saving} value={s.clip_permission} onChange={e => void save({ clip_permission: e.target.value })}>{[['SIGNED_IN', 'Signed-in viewers'], ['FOLLOWERS', 'Followers'], ['SUBSCRIBERS', 'Subscribers'], ['MODS', 'You and your mods'], ['OFF', 'Nobody']].map(([v, label]) => <option key={v} value={v}>{label}</option>)}</select></label>
      <label className="checkbox"><input type="checkbox" disabled={saving} checked={s.clip_approval} onChange={e => void save({ clip_approval: e.target.checked })} />Review viewer clips before publishing</label>
      <p className="muted">Your clips and your mods&apos; clips skip viewer approval. MAGNet suggestions always wait for approval.</p>
    </Section>
    <Section title="Highlight storage" intro={`${duration(data.highlight_used_ms)} used of ${duration(data.highlight_limit_ms)}. Delete a Highlight to free space.`}><progress value={data.highlight_used_ms} max={data.highlight_limit_ms} aria-label="Highlight storage used" /></Section>
    <Section title="Live marker" intro="Mark a moment while recording. You and your mods can also use !marker in chat."><form className="row" onSubmit={async e => { e.preventDefault(); const result = await send("POST", `/api/channels/${data.username}/marker`, { label: new FormData(e.currentTarget).get("label") }); setMessage(result.ok ? "Marker added." : result.error); }}><label className="field"><span>Label (optional)</span><input name="label" maxLength={100} /></label><button>Add marker</button></form></Section>
    <Section title="Library" intro="Open a video to watch, edit chapters, pick a thumbnail, save a Highlight or download it."><label className="field"><span>Show</span><select value={filter} onChange={e => { setFilter(e.target.value); setOffset(0); }}>{[['ALL','All videos'],['PENDING','Approval queue'],['VOD','Past broadcasts'],['HIGHLIGHT','Highlights'],['CLIP','Clips']].map(([v,l]) => <option value={v} key={v}>{l}</option>)}</select></label>
      <ul className="video-studio-list">{data.videos.filter(v => filter === "ALL" || (filter === "PENDING" ? v.approval === "PENDING" : v.kind === filter)).map(v => <li key={v.id}><h3><Link href={videoPath(v)}>{v.title}</Link></h3><p className="muted">{v.kind === "VOD" ? "Past broadcast" : v.kind === "CLIP" ? "Clip" : "Highlight"} · {duration(v.duration_ms)} · {v.visibility.toLowerCase()} · {v.approval === "PENDING" ? "Waiting for review" : v.status.toLowerCase()}</p></li>)}</ul>{!data.videos.length && <p>No videos match this view.</p>}<nav className="video-actions" aria-label="Library pages">{offset > 0 && <button className="quiet" onClick={() => setOffset(Math.max(0, offset - 100))}>Previous videos</button>}{data.has_more && <button className="quiet" onClick={() => setOffset(offset + 100)}>More videos</button>}</nav>
    </Section>
    <Section title="Editors" intro="Editors can download your past broadcasts and Highlights, including private recordings."><form className="row" onSubmit={async e => { e.preventDefault(); await editor(String(new FormData(e.currentTarget).get("username")), true); }}><label className="field"><span>Username</span><input name="username" required maxLength={25} /></label><button>Add editor</button></form><ul className="list">{data.editors.map(e => <li key={e.username}>{e.display_name} <button className="small quiet" onClick={() => void editor(e.username ?? "", false)}>Remove</button></li>)}</ul></Section>
    {data.editing.length > 0 && <Section title="Shared with you" intro="Recordings whose creators appointed you as an editor."><ul className="video-studio-list">{data.editing.map(v => <li key={v.id}>{v.title} <button className="small quiet" onClick={async () => { const result = await send<{ url?: string }>("POST", `/api/videos/${v.id}/download`); if (result.ok) { setDownload(result.data.url ?? ""); setMessage(result.data.url ? "Download ready." : "Preparing MP4. Check again shortly."); } else setMessage(result.error); }}>Prepare download</button></li>)}</ul>{download && <a href={download} className="button">Download MP4</a>}</Section>}
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}
