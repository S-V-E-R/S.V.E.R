"use client";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { Section } from "../../../components/Form";
import { GamePicker } from "../../../components/GamePicker";
import { send, useLoad } from "../../../lib/client-api";
import { beaconPath, compact, type BeaconItem } from "../../../lib/beacons";
import type { Chip } from "../../../lib/types";
import { duration } from "../../../lib/videos";
import "../../../styles/videos.css";
import "../../../styles/beacons.css";

type Crop = { x: number; y: number; width: number };
type Clip = { id: string; title: string; duration_ms: number; category: string | null; mature: boolean; created_at: string; clipper: Chip | null; file: string; thumbnail: string | null };
type Studio = { eligible: boolean; configured: boolean; uploads: boolean; daily_limit: number; remaining: number; last_crop: Crop | null; beacons: BeaconItem[]; clips: Clip[] };

const STATUS: Record<string, string> = { DRAFT: "Waiting for upload", PROCESSING: "Processing", READY: "Ready to publish", PUBLISHED: "Published", REMOVED: "Removed by staff", FAILED: "Failed" };
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * The 9:16 crop window over a clip. Drag it, or use the sliders or arrow keys. Fractions are of the
 * clip's own frame, so the server frames exactly what the creator saw.
 */
function CropTool({ src, crop, onChange }: { src: string; crop: Crop; onChange: (crop: Crop) => void }) {
  const [aspect, setAspect] = useState(16 / 9);
  const box = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; crop: Crop } | null>(null);
  const full = Math.min(1, (9 / 16) / aspect);
  const width = clamp(crop.width, full * 0.4, full);
  const height = (width * aspect * 16) / 9;
  const fit = useCallback((c: Crop): Crop => {
    const w = clamp(c.width, full * 0.4, full), h = (w * aspect * 16) / 9;
    return { width: w, x: clamp(c.x, 0, 1 - w), y: clamp(c.y, 0, Math.max(0, 1 - h)) };
  }, [aspect, full]);
  useEffect(() => { const fitted = fit(crop); if (fitted.x !== crop.x || fitted.y !== crop.y || fitted.width !== crop.width) onChange(fitted); }, [crop, fit, onChange]);
  function down(e: PointerEvent<HTMLDivElement>) { e.currentTarget.setPointerCapture(e.pointerId); drag.current = { x: e.clientX, y: e.clientY, crop }; }
  function moveTo(e: PointerEvent<HTMLDivElement>) {
    const start = drag.current, rect = box.current?.getBoundingClientRect();
    if (!start || !rect) return;
    onChange(fit({ ...start.crop, x: start.crop.x + (e.clientX - start.x) / rect.width, y: start.crop.y + (e.clientY - start.y) / rect.height }));
  }
  function key(e: KeyboardEvent<HTMLDivElement>) {
    const step = e.shiftKey ? 0.05 : 0.01;
    const moves: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    const m = moves[e.key];
    if (!m) return;
    e.preventDefault();
    onChange(fit({ ...crop, x: crop.x + m[0], y: crop.y + m[1] }));
  }
  return <div>
    <div className="beacon-crop" ref={box} style={{ aspectRatio: String(aspect) }}>
      <video src={src} muted loop autoPlay playsInline onLoadedMetadata={e => { const v = e.currentTarget; if (v.videoWidth && v.videoHeight) setAspect(v.videoWidth / v.videoHeight); }} aria-label="Clip preview" />
      <div className="beacon-crop-window" tabIndex={0} role="group" aria-label="9:16 crop window. Drag, or use the arrow keys to move it." onKeyDown={key}
        style={{ left: `${crop.x * 100}%`, top: `${crop.y * 100}%`, width: `${width * 100}%`, height: `${Math.min(1, height) * 100}%` }}
        onPointerDown={down} onPointerMove={moveTo} onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }} />
    </div>
    <div className="row">
      <label className="field"><span>Left to right</span><input type="range" min={0} max={1000} value={Math.round((crop.x / Math.max(0.0001, 1 - width)) * 1000) || 0} onChange={e => onChange(fit({ ...crop, x: (Number(e.target.value) / 1000) * (1 - width) }))} /></label>
      <label className="field"><span>Zoom</span><input type="range" min={40} max={100} value={Math.round((width / full) * 100)} onChange={e => onChange(fit({ ...crop, width: (full * Number(e.target.value)) / 100 }))} /></label>
      {height < 0.999 && <label className="field"><span>Top to bottom</span><input type="range" min={0} max={1000} value={Math.round((crop.y / Math.max(0.0001, 1 - height)) * 1000) || 0} onChange={e => onChange(fit({ ...crop, y: (Number(e.target.value) / 1000) * (1 - height) }))} /></label>}
    </div>
  </div>;
}

/** PUT straight to the signed upload URL, with progress. */
function upload(url: string, file: File, progress: (fraction: number) => void) {
  return new Promise<boolean>(resolve => {
    const request = new XMLHttpRequest();
    request.open("PUT", url);
    request.upload.onprogress = e => { if (e.lengthComputable) progress(e.loaded / e.total); };
    request.onload = () => resolve(request.status >= 200 && request.status < 300);
    request.onerror = () => resolve(false);
    request.send(file);
  });
}

