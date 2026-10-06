"use client";
import Link from "next/link";
import { useCallback, useEffect, useRef, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { duration, type VideoPage } from "../lib/videos";
import { RecordedPlayer } from "./RecordedPlayer";
import { VideoCut } from "./VideoCut";
import { ReportButton, TakeDownLink, type ReportTarget } from "./Report";

function Replay({ id, at, ageAck }: { id: string; at: number; ageAck: boolean }) {
  const [messages, setMessages] = useState<{ id: string; offset_ms: number; author: Chip; body: string }[]>([]);
  const [note, setNote] = useState("");
  const position = useRef(at);
  useEffect(() => { position.current = at; }, [at]);
  useEffect(() => {
    let stopped = false;
    async function load() {
      const result = await send<{ enabled: boolean; messages: typeof messages }>("GET", `/api/videos/${id}/chat?at_ms=${position.current}&age_ack=${ageAck}`);
      if (stopped) return;
      if (result.ok) { setMessages(result.data.messages); setNote(result.data.enabled ? "" : "Chat replay is turned off."); }
      else { setMessages([]); setNote(result.error); }
    }
    void load(); const timer = setInterval(() => void load(), 3000);
    return () => { stopped = true; clearInterval(timer); };
  }, [id, ageAck]);
  return <aside className="video-replay panel" aria-label="Chat replay"><h2>Chat replay</h2><p className="muted">Messages from this moment in the broadcast.</p>{note ? <p role="status">{note}</p> : <ol>{messages.map(m => <li key={m.id}><time>{duration(m.offset_ms)}</time><strong>{m.author.display_name}</strong> {m.body}</li>)}</ol>}</aside>;
}
export function VideoWatch({ id, initial = null, embed = false }: { id: string; initial?: VideoPage | null; embed?: boolean }) {
  const [data, setData] = useState(initial);
  const [ageAck, setAgeAck] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [cut, setCut] = useState<"CLIP" | "HIGHLIGHT" | null>(null);
  const [time, setTime] = useState(0);
  const [download, setDownload] = useState("");
  const [deleting, setDeleting] = useState(false);
  const load = useCallback(async () => {
    const result = await send<VideoPage>("GET", `/api/videos/${id}?age_ack=${ageAck}`);
    if (result.ok) { setData(result.data); setError(""); } else { setData(null); setError(result.error); }
  }, [id, ageAck]);
  useLoad(load);
  useEffect(() => { const timer = setInterval(() => void load(), 15000); return () => clearInterval(timer); }, [load]);
  async function action(method: string, path: string, body?: unknown) { const result = await send(method, path, body); setMessage(result.ok ? "Saved." : result.error); if (result.ok) await load(); return result.ok; }
  async function requestDownload() { const result = await send<{ url?: string }>("POST", `/api/videos/${id}/download`); if (result.ok) { setDownload(result.data.url ?? ""); setMessage(result.data.url ? "Your download is ready." : "Your MP4 is being prepared. Check again shortly."); } else setMessage(result.error); }
  if (!data) return <section className="panel video-manage"><h1>Recording</h1><p role="status">{error || "Loading…"}</p>{error.includes("18") && <button onClick={() => setAgeAck(true)} disabled={ageAck}>I am 18 or older</button>} {error && <Link href="/login">Sign in</Link>}</section>;
  const v = data.video;
  const target: ReportTarget = { target_type: v.kind === "CLIP" ? "clip" : v.kind === "HIGHLIGHT" ? "highlight" : "vod", target_id: id };
  const player = v.status === "PROCESSING" ? <p className="panel" role="status">Your video is processing. This page will update when it is ready.</p> : !v.recording ? <p className="panel">Recording is off for this broadcast. You can create a clip from the last two minutes.</p> : <RecordedPlayer id={id} ageAck={ageAck} chapters={embed ? [] : data.chapters} onTime={setTime} />;
  if (embed) return <section className="video-embed">{player}<footer><a href={`/${data.channel.username}/live`} target="_blank" rel="noopener noreferrer">{data.channel.display_name} on S.V.E.R</a></footer></section>;
  return <div className="video-watch"><section>
    {player}<h1>{v.title}</h1>
    <p><Link href={`/${data.channel.username}`}>{data.channel.display_name}</Link> · {v.category} {v.faction && <>· <Link href={`/factions/${v.faction}`}>{v.faction}</Link></>}</p>
    <p className="muted">{duration(v.duration_ms)} · {v.views.toLocaleString()} views · {new Date(v.started_at).toLocaleDateString()} {v.expires_at && <>· Available until {new Date(v.expires_at).toLocaleString()}</>}</p>
    {v.approval === "PENDING" && <p className="notice">This clip is waiting for approval.</p>}
    <div className="video-actions">
      {data.live && <Link className="button" href={`/${data.channel.username}/live`}>Watch live</Link>}
      {data.signed_in && v.kind !== "CLIP" && data.clip_permission !== "OFF" && <button className="quiet" onClick={() => setCut(cut === "CLIP" ? null : "CLIP")}>Create clip</button>}
      {data.can_highlight && v.recording && v.kind === "VOD" && <button className="quiet" onClick={() => setCut(cut === "HIGHLIGHT" ? null : "HIGHLIGHT")}>Save Highlight</button>}
      <button className="quiet" onClick={async () => { try { await navigator.clipboard.writeText(window.location.href.split("?")[0]); setMessage("Link copied."); } catch { setMessage("Copy the address from your browser to share this video."); } }}>Share</button>
      {data.can_download && v.kind !== "CLIP" && <button className="quiet" onClick={() => void requestDownload()}>Prepare download</button>}
      {download && <a className="button" href={download}>Download MP4</a>}
      {data.signed_in && !data.is_owner ? <ReportButton target={target} /> : <TakeDownLink target={target} />}
      {data.can_delete && <button className="quiet" onClick={() => setDeleting(true)}>Delete</button>}
    </div>
    {deleting && <div className="panel video-manage"><p>Delete this {v.kind === "CLIP" ? "clip" : "recording"} and its files? This cannot be undone.</p><button onClick={async () => { if (await action("DELETE", `/api/videos/${id}`)) setDeleting(false); }}>Delete permanently</button> <button className="quiet" onClick={() => setDeleting(false)}>Cancel</button></div>}
    {cut && <VideoCut key={`${id}-${cut}`} video={v} highlight={cut === "HIGHLIGHT"} ageAck={ageAck} />}
    {data.can_manage && v.kind === "CLIP" && v.approval === "PENDING" && <form className="panel video-manage" onSubmit={async e => { e.preventDefault(); const f = new FormData(e.currentTarget); await action("POST", `/api/videos/${id}/approval`, { approve: f.get("decision") === "approve", reason: f.get("reason") }); }}><h2>Review clip</h2><label className="field"><span>Decision</span><select name="decision"><option value="approve">Approve</option><option value="reject">Reject and delete</option></select></label><label className="field"><span>Reason</span><input name="reason" required maxLength={500} /></label><button>Save decision</button></form>}
    {data.is_owner && <details className="panel video-manage"><summary>Manage recording</summary>
      <form onSubmit={async e => { e.preventDefault(); const f = new FormData(e.currentTarget); await action("PATCH", `/api/videos/${id}`, { title: f.get("title"), visibility: f.get("visibility") }); }}><label className="field"><span>Title</span><input key={v.title} name="title" defaultValue={v.title} maxLength={v.kind === "CLIP" ? 100 : 140} required /></label><label className="field"><span>Visibility</span><select key={v.visibility} name="visibility" defaultValue={v.visibility}><option value="PUBLIC">Public</option><option value="SUBSCRIBERS">Subscribers</option><option value="PRIVATE">Private: you and your mods</option></select></label><button>Save</button></form>
      {v.kind !== "CLIP" && <p><button className="quiet" onClick={() => void action("POST", `/api/videos/${id}/thumbnail`, { offset_ms: time })}>Use frame at {duration(time)} as thumbnail</button></p>}
      {data.chapters.length > 0 && <><h2>Chapters</h2>{data.chapters.map(c => <form className="row" key={c.id} onSubmit={async e => { e.preventDefault(); await action("PUT", `/api/videos/${id}/chapters`, { id: c.id, label: new FormData(e.currentTarget).get("label") }); }}><span>{duration(c.offset_ms)}</span><label className="field"><span className="sr-only">Chapter label</span><input key={c.label} name="label" required defaultValue={c.label} maxLength={100} /></label><button className="small">Rename</button><button className="small quiet" type="button" onClick={() => void action("PUT", `/api/videos/${id}/chapters`, { id: c.id, merge_next: true })}>Merge next</button><button className="small quiet" type="button" onClick={() => void action("PUT", `/api/videos/${id}/chapters`, { id: c.id })}>Remove</button></form>)}</>}
    </details>}
    {message && <p role="status">{message}</p>}
  </section>{data.chat_replay && <Replay id={id} at={time} ageAck={ageAck} />}</div>;
}