function BeaconStudio() {
  const params = useSearchParams();
  const [data, setData] = useState<Studio | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [clip, setClip] = useState<Clip | null>(null);
  const [crop, setCrop] = useState<Crop>({ x: 0.34, y: 0, width: 0.3164 });
  const [title, setTitle] = useState("");
  const [uploadTitle, setUploadTitle] = useState("");
  const [publish, setPublish] = useState(true);
  const [category, setCategory] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const picked = useRef(false);
  const choose = useCallback((c: Clip, last: Crop | null) => { setClip(c); setTitle(c.title.slice(0, 120)); if (last) setCrop(last); }, []);
  const load = useCallback(async () => {
    const result = await send<Studio>("GET", "/api/me/beacons");
    if (!result.ok) { setMessage(result.error); return; }
    setData(result.data);
    // /studio/beacons?clip=ID preselects a clip (the "Make a Beacon" button on a clip page).
    const wanted = params.get("clip");
    if (wanted && !picked.current) {
      picked.current = true;
      const match = result.data.clips.find(c => c.id === wanted);
      if (match) choose(match, result.data.last_crop);
    }
  }, [params, choose]);
  useLoad(load);
  // Keep the list fresh while anything is processing.
  const pending = data?.beacons.some(b => ["PROCESSING", "DELETING"].includes(b.beacon.status)) ?? false;
  useEffect(() => { if (!pending) return; const timer = setInterval(() => void load(), 4000); return () => clearInterval(timer); }, [pending, load]);
  async function act(method: string, path: string, body?: unknown, done = "Saved.") {
    setMessage("");
    const result = await send(method, path, body);
    setMessage(result.ok ? done : result.error);
    if (result.ok) await load();
    return result.ok;
  }
  async function fromClip() {
    if (!clip || busy) return;
    setBusy(true);
    const ok = await act("POST", "/api/me/beacons", { source: "CLIP", clip_id: clip.id, crop, title, publish, request_id: crypto.randomUUID() }, publish ? "Your Beacon is processing and will publish when it's ready." : "Your Beacon is processing. Publish it when it's ready.");
    setBusy(false);
    if (ok) { setClip(null); setTitle(""); }
  }
  async function fromUpload() {
    if (!file || busy) return;
    if (file.size > 200 * 1024 * 1024) { setMessage("Videos can be up to 200 MB."); return; }
    setBusy(true); setMessage("");
    const created = await send<{ id: string; upload: { url: string } | null }>("POST", "/api/me/beacons", { source: "UPLOAD", title: uploadTitle, category_id: category, publish, request_id: crypto.randomUUID() });
    if (!created.ok || !created.data.upload) { setBusy(false); setMessage(created.ok ? "The upload couldn't start." : created.error); return; }
    setProgress(0);
    const sent = await upload(created.data.upload.url, file, setProgress);
    setProgress(null);
    if (!sent) { setBusy(false); setMessage("The upload was interrupted. Try again."); await load(); return; }
    await act("POST", `/api/beacons/${created.data.id}/complete`, undefined, "Uploaded. We're checking and processing your video.");
    setBusy(false); setFile(null); setUploadTitle("");
  }
  async function download(id: string) {
    const result = await send<{ url: string }>("POST", `/api/beacons/${id}/download`);
    if (result.ok) window.location.assign(result.data.url); else setMessage(result.error);
  }

  if (!data) return <p role="status">{message || "Loading Beacons…"}</p>;
  const canPost = data.eligible && data.configured && data.remaining > 0;
  return <><h1>Beacons</h1>
    <p className="intro">Short vertical videos that lead people to your channel and your live stream. They carry a small @username · sver.tv mark that moves between corners, so a repost still points back to you. Your clean copy is yours to download.</p>
    {!data.configured && <p className="notice">Beacon storage is not available yet.</p>}
    {!data.eligible && <p className="notice">Go live on S.V.E.R once and you can post Beacons. <Link href="/studio/stream">Set up your stream</Link></p>}
    {data.eligible && <p className="muted">{data.remaining} of {data.daily_limit} Beacons left today.</p>}

    <Section title="Make a Beacon from a clip" intro="Choose one of your channel's approved clips, then drag the 9:16 window to frame it. Viewer clips credit the clipper.">
      {!data.clips.length ? <p className="muted">No approved clips yet. Clips from your streams appear here. <Link href="/studio/videos">Manage clips</Link></p>
        : !clip ? <ul className="beacon-clip-picker">{data.clips.map(c => <li key={c.id}><button type="button" className="quiet" disabled={!canPost} onClick={() => choose(c, data.last_crop)}>
          {/* eslint-disable-next-line @next/next/no-img-element */}
          {c.thumbnail ? <img src={c.thumbnail} alt="" loading="lazy" width={320} height={180} /> : <span className="muted">No thumbnail</span>}
          <strong>{c.title}</strong><span className="muted">{duration(c.duration_ms)}{c.clipper ? ` · by ${c.clipper.display_name}` : ""}{c.mature ? " · 18+" : ""}</span>
        </button></li>)}</ul>
          : <form onSubmit={e => { e.preventDefault(); void fromClip(); }}>
            <p><strong>{clip.title}</strong> <button type="button" className="small quiet" onClick={() => setClip(null)}>Choose another clip</button></p>
            <CropTool src={clip.file} crop={crop} onChange={setCrop} />
            <label className="field"><span>Title</span><input value={title} onChange={e => setTitle(e.target.value)} required maxLength={120} /></label>
            <label className="checkbox"><input type="checkbox" checked={publish} onChange={e => setPublish(e.target.checked)} />Publish as soon as it&apos;s ready</label>
            <button disabled={busy || !canPost || !title.trim()}>{busy ? "Creating…" : "Create Beacon"}</button>
          </form>}
    </Section>

    {data.uploads && <Section title="Upload a video" intro="MP4, MOV or WebM, 5–60 seconds, up to 200 MB. Anything that isn't 9:16 is padded, never stretched.">
      <form onSubmit={e => { e.preventDefault(); void fromUpload(); }}>
        <label className="field"><span>Video file</span><input type="file" accept="video/mp4,video/quicktime,video/webm,.mp4,.mov,.webm" required disabled={busy || !canPost} onChange={e => setFile(e.target.files?.[0] ?? null)} /></label>
        <label className="field"><span>Title</span><input value={uploadTitle} onChange={e => setUploadTitle(e.target.value)} required maxLength={120} disabled={busy || !canPost} /></label>
        <GamePicker value={category} disabled={busy || !canPost} onChange={setCategory} />
        <label className="checkbox"><input type="checkbox" checked={publish} onChange={e => setPublish(e.target.checked)} />Publish as soon as it&apos;s ready</label>
        {progress !== null && <progress value={progress} max={1} aria-label="Upload progress" />}
        <button disabled={busy || !canPost || !file || !category || !uploadTitle.trim()}>{busy ? "Uploading…" : "Upload Beacon"}</button>
      </form>
    </Section>}

    <Section title="Your Beacons" intro="Views count after 3 seconds of real watching. Completions, follows and live joins show whether your Beacons bring people to you; only you can see them.">
      {!data.beacons.length ? <p className="muted">No Beacons yet.</p> : <ul className="beacon-studio-list">{data.beacons.map(item => {
        const b = item.beacon, s = item.stats;
        return <li key={b.id}>
          <span className="beacon-poster">
            {/* eslint-disable-next-line @next/next/no-img-element */}
            {item.thumbnail ? <img src={item.thumbnail} alt="" width={72} height={128} /> : <span>{b.mature ? "18+" : "…"}</span>}
          </span>
          <div>
            <h3>{["READY", "PUBLISHED"].includes(b.status) ? <Link href={beaconPath(b.id)}>{b.title}</Link> : b.title}</h3>
            <p className="muted">{STATUS[b.status] ?? b.status}{b.hidden && b.status === "PUBLISHED" ? " · Hidden by staff" : ""} · {b.source === "CLIP" ? "From a clip" : "Upload"}{b.duration_ms ? ` · ${duration(b.duration_ms)}` : ""}{item.clipper?.display_name ? ` · clipped by ${item.clipper.display_name}` : ""}</p>
            {b.failure && <p className="notice">{b.failure}</p>}
            {b.status === "PUBLISHED" && <p className="muted">{compact(b.views)} views · {compact(b.likes)} likes{s ? ` · ${s.completions} completions · ${s.follows} follows · ${s.live_joins} live joins` : ""}</p>}
            <div className="video-actions">
              {b.status === "READY" && <button className="small" onClick={() => void act("POST", `/api/beacons/${b.id}/publish`, undefined, "Published.")}>Publish</button>}
              {b.status === "FAILED" && <button className="small" onClick={() => void act("POST", `/api/beacons/${b.id}/retry`, undefined, "Trying again.")}>Retry</button>}
              {["READY", "PUBLISHED"].includes(b.status) && <button className="small quiet" onClick={() => void download(b.id)}>Download clean copy</button>}
              <button className="small quiet" onClick={() => setDeleting(b.id)}>Delete</button>
            </div>
            <form className="row" onSubmit={e => { e.preventDefault(); void act("PATCH", `/api/beacons/${b.id}`, { title: new FormData(e.currentTarget).get("title") }, "Title saved."); }}>
              <label className="field"><span className="sr-only">Title</span><input key={b.title} name="title" defaultValue={b.title} maxLength={120} required /></label><button className="small quiet">Rename</button>
            </form>
            {deleting === b.id && <div className="panel video-manage"><p>Delete this Beacon, every copy and its clean download? This cannot be undone.</p><button className="danger" onClick={async () => { if (await act("DELETE", `/api/beacons/${b.id}`, undefined, "Deleting.")) setDeleting(null); }}>Delete permanently</button> <button className="quiet" onClick={() => setDeleting(null)}>Cancel</button></div>}
          </div>
        </li>;
      })}</ul>}
    </Section>
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}

export default function Page() {
  return <Suspense fallback={<p role="status">Loading Beacons…</p>}><BeaconStudio /></Suspense>;
}
